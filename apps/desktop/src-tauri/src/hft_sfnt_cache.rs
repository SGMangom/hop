//! Clean-room HFT -> SFNT/TrueType materialization for the desktop runtime.
//!
//! Hancom HFT bytes are read only from the user's configured/local font directory.
//! This module never vendors source HFT data. Derived TTF files are written to an
//! app-private cache keyed by SHA-256 of the source bytes.

use crate::hft_catalog::{collect_desktop_hft_font_entries, HftFontEntry};
use crate::hft_outline_codec::{
    bitmap_payload_len, decode_bitmap_glyph_record, decode_hnc_obfuscation_in_place,
    decode_human_obfuscation_in_place, decode_mapsi_obfuscation_in_place, decode_outline_stream,
    locate_composite_components, locate_packed_special_index, validate_hft_header, BitmapMetrics,
    DecodedOutline, OutlineOp, Point,
};
use encoding_rs::EUC_KR;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use tauri::{AppHandle, Manager};

const CACHE_VERSION: &str = "hft-sfnt-v1";
const TARGET_UNITS_PER_EM: u16 = 1000;
/// Maximum geometric error, in target font units, when converting one cubic HFT
/// segment to one or more TrueType quadratic segments. The conversion is adaptive;
/// unlike the old fixed 12-line flattening, curves remain curves in `glyf`.
const CUBIC_QUADRATIC_MAX_ERROR: f64 = 0.125;
const CUBIC_QUADRATIC_MAX_DEPTH: u8 = 12;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DerivedHftFont {
    pub family: String,
    pub style: String,
    pub weight: u16,
    pub source_kind: String,
    /// First source for backwards-compatible diagnostics. `source_paths` is canonical.
    pub source_path: String,
    pub source_paths: Vec<String>,
    pub languages: Vec<String>,
    pub source_count: usize,
    pub derived_path: String,
    /// SHA-256 over converter version + all source file names/content hashes in this face.
    pub content_hash: String,
    pub glyph_count: usize,
}

#[derive(Debug, Clone)]
struct SfntGlyph {
    codepoint: u32,
    advance: u16,
    contours: GlyphContours,
}

#[derive(Debug, Clone)]
struct DecodedSourceFace {
    file_name: String,
    path: String,
    language: String,
    content_hash: String,
    family: String,
    style: String,
    weight: u16,
    glyphs: Vec<SfntGlyph>,
}

#[derive(Debug, Clone, Copy, Default)]
struct Bounds {
    x_min: i16,
    y_min: i16,
    x_max: i16,
    y_max: i16,
    has_points: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SfntPoint {
    x: i16,
    y: i16,
    on_curve: bool,
}

impl SfntPoint {
    fn on_curve(x: i16, y: i16) -> Self {
        Self {
            x,
            y,
            on_curve: true,
        }
    }

    fn off_curve(x: i16, y: i16) -> Self {
        Self {
            x,
            y,
            on_curve: false,
        }
    }

    fn same_position(self, other: Self) -> bool {
        self.x == other.x && self.y == other.y
    }
}

#[derive(Debug, Clone, Copy)]
struct FloatPoint {
    x: f64,
    y: f64,
}

#[derive(Debug, Clone, Copy)]
struct QuadraticSegment {
    control: FloatPoint,
    to: FloatPoint,
}

type Contour = Vec<SfntPoint>;
type GlyphContours = Vec<Contour>;
type PhysicalGlyphs = Vec<Option<GlyphContours>>;

#[derive(Debug, Clone, Copy)]
struct PhysicalGlyphDecodeSpec {
    range_pos: usize,
    range_end: usize,
    block_format: u16,
    block_em: u16,
    flags: u16,
    glyph_count: usize,
    data_base: usize,
}

#[derive(Debug, Clone)]
enum WidthKind {
    Fixed(u16),
    PerCode(Vec<u16>),
    Unknown,
}

#[derive(Debug, Clone)]
struct WidthRecord {
    start: u16,
    end: u16,
    kind: WidthKind,
}

/// Materialize every indexed desktop HFT into the app-private cache.
///
/// Hancom's legacy catalog often splits one logical family/style into separate
/// latin/hangul/hanja/japanese/symbol HFT files. All indexed files are decoded,
/// then compatible parts are merged into one SFNT face so fontdb/CoreText/CSS do
/// not nondeterministically select a language fragment with an incomplete cmap.
pub fn materialize_desktop_hft_sfnt_cache(app: &AppHandle) -> Result<Vec<DerivedHftFont>, String> {
    let cache_root = app
        .path()
        .app_cache_dir()
        .map_err(|error| format!("앱 캐시 디렉터리를 확인할 수 없습니다: {error}"))?
        .join(CACHE_VERSION);
    let entries = collect_desktop_hft_font_entries()?;
    materialize_hft_sfnt_cache(&entries, &cache_root)
}

/// Pure filesystem variant used by focused tests and runtime wiring.
pub fn materialize_hft_sfnt_cache(
    entries: &[HftFontEntry],
    cache_root: &Path,
) -> Result<Vec<DerivedHftFont>, String> {
    create_private_cache_dir(cache_root)?;
    let mut groups = BTreeMap::<(String, String, u16), Vec<DecodedSourceFace>>::new();

    for entry in entries {
        let source_path = Path::new(&entry.path);
        let bytes = fs::read(source_path).map_err(|error| {
            format!(
                "HFT 글꼴을 읽을 수 없습니다: {} ({error})",
                source_path.display()
            )
        })?;
        if bytes.len() as u64 != entry.byte_len {
            return Err(format!(
                "HFT 글꼴 크기가 인덱스 이후 변경되었습니다: {}",
                source_path.display()
            ));
        }
        if bytes.len() > 32 * 1024 * 1024 {
            return Err(format!(
                "HFT 글꼴이 32MiB 안전 제한을 초과합니다: {}",
                source_path.display()
            ));
        }

        let source_hash = sha256_hex(&bytes);
        let source = decode_source_face(entry, &bytes, source_hash)
            .map_err(|error| format!("{}: {error}", entry.file_name))?;
        groups
            .entry((source.family.clone(), source.style.clone(), source.weight))
            .or_default()
            .push(source);
    }

    let mut derived = Vec::with_capacity(groups.len());
    for ((family, style, weight), mut sources) in groups {
        sources.sort_by(|left, right| {
            language_priority(&left.language)
                .cmp(&language_priority(&right.language))
                .then(left.file_name.cmp(&right.file_name))
        });

        let content_hash = merged_face_hash(&family, &style, weight, &sources);
        let mut by_unicode = BTreeMap::<u32, SfntGlyph>::new();
        for source in &sources {
            for glyph in &source.glyphs {
                by_unicode
                    .entry(glyph.codepoint)
                    .or_insert_with(|| glyph.clone());
            }
        }
        if by_unicode.is_empty() {
            return Err(format!(
                "HFT family/style에 변환 가능한 glyph가 없습니다: {family} {style}"
            ));
        }
        let glyphs = by_unicode.into_values().collect::<Vec<_>>();
        let post_script = post_script_name(&family, &content_hash);
        let ttf = build_ttf(
            &family,
            &style,
            weight,
            &post_script,
            TARGET_UNITS_PER_EM,
            &glyphs,
        )?;

        let stem = cache_face_stem(&family, &style, weight, &sources[0].file_name);
        let target = cache_root.join(format!("{stem}-{}.ttf", &content_hash[..20]));
        if !target.is_file() || !cached_sfnt_matches(&target, &ttf)? {
            atomic_private_write(&target, &ttf)?;
        }

        let source_paths = sources
            .iter()
            .map(|source| source.path.clone())
            .collect::<Vec<_>>();
        let mut languages = sources
            .iter()
            .map(|source| source.language.clone())
            .collect::<Vec<_>>();
        languages.sort_by_key(|language| (language_priority(language), language.clone()));
        languages.dedup();
        derived.push(DerivedHftFont {
            family,
            style,
            weight,
            source_kind: "hancom-hft-derived-sfnt".to_string(),
            source_path: source_paths.first().cloned().unwrap_or_default(),
            source_count: source_paths.len(),
            source_paths,
            languages,
            derived_path: target.to_string_lossy().to_string(),
            content_hash,
            glyph_count: read_sfnt_num_glyphs(&ttf).unwrap_or(0) as usize,
        });
    }

    derived.sort_by(|left, right| {
        left.family
            .cmp(&right.family)
            .then(left.weight.cmp(&right.weight))
            .then(left.style.cmp(&right.style))
            .then(left.derived_path.cmp(&right.derived_path))
    });
    Ok(derived)
}

fn decode_source_face(
    entry: &HftFontEntry,
    bytes: &[u8],
    content_hash: String,
) -> Result<DecodedSourceFace, String> {
    let mut sfnt_glyphs = parse_hft_glyphs(bytes, &entry.language)?;
    if sfnt_glyphs.is_empty() {
        return Err(format!(
            "HFT 글꼴에 변환 가능한 glyph가 없습니다: {}",
            entry.family
        ));
    }
    let (style, weight) = hft_style_and_weight(bytes);
    sfnt_glyphs.sort_by_key(|glyph| glyph.codepoint);
    sfnt_glyphs.dedup_by_key(|glyph| glyph.codepoint);
    Ok(DecodedSourceFace {
        file_name: entry.file_name.clone(),
        path: entry.path.clone(),
        language: entry.language.clone(),
        content_hash,
        family: entry.family.clone(),
        style: style.to_string(),
        weight,
        glyphs: sfnt_glyphs,
    })
}

#[cfg(test)]
fn convert_hft_to_ttf(
    bytes: &[u8],
    family: &str,
    language: &str,
    content_hash: &str,
) -> Result<Vec<u8>, String> {
    let entry = HftFontEntry {
        family: family.to_string(),
        file_name: "SAMPLE.HFT".to_string(),
        language: language.to_string(),
        source_kind: "hancom-hft".to_string(),
        path: "SAMPLE.HFT".to_string(),
        byte_len: bytes.len() as u64,
    };
    let source = decode_source_face(&entry, bytes, content_hash.to_string())?;
    let post_script = post_script_name(family, content_hash);
    build_ttf(
        family,
        &source.style,
        source.weight,
        &post_script,
        TARGET_UNITS_PER_EM,
        &source.glyphs,
    )
}

fn language_priority(language: &str) -> u8 {
    match language {
        "latin" => 0,
        "hangul" => 1,
        "hanja" => 2,
        "japanese" => 3,
        "symbol" => 4,
        "other" => 5,
        "user" => 6,
        _ => 7,
    }
}

fn merged_face_hash(
    family: &str,
    style: &str,
    weight: u16,
    sources: &[DecodedSourceFace],
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(CACHE_VERSION.as_bytes());
    hasher.update([0]);
    hasher.update(family.as_bytes());
    hasher.update([0]);
    hasher.update(style.as_bytes());
    hasher.update([0]);
    hasher.update(weight.to_be_bytes());
    for source in sources {
        hasher.update([0xff]);
        hasher.update(source.file_name.as_bytes());
        hasher.update([0]);
        hasher.update(source.language.as_bytes());
        hasher.update([0]);
        hasher.update(source.content_hash.as_bytes());
    }
    let digest = hasher.finalize();
    hex_bytes(&digest)
}

fn cache_face_stem(family: &str, style: &str, weight: u16, first_file: &str) -> String {
    let mut stem = family
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(*ch, '-' | '_'))
        .take(36)
        .collect::<String>();
    if stem.is_empty() {
        stem = first_file
            .trim_end_matches(".HFT")
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric() || matches!(*ch, '-' | '_'))
            .take(36)
            .collect();
    }
    if stem.is_empty() {
        stem.push_str("HFT");
    }
    format!("{stem}-{style}-{weight}")
}

