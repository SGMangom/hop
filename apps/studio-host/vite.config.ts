import { defineConfig, normalizePath } from 'vite';
import { basename, dirname, relative, resolve } from 'node:path';
import { copyFileSync, createReadStream, existsSync, mkdirSync, readdirSync, readFileSync, statSync } from 'node:fs';
import type { Plugin } from 'vite';
import { createHopOverrides } from './hop-overrides.ts';

const desktopConfig = JSON.parse(
  readFileSync(resolve(import.meta.dirname, '../desktop/src-tauri/tauri.conf.json'), 'utf-8'),
);
const upstreamStudioDir = resolve(import.meta.dirname, '../../third_party/rhwp/rhwp-studio');
const upstreamSrc = resolve(import.meta.dirname, '../../third_party/rhwp/rhwp-studio/src');
const hopSrc = resolve(import.meta.dirname, 'src');
const rhwpWasmModule = normalizePath(resolve(import.meta.dirname, '../../target-local/rhwp-wasm/rhwp.js'));
const rhwpWasmDir = dirname(rhwpWasmModule);
const rhwpWasmPackage = JSON.parse(readFileSync(resolve(import.meta.dirname, 'vendor/rhwp-core/package.json'), 'utf-8'));
const fontAssetsDir = resolve(import.meta.dirname, '../../assets/fonts');
const computerModernDir = resolve(fontAssetsDir, 'computer-modern');
const canvasKitPackageDir = resolve(import.meta.dirname, 'node_modules/canvaskit-wasm');

function hopFontAssets(): Plugin {
  return {
    name: 'hop-font-assets',
    configureServer(server) {
      server.middlewares.use('/fonts', (req, res, next) => {
        const fontName = basename(decodePath(req.url?.split('?')[0] ?? ''));
        if (!fontName.endsWith('.woff2') && !/^ComputerModern-(Regular|Italic|Bold)\.ttf$/.test(fontName)) {
          next();
          return;
        }

        const fontPath = resolve(fontName.endsWith('.ttf') ? computerModernDir : fontAssetsDir, fontName);
        const relativeFontPath = relative(fontAssetsDir, fontPath);
        if (relativeFontPath.startsWith('..') || relativeFontPath === '' || !existsSync(fontPath)) {
          next();
          return;
        }

        res.setHeader('Content-Type', fontName.endsWith('.ttf') ? 'font/ttf' : 'font/woff2');
        res.setHeader('Cache-Control', 'public, max-age=31536000, immutable');
        createReadStream(fontPath).pipe(res);
      });
    },
    closeBundle() {
      const outDir = resolve(import.meta.dirname, 'dist/fonts');
      mkdirSync(outDir, { recursive: true });
      for (const fileName of readdirSync(fontAssetsDir)) {
        const source = resolve(fontAssetsDir, fileName);
        if (!fileName.endsWith('.woff2') || !statSync(source).isFile()) continue;
        copyFileSync(source, resolve(outDir, fileName));
      }
      for (const style of ['Regular', 'Italic', 'Bold']) {
        const fileName = `ComputerModern-${style}.ttf`;
        copyFileSync(resolve(computerModernDir, fileName), resolve(outDir, fileName));
      }
    },
  };
}

function decodePath(path: string): string {
  try {
    return decodeURIComponent(path);
  } catch {
    return '';
  }
}

export default defineConfig({
  base: './',
  plugins: [hopFontAssets()],
  define: {
    __APP_VERSION__: JSON.stringify(rhwpWasmPackage.version),
    __HOP_VERSION__: JSON.stringify(desktopConfig.version),
  },
  resolve: {
    alias: [
      ...createHopOverrides(hopSrc),
      { find: '@wasm/rhwp.js', replacement: rhwpWasmModule },
      // The imported renderer lives under third_party, outside this workspace package's
      // node_modules ancestry. Pin CanvasKit to studio-host's declared dependency so both
      // the JS API and canvaskit.wasm resolve in dev and production builds.
      { find: 'canvaskit-wasm', replacement: canvasKitPackageDir },
      { find: '@/upstream', replacement: resolve(hopSrc, 'upstream') },
      { find: '@upstream', replacement: upstreamSrc },
      { find: '@', replacement: upstreamSrc },
    ],
  },
  server: {
    host: '127.0.0.1',
    port: 7700,
    fs: {
      allow: [
        import.meta.dirname,
        rhwpWasmDir,
        fontAssetsDir,
        upstreamStudioDir,
      ],
    },
  },
});
