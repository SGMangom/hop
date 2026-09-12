use hop_rhwp_adapter::{searchable_pdf_from_svg_pages, DocumentCore};
use std::path::{Path, PathBuf};

use crate::commands::PageRange;
use crate::pdf_font_fallbacks::add_font_fallbacks;
use crate::state::atomic_write;

pub fn export_core_to_pdf(
    core: &DocumentCore,
    target_path: &Path,
    page_range: Option<PageRange>,
    mut font_dirs: Vec<PathBuf>,
    mut on_progress: impl FnMut(&str, u32, u32, String),
) -> Result<u32, String> {
    ensure_pdf_path(target_path)?;
    on_progress("start", 0, 1, "PDF 내보내기를 시작합니다".to_string());

    let page_count = core.page_count();
    let pages = resolve_page_range(page_range, page_count)?;
    let total = pages.len() as u32;
    let exact_hft_fonts = crate::font_catalog::hft_derived_font_family_map();

    let mut svg_pages = Vec::with_capacity(pages.len());
    for (idx, page) in pages.iter().enumerate() {
        let svg = core
            .render_page_svg_searchable_native(*page)
            .map_err(|e| format!("페이지 {} 렌더링 실패: {}", page + 1, e))?;
        svg_pages.push(add_font_fallbacks(&svg, &exact_hft_fonts));
        on_progress(
            "render",
            idx as u32 + 1,
            total,
            format!("{} / {} 페이지 렌더링", idx + 1, total),
        );
    }

    add_optional_hyhwpeq_font_dir(&mut font_dirs);
    let pdf_bytes = searchable_pdf_from_svg_pages(&svg_pages, font_dirs)?;
    atomic_write(target_path, &pdf_bytes)?;
    on_progress("write", total, total, "PDF 파일을 저장했습니다".to_string());

    Ok(total)
}

const HOP_HYHWPEQ_PATH_ENV: &str = "HOP_HYHWPEQ_PATH";

fn add_optional_hyhwpeq_font_dir(font_dirs: &mut Vec<PathBuf>) {
    let mut candidates = Vec::new();
    if let Some(configured) = std::env::var_os(HOP_HYHWPEQ_PATH_ENV) {
        candidates.push(PathBuf::from(configured));
    }
    if let Some(home) = std::env::var_os("HOME") {
        candidates.push(PathBuf::from(home).join("Downloads/HYHWPEQ"));
    }

    for candidate in candidates {
        let Some(dir) = hyhwpeq_font_dir_from_candidate(&candidate) else {
            continue;
        };
        if !font_dirs
            .iter()
            .any(|existing| same_existing_path(existing, &dir))
        {
            font_dirs.push(dir);
        }
        break;
    }
}

fn hyhwpeq_font_dir_from_candidate(candidate: &Path) -> Option<PathBuf> {
    let dir = if candidate.is_file() {
        let name = candidate.file_name()?.to_string_lossy();
        if !name.eq_ignore_ascii_case("HYHWPEQ.TTF") {
            return None;
        }
        candidate.parent()?.to_path_buf()
    } else if candidate.is_dir() {
        let contains_face = ["HYHWPEQ.TTF", "HyhwpEQ.ttf", "hyhwpeq.ttf"]
            .iter()
            .any(|name| candidate.join(name).is_file());
        if !contains_face {
            return None;
        }
        candidate.to_path_buf()
    } else {
        return None;
    };
    std::fs::canonicalize(&dir).ok().or(Some(dir))
}