fn parse_hft_glyphs(bytes: &[u8], language: &str) -> Result<Vec<SfntGlyph>, String> {
    validate_hft_header(bytes)
        .map_err(|error| format!("올바른 Han Unified Font File 1.0 파일이 아닙니다: {error}"))?;

    let width_records = parse_width_records(bytes)?;
    let font_em = le16(bytes, 0x17a)?;
    if !(8..=16384).contains(&font_em) {
        return Err(format!(
            "HFT header units-per-em이 올바르지 않습니다: {font_em}"
        ));
    }
    let human_legacy = bytes[0x134..0x154].starts_with(b"Human Font for HWP 2.1");
    let mut block_pos = le32(bytes, 0x1ae)? as usize;
    let mut by_unicode = BTreeMap::<u32, SfntGlyph>::new();

    // HFT files terminate after the outline/bitmap block chain. Header block counters are
    // advisory in old vendor files, so follow the validated span chain like the low-level
    // codec sweep rather than trusting a single count field.
    while block_pos + 14 <= bytes.len() {
        let block_span = le32(bytes, block_pos)? as usize;
        if block_span < 14 || block_pos + block_span > bytes.len() {
            break;
        }
        let block_em = le16(bytes, block_pos + 4)?;
        let block_format = le16(bytes, block_pos + 6)?;
        let range_count = le16(bytes, block_pos + 8)? as usize;
        let range_rel = le32(bytes, block_pos + 10)? as usize;
        if !(8..=16384).contains(&block_em) || !matches!(block_format, 0 | 1) {
            return Err(format!(
                "HFT glyph block header가 올바르지 않습니다: 0x{block_pos:x}"
            ));
        }
        let block_end = block_pos + block_span;
        let mut range_pos = block_pos
            .checked_add(range_rel)
            .ok_or_else(|| "HFT range 오프셋 overflow".to_string())?;

        for range_index in 0..range_count {
            ensure_range(bytes, range_pos, 22, "HFT range")?;
            if range_pos + 22 > block_end {
                return Err("HFT range가 glyph block 범위를 벗어납니다".to_string());
            }
            let range_span = le32(bytes, range_pos)? as usize;
            if range_span < 22 {
                return Err(format!(
                    "HFT range span이 올바르지 않습니다: 0x{range_pos:x}"
                ));
            }
            let declared_range_end = range_pos
                .checked_add(range_span)
                .ok_or_else(|| "HFT range span overflow".to_string())?;
            // A few legacy HFTs carry a stale final-range span. HncBaseDraw follows the
            // containing block boundary; the low-level decoder's 387-file sweep confirms it.
            let range_end = if range_index + 1 == range_count {
                block_end
            } else {
                if declared_range_end > block_end {
                    return Err("HFT range가 glyph block 범위를 벗어납니다".to_string());
                }
                declared_range_end
            };

            let flags = le16(bytes, range_pos + 4)?;
            let subtype = (flags & 0x0f) as u8;
            let code_start = le16(bytes, range_pos + 6)?;
            let code_end = le16(bytes, range_pos + 8)?;
            let glyph_count = le16(bytes, range_pos + 10)? as usize;
            let payload = range_pos + 22;
            let cache_size = if matches!(subtype, 1 | 2 | 4) {
                ensure_range(bytes, payload, 4, "HFT locator cache")?;
                let size = le16(bytes, payload)? as usize;
                if size < 4 || payload + size > range_end {
                    return Err(format!("HFT subtype {subtype} cache가 잘렸습니다"));
                }
                size
            } else {
                0
            };
            let data_base = payload + cache_size;
            let physical = decode_physical_glyphs(
                bytes,
                PhysicalGlyphDecodeSpec {
                    range_pos,
                    range_end,
                    block_format,
                    block_em,
                    flags,
                    glyph_count,
                    data_base,
                },
            )?;

            match subtype {
                0 => {
                    for glyph_index in 0..glyph_count {
                        let source_code = match code_start.checked_add(glyph_index as u16) {
                            Some(code) if code <= code_end => code,
                            _ => break,
                        };
                        let Some(contours) = physical.get(glyph_index).and_then(|g| g.clone())
                        else {
                            continue;
                        };
                        insert_logical_glyph(
                            &mut by_unicode,
                            source_code_to_unicode(source_code, language),
                            source_code,
                            contours,
                            &width_records,
                            font_em,
                        );
                    }
                }
                1 => {
                    let mode = le16(bytes, payload + 2)?;
                    if cache_size == 4 && mode == 0xffff && glyph_count == 2350 {
                        // Wansung cache: 2,350 KS X 1001 Hangul glyphs are stored in
                        // EUC-KR B0A1..C8FE physical order, independent of HNC bit layout.
                        for glyph_index in 0..glyph_count {
                            let Some(codepoint) = wansung_index_to_unicode(glyph_index) else {
                                continue;
                            };
                            let Some(contours) = physical.get(glyph_index).and_then(|g| g.clone())
                            else {
                                continue;
                            };
                            let source_code =
                                unicode_hangul_to_kssm(codepoint).unwrap_or(code_start);
                            insert_logical_glyph(
                                &mut by_unicode,
                                codepoint,
                                source_code,
                                contours,
                                &width_records,
                                font_em,
                            );
                        }
                    } else if cache_size >= 4 + glyph_count.saturating_mul(2) {
                        for glyph_index in 0..glyph_count {
                            let source_code = le16(bytes, payload + 4 + glyph_index * 2)?;
                            let Some(contours) = physical.get(glyph_index).and_then(|g| g.clone())
                            else {
                                continue;
                            };
                            insert_logical_glyph(
                                &mut by_unicode,
                                source_code_to_unicode(source_code, language),
                                source_code,
                                contours,
                                &width_records,
                                font_em,
                            );
                        }
                    } else {
                        return Err(format!(
                            "지원되지 않는 subtype-1 locator cache: size={cache_size}, mode=0x{mode:04x}, count={glyph_count}"
                        ));
                    }
                }
                2 | 4 => {
                    let cache = &bytes[payload..payload + cache_size];
                    for logical in code_start as u32..=code_end as u32 {
                        let source_code = logical as u16;
                        let components = locate_composite_components(
                            cache,
                            subtype,
                            source_code,
                            glyph_count,
                            human_legacy,
                        )
                        .map_err(|error| format!("HFT subtype-{subtype} locator 실패: {error}"))?;
                        if components.is_empty() {
                            continue;
                        }
                        let mut contours = Vec::new();
                        for component in components {
                            let Some(part) = physical
                                .get(component.glyph_index)
                                .and_then(|glyph| glyph.as_ref())
                            else {
                                return Err(format!(
                                    "HFT composite가 없는 physical glyph {}를 참조합니다",
                                    component.glyph_index
                                ));
                            };
                            let dx =
                                scale_i32(component.x_offset as i32, block_em, TARGET_UNITS_PER_EM);
                            let dy =
                                scale_i32(component.y_offset as i32, block_em, TARGET_UNITS_PER_EM);
                            contours.extend(translate_contours(part, dx, dy)?);
                        }
                        insert_logical_glyph(
                            &mut by_unicode,
                            source_code_to_unicode(source_code, language),
                            source_code,
                            contours,
                            &width_records,
                            font_em,
                        );
                    }
                }
                3 => {
                    for logical in code_start as u32..=code_end as u32 {
                        let source_code = logical as u16;
                        let Some(glyph_index) = locate_packed_special_index(source_code) else {
                            continue;
                        };
                        if glyph_index >= glyph_count {
                            return Err(format!(
                                "HFT subtype-3 physical index가 범위를 벗어납니다: {glyph_index}/{glyph_count}"
                            ));
                        }
                        let Some(contours) = physical.get(glyph_index).and_then(|g| g.clone())
                        else {
                            continue;
                        };
                        insert_logical_glyph(
                            &mut by_unicode,
                            source_code_to_unicode(source_code, language),
                            source_code,
                            contours,
                            &width_records,
                            font_em,
                        );
                    }
                }
                other => {
                    return Err(format!("지원되지 않는 HFT range subtype: {other}"));
                }
            }

            range_pos = declared_range_end;
        }
        block_pos = block_end;
    }

    Ok(by_unicode.into_values().collect())
}

