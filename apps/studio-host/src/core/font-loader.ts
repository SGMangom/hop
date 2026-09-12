import {
  detectLocalFontEntries,
  ensureLocalFontsAvailable,
  hasExactLocalDerivedFont,
  resolveLocalFont,
} from './local-fonts';
import { isAuthoringBlockedFontFamily } from './font-authoring-policy';
import { FONT_LIST, REGISTERED_FONTS } from './font-catalog';
import type { FontEntry } from './font-catalog';
import type { CanvasKitBundledFontSource } from '@/upstream/core';

export { REGISTERED_FONTS } from './font-catalog';
export type { CanvasKitBundledFontSource };

export interface CanvasKitFontPlan {
  sources: CanvasKitBundledFontSource[];
  unavailableFonts: string[];
}

const CRITICAL_FONTS = new Set(['함초롬바탕', '함초롬돋움', 'Computer Modern']);
const OS_FONT_CANDIDATES = [
  '맑은 고딕', 'Malgun Gothic', '바탕', 'Batang', '돋움', 'Dotum',
  '굴림', 'Gulim', '굴림체', 'GulimChe', '바탕체', 'BatangChe', '궁서', 'Gungsuh',
  'Apple SD Gothic Neo', 'AppleMyungjo', 'AppleGothic',
  'Noto Sans KR', 'Noto Serif KR',
];

let fontFaceRegistered = false;
const loadedFiles = new Set<string>();
const detectedOSFonts = new Set<string>();
let substituteFontStyle: HTMLStyleElement | null = null;

const CANVASKIT_SUBSTITUTES = new Map([
  [normalizeCanvasKitFontFamily('휴먼명조'), normalizeCanvasKitFontFamily('HY신명조')],
  [normalizeCanvasKitFontFamily('한양중고딕'), normalizeCanvasKitFontFamily('HY중고딕')],
  [normalizeCanvasKitFontFamily('한컴 윤고딕 230'), normalizeCanvasKitFontFamily('Noto Sans KR')],
]);

export function getDetectedOSFonts(): ReadonlySet<string> {
  return detectedOSFonts;
}

/**
 * CanvasKit은 CSS FontFace fallback을 볼 수 없으므로 첫 replay 전에 실제 byte source를
 * 준비해야 한다. HOP의 private HFT-derived face가 있으면 그 face를 local-font 경로로
 * 준비하고, 동시에 배포 가능한 bundled face가 있으면 안전한 fallback source로 유지한다.
 */
export function resolveCanvasKitFontPlan(requiredFontFamilies: readonly string[]): CanvasKitFontPlan {
  const entriesByFamily = new Map<string, FontEntry>();
  for (const entry of FONT_LIST) {
    const key = normalizeCanvasKitFontFamily(entry.name);
    // 같은 family의 italic/bold face보다 카탈로그에 먼저 정의된 regular face를 기본으로 쓴다.
    if (key && !entriesByFamily.has(key)) entriesByFamily.set(key, entry);
  }

  const sourcesByFile = new Map<string, Set<string>>();
  const unavailable = new Map<string, string>();
  for (const rawRequested of requiredFontFamilies) {
    const requested = rawRequested.trim();
    const key = normalizeCanvasKitFontFamily(requested);
    if (!key) continue;

    const local = resolveLocalFont(requested);
    const entry = entriesByFamily.get(key)
      ?? entriesByFamily.get(CANVASKIT_SUBSTITUTES.get(key) ?? '');
    if (!local && !entry) {
      unavailable.set(key, requested);
      continue;
    }
    if (!entry) continue;

    const aliases = sourcesByFile.get(entry.file) ?? new Set<string>();
    aliases.add(requested);
    for (const candidate of FONT_LIST) {
      if (candidate.file === entry.file) aliases.add(candidate.name);
    }
    sourcesByFile.set(entry.file, aliases);
  }

  return {
    sources: [...sourcesByFile.entries()].map(([url, aliases]) => ({
      url,
      aliases: [...aliases].sort((left, right) => left.localeCompare(right, 'ko')),
    })),
    unavailableFonts: [...unavailable.values()].sort((left, right) => left.localeCompare(right, 'ko')),
  };
}

