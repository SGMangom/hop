import { resolveCanvasKitFontPlan } from '@/core/font-loader';
import { loadStoredLocalFonts } from '@/core/local-fonts';
import { RendererSession } from '@/upstream/view';
import { withCanvasKitSurfaceBlockers } from '@/upstream/canvaskit-preflight';

/**
 * HOP desktop는 CanvasKit을 auto 정책으로 사용한다. 문서 preflight가 안전한 경우에만
 * CanvasKit을 선택하고, HFT-derived/local face 및 bundled fallback bytes를 첫 replay 전에
 * 준비한다. 지원하지 않는 문서는 upstream revision protocol에 따라 Canvas2D로 남는다.
 */
export function createRendererSession(): RendererSession {
  return new RendererSession(
    { backend: 'auto', source: 'default' },
    { mode: 'default', source: 'default' },
    { preference: 'auto', requested: 'auto' },
    'screen',
    async (mode, surface) => {
      const { CanvasKitLayerRenderer } = await import('@/upstream/canvaskit-renderer');
      return CanvasKitLayerRenderer.create(mode, surface, {
        requirePreparedFontFamilies: true,
      });
    },
    {
      transformCanvasKitPreflight(report) {
        const plan = resolveCanvasKitFontPlan(report.requiredFontFamilies);
        return withCanvasKitSurfaceBlockers(
          report,
          plan.unavailableFonts.map((font) => `fontUnavailable:${font}`),
        );
      },
      async prepareCanvasKitDocument(renderer, report) {
        const plan = resolveCanvasKitFontPlan(report.requiredFontFamilies);
        if (plan.unavailableFonts.length > 0) {
          throw new Error(`CanvasKit font family가 준비되지 않았습니다: ${plan.unavailableFonts.join(', ')}`);
        }

        // Desktop catalog는 loadWebFonts()에서 이미 hydrate되지만, browser/local-font 경로도
        // 저장된 권한을 재사용할 수 있게 upstream과 동일한 준비 순서를 유지한다.
        await loadStoredLocalFonts().catch(() => null);
        await renderer.prepareLocalFonts(report.requiredFontFamilies);
        await renderer.prepareBundledFonts(plan.sources);
      },
    },
  );
}