fn decode_physical_glyphs(
    bytes: &[u8],
    spec: PhysicalGlyphDecodeSpec,
) -> Result<PhysicalGlyphs, String> {
    let PhysicalGlyphDecodeSpec {
        range_pos,
        range_end,
        block_format,
        block_em,
        flags,
        glyph_count,
        data_base,
    } = spec;
    let mut result = vec![None; glyph_count];
    if glyph_count == 0 {
        return Ok(result);
    }

    match block_format {
        1 => {
            if data_base + glyph_count.saturating_mul(4) > range_end {
                return Err("HFT outline offset table이 잘렸습니다".to_string());
            }
            for (glyph_index, slot) in result.iter_mut().enumerate() {
                let rel = le32(bytes, data_base + glyph_index * 4)? as usize;
                if rel == 0 {
                    continue;
                }
                let glyph_pos = data_base
                    .checked_add(rel)
                    .ok_or_else(|| "HFT outline glyph offset overflow".to_string())?;
                let metric_prefix = if flags & 0x10 == 0 { 8 } else { 0 };
                let length_pos = glyph_pos
                    .checked_add(metric_prefix)
                    .ok_or_else(|| "HFT outline length offset overflow".to_string())?;
                if length_pos + 2 > range_end {
                    return Err("HFT outline glyph length가 잘렸습니다".to_string());
                }
                let byte_len = le16(bytes, length_pos)? as usize;
                let stream_start = length_pos + 2;
                if stream_start + byte_len > range_end {
                    return Err("HFT outline glyph bytecode가 잘렸습니다".to_string());
                }
                if byte_len == 0 {
                    *slot = Some(Vec::new());
                    continue;
                }
                let mut stream = bytes[stream_start..stream_start + byte_len].to_vec();
                decode_vendor_stream(bytes, &mut stream)?;
                let decoded = decode_outline_stream(&stream)
                    .map_err(|error| format!("HFT outline decode 실패: {error}"))?;
                if decoded.consumed != stream.len() {
                    return Err("HFT outline decoder가 trailing bytecode를 남겼습니다".to_string());
                }
                *slot = Some(outline_to_contours(
                    &decoded,
                    block_em,
                    TARGET_UNITS_PER_EM,
                )?);
            }
        }
        0 => {
            if flags & 0x10 != 0 {
                let metrics = BitmapMetrics {
                    x: le_i16(bytes, range_pos + 14)?,
                    y: le_i16(bytes, range_pos + 16)?,
                    width: le_i16(bytes, range_pos + 18)?,
                    height: le_i16(bytes, range_pos + 20)?,
                };
                let byte_len = bitmap_payload_len(metrics)
                    .map_err(|error| format!("HFT bitmap metrics 실패: {error}"))?;
                let all_len = byte_len
                    .checked_mul(glyph_count)
                    .ok_or_else(|| "HFT bitmap range length overflow".to_string())?;
                if data_base + all_len > range_end {
                    return Err("HFT fixed-metric bitmap data가 잘렸습니다".to_string());
                }
                for (glyph_index, slot) in result.iter_mut().enumerate() {
                    let glyph_pos = data_base + glyph_index * byte_len;
                    let decoded =
                        decode_bitmap_glyph_record(&bytes[glyph_pos..range_end], Some(metrics))
                            .map_err(|error| format!("HFT bitmap decode 실패: {error}"))?;
                    *slot = Some(bitmap_to_contours(&decoded, block_em, TARGET_UNITS_PER_EM)?);
                }
            } else {
                if data_base + glyph_count.saturating_mul(4) > range_end {
                    return Err("HFT bitmap offset table이 잘렸습니다".to_string());
                }
                for (glyph_index, slot) in result.iter_mut().enumerate() {
                    let rel = le32(bytes, data_base + glyph_index * 4)? as usize;
                    if rel == 0 {
                        continue;
                    }
                    let glyph_pos = data_base
                        .checked_add(rel)
                        .ok_or_else(|| "HFT bitmap glyph offset overflow".to_string())?;
                    if glyph_pos >= range_end {
                        return Err("HFT bitmap glyph가 range 밖을 가리킵니다".to_string());
                    }
                    let decoded = decode_bitmap_glyph_record(&bytes[glyph_pos..range_end], None)
                        .map_err(|error| format!("HFT bitmap decode 실패: {error}"))?;
                    if glyph_pos + decoded.consumed > range_end {
                        return Err("HFT bitmap glyph가 잘렸습니다".to_string());
                    }
                    *slot = Some(bitmap_to_contours(&decoded, block_em, TARGET_UNITS_PER_EM)?);
                }
            }
        }
        _ => return Err(format!("지원되지 않는 HFT block format: {block_format}")),
    }
    Ok(result)
}

fn bitmap_to_contours(
    bitmap: &crate::hft_outline_codec::DecodedBitmapGlyph<'_>,
    source_em: u16,
    target_em: u16,
) -> Result<GlyphContours, String> {
    let width = bitmap.metrics.width.max(0) as usize;
    let height = bitmap.metrics.height.max(0) as usize;
    let mut contours = Vec::new();
    for row in 0..height {
        let row_start = row * bitmap.row_bytes;
        let mut column = 0usize;
        while column < width {
            let set = bitmap_pixel(bitmap.pixels, row_start, column);
            if !set {
                column += 1;
                continue;
            }
            let run_start = column;
            column += 1;
            while column < width && bitmap_pixel(bitmap.pixels, row_start, column) {
                column += 1;
            }
            let run_end = column;
            let raw_left = bitmap.metrics.x as i32 + run_start as i32;
            let raw_right = bitmap.metrics.x as i32 + run_end as i32;
            let raw_top = bitmap.metrics.y as i32 - row as i32;
            let raw_bottom = raw_top - 1;
            let left = checked_i16(scale_i32(raw_left, source_em, target_em), "bitmap x")?;
            let right = checked_i16(scale_i32(raw_right, source_em, target_em), "bitmap x")?;
            let top = checked_i16(scale_i32(raw_top, source_em, target_em), "bitmap y")?;
            let bottom = checked_i16(scale_i32(raw_bottom, source_em, target_em), "bitmap y")?;
            if left != right && top != bottom {
                contours.push(vec![
                    SfntPoint::on_curve(left, bottom),
                    SfntPoint::on_curve(left, top),
                    SfntPoint::on_curve(right, top),
                    SfntPoint::on_curve(right, bottom),
                ]);
            }
        }
    }
    Ok(contours)
}

fn bitmap_pixel(pixels: &[u8], row_start: usize, column: usize) -> bool {
    let byte = pixels.get(row_start + (column >> 3)).copied().unwrap_or(0);
    byte & (0x80 >> (column & 7)) != 0
}

fn translate_contours(
    contours: &[Contour],
    dx: i32,
    dy: i32,
) -> Result<GlyphContours, String> {
    let mut translated = Vec::with_capacity(contours.len());
    for contour in contours {
        let mut target = Vec::with_capacity(contour.len());
        for &point in contour {
            target.push(SfntPoint {
                x: checked_i16(point.x as i32 + dx, "composite x")?,
                y: checked_i16(point.y as i32 + dy, "composite y")?,
                on_curve: point.on_curve,
            });
        }
        translated.push(target);
    }
    Ok(translated)
}

fn insert_logical_glyph(
    by_unicode: &mut BTreeMap<u32, SfntGlyph>,
    codepoint: u32,
    source_code: u16,
    contours: GlyphContours,
    widths: &[WidthRecord],
    font_em: u16,
) {
    if codepoint > 0x10ffff || (0xd800..=0xdfff).contains(&codepoint) {
        return;
    }
    let advance_raw = width_for_code(widths, source_code)
        .unwrap_or(font_em)
        .max(1);
    let advance = scale_u16(advance_raw, font_em, TARGET_UNITS_PER_EM).max(1);
    by_unicode.entry(codepoint).or_insert(SfntGlyph {
        codepoint,
        advance,
        contours,
    });
}

