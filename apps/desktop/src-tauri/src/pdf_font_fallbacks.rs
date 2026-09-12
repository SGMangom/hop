use std::collections::BTreeMap;

const RESTRICTED_SERIF_FONTS: &[&str] = &["휴먼명조", "HY견명조", "HYMyeongJo-Extra", "HY신명조"];
const RESTRICTED_SANS_FONTS: &[&str] = &[
    "휴먼고딕",
    "휴먼옛체",
    "휴먼매직체",
    "휴먼편지체",
    "휴먼둥근헤드라인",
    "HCI Poppy",
    "HY헤드라인M",
    "HYHeadLine M",
    "HYHeadLine Medium",
    "HY견고딕",
    "HYGothic-Extra",
    "HY그래픽",
    "HY그래픽M",
    "HYGraphic-Medium",
    "HY중고딕",
];
const SERIF_FALLBACK: &str = "함초롬바탕, 바탕, AppleMyungjo, serif";
const SANS_FALLBACK: &str = "함초롬돋움, 맑은 고딕, Apple SD Gothic Neo, sans-serif";

pub(crate) fn add_font_fallbacks(svg: &str, exact_hft_fonts: &BTreeMap<String, String>) -> String {
    rewrite_font_family_attrs(svg, exact_hft_fonts)
}

fn rewrite_font_family_attrs(svg: &str, exact_hft_fonts: &BTreeMap<String, String>) -> String {
    const MARKER: &str = "font-family=\"";

    let mut output = String::with_capacity(svg.len());
    let mut rest = svg;

    while let Some(marker_start) = rest.find(MARKER) {
        let value_start = marker_start + MARKER.len();
        output.push_str(&rest[..value_start]);

        let value_and_tail = &rest[value_start..];
        let Some(value_end) = value_and_tail.find('"') else {
            output.push_str(value_and_tail);
            return output;
        };

        output.push_str(&sanitize_svg_font_family(
            &value_and_tail[..value_end],
            exact_hft_fonts,
        ));
        rest = &value_and_tail[value_end..];
    }

    output.push_str(rest);
    output
}

fn sanitize_svg_font_family(value: &str, exact_hft_fonts: &BTreeMap<String, String>) -> String {
    let original_families = split_svg_font_family(value);
    let families = original_families
        .into_iter()
        .map(|family| canonical_exact_hft_family(&family, exact_hft_fonts).unwrap_or(family))
        .collect::<Vec<_>>();
    let canonicalized_exact_family = families != split_svg_font_family(value);
    let Some(first) = families.first() else {
        return value.to_string();
    };

    if is_restricted_serif_font(first) && !has_exact_hft_font(first, exact_hft_fonts) {
        return SERIF_FALLBACK.to_string();
    }
    if is_restricted_sans_font(first) && !has_exact_hft_font(first, exact_hft_fonts) {
        return SANS_FALLBACK.to_string();
    }

    let safe_families = families
        .iter()
        .filter(|family| !is_restricted_font(family) || has_exact_hft_font(family, exact_hft_fonts))
        .cloned()
        .collect::<Vec<_>>();
    let removed_restricted_families = safe_families.len() != families.len();
    let Some(first_safe_family) = safe_families.first() else {
        return SANS_FALLBACK.to_string();
    };

    if font_family_eq(first_safe_family, "바탕체") && safe_families.len() > 1 {
        return prepend_font_fallbacks(&safe_families, &["바탕", "AppleMyungjo"]);
    }
    if font_family_eq(first_safe_family, "굴림체") && safe_families.len() > 1 {
        return prepend_font_fallbacks(
            &safe_families,
            &["굴림", "맑은 고딕", "Apple SD Gothic Neo"],
        );
    }
    if removed_restricted_families {
        return safe_families.join(", ");
    }
    if canonicalized_exact_family {
        return safe_families.join(", ");
    }

    value.to_string()
}

fn canonical_exact_hft_family(
    family: &str,
    exact_hft_fonts: &BTreeMap<String, String>,
) -> Option<String> {
    let normalized = normalize_svg_font_family(family);
    exact_hft_fonts
        .get(&crate::font_catalog::font_family_alias_key(&normalized))
        .cloned()
}

fn has_exact_hft_font(family: &str, exact_hft_fonts: &BTreeMap<String, String>) -> bool {
    canonical_exact_hft_family(family, exact_hft_fonts).is_some()
}

fn is_restricted_font(family: &str) -> bool {
    is_restricted_serif_font(family) || is_restricted_sans_font(family)
}

fn is_restricted_serif_font(family: &str) -> bool {
    RESTRICTED_SERIF_FONTS
        .iter()
        .any(|font| font_family_eq(family, font))
}

fn is_restricted_sans_font(family: &str) -> bool {
    RESTRICTED_SANS_FONTS
        .iter()
        .any(|font| font_family_eq(family, font))
}

fn split_svg_font_family(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|family| family.trim().to_string())
        .filter(|family| !family.is_empty())
        .collect()
}

