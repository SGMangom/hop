import type {
  DetectLocalFontsOptions,
  GetLocalFontsOptions,
  LocalFontRecord,
  LocalFontSnapshot,
  LocalFontState,
} from '@/upstream/local-fonts';
import { REGISTERED_FONTS } from './font-catalog';
import {
  filterAuthoringFontFamilies,
  isAuthoringFontFamilyAllowed,
} from './font-authoring-policy';

export interface LocalFontEntry {
  family: string;
  postScriptName: string;
  style: string;
  weight?: number;
  sourceKind: 'system-installed' | 'file-backed' | 'hft-derived';
  path?: string | null;
  aliases?: string[];
}

let cachedFontEntries: LocalFontEntry[] | null = null;
let detectedAt: string | null = null;
let lastError: string | null = null;
interface DesktopFontFaceState {
  pending: Promise<FontFace>;
  face?: FontFace;
}
const loadedFontFaces = new Map<string, DesktopFontFaceState>();
const fontBinaryCache = new Map<string, Promise<Uint8Array>>();

export function isDesktopTauriRuntime(): boolean {
  return typeof window !== 'undefined'
    && ('__TAURI_INTERNALS__' in window || window.location?.protocol === 'tauri:');
}

export async function loadStoredDesktopFonts(): Promise<LocalFontSnapshot | null> {
  return cachedFontEntries ? desktopSnapshot() : null;
}

export function clearStoredDesktopFonts(): void {
  resetDesktopFonts();
}

export async function detectDesktopFontEntries(force = false): Promise<LocalFontEntry[]> {
  if (cachedFontEntries && !force) return cachedFontEntries;
  try {
    const { invoke } = await import('@tauri-apps/api/core');
    const entries = await invoke<LocalFontEntry[]>('list_local_fonts');
    if (force) clearLoadedDesktopFontFaces();
    cachedFontEntries = normalizeFontEntries(entries);
    detectedAt = new Date().toISOString();
    lastError = null;
    return cachedFontEntries;
  } catch (error) {
    lastError = error instanceof Error ? error.message : String(error);
    throw error;
  }
}

export async function detectDesktopFonts(options: DetectLocalFontsOptions = {}): Promise<string[]> {
  await detectDesktopFontEntries(options.force);
  return getDesktopFonts(options);
}

export function getDesktopFontRecords(options: GetLocalFontsOptions = {}): LocalFontRecord[] {
  const records = (cachedFontEntries ?? []).map(toLocalFontRecord);
  if (options.includeRegistered) return records;
  return records.filter((record) => !record.aliases.some((name) => REGISTERED_FONTS.has(name)));
}

export function getDesktopFonts(options: GetLocalFontsOptions = {}): string[] {
  return uniqueAuthoringFamilies(getDesktopFontRecords(options).map((record) => record.displayName));
}

export function getDetectedDesktopFonts(): string[] {
  return uniqueFamilies((cachedFontEntries ?? []).map((entry) => entry.family));
}

export function resolveDesktopFont(fontName: string): LocalFontRecord | null {
  const normalized = normalizeFontName(fontName);
  const entries = cachedFontEntries ?? [];
  const postscriptMatch = entries.find((entry) => normalizeFontName(entry.postScriptName) === normalized);
  if (postscriptMatch) return toLocalFontRecord(postscriptMatch);

  const familyMatches = entries.filter((entry) => normalizeFontName(entry.family) === normalized);
  if (familyMatches.length === 1) return toLocalFontRecord(familyMatches[0]);
  const regularMatch = preferredRegularEntry(familyMatches);
  if (regularMatch) return toLocalFontRecord(regularMatch);

  // HFT/HWP family spelling frequently differs only by spaces, '-' or '_'
  // (e.g. "HCI Poppy" vs "HCIPoppy").  Never apply this fuzzy key to
  // arbitrary local/system fonts: it is safe only for a face proven to come
  // from HOP's private HFT-derived cache.
  const aliasKey = fontFamilyAliasKey(fontName);
  if (!aliasKey) return null;
  const derivedMatches = entries.filter((entry) => isHftDerivedEntry(entry)
    && entryNames(entry).some((name) => fontFamilyAliasKey(name) === aliasKey));
  if (derivedMatches.length === 1) return toLocalFontRecord(derivedMatches[0]);
  const derivedRegular = preferredRegularEntry(derivedMatches);
  return derivedRegular ? toLocalFontRecord(derivedRegular) : null;
}