fn wansung_index_to_unicode(index: usize) -> Option<u32> {
    if index >= 2350 {
        return None;
    }
    let lead = 0xB0u8.checked_add((index / 94) as u8)?;
    let trail = 0xA1u8.checked_add((index % 94) as u8)?;
    let input = [lead, trail];
    let (text, _, had_errors) = EUC_KR.decode(&input);
    if had_errors {
        return None;
    }
    let mut chars = text.chars();
    let value = chars.next()? as u32;
    (chars.next().is_none()).then_some(value)
}

fn unicode_hangul_to_kssm(codepoint: u32) -> Option<u16> {
    if !(0xac00..=0xd7a3).contains(&codepoint) {
        return None;
    }
    let syllable = codepoint - 0xac00;
    let cho = (syllable / (21 * 28)) as usize;
    let jung = ((syllable / 28) % 21) as usize;
    let jong = (syllable % 28) as usize;
    const CHO_BITS: [u16; 19] = [
        2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20,
    ];
    const JUNG_BITS: [u16; 21] = [
        3, 4, 5, 6, 7, 10, 11, 12, 13, 14, 15, 18, 19, 20, 21, 22, 23, 26, 27, 28, 29,
    ];
    const JONG_BITS: [u16; 28] = [
        1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 19, 20, 21, 22, 23, 24, 25, 26,
        27, 28, 29,
    ];
    Some(0x8000 | (CHO_BITS[cho] << 10) | (JUNG_BITS[jung] << 5) | JONG_BITS[jong])
}

fn checked_i16(value: i32, label: &str) -> Result<i16, String> {
    i16::try_from(value).map_err(|_| format!("HFT {label} 좌표가 TrueType i16 범위를 벗어납니다"))
}

fn parse_width_records(bytes: &[u8]) -> Result<Vec<WidthRecord>, String> {
    let mut pos = le32(bytes, 0x1aa)? as usize;
    let end = le32(bytes, 0x1ae)? as usize;
    if pos > end || end > bytes.len() {
        return Err("HFT metric directory 범위가 올바르지 않습니다".to_string());
    }
    let mut records = Vec::new();
    while pos < end {
        ensure_range(bytes, pos, 10, "HFT metric record")?;
        let span = le32(bytes, pos)? as usize;
        let start = le16(bytes, pos + 4)?;
        let finish = le16(bytes, pos + 6)?;
        let kind = le16(bytes, pos + 8)?;
        if span < 10 || pos + span > end {
            return Err("HFT metric record span이 올바르지 않습니다".to_string());
        }
        let width_kind = match kind {
            0 if span >= 12 => WidthKind::Fixed(le16(bytes, pos + 10)?),
            1 => {
                let available = (span - 10) / 2;
                let expected = finish.wrapping_sub(start) as usize + 1;
                if available < expected {
                    WidthKind::Unknown
                } else {
                    let mut widths = Vec::with_capacity(expected);
                    for index in 0..expected {
                        widths.push(le16(bytes, pos + 10 + index * 2)?);
                    }
                    WidthKind::PerCode(widths)
                }
            }
            _ => WidthKind::Unknown,
        };
        records.push(WidthRecord {
            start,
            end: finish,
            kind: width_kind,
        });
        pos += span;
    }
    Ok(records)
}

fn width_for_code(records: &[WidthRecord], code: u16) -> Option<u16> {
    for record in records {
        if code < record.start || code > record.end {
            continue;
        }
        match &record.kind {
            WidthKind::Fixed(width) => return Some(*width),
            WidthKind::PerCode(widths) => {
                return widths
                    .get(code.wrapping_sub(record.start) as usize)
                    .copied()
            }
            WidthKind::Unknown => {}
        }
    }
    None
}

fn decode_vendor_stream(file: &[u8], stream: &mut [u8]) -> Result<(), String> {
    ensure_range(file, 0x134, 0x20, "HFT vendor marker")?;
    let marker = &file[0x134..0x154];
    if marker.starts_with(b"Hanyang outline font for HWP ") {
        decode_hnc_obfuscation_in_place(stream);
    } else if marker.starts_with(b"Hangul Mapsi font for HWP 2.5") {
        decode_mapsi_obfuscation_in_place(stream);
    } else if marker.starts_with(b"Human Font for HWP 2.1") {
        ensure_range(file, 0x151, 2, "HFT Human seed")?;
        decode_human_obfuscation_in_place(stream, u16::from_be_bytes([file[0x151], file[0x152]]));
    } else if marker.iter().any(|byte| *byte != 0) {
        return Err("알 수 없는 HFT outline transform marker입니다".to_string());
    }
    Ok(())
}

fn source_code_to_unicode(code: u16, language: &str) -> u32 {
    if code <= 0x7f {
        return code as u32;
    }
    if language == "hanja" {
        if let Some(value) = ks_x_1001_hanja_to_unicode(code) {
            return value;
        }
    }
    if let Some(value) = decode_kssm_modern_syllable(code) {
        return value;
    }
    // Preserve otherwise-unmapped legacy HFT codepoints in Supplementary PUA-A.
    // This is collision-free inside one face and keeps the glyph addressable while
    // dedicated legacy-code mapping is expanded independently.
    0xF0000 + code as u32
}

fn decode_kssm_modern_syllable(code: u16) -> Option<u32> {
    if code < 0x8000 {
        return None;
    }
    const CHO: [i8; 32] = [
        -1, -1, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, -1, -1, -1, -1,
        -1, -1, -1, -1, -1, -1, -1,
    ];
    const JUNG: [i8; 32] = [
        -1, -1, -1, 0, 1, 2, 3, 4, -1, -1, 5, 6, 7, 8, 9, 10, -1, -1, 11, 12, 13, 14, 15, 16, -1,
        -1, 17, 18, 19, 20, -1, -1,
    ];
    const JONG: [i8; 32] = [
        -1, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, -1, 17, 18, 19, 20, 21, 22,
        23, 24, 25, 26, 27, -1, -1,
    ];
    let cho = CHO[((code >> 10) & 0x1f) as usize];
    let jung = JUNG[((code >> 5) & 0x1f) as usize];
    let jong = JONG[(code & 0x1f) as usize];
    if cho < 0 || jung < 0 || jong < 0 {
        return None;
    }
    Some(0xAC00 + cho as u32 * 21 * 28 + jung as u32 * 28 + jong as u32)
}

fn ks_x_1001_hanja_to_unicode(code: u16) -> Option<u32> {
    // HFT Han-character faces index KS X 1001's 4,888 Hanja consecutively at
    // 0x4000..0x5317. KS X 1001 stores those at EUC-KR rows 0xCA..0xFD.
    if !(0x4000..=0x5317).contains(&code) {
        return None;
    }
    let index = (code - 0x4000) as usize;
    let lead = 0xCAu8.checked_add((index / 94) as u8)?;
    let trail = 0xA1u8.checked_add((index % 94) as u8)?;
    let input = [lead, trail];
    let (text, _, had_errors) = EUC_KR.decode(&input);
    if had_errors {
        return None;
    }
    let mut chars = text.chars();
    let value = chars.next()? as u32;
    if chars.next().is_some() {
        return None;
    }
    Some(value)
}

fn hft_style_and_weight(bytes: &[u8]) -> (&'static str, u16) {
    let italic = le16(bytes, 0x158).unwrap_or(0) != 0;
    // HFT +0x18c follows the classic 2..9 Panose-like weight ladder used by
    // Hancom faces (book=4, bold=7, heavy=8). Map it to CSS/OS2 weight classes.
    let weight_class = match le16(bytes, 0x18c).unwrap_or(4) {
        2 => 100,
        3 => 300,
        4 => 400,
        5 => 500,
        6 => 600,
        7 => 700,
        8 => 800,
        9.. => 900,
        _ => 400,
    };
    (if italic { "italic" } else { "normal" }, weight_class)
}

fn outline_to_contours(
    outline: &DecodedOutline,
    source_em: u16,
    target_em: u16,
) -> Result<GlyphContours, String> {
    let mut contours = GlyphContours::new();
    let mut current = Contour::new();
    let mut current_point: Option<Point> = None;

    let mut finish_contour = |current: &mut Contour| {
        if current.len() > 1 {
            let first = current[0];
            let last = *current.last().expect("non-empty contour");
            if first.on_curve && last.on_curve && first.same_position(last) {
                current.pop();
            }
        }
        if !current.is_empty() {
            contours.push(std::mem::take(current));
        }
    };

    for op in &outline.ops {
        match *op {
            OutlineOp::MoveTo(point) => {
                finish_contour(&mut current);
                current.push(scale_sfnt_point(point, source_em, target_em, true)?);
                current_point = Some(point);
            }
            OutlineOp::LineTo(point) => {
                current.push(scale_sfnt_point(point, source_em, target_em, true)?);
                current_point = Some(point);
            }
            OutlineOp::CubicTo {
                control1,
                control2,
                to,
            } => {
                let from = current_point.unwrap_or(control1);
                let cubic = [
                    scale_float_point(from, source_em, target_em),
                    scale_float_point(control1, source_em, target_em),
                    scale_float_point(control2, source_em, target_em),
                    scale_float_point(to, source_em, target_em),
                ];
                let mut quadratic = Vec::new();
                approximate_cubic_with_quadratics(cubic, 0, &mut quadratic);
                for segment in quadratic {
                    current.push(quantize_float_point(segment.control, false)?);
                    current.push(quantize_float_point(segment.to, true)?);
                }
                current_point = Some(to);
            }
            OutlineOp::Close => {
                finish_contour(&mut current);
                current_point = None;
            }
        }
    }
    finish_contour(&mut current);
    Ok(contours)
}