fn same_existing_path(left: &Path, right: &Path) -> bool {
    match (std::fs::canonicalize(left), std::fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

pub(crate) fn ensure_pdf_path(path: &Path) -> Result<(), String> {
    if path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("pdf"))
        != Some(true)
    {
        return Err("PDF 파일 경로는 .pdf 확장자여야 합니다".to_string());
    }
    Ok(())
}

fn resolve_page_range(page_range: Option<PageRange>, page_count: u32) -> Result<Vec<u32>, String> {
    if page_count == 0 {
        return Err("내보낼 페이지가 없습니다".to_string());
    }
    let Some(range) = page_range else {
        return Ok((0..page_count).collect());
    };
    let start = range.start.unwrap_or(0);
    let end = range.end.unwrap_or(page_count - 1);
    if start > end || end >= page_count {
        return Err(format!(
            "페이지 범위가 올바르지 않습니다: {}..{} / 총 {}페이지",
            start + 1,
            end + 1,
            page_count
        ));
    }
    Ok((start..=end).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hyhwpeq_candidate_accepts_exact_face_file_or_parent_directory() {
        let temp = tempfile::tempdir().unwrap();
        let font = temp.path().join("HYHWPEQ.TTF");
        std::fs::write(&font, b"local-only-test-face").unwrap();
        let expected = std::fs::canonicalize(temp.path()).unwrap();

        assert_eq!(
            hyhwpeq_font_dir_from_candidate(&font),
            Some(expected.clone())
        );
        assert_eq!(hyhwpeq_font_dir_from_candidate(temp.path()), Some(expected));
        assert_eq!(
            hyhwpeq_font_dir_from_candidate(&temp.path().join("missing.ttf")),
            None
        );
    }

    #[test]
    fn ensure_pdf_path_accepts_pdf_case_insensitively() {
        assert!(ensure_pdf_path(Path::new("out.pdf")).is_ok());
        assert!(ensure_pdf_path(Path::new("out.PDF")).is_ok());
    }

    #[test]
    fn ensure_pdf_path_rejects_non_pdf_paths() {
        assert_eq!(
            ensure_pdf_path(Path::new("out.hwp")).unwrap_err(),
            "PDF 파일 경로는 .pdf 확장자여야 합니다"
        );
        assert!(ensure_pdf_path(Path::new("out")).is_err());
    }

    #[test]
    fn resolve_page_range_defaults_to_all_pages() {
        assert_eq!(resolve_page_range(None, 3).unwrap(), vec![0, 1, 2]);
    }

    #[test]
    fn resolve_page_range_supports_open_ended_ranges() {
        assert_eq!(
            resolve_page_range(
                Some(PageRange {
                    start: Some(1),
                    end: None,
                }),
                4,
            )
            .unwrap(),
            vec![1, 2, 3]
        );
        assert_eq!(
            resolve_page_range(
                Some(PageRange {
                    start: None,
                    end: Some(1),
                }),
                4,
            )
            .unwrap(),
            vec![0, 1]
        );
    }

    #[test]
    fn resolve_page_range_rejects_empty_and_invalid_ranges() {
        assert_eq!(
            resolve_page_range(None, 0).unwrap_err(),
            "내보낼 페이지가 없습니다"
        );
        assert!(resolve_page_range(
            Some(PageRange {
                start: Some(2),
                end: Some(1),
            }),
            4,
        )
        .unwrap_err()
        .contains("페이지 범위가 올바르지 않습니다"));
        assert!(resolve_page_range(
            Some(PageRange {
                start: Some(0),
                end: Some(4),
            }),
            4,
        )
        .unwrap_err()
        .contains("총 4페이지"));
    }

    #[test]
    fn equation_hwp_roundtrip_exports_exact_local_or_bundled_equation_font() {
        let script = r"i\hbar\frac{\partial\psi}{\partial t}=-\frac{\hbar^2}{2m}\nabla^2\psi+V\psi";
        let font_dir =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../assets/fonts/computer-modern");
        let temporary = tempfile::tempdir().unwrap();
        // Explicitly opt in to retaining the real document and export for visual inspection.
        let output = std::env::var_os("HOP_EQUATION_PROOF_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| temporary.path().to_path_buf());
        std::fs::create_dir_all(&output).unwrap();

        let mut core = DocumentCore::new_empty();
        core.create_blank_document_native().unwrap();
        let inserted: serde_json::Value = serde_json::from_str(
            &core
                .insert_equation_native(0, 0, 0, script, 1800, 0)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(inserted["ok"], true);
        let control = inserted["controlIdx"].as_u64().unwrap() as usize;
        let hwp_path = output.join("schrodinger.hwp");
        atomic_write(&hwp_path, &core.export_hwp_native().unwrap()).unwrap();
        let reopened = DocumentCore::from_bytes(&std::fs::read(&hwp_path).unwrap()).unwrap();
        let properties: serde_json::Value = serde_json::from_str(
            &reopened
                .get_equation_properties_native(0, 0, control, None, None)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(properties["script"], script);
        assert_eq!(properties["fontSize"], 1800);
        assert_eq!(reopened.page_count(), 1);

        let svg = reopened.render_page_svg_native(0).unwrap();
        assert!(svg.contains("Computer Modern"));
        for symbol in ["ℏ", "∂", "ψ", "∇"] {
            assert!(svg.contains(symbol), "missing {symbol}");
        }
        assert!(!svg.contains("Latin Modern"));
        atomic_write(&output.join("schrodinger.svg"), svg.as_bytes()).unwrap();
        let pdf_path = output.join("schrodinger.pdf");
        assert_eq!(
            export_core_to_pdf(&reopened, &pdf_path, None, vec![font_dir], |_, _, _, _| {})
                .unwrap(),
            1
        );
        let pdf = std::fs::read(&pdf_path).unwrap();
        assert!(pdf.starts_with(b"%PDF-"));
        let text = String::from_utf8_lossy(&pdf);
        assert!(
            text.contains("HyhwpEQ") || text.contains("ComputerModern"),
            "PDF must use local HyhwpEQ when available or bundled Computer Modern otherwise"
        );
        assert!(text.contains("/FontFile"), "PDF must embed font data");
        assert!(text.contains("/ToUnicode"));
        assert!(!text.contains("LatinModern"));
    }

    #[test]
    fn searchable_pdf_contains_a_unicode_text_map() {
        let font_dir =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../third_party/rhwp/ttfs/opensource");
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="320" height="120">
          <text x="16" y="64" font-family="Noto Sans KR" font-size="24">검색 가능한 PDF</text>
        </svg>"#;

        let pdf = searchable_pdf_from_svg_pages(&[svg.to_string()], vec![font_dir]).unwrap();

        assert!(pdf
            .windows(b"/ToUnicode".len())
            .any(|bytes| bytes == b"/ToUnicode"));
    }
}