export async function loadWebFonts(
  docFonts?: string[],
  onProgress?: (loaded: number, total: number) => void,
): Promise<void> {
  const targetSet = new Set([...(docFonts ?? []), ...CRITICAL_FONTS]);
  await hydrateDetectedFonts(targetSet);

  if (!fontFaceRegistered) {
    registerFontFaces();
    fontFaceRegistered = true;
  } else {
    syncRegisteredFontFaces();
  }

  const targetFonts = FONT_LIST.filter((font) => {
    if (!targetSet.has(font.name)) return false;
    return !detectedOSFonts.has(font.name);
  });
  const toLoad = uniqueFonts(targetFonts);

  if (toLoad.length === 0) return;

  const fileToNames = mapFontAliases(toLoad);
  let completed = 0;
  const total = toLoad.length;
  for (const font of toLoad) {
    try {
      for (const alias of fileToNames.get(font.file) ?? [font]) {
        const face = new FontFace(
          alias.name,
          `url("${font.file}") format("${font.format ?? 'woff2'}")`,
          { style: alias.style ?? 'normal', weight: alias.weight ?? '400', ...(font.unicodeRange ? { unicodeRange: font.unicodeRange } : {}) },
        );
        document.fonts.add(await face.load());
      }
      loadedFiles.add(font.file);
    } catch {
      // Missing fonts degrade to CSS fallback families; document loading should continue.
    } finally {
      completed += 1;
      onProgress?.(completed, total);
    }
  }
}

async function hydrateDetectedFonts(targetFonts: Set<string>): Promise<void> {
  const localFontEntries = await detectLocalFontEntries().catch(() => []);
  for (const entry of localFontEntries) {
    if (entry.sourceKind !== 'system-installed') continue;
    if (isAuthoringBlockedFontFamily(entry.family)) continue;
    detectedOSFonts.add(entry.family);
  }

  // Desktop exact HFT-derived faces are deliberately allowed through the
  // binary local-font path.  Restricted system/file-backed faces still remain
  // excluded by ensureLocalFontsAvailable(), so an old substitute is removed
  // only when the exact private derived face was actually loaded.
  const availableFonts = await ensureLocalFontsAvailable(Array.from(targetFonts))
    .catch(() => new Set<string>());
  for (const family of availableFonts) {
    if (isAuthoringBlockedFontFamily(family) && !hasExactLocalDerivedFont(family)) continue;
    detectedOSFonts.add(family);
  }

  if (detectedOSFonts.size === 0) {
    detectFallbackBrowserFonts();
  }
}

function mapFontAliases(fontsToLoad: FontEntry[]): Map<string, FontEntry[]> {
  const aliases = new Map<string, FontEntry[]>();
  const filesToLoad = new Set(fontsToLoad.map((font) => font.file));
  for (const font of FONT_LIST) {
    if (!filesToLoad.has(font.file) || detectedOSFonts.has(font.name)) continue;
    const names = aliases.get(font.file) ?? [];
    names.push(font);
    aliases.set(font.file, names);
  }
  return aliases;
}

function detectFallbackBrowserFonts(): void {
  for (const name of OS_FONT_CANDIDATES) {
    try {
      if (document.fonts.check(`16px "${name}"`)) {
        detectedOSFonts.add(name);
      }
    } catch {
      // Font detection is best-effort only.
    }
  }
}

function registerFontFaces(): void {
  substituteFontStyle = document.createElement('style');
  document.head.appendChild(substituteFontStyle);
  syncRegisteredFontFaces();
}

function syncRegisteredFontFaces(): void {
  if (!substituteFontStyle) return;

  substituteFontStyle.textContent = FONT_LIST
    .filter((font) => !detectedOSFonts.has(font.name))
    .map((font) => {
      const unicodeRange = font.unicodeRange ? ` unicode-range: ${font.unicodeRange};` : '';
      return `@font-face { font-family: "${font.name}"; src: url("${font.file}") format("${font.format ?? 'woff2'}"); font-style: ${font.style ?? 'normal'}; font-weight: ${font.weight ?? '400'}; font-display: swap;${unicodeRange} }`;
    })
    .join('\n');
}

function uniqueFonts(fonts: FontEntry[]): FontEntry[] {
  const seenFiles = new Set<string>();
  const result: FontEntry[] = [];
  for (const font of fonts) {
    if (loadedFiles.has(font.file) || seenFiles.has(font.file)) continue;
    seenFiles.add(font.file);
    result.push(font);
  }
  return result;
}

function normalizeCanvasKitFontFamily(value: string): string {
  return value
    .replace(/\u0000/g, '')
    .normalize('NFC')
    .replace(/\s+/g, ' ')
    .trim()
    .toLocaleLowerCase('en-US');
}