fn scale_sfnt_point(
    point: Point,
    source_em: u16,
    target_em: u16,
    on_curve: bool,
) -> Result<SfntPoint, String> {
    quantize_float_point(scale_float_point(point, source_em, target_em), on_curve)
}

fn scale_float_point(point: Point, source_em: u16, target_em: u16) -> FloatPoint {
    let scale = target_em as f64 / source_em as f64;
    FloatPoint {
        x: point.x as f64 * scale,
        y: point.y as f64 * scale,
    }
}

fn quantize_float_point(point: FloatPoint, on_curve: bool) -> Result<SfntPoint, String> {
    fn coordinate(value: f64, axis: &str) -> Result<i16, String> {
        if !value.is_finite() {
            return Err(format!("HFT glyph {axis} 좌표가 finite 값이 아닙니다"));
        }
        let rounded = value.round();
        if rounded < i16::MIN as f64 || rounded > i16::MAX as f64 {
            return Err(format!(
                "HFT glyph {axis} 좌표가 TrueType i16 범위를 벗어납니다"
            ));
        }
        Ok(rounded as i16)
    }

    let x = coordinate(point.x, "x")?;
    let y = coordinate(point.y, "y")?;
    Ok(if on_curve {
        SfntPoint::on_curve(x, y)
    } else {
        SfntPoint::off_curve(x, y)
    })
}

/// Approximate a cubic Bézier with one or more quadratic Béziers while preserving
/// actual curve points in the TrueType outline. For the midpoint-matched quadratic,
/// the exact difference from the cubic is
///
/// `D * t * (t - 1/2) * (t - 1)`, where
/// `D = p3 - 3p2 + 3p1 - p0`.
///
/// Therefore the maximum Euclidean error over the segment is
/// `|D| / (12 * sqrt(3))`. Bisecting a cubic scales `D` by 1/8, so recursive
/// subdivision gives a deterministic error bound without sampling.
fn approximate_cubic_with_quadratics(
    cubic: [FloatPoint; 4],
    depth: u8,
    out: &mut Vec<QuadraticSegment>,
) {
    let [p0, p1, p2, p3] = cubic;
    let dx = p3.x - 3.0 * p2.x + 3.0 * p1.x - p0.x;
    let dy = p3.y - 3.0 * p2.y + 3.0 * p1.y - p0.y;
    let max_error = dx.hypot(dy) / (12.0 * 3.0_f64.sqrt());

    if max_error <= CUBIC_QUADRATIC_MAX_ERROR || depth >= CUBIC_QUADRATIC_MAX_DEPTH {
        // Match the cubic at t=0, 1/2, 1. Solving the quadratic midpoint equation
        // gives q = (-p0 + 3p1 + 3p2 - p3) / 4.
        out.push(QuadraticSegment {
            control: FloatPoint {
                x: (-p0.x + 3.0 * p1.x + 3.0 * p2.x - p3.x) * 0.25,
                y: (-p0.y + 3.0 * p1.y + 3.0 * p2.y - p3.y) * 0.25,
            },
            to: p3,
        });
        return;
    }

    // de Casteljau split at t=1/2. This is exact, and preserves the original cubic
    // before each half is independently approximated by a quadratic.
    let p01 = midpoint(p0, p1);
    let p12 = midpoint(p1, p2);
    let p23 = midpoint(p2, p3);
    let p012 = midpoint(p01, p12);
    let p123 = midpoint(p12, p23);
    let p0123 = midpoint(p012, p123);
    approximate_cubic_with_quadratics([p0, p01, p012, p0123], depth + 1, out);
    approximate_cubic_with_quadratics([p0123, p123, p23, p3], depth + 1, out);
}

fn midpoint(a: FloatPoint, b: FloatPoint) -> FloatPoint {
    FloatPoint {
        x: (a.x + b.x) * 0.5,
        y: (a.y + b.y) * 0.5,
    }
}

fn scale_i32(value: i32, source_em: u16, target_em: u16) -> i32 {
    ((value as i64 * target_em as i64 + (source_em as i64 / 2)) / source_em as i64) as i32
}

fn scale_u16(value: u16, source_em: u16, target_em: u16) -> u16 {
    let scaled = (value as u32 * target_em as u32 + source_em as u32 / 2) / source_em as u32;
    scaled.min(u16::MAX as u32) as u16
}

fn build_ttf(
    family: &str,
    style: &str,
    weight: u16,
    post_script: &str,
    units_per_em: u16,
    glyphs: &[SfntGlyph],
) -> Result<Vec<u8>, String> {
    if glyphs.len() >= u16::MAX as usize {
        return Err("TrueType glyph 수가 65534개를 초과합니다".to_string());
    }

    let mut glyf = Vec::new();
    let mut loca = Vec::<u32>::with_capacity(glyphs.len() + 2);
    let mut hmetrics = Vec::<(u16, i16)>::with_capacity(glyphs.len() + 1);
    let mut global_bounds = Bounds::default();
    let mut max_points = 0u16;
    let mut max_contours = 0u16;

    // Glyph 0: .notdef (empty). All source glyph IDs follow in codepoint order.
    loca.push(0);
    hmetrics.push((units_per_em / 2, 0));
    append_empty_glyph(&mut glyf);
    pad4(&mut glyf);
    loca.push(glyf.len() as u32);

    for glyph in glyphs {
        let (encoded, bounds, point_count, contour_count) = encode_simple_glyph(&glyph.contours)?;
        global_bounds.include(bounds);
        max_points = max_points.max(point_count);
        max_contours = max_contours.max(contour_count);
        let lsb = if bounds.has_points { bounds.x_min } else { 0 };
        hmetrics.push((glyph.advance, lsb));
        glyf.extend_from_slice(&encoded);
        pad4(&mut glyf);
        loca.push(glyf.len() as u32);
    }

    if !global_bounds.has_points {
        global_bounds = Bounds {
            x_min: 0,
            y_min: 0,
            x_max: units_per_em as i16,
            y_max: units_per_em as i16,
            has_points: true,
        };
    }
    let ascender = global_bounds
        .y_max
        .max((units_per_em as i32 * 4 / 5) as i16);
    let descender = global_bounds.y_min.min(-((units_per_em as i32 / 5) as i16));
    let num_glyphs = (glyphs.len() + 1) as u16;

    let mut loca_bytes = Vec::with_capacity(loca.len() * 4);
    for offset in loca {
        push_u32(&mut loca_bytes, offset);
    }
    let mut hmtx = Vec::with_capacity(hmetrics.len() * 4);
    for (advance, lsb) in &hmetrics {
        push_u16(&mut hmtx, *advance);
        push_i16(&mut hmtx, *lsb);
    }

    let cmap_pairs = glyphs
        .iter()
        .enumerate()
        .map(|(index, glyph)| (glyph.codepoint, (index + 1) as u16))
        .collect::<Vec<_>>();
    let cmap = build_cmap(&cmap_pairs)?;
    let head = build_head(units_per_em, global_bounds, style, weight);
    let hhea = build_hhea(ascender, descender, &hmetrics, global_bounds, num_glyphs);
    let maxp = build_maxp(num_glyphs, max_points, max_contours);
    let name = build_name_table(family, style, weight, post_script);
    let post = build_post(style);
    let os2 = build_os2(
        units_per_em,
        ascender,
        descender,
        weight,
        style,
        &hmetrics,
        &cmap_pairs,
    );

    let mut tables = vec![
        (*b"OS/2", os2),
        (*b"cmap", cmap),
        (*b"glyf", glyf),
        (*b"head", head),
        (*b"hhea", hhea),
        (*b"hmtx", hmtx),
        (*b"loca", loca_bytes),
        (*b"maxp", maxp),
        (*b"name", name),
        (*b"post", post),
    ];
    tables.sort_by_key(|(tag, _)| *tag);
    assemble_sfnt(tables)
}

fn encode_simple_glyph(
    contours: &[Contour],
) -> Result<(Vec<u8>, Bounds, u16, u16), String> {
    let filtered = contours
        .iter()
        .filter(|contour| !contour.is_empty())
        .collect::<Vec<_>>();
    if filtered.is_empty() {
        let mut out = Vec::with_capacity(10);
        push_i16(&mut out, 0);
        for _ in 0..4 {
            push_i16(&mut out, 0);
        }
        return Ok((out, Bounds::default(), 0, 0));
    }
    if filtered.len() > i16::MAX as usize {
        return Err("TrueType contour 수가 i16 범위를 벗어납니다".to_string());
    }

    let total_points = filtered.iter().map(|contour| contour.len()).sum::<usize>();
    if total_points > u16::MAX as usize {
        return Err("TrueType glyph point 수가 u16 범위를 벗어납니다".to_string());
    }
    let mut bounds = Bounds::default();
    for contour in &filtered {
        for point in *contour {
            bounds.include_point(point.x, point.y);
        }
    }

    let mut out = Vec::new();
    push_i16(&mut out, filtered.len() as i16);
    push_i16(&mut out, bounds.x_min);
    push_i16(&mut out, bounds.y_min);
    push_i16(&mut out, bounds.x_max);
    push_i16(&mut out, bounds.y_max);

    let mut endpoint = 0usize;
    for contour in &filtered {
        endpoint += contour.len();
        push_u16(&mut out, (endpoint - 1) as u16);
    }
    push_u16(&mut out, 0); // instructionLength

    // Bit 0 is the TrueType on-curve flag. Cubic HFT segments are converted to
    // bounded-error quadratic segments above, so their controls stay off-curve.
    for contour in &filtered {
        out.extend(contour.iter().map(|point| u8::from(point.on_curve)));
    }

    let points = filtered
        .iter()
        .flat_map(|contour| contour.iter().copied())
        .collect::<Vec<_>>();
    let mut previous = 0i16;
    for point in &points {
        let delta = point.x.wrapping_sub(previous);
        push_i16(&mut out, delta);
        previous = point.x;
    }
    previous = 0;
    for point in &points {
        let delta = point.y.wrapping_sub(previous);
        push_i16(&mut out, delta);
        previous = point.y;
    }

    Ok((out, bounds, total_points as u16, filtered.len() as u16))
}