export function hasExactDesktopDerivedFont(fontName: string): boolean {
  const key = fontFamilyAliasKey(fontName);
  if (!key) return false;
  return (cachedFontEntries ?? []).some((entry) => isHftDerivedEntry(entry)
    && entryNames(entry).some((name) => fontFamilyAliasKey(name) === key));
}

export async function loadDesktopFontBytesFor(
  fontNames: readonly string[],
): Promise<Map<string, ArrayBuffer>> {
  const loaded = await Promise.all(fontNames.map(async (fontName) => {
    const record = resolveDesktopFont(fontName);
    if (!record) return null;
    const bytes = await loadDesktopFontBytes(record.postscriptName || record.family);
    return bytes ? ([localFontFaceKey(record), bytes] as const) : null;
  }));
  return new Map(loaded.filter((entry): entry is readonly [string, ArrayBuffer] => entry !== null));
}

export async function loadDesktopFontBytes(fontName: string): Promise<ArrayBuffer | null> {
  const record = resolveDesktopFont(fontName);
  if (!record) return null;
  const entry = (cachedFontEntries ?? []).find(
    (candidate) => normalizeFontName(candidate.postScriptName) === normalizeFontName(record.postscriptName),
  );
  if (!entry?.path) return null;
  return (await readDesktopFontBytes(entry.path)).slice().buffer as ArrayBuffer;
}

export function getDesktopFontState(): LocalFontState {
  return {
    supported: true,
    method: 'local-font-access',
    loaded: cachedFontEntries !== null,
    stored: cachedFontEntries !== null,
    source: cachedFontEntries ? 'local-font-access' : null,
    complete: cachedFontEntries !== null,
    storage: 'none',
    count: cachedFontEntries?.length ?? 0,
    checkedFamilies: [],
    detectedAt,
    lastError,
  };
}

export function resetDesktopFonts(): void {
  cachedFontEntries = null;
  detectedAt = null;
  lastError = null;
  clearLoadedDesktopFontFaces();
}

function clearLoadedDesktopFontFaces(): void {
  if (typeof document !== 'undefined' && document.fonts) {
    for (const state of loadedFontFaces.values()) {
      if (state.face) document.fonts.delete?.(state.face);
    }
  }
  loadedFontFaces.clear();
  fontBinaryCache.clear();
}

export async function ensureDesktopFontsAvailable(
  targetFamilies?: Iterable<string>,
): Promise<Set<string>> {
  const entries = await detectDesktopFontEntries();
  const available = new Set(entries
    .filter((entry) => entry.sourceKind === 'system-installed')
    .filter((entry) => isAuthoringFontFamilyAllowed(entry.family, false))
    .map((entry) => entry.family));
  if (!supportsBinaryFontLoading()) return available;

  const requested = resolveRequestedEntries(entries, targetFamilies);
  const groups = groupEntriesByPath([...requested.keys()]);
  await Promise.all([...groups].map(async ([path, pathEntries]) => {
    let fontBytes: Uint8Array;
    try {
      fontBytes = await readDesktopFontBytes(path);
    } catch {
      return;
    }
    for (const entry of pathEntries) {
      const requestedNames = requested.get(entry) ?? [entry.family];
      try {
        if (!await ensureDesktopFontFaces(entry, fontBytes, requestedNames)) continue;
      } catch {
        continue;
      }
      const exactDerived = isHftDerivedEntry(entry);
      if (isAuthoringFontFamilyAllowed(entry.family, exactDerived)) {
        available.add(entry.family);
        for (const alias of entry.aliases ?? []) available.add(alias);
        for (const requestedName of requestedNames) available.add(requestedName);
      }
    }
  }));
  return available;
}

