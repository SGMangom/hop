import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { cpSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';
import { run, repoRoot, upstreamDir } from './lib/rhwp-upstream.mjs';

export const engineDir = join(repoRoot, 'target-local/rhwp-engine');
export const wasmDir = join(repoRoot, 'target-local/rhwp-wasm');
const patchPaths = [
  join(repoRoot, 'patches/rhwp-computer-modern.patch'),
  join(repoRoot, 'patches/rhwp-hop-compat.patch'),
  join(repoRoot, 'patches/rhwp-roundtrip-gates.patch'),
  join(repoRoot, 'patches/rhwp-pdf-search-text.patch'),
  join(repoRoot, 'patches/rhwp-hyperlink-insert.patch'),
  join(repoRoot, 'patches/rhwp-document-statistics.patch'),
  join(repoRoot, 'patches/rhwp-new-document-hwpx-refs.patch'),
  join(repoRoot, 'patches/rhwp-column-width-presets.patch'),
  join(repoRoot, 'patches/rhwp-hidden-comment-roundtrip.patch'),
  join(repoRoot, 'patches/rhwp-legacy-hyperlink-canonical.patch'),
  join(repoRoot, 'patches/rhwp-hwp5-memo-roundtrip.patch'),
  join(repoRoot, 'patches/rhwp-header-footer-editing.patch'),
];
const fontsDir = join(repoRoot, 'assets/fonts/computer-modern');
const fontFiles = ['ComputerModern-Regular.ttf', 'ComputerModern-Italic.ttf', 'ComputerModern-Bold.ttf'];
const sourceSupportFiles = [
  'llms.txt',
  'mydocs/manual/agent_knowledge_map.md',
  'mydocs/manual/agent_troubleshooting_guide.md',
  'mydocs/manual/recipes/01_fill_form_and_submit.md',
  'mydocs/manual/recipes/02_table_csv_roundtrip.md',
  'mydocs/manual/recipes/03_redact_before_sharing.md',
  'mydocs/manual/recipes/04_safety_check_untrusted_doc.md',
  'mydocs/manual/recipes/05_mail_merge_batch_fill.md',
  'mydocs/manual/recipes/06_visual_regression_before_after.md',
];
const hash = (bytes) => createHash('sha256').update(bytes).digest('hex');

function sourceDigest(directory) {
  const digest = createHash('sha256');
  const visit = (path) => {
    for (const entry of readdirSync(path, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
      const file = join(path, entry.name);
      if (entry.isDirectory()) visit(file);
      else {
        digest.update(relative(directory, file));
        digest.update(readFileSync(file));
      }
    }
  };
  visit(join(directory, 'src'));
  digest.update(readFileSync(join(directory, 'Cargo.toml')));
  for (const name of fontFiles) digest.update(readFileSync(join(directory, 'assets/fonts/computer-modern', name)));
  return digest.digest('hex');
}

function readJsonIfPresent(path) {
  return existsSync(path) ? JSON.parse(readFileSync(path, 'utf8')) : null;
}

export function prepareEquationEngine() {
  const upstream = JSON.parse(readFileSync(join(repoRoot, 'config/rhwp-upstream.json'), 'utf8'));
  if (run('git', ['rev-parse', 'HEAD'], { cwd: upstreamDir }) !== upstream.commit) {
    throw new Error('Initialize the pinned rhwp submodule before building equations.');
  }
  if (run('git', ['diff', 'HEAD', '--name-only'], { cwd: upstreamDir })) {
    throw new Error('The rhwp submodule must remain read-only; put engine changes in the HOP patch.');
  }
  const inputs = {
    upstreamCommit: upstream.commit,
    patches: Object.fromEntries(patchPaths.map((path) => [relative(repoRoot, path), hash(readFileSync(path))])),
    fonts: Object.fromEntries(fontFiles.map((name) => [name, hash(readFileSync(join(fontsDir, name)))])),
  };
  const inputHash = hash(JSON.stringify(inputs));
  const stamp = join(engineDir, 'HOP-SOURCE.json');
  const previous = readJsonIfPresent(stamp);
  if (
    previous?.inputHash === inputHash
    && previous.sourceHash === sourceDigest(engineDir)
    && sourceSupportFiles.every((name) => existsSync(join(engineDir, name)))
  ) return previous;

  mkdirSync(engineDir, { recursive: true });
  // Refresh source only: keep Cargo's build cache in the generated directory.
  for (const name of ['src', 'saved']) {
    rmSync(join(engineDir, name), { recursive: true, force: true });
    cpSync(join(upstreamDir, name), join(engineDir, name), { recursive: true });
  }
  for (const name of ['Cargo.toml', 'Cargo.lock', 'LICENSE']) {
    cpSync(join(upstreamDir, name), join(engineDir, name));
  }
  const manifestPath = join(engineDir, 'Cargo.toml');
  const manifest = readFileSync(manifestPath, 'utf8');
  const standalone = manifest.replace(/^members = \[.*\]$/m, 'members = ["."]');
  if (standalone === manifest) throw new Error('Unexpected upstream workspace manifest; review the equation overlay.');
  writeFileSync(manifestPath, standalone);
  const generatedFonts = join(engineDir, 'assets/fonts/computer-modern');
  mkdirSync(generatedFonts, { recursive: true });
  for (const name of fontFiles) cpSync(join(fontsDir, name), join(generatedFonts, name));
  // Only the fixtures referenced at compile time; large sample collections are unnecessary.
  for (const path of [
    ...sourceSupportFiles,
    'examples/pr599_png_gateway.rs',
    'tests/fixtures/fonts/RHWPBitmapSvgGlyphSmoke.ttf',
    'tests/fixtures/fonts/RHWPExactFaceSmoke.ttc',
    'assets/logo/logo-32.png',
    'samples/hml/formatting_table.hml',
    'samples/hwpx/aift.hwpx',
    'samples/hwpx/ref/ref_empty.hwpx',
    'samples/render-p35-font-native-bitmap.hwpx',
  ]) {
    const target = join(engineDir, path);
    mkdirSync(dirname(target), { recursive: true });
    writeFileSync(target, execFileSync('git', ['show', `${upstream.commit}:${path}`], { cwd: upstreamDir, maxBuffer: 64 * 1024 * 1024 }));
  }
  for (const patchPath of patchPaths) {
    run('git', ['apply', `--directory=${relative(repoRoot, engineDir)}`, patchPath]);
  }
  const provenance = { ...inputs, inputHash, sourceHash: sourceDigest(engineDir) };
  writeFileSync(stamp, `${JSON.stringify(provenance, null, 2)}\n`);
  return provenance;
}

export function buildEquationWasm() {
  const source = prepareEquationEngine();
  const provenancePath = join(wasmDir, 'HOP-PROVENANCE.json');
  const previous = readJsonIfPresent(provenancePath);
  const outputs = ['rhwp.js', 'rhwp_bg.wasm', 'rhwp.d.ts', 'rhwp_bg.wasm.d.ts'];
  if (previous?.sourceHash === source.sourceHash && outputs.every((name) =>
    existsSync(join(wasmDir, name)) && hash(readFileSync(join(wasmDir, name))) === previous.artifacts?.[name])) return;

  run('wasm-pack', ['build', engineDir, '--target', 'web', '--out-dir', wasmDir, '--release', '--no-opt'], { stdio: 'inherit' });
  const provenance = {
    ...source,
    rustc: run('rustc', ['--version']),
    wasmPack: run('wasm-pack', ['--version']),
    artifacts: Object.fromEntries(outputs.map((name) => [name, hash(readFileSync(join(wasmDir, name)))])),
  };
  writeFileSync(provenancePath, `${JSON.stringify(provenance, null, 2)}\n`);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  if (process.argv.includes('--wasm')) buildEquationWasm();
  else prepareEquationEngine();
}