fn append_empty_glyph(out: &mut Vec<u8>) {
    push_i16(out, 0);
    for _ in 0..4 {
        push_i16(out, 0);
    }
}

fn build_cmap(pairs: &[(u32, u16)]) -> Result<Vec<u8>, String> {
    let bmp = pairs
        .iter()
        .copied()
        .filter(|(cp, _)| *cp <= 0xffff && *cp != 0xffff)
        .collect::<Vec<_>>();
    let format4 = build_cmap_format4(&bmp)?;
    let format12 = build_cmap_format12(pairs);

    let header_len = 4 + 3 * 8;
    let format4_offset = header_len as u32;
    let format12_offset = (header_len + format4.len()) as u32;
    let mut out = Vec::with_capacity(header_len + format4.len() + format12.len());
    push_u16(&mut out, 0);
    push_u16(&mut out, 3);
    push_u16(&mut out, 0);
    push_u16(&mut out, 4);
    push_u32(&mut out, format12_offset);
    push_u16(&mut out, 3);
    push_u16(&mut out, 1);
    push_u32(&mut out, format4_offset);
    push_u16(&mut out, 3);
    push_u16(&mut out, 10);
    push_u32(&mut out, format12_offset);
    out.extend_from_slice(&format4);
    out.extend_from_slice(&format12);
    Ok(out)
}

fn mapping_runs(pairs: &[(u32, u16)]) -> Vec<(u32, u32, u16)> {
    if pairs.is_empty() {
        return Vec::new();
    }
    let mut runs = Vec::new();
    let mut start_cp = pairs[0].0;
    let mut end_cp = start_cp;
    let mut start_gid = pairs[0].1;
    let mut previous_gid = start_gid;
    for &(cp, gid) in &pairs[1..] {
        if cp == end_cp + 1 && gid == previous_gid.wrapping_add(1) {
            end_cp = cp;
            previous_gid = gid;
        } else {
            runs.push((start_cp, end_cp, start_gid));
            start_cp = cp;
            end_cp = cp;
            start_gid = gid;
            previous_gid = gid;
        }
    }
    runs.push((start_cp, end_cp, start_gid));
    runs
}

fn build_cmap_format4(pairs: &[(u32, u16)]) -> Result<Vec<u8>, String> {
    let runs = mapping_runs(pairs);
    let seg_count = runs.len() + 1; // terminal 0xffff segment
    if seg_count > 0x7fff {
        return Err("cmap format 4 segment 수가 너무 많습니다".to_string());
    }
    let length = 16usize
        .checked_add(seg_count * 8)
        .ok_or_else(|| "cmap format 4 길이 overflow".to_string())?;
    if length > u16::MAX as usize {
        return Err("cmap format 4가 65535 bytes를 초과합니다".to_string());
    }
    let seg_count_x2 = (seg_count * 2) as u16;
    let pow2 = 1usize << ((usize::BITS - (seg_count as u32).leading_zeros() - 1) as usize);
    let search_range = (pow2 * 2) as u16;
    let entry_selector = pow2.trailing_zeros() as u16;
    let range_shift = seg_count_x2 - search_range;

    let mut out = Vec::with_capacity(length);
    push_u16(&mut out, 4);
    push_u16(&mut out, length as u16);
    push_u16(&mut out, 0);
    push_u16(&mut out, seg_count_x2);
    push_u16(&mut out, search_range);
    push_u16(&mut out, entry_selector);
    push_u16(&mut out, range_shift);
    for &(_, end, _) in &runs {
        push_u16(&mut out, end as u16);
    }
    push_u16(&mut out, 0xffff);
    push_u16(&mut out, 0);
    for &(start, _, _) in &runs {
        push_u16(&mut out, start as u16);
    }
    push_u16(&mut out, 0xffff);
    for &(start, _, gid) in &runs {
        let delta = gid.wrapping_sub(start as u16);
        push_u16(&mut out, delta);
    }
    push_u16(&mut out, 1);
    for _ in 0..seg_count {
        push_u16(&mut out, 0);
    }
    debug_assert_eq!(out.len(), length);
    Ok(out)
}

fn build_cmap_format12(pairs: &[(u32, u16)]) -> Vec<u8> {
    let runs = mapping_runs(pairs);
    let length = 16 + runs.len() * 12;
    let mut out = Vec::with_capacity(length);
    push_u16(&mut out, 12);
    push_u16(&mut out, 0);
    push_u32(&mut out, length as u32);
    push_u32(&mut out, 0);
    push_u32(&mut out, runs.len() as u32);
    for (start, end, gid) in runs {
        push_u32(&mut out, start);
        push_u32(&mut out, end);
        push_u32(&mut out, gid as u32);
    }
    out
}

fn build_head(units_per_em: u16, bounds: Bounds, style: &str, weight: u16) -> Vec<u8> {
    let mut out = Vec::with_capacity(54);
    push_u32(&mut out, 0x0001_0000);
    push_u32(&mut out, 0x0001_0000);
    push_u32(&mut out, 0); // checkSumAdjustment, filled after assembly
    push_u32(&mut out, 0x5F0F_3CF5);
    push_u16(&mut out, 0x000b);
    push_u16(&mut out, units_per_em);
    push_u64(&mut out, 0);
    push_u64(&mut out, 0);
    push_i16(&mut out, bounds.x_min);
    push_i16(&mut out, bounds.y_min);
    push_i16(&mut out, bounds.x_max);
    push_i16(&mut out, bounds.y_max);
    let mut mac_style = 0u16;
    if weight >= 700 {
        mac_style |= 1;
    }
    if style == "italic" {
        mac_style |= 2;
    }
    push_u16(&mut out, mac_style);
    push_u16(&mut out, 8);
    push_i16(&mut out, 2);
    push_i16(&mut out, 1); // long loca offsets
    push_i16(&mut out, 0);
    debug_assert_eq!(out.len(), 54);
    out
}

fn build_hhea(
    ascender: i16,
    descender: i16,
    hmetrics: &[(u16, i16)],
    global_bounds: Bounds,
    num_glyphs: u16,
) -> Vec<u8> {
    let advance_max = hmetrics.iter().map(|metric| metric.0).max().unwrap_or(0);
    let min_lsb = hmetrics.iter().map(|metric| metric.1).min().unwrap_or(0);
    let min_rsb = 0i16;
    let extent = global_bounds.x_max;
    let mut out = Vec::with_capacity(36);
    push_u32(&mut out, 0x0001_0000);
    push_i16(&mut out, ascender);
    push_i16(&mut out, descender);
    push_i16(&mut out, 0);
    push_u16(&mut out, advance_max);
    push_i16(&mut out, min_lsb);
    push_i16(&mut out, min_rsb);
    push_i16(&mut out, extent);
    push_i16(&mut out, 1);
    push_i16(&mut out, 0);
    push_i16(&mut out, 0);
    for _ in 0..4 {
        push_i16(&mut out, 0);
    }
    push_i16(&mut out, 0);
    push_u16(&mut out, num_glyphs);
    debug_assert_eq!(out.len(), 36);
    out
}

fn build_maxp(num_glyphs: u16, max_points: u16, max_contours: u16) -> Vec<u8> {
    let mut out = Vec::with_capacity(32);
    push_u32(&mut out, 0x0001_0000);
    push_u16(&mut out, num_glyphs);
    push_u16(&mut out, max_points);
    push_u16(&mut out, max_contours);
    push_u16(&mut out, 0); // maxCompositePoints
    push_u16(&mut out, 0); // maxCompositeContours
    push_u16(&mut out, 2); // maxZones
    push_u16(&mut out, 0); // maxTwilightPoints
    push_u16(&mut out, 0); // maxStorage
    push_u16(&mut out, 0); // maxFunctionDefs
    push_u16(&mut out, 0); // maxInstructionDefs
    push_u16(&mut out, 0); // maxStackElements
    push_u16(&mut out, 0); // maxSizeOfInstructions
    push_u16(&mut out, 0); // maxComponentElements
    push_u16(&mut out, 0); // maxComponentDepth
    debug_assert_eq!(out.len(), 32);
    out
}