async function readDesktopFontBytes(path: string): Promise<Uint8Array> {
  let pending = fontBinaryCache.get(path);
  if (!pending) {
    pending = (async () => {
      const { invoke } = await import('@tauri-apps/api/core');
      return new Uint8Array(await invoke<number[]>('read_local_font', { path }));
    })();
    fontBinaryCache.set(path, pending);
    void pending.then(
      () => { if (fontBinaryCache.get(path) === pending) fontBinaryCache.delete(path); },
      () => { if (fontBinaryCache.get(path) === pending) fontBinaryCache.delete(path); },
    );
  }
  return pending;
}

async function ensureDesktopFontFaces(
  entry: LocalFontEntry,
  bytes: Uint8Array,
  requestedNames: readonly string[],
): Promise<boolean> {
  const familyNames = uniqueFamilies([entry.family, ...(entry.aliases ?? []), ...requestedNames]);
  const results = await Promise.all(familyNames.map((family) => ensureDesktopFontFace(entry, family, bytes)));
  return results.some(Boolean);
}

async function ensureDesktopFontFace(
  entry: LocalFontEntry,
  family: string,
  bytes: Uint8Array,
): Promise<boolean> {
  const key = `${fontEntryKey(entry)}\u0000${normalizeFontName(family)}`;
  let state = loadedFontFaces.get(key);
  if (!state) {
    state = { pending: loadDesktopFontFace(entry, family, bytes) };
    loadedFontFaces.set(key, state);
  }
  let face: FontFace;
  try {
    face = await state.pending;
  } catch (error) {
    if (loadedFontFaces.get(key) === state) loadedFontFaces.delete(key);
    throw error;
  }
  if (loadedFontFaces.get(key) !== state) return false;
  if (!state.face) {
    document.fonts.add(face);
    state.face = face;
  }
  return true;
}

async function loadDesktopFontFace(entry: LocalFontEntry, family: string, bytes: Uint8Array): Promise<FontFace> {
  const descriptors: FontFaceDescriptors = { style: entry.style || 'normal' };
  if (entry.weight) descriptors.weight = String(entry.weight);
  const face = new FontFace(family, bytes.slice(), descriptors);
  return face.load();
}

function normalizeFontEntries(entries: LocalFontEntry[]): LocalFontEntry[] {
  const byKey = new Map<string, LocalFontEntry>();
  for (const entry of entries) {
    const family = entry.family.trim();
    if (!family) continue;
    const normalized: LocalFontEntry = {
      family,
      postScriptName: entry.postScriptName?.trim() || family,
      style: entry.style?.trim() || 'normal',
      weight: entry.weight,
      sourceKind: entry.sourceKind ?? 'system-installed',
      path: entry.path ?? null,
      aliases: uniqueFamilies((entry.aliases ?? []).map((alias) => alias.trim()).filter(Boolean)),
    };
    byKey.set(fontEntryKey(normalized), normalized);
  }
  return [...byKey.values()].sort((left, right) =>
    left.family.localeCompare(right.family, 'ko')
    || left.style.localeCompare(right.style, 'en')
    || left.postScriptName.localeCompare(right.postScriptName, 'en'),
  );
}

function toLocalFontRecord(entry: LocalFontEntry): LocalFontRecord {
  return {
    family: entry.family,
    fullName: entry.postScriptName || entry.family,
    postscriptName: entry.postScriptName,
    style: entry.style,
    displayName: entry.family,
    aliases: Array.from(new Set([entry.family, entry.postScriptName, ...(entry.aliases ?? [])].filter(Boolean))),
  };
}