fn prepend_font_fallbacks(families: &[String], fallback_families: &[&str]) -> String {
    let mut result = Vec::with_capacity(families.len() + fallback_families.len());
    result.push(families[0].clone());
    for fallback in fallback_families {
        if !families
            .iter()
            .any(|family| font_family_eq(family, fallback))
        {
            result.push((*fallback).to_string());
        }
    }
    result.extend(families.iter().skip(1).cloned());
    result.join(", ")
}

fn font_family_eq(left: &str, right: &str) -> bool {
    let left = normalize_svg_font_family(left);
    let right = normalize_svg_font_family(right);
    left == right || left.eq_ignore_ascii_case(&right)
}

fn normalize_svg_font_family(family: &str) -> String {
    let mut normalized = family.trim();
    loop {
        let unwrapped = if normalized.len() >= 12
            && normalized.starts_with("&apos;")
            && normalized.ends_with("&apos;")
        {
            Some(&normalized[6..normalized.len() - 6])
        } else if normalized.len() >= 12
            && normalized.starts_with("&quot;")
            && normalized.ends_with("&quot;")
        {
            Some(&normalized[6..normalized.len() - 6])
        } else {
            None
        };
        let Some(unwrapped) = unwrapped else {
            break;
        };
        normalized = unwrapped.trim();
    }
    normalized
        .trim_matches(|ch| ch == '\'' || ch == '"')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_font_fallbacks_replaces_restricted_font_families() {
        let svg = r#"<text font-family="휴먼명조">A</text><text font-family="HY헤드라인M, sans-serif">B</text>"#;

        let result = add_font_fallbacks(svg, &BTreeMap::new());

        assert!(result.contains(r#"font-family="함초롬바탕, 바탕, AppleMyungjo, serif""#));
        assert!(result
            .contains(r#"font-family="함초롬돋움, 맑은 고딕, Apple SD Gothic Neo, sans-serif""#));
        assert!(!result.contains(r#"font-family="휴먼명조""#));
        assert!(!result.contains(r#"font-family="HY헤드라인M"#));
    }

    #[test]
    fn add_font_fallbacks_does_not_duplicate_existing_safe_fallbacks() {
        let svg = r#"<text font-family="바탕체, 바탕, HY신명조, serif">A</text><text font-family="'HCI Poppy', sans-serif">B</text>"#;

        let result = add_font_fallbacks(svg, &BTreeMap::new());

        assert!(result.contains(r#"font-family="바탕체, AppleMyungjo, 바탕, serif""#));
        assert!(result
            .contains(r#"font-family="함초롬돋움, 맑은 고딕, Apple SD Gothic Neo, sans-serif""#));
        assert!(!result.contains("HY신명조"));
        assert!(!result.contains("HCI Poppy"));
    }

    #[test]
    fn add_font_fallbacks_handles_searchable_svg_xml_escaped_family_names() {
        let svg = r#"<text font-family="&apos;HY신명조&apos;,&apos;Batang&apos;,serif">A</text><text font-family="&quot;HY헤드라인M&quot;,sans-serif">B</text>"#;

        let result = add_font_fallbacks(svg, &BTreeMap::new());

        assert!(result.contains(r#"font-family="함초롬바탕, 바탕, AppleMyungjo, serif""#));
        assert!(result
            .contains(r#"font-family="함초롬돋움, 맑은 고딕, Apple SD Gothic Neo, sans-serif""#));
        assert!(!result.contains("HY신명조"));
        assert!(!result.contains("HY헤드라인M"));
    }

    #[test]
    fn add_font_fallbacks_canonicalizes_xml_escaped_exact_hft_family() {
        let svg = r#"<text font-family="&apos;HY신명조&apos;,&apos;Batang&apos;,serif">A</text>"#;
        let exact = [(
            crate::font_catalog::font_family_alias_key("HY신명조"),
            "한양신명조".to_string(),
        )]
        .into_iter()
        .collect();

        let result = add_font_fallbacks(svg, &exact);

        assert!(result.contains(r#"font-family="한양신명조, &apos;Batang&apos;, serif""#));
        assert!(!result.contains("함초롬바탕"));
    }

    #[test]
    fn add_font_fallbacks_preserves_restricted_family_when_exact_derived_face_exists() {
        let svg = r#"<text font-family="HCI Poppy, sans-serif">A</text><text font-family="HY신명조">B</text>"#;
        let exact = [
            (
                crate::font_catalog::font_family_alias_key("HCIPoppy"),
                "HCIPoppy".to_string(),
            ),
            (
                crate::font_catalog::font_family_alias_key("HY신명조"),
                "한양신명조".to_string(),
            ),
        ]
        .into_iter()
        .collect();

        let result = add_font_fallbacks(svg, &exact);

        assert!(result.contains(r#"font-family="HCIPoppy, sans-serif""#));
        assert!(result.contains(r#"font-family="한양신명조""#));
        assert!(!result.contains("함초롬돋움"));
        assert!(!result.contains("함초롬바탕"));
    }
}