fn build_name_table(family: &str, style: &str, weight: u16, post_script: &str) -> Vec<u8> {
    let subfamily = match (weight >= 700, style == "italic") {
        (true, true) => "Bold Italic",
        (true, false) => "Bold",
        (false, true) => "Italic",
        (false, false) => "Regular",
    };
    let full_name = if subfamily == "Regular" {
        family.to_string()
    } else {
        format!("{family} {subfamily}")
    };
    let unique = format!("HOP-HFT;1.0;{post_script}");
    let entries = [
        (1u16, family.to_string()),
        (2u16, subfamily.to_string()),
        (3u16, unique),
        (4u16, full_name),
        (
            5u16,
            "Version 1.0; HOP clean-room HFT conversion".to_string(),
        ),
        (6u16, post_script.to_string()),
    ];
    let mut encoded = Vec::<(u16, Vec<u8>)>::new();
    for (name_id, value) in entries {
        let mut bytes = Vec::with_capacity(value.len() * 2);
        for unit in value.encode_utf16() {
            push_u16(&mut bytes, unit);
        }
        encoded.push((name_id, bytes));
    }
    let count = encoded.len() as u16;
    let string_offset = 6 + encoded.len() * 12;
    let mut out = Vec::new();
    push_u16(&mut out, 0);
    push_u16(&mut out, count);
    push_u16(&mut out, string_offset as u16);
    let mut offset = 0usize;
    for (name_id, bytes) in &encoded {
        push_u16(&mut out, 3);
        push_u16(&mut out, 1);
        push_u16(&mut out, 0x0409);
        push_u16(&mut out, *name_id);
        push_u16(&mut out, bytes.len() as u16);
        push_u16(&mut out, offset as u16);
        offset += bytes.len();
    }
    for (_, bytes) in encoded {
        out.extend_from_slice(&bytes);
    }
    out
}

fn build_post(style: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(32);
    push_u32(&mut out, 0x0003_0000);
    push_u32(&mut out, if style == "italic" { 0x000C_0000 } else { 0 });
    push_i16(&mut out, -75);
    push_i16(&mut out, 50);
    push_u32(&mut out, 0);
    for _ in 0..4 {
        push_u32(&mut out, 0);
    }
    debug_assert_eq!(out.len(), 32);
    out
}

fn build_os2(
    units_per_em: u16,
    ascender: i16,
    descender: i16,
    weight: u16,
    style: &str,
    hmetrics: &[(u16, i16)],
    cmap_pairs: &[(u32, u16)],
) -> Vec<u8> {
    let avg = if hmetrics.len() > 1 {
        (hmetrics[1..]
            .iter()
            .map(|metric| metric.0 as u64)
            .sum::<u64>()
            / (hmetrics.len() - 1) as u64) as i16
    } else {
        (units_per_em / 2) as i16
    };
    let first = cmap_pairs
        .iter()
        .map(|(cp, _)| *cp)
        .filter(|cp| *cp <= 0xffff)
        .min()
        .unwrap_or(0xffff) as u16;
    let last = cmap_pairs
        .iter()
        .map(|(cp, _)| *cp)
        .filter(|cp| *cp <= 0xffff)
        .max()
        .unwrap_or(0xffff) as u16;
    let mut selection = 0u16;
    if style == "italic" {
        selection |= 0x0001;
    }
    if weight >= 700 {
        selection |= 0x0020;
    }
    if selection == 0 {
        selection |= 0x0040;
    }
    let win_ascent = ascender.max(0) as u16;
    let win_descent = descender.saturating_abs() as u16;

    let mut out = Vec::with_capacity(78);
    push_u16(&mut out, 0); // version
    push_i16(&mut out, avg);
    push_u16(&mut out, weight.clamp(1, 1000));
    push_u16(&mut out, 5); // medium width
    push_u16(&mut out, 0); // installable embedding
    push_i16(&mut out, (units_per_em as i32 * 65 / 100) as i16);
    push_i16(&mut out, (units_per_em as i32 * 60 / 100) as i16);
    push_i16(&mut out, 0);
    push_i16(&mut out, (units_per_em as i32 * 8 / 100) as i16);
    push_i16(&mut out, (units_per_em as i32 * 65 / 100) as i16);
    push_i16(&mut out, (units_per_em as i32 * 60 / 100) as i16);
    push_i16(&mut out, 0);
    push_i16(&mut out, (units_per_em as i32 * 45 / 100) as i16);
    push_i16(&mut out, 0); // yStrikeoutSize
    push_i16(&mut out, 0); // yStrikeoutPosition
    push_i16(&mut out, 0); // sFamilyClass
    out.extend_from_slice(&[0; 10]); // Panose
    for _ in 0..4 {
        push_u32(&mut out, 0);
    }
    out.extend_from_slice(b"HOP ");
    push_u16(&mut out, selection);
    push_u16(&mut out, first);
    push_u16(&mut out, last);
    push_i16(&mut out, ascender);
    push_i16(&mut out, descender);
    push_i16(&mut out, 0);
    push_u16(&mut out, win_ascent);
    push_u16(&mut out, win_descent);
    debug_assert_eq!(out.len(), 78);
    out
}

fn assemble_sfnt(mut tables: Vec<([u8; 4], Vec<u8>)>) -> Result<Vec<u8>, String> {
    let num_tables = tables.len();
    if num_tables > u16::MAX as usize {
        return Err("SFNT table 수가 너무 많습니다".to_string());
    }
    tables.sort_by_key(|(tag, _)| *tag);
    let pow2 = 1usize << ((usize::BITS - (num_tables as u32).leading_zeros() - 1) as usize);
    let search_range = (pow2 * 16) as u16;
    let entry_selector = pow2.trailing_zeros() as u16;
    let range_shift = (num_tables * 16) as u16 - search_range;
    let directory_len = 12 + num_tables * 16;
    let mut offsets = Vec::with_capacity(num_tables);
    let mut cursor = directory_len;
    for (_, table) in &tables {
        cursor = (cursor + 3) & !3;
        offsets.push(cursor);
        cursor += table.len();
    }
    let mut out = Vec::with_capacity((cursor + 3) & !3);
    push_u32(&mut out, 0x0001_0000);
    push_u16(&mut out, num_tables as u16);
    push_u16(&mut out, search_range);
    push_u16(&mut out, entry_selector);
    push_u16(&mut out, range_shift);
    for ((tag, table), offset) in tables.iter().zip(offsets.iter().copied()) {
        out.extend_from_slice(tag);
        push_u32(&mut out, table_checksum(table));
        push_u32(&mut out, offset as u32);
        push_u32(&mut out, table.len() as u32);
    }
    for ((_, table), offset) in tables.iter().zip(offsets.iter().copied()) {
        while out.len() < offset {
            out.push(0);
        }
        out.extend_from_slice(table);
        pad4(&mut out);
    }

    let head_offset = find_table_offset(&out, b"head")
        .ok_or_else(|| "생성된 SFNT에 head table이 없습니다".to_string())?;
    let adjustment_offset = head_offset + 8;
    if adjustment_offset + 4 > out.len() {
        return Err("생성된 head table이 잘렸습니다".to_string());
    }
    out[adjustment_offset..adjustment_offset + 4].copy_from_slice(&[0, 0, 0, 0]);
    let checksum = sfnt_checksum(&out);
    let adjustment = 0xB1B0_AFBAu32.wrapping_sub(checksum);
    out[adjustment_offset..adjustment_offset + 4].copy_from_slice(&adjustment.to_be_bytes());
    Ok(out)
}

fn table_checksum(table: &[u8]) -> u32 {
    let mut sum = 0u32;
    for chunk in table.chunks(4) {
        let mut word = [0u8; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        sum = sum.wrapping_add(u32::from_be_bytes(word));
    }
    sum
}

fn sfnt_checksum(data: &[u8]) -> u32 {
    table_checksum(data)
}

fn find_table_offset(data: &[u8], tag: &[u8; 4]) -> Option<usize> {
    if data.len() < 12 {
        return None;
    }
    let num_tables = u16::from_be_bytes([data[4], data[5]]) as usize;
    for index in 0..num_tables {
        let pos = 12 + index * 16;
        if pos + 16 > data.len() {
            return None;
        }
        if &data[pos..pos + 4] == tag {
            return Some(u32::from_be_bytes([
                data[pos + 8],
                data[pos + 9],
                data[pos + 10],
                data[pos + 11],
            ]) as usize);
        }
    }
    None
}

fn read_sfnt_num_glyphs(data: &[u8]) -> Option<u16> {
    let offset = find_table_offset(data, b"maxp")?;
    if offset + 6 > data.len() {
        return None;
    }
    Some(u16::from_be_bytes([data[offset + 4], data[offset + 5]]))
}

fn cached_sfnt_matches(path: &Path, expected: &[u8]) -> Result<bool, String> {
    let metadata = match fs::metadata(path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(format!("HFT 캐시 메타데이터를 읽을 수 없습니다: {error}"));
        }
    };
    if metadata.len() != expected.len() as u64 {
        return Ok(false);
    }
    let existing =
        fs::read(path).map_err(|error| format!("HFT 캐시를 읽을 수 없습니다: {error}"))?;
    Ok(existing == expected)
}

fn create_private_cache_dir(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path)
        .map_err(|error| format!("HFT 파생 글꼴 캐시 디렉터리를 만들 수 없습니다: {error}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|error| format!("HFT 캐시 권한을 설정할 수 없습니다: {error}"))?;
    }
    Ok(())
}

fn atomic_private_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "HFT 캐시 경로에 상위 디렉터리가 없습니다".to_string())?;
    create_private_cache_dir(parent)?;
    let temp = parent.join(format!(
        ".hft-{}-{}.tmp",
        std::process::id(),
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("font")
    ));
    fs::write(&temp, bytes).map_err(|error| format!("HFT 파생 글꼴을 쓸 수 없습니다: {error}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&temp, fs::Permissions::from_mode(0o600))
            .map_err(|error| format!("HFT 파생 글꼴 권한을 설정할 수 없습니다: {error}"))?;
    }
    if let Err(error) = fs::rename(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(format!("HFT 파생 글꼴 캐시를 교체할 수 없습니다: {error}"));
    }
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    hex_bytes(&digest)
}