function desktopSnapshot(): LocalFontSnapshot {
  return {
    version: 2,
    detectedAt: detectedAt ?? new Date().toISOString(),
    families: getDetectedDesktopFonts(),
    fontRecords: getDesktopFontRecords({ includeRegistered: true }),
    source: 'local-font-access',
  };
}

function resolveRequestedEntries(
  entries: LocalFontEntry[],
  targetFamilies?: Iterable<string>,
): Map<LocalFontEntry, string[]> {
  const requestedNames = targetFamilies
    ? Array.from(targetFamilies).map((family) => family.trim()).filter(Boolean)
    : entries.map((entry) => entry.family);
  const requested = new Map<LocalFontEntry, string[]>();
  for (const entry of entries) {
    if (!entry.path || (entry.sourceKind !== 'file-backed' && entry.sourceKind !== 'hft-derived')) continue;
    const exactDerived = isHftDerivedEntry(entry);
    if (!isAuthoringFontFamilyAllowed(entry.family, exactDerived)) continue;
    const names = entryNames(entry);
    const strictKeys = new Set(names.map(normalizeFontName));
    const aliasKeys = exactDerived ? new Set(names.map(fontFamilyAliasKey)) : null;
    const matches = requestedNames.filter((requestedName) => {
      if (strictKeys.has(normalizeFontName(requestedName))) return true;
      return aliasKeys?.has(fontFamilyAliasKey(requestedName)) ?? false;
    });
    if (matches.length > 0) requested.set(entry, matches);
  }
  return requested;
}

function groupEntriesByPath(entries: LocalFontEntry[]): Map<string, LocalFontEntry[]> {
  const grouped = new Map<string, LocalFontEntry[]>();
  for (const entry of entries) {
    if (!entry.path) continue;
    grouped.set(entry.path, [...(grouped.get(entry.path) ?? []), entry]);
  }
  return grouped;
}

function fontEntryKey(entry: LocalFontEntry): string {
  return [entry.family, entry.postScriptName, entry.style, entry.weight ?? '', entry.path ?? ''].join('\u0000');
}

function localFontFaceKey(record: Pick<LocalFontRecord, 'family' | 'fullName' | 'postscriptName'>): string {
  return normalizeFontName(record.postscriptName || record.fullName || record.family);
}

function uniqueAuthoringFamilies(families: Iterable<string>): string[] {
  return uniqueFamilies(filterAuthoringFontFamilies(families, hasExactDesktopDerivedFont));
}

function uniqueFamilies(families: Iterable<string>): string[] {
  return Array.from(new Set(families)).sort((left, right) => left.localeCompare(right, 'ko'));
}

function normalizeFontName(value: string): string {
  return value.normalize('NFC').replace(/\s+/g, ' ').trim().toLocaleLowerCase('en-US');
}

export function fontFamilyAliasKey(value: string): string {
  return value
    .replace(/\u0000/g, '')
    .normalize('NFC')
    .replace(/["']/g, '')
    .replace(/[\s_-]+/g, '')
    .trim()
    .toLocaleLowerCase('ko-KR');
}

function isHftDerivedEntry(entry: LocalFontEntry): boolean {
  return entry.sourceKind === 'hft-derived';
}

function entryNames(entry: LocalFontEntry): string[] {
  return [entry.family, entry.postScriptName, ...(entry.aliases ?? [])].filter(Boolean);
}

function preferredRegularEntry(entries: readonly LocalFontEntry[]): LocalFontEntry | undefined {
  return entries
    .filter((entry) => /^(normal|regular|roman|book)$/i.test(entry.style))
    .sort((left, right) => Math.abs((left.weight ?? 400) - 400) - Math.abs((right.weight ?? 400) - 400))[0];
}

function supportsBinaryFontLoading(): boolean {
  return typeof document !== 'undefined' && !!document.fonts && typeof FontFace === 'function';
}