fn hex_bytes(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

fn post_script_name(family: &str, content_hash: &str) -> String {
    let mut name = family
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .take(40)
        .collect::<String>();
    if name.is_empty() {
        name.push_str("HOPHFT");
    }
    name.push('-');
    name.push_str(&content_hash[..12]);
    name
}

impl Bounds {
    fn include_point(&mut self, x: i16, y: i16) {
        if !self.has_points {
            self.x_min = x;
            self.x_max = x;
            self.y_min = y;
            self.y_max = y;
            self.has_points = true;
            return;
        }
        self.x_min = self.x_min.min(x);
        self.x_max = self.x_max.max(x);
        self.y_min = self.y_min.min(y);
        self.y_max = self.y_max.max(y);
    }

    fn include(&mut self, other: Bounds) {
        if !other.has_points {
            return;
        }
        self.include_point(other.x_min, other.y_min);
        self.include_point(other.x_max, other.y_max);
    }
}

fn le16(bytes: &[u8], offset: usize) -> Result<u16, String> {
    ensure_range(bytes, offset, 2, "HFT u16")?;
    Ok(u16::from_le_bytes([bytes[offset], bytes[offset + 1]]))
}

fn le_i16(bytes: &[u8], offset: usize) -> Result<i16, String> {
    Ok(le16(bytes, offset)? as i16)
}

fn le32(bytes: &[u8], offset: usize) -> Result<u32, String> {
    ensure_range(bytes, offset, 4, "HFT u32")?;
    Ok(u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ]))
}

fn ensure_range(bytes: &[u8], offset: usize, len: usize, label: &str) -> Result<(), String> {
    let end = offset
        .checked_add(len)
        .ok_or_else(|| format!("{label} 오프셋 overflow"))?;
    if end > bytes.len() {
        return Err(format!("{label}가 파일 범위를 벗어납니다"));
    }
    Ok(())
}

fn push_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_be_bytes());
}
fn push_i16(out: &mut Vec<u8>, value: i16) {
    out.extend_from_slice(&value.to_be_bytes());
}
fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_be_bytes());
}
fn push_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_be_bytes());
}
fn pad4(out: &mut Vec<u8>) {
    let padding = (4 - out.len() % 4) % 4;
    out.resize(out.len() + padding, 0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn synthetic_triangle() -> SfntGlyph {
        SfntGlyph {
            codepoint: 'A' as u32,
            advance: 650,
            contours: vec![vec![(50, 0), (325, 700), (600, 0)]],
        }
    }

    #[test]
    fn kssm_modern_hangul_mapping_matches_unicode_formula() {
        assert_eq!(decode_kssm_modern_syllable(0x8861), Some('가' as u32));
        assert_eq!(decode_kssm_modern_syllable(0x8862), Some('각' as u32));
        assert_eq!(decode_kssm_modern_syllable(0xd3c5), None); // araea old Hangul
    }

    #[test]
    fn ks_x_1001_hanja_index_maps_first_and_last_entries() {
        assert!(ks_x_1001_hanja_to_unicode(0x4000).is_some());
        assert!(ks_x_1001_hanja_to_unicode(0x5317).is_some());
        assert!(ks_x_1001_hanja_to_unicode(0x5318).is_none());
    }

    #[test]
    fn synthetic_ttf_has_valid_checksum_and_fontdb_face() {
        let glyphs = vec![synthetic_triangle()];
        let ttf = build_ttf(
            "HOP HFT Synthetic",
            "normal",
            400,
            "HOPHFTSynthetic",
            1000,
            &glyphs,
        )
        .unwrap();
        assert_eq!(sfnt_checksum(&ttf), 0xB1B0_AFBA);
        assert_eq!(read_sfnt_num_glyphs(&ttf), Some(2));

        let mut database = usvg::fontdb::Database::new();
        database.load_font_data(ttf);
        assert!(database.faces().any(|face| face
            .families
            .iter()
            .any(|(family, _)| family == "HOP HFT Synthetic")));
    }

    #[test]
    fn cache_key_is_sha256_and_deterministic() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn generated_ttf_passes_available_external_sfnt_validators() {
        use std::process::Command;

        let ttf = build_ttf(
            "HOP HFT Validator",
            "normal",
            400,
            "HOPHFTValidator",
            1000,
            &[synthetic_triangle()],
        )
        .unwrap();
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("validator.ttf");
        fs::write(&path, ttf).unwrap();

        for (program, args) in [
            ("fc-scan", vec![path.as_os_str()]),
            (
                "otfinfo",
                vec![std::ffi::OsStr::new("-i"), path.as_os_str()],
            ),
        ] {
            let available = Command::new("sh")
                .arg("-c")
                .arg(format!("command -v {program} >/dev/null 2>&1"))
                .status()
                .map(|status| status.success())
                .unwrap_or(false);
            if !available {
                eprintln!("{program} unavailable; validator skipped");
                continue;
            }
            let output = Command::new(program).args(args).output().unwrap();
            assert!(
                output.status.success(),
                "{program} rejected generated TTF: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    #[test]
    fn local_full_enggti_materializes_when_requested() {
        if std::env::var_os("HOP_HFT_FULL_SWEEP").is_none() {
            eprintln!("HOP_HFT_FULL_SWEEP is unset; full local sweep skipped");
            return;
        }
        let entries = collect_desktop_hft_font_entries().unwrap();
        assert_eq!(entries.len(), 387, "expected complete ENGGTI catalog");
        let cache = tempfile::tempdir().unwrap();
        let derived = materialize_hft_sfnt_cache(&entries, cache.path()).unwrap();
        assert_eq!(
            derived.iter().map(|font| font.source_count).sum::<usize>(),
            387
        );
        assert!(
            derived.len() < 387,
            "language-part HFT files should be merged"
        );
        assert!(derived.iter().all(|font| font.glyph_count > 1));
        let myeongjo = derived
            .iter()
            .find(|font| font.family == "명조" && font.style == "normal")
            .expect("merged 명조 face");
        assert_eq!(myeongjo.source_count, 7);
        assert!(myeongjo
            .languages
            .iter()
            .any(|language| language == "hangul"));
        assert!(myeongjo
            .languages
            .iter()
            .any(|language| language == "hanja"));
        assert!(myeongjo
            .languages
            .iter()
            .any(|language| language == "latin"));

        let mut database = usvg::fontdb::Database::new();
        for font in &derived {
            database.load_font_file(&font.derived_path).unwrap();

            let bytes = fs::read(&font.derived_path).unwrap();
            assert_eq!(
                sfnt_checksum(&bytes),
                0xB1B0_AFBA,
                "invalid whole-font SFNT checksum: {}",
                font.derived_path
            );
            assert_eq!(
                read_sfnt_num_glyphs(&bytes).map(usize::from),
                Some(font.glyph_count),
                "maxp glyph count drifted from cache metadata: {}",
                font.derived_path
            );

            for (program, args) in [
                (
                    "fc-scan",
                    vec![
                        std::ffi::OsString::from("--format=%{family}\\n"),
                        font.derived_path.clone().into(),
                    ],
                ),
                (
                    "otfinfo",
                    vec![
                        std::ffi::OsString::from("-i"),
                        font.derived_path.clone().into(),
                    ],
                ),
                (
                    "hb-shape",
                    vec![
                        font.derived_path.clone().into(),
                        std::ffi::OsString::from("A가漢"),
                    ],
                ),
            ] {
                let available = std::process::Command::new("sh")
                    .arg("-c")
                    .arg(format!("command -v {program} >/dev/null 2>&1"))
                    .status()
                    .map(|status| status.success())
                    .unwrap_or(false);
                if !available {
                    continue;
                }
                let output = std::process::Command::new(program)
                    .args(args)
                    .output()
                    .unwrap();
                assert!(
                    output.status.success(),
                    "{program} rejected generated HFT TTF {}: {}",
                    font.derived_path,
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }
        assert!(database.faces().count() >= derived.len());
    }

    #[test]
    fn local_owned_hft_can_materialize_when_available() {
        let sample = std::env::var_os("HOP_HFT_SAMPLE")
            .map(PathBuf::from)
            .or_else(|| {
                let path = PathBuf::from("/Users/kcu/Downloads/ENGGTI/ENBASKVL.HFT");
                path.is_file().then_some(path)
            });
        let Some(sample) = sample else {
            eprintln!("local HFT sample unavailable; conditional test skipped");
            return;
        };
        let bytes = fs::read(&sample).unwrap();
        let hash = sha256_hex(&bytes);
        let ttf = convert_hft_to_ttf(&bytes, "BaskervilleBT", "latin", &hash).unwrap();
        assert_eq!(sfnt_checksum(&ttf), 0xB1B0_AFBA);
        assert!(read_sfnt_num_glyphs(&ttf).unwrap() >= 90);
        let mut database = usvg::fontdb::Database::new();
        database.load_font_data(ttf);
        assert!(database.faces().any(|face| {
            face.families
                .iter()
                .any(|(family, _)| family == "BaskervilleBT")
        }));
    }
}
