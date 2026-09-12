use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

// File-name/family/language metadata only. No HFT font bytes are vendored into HOP.
// Discovery prefers HOP_HFT_DIR; on macOS it otherwise checks $HOME/Downloads/ENGGTI.
const HFT_METADATA: &str = include_str!("hft_metadata.tsv");
const HFT_DIR_ENV: &str = "HOP_HFT_DIR";
const MAX_HFT_FILE_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HftFontEntry {
    pub family: String,
    pub file_name: String,
    pub language: String,
    pub source_kind: String,
    pub path: String,
    pub byte_len: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HftMetadataEntry<'a> {
    family: &'a str,
    language: &'a str,
}

pub fn desktop_hft_font_dirs() -> Vec<PathBuf> {
    let configured = std::env::var_os(HFT_DIR_ENV);
    let home = std::env::var_os("HOME");
    hft_font_dir_candidates(configured, home, cfg!(target_os = "macos"))
        .into_iter()
        .filter_map(|path| normalize_existing_path(&path))
        .collect()
}

pub fn collect_desktop_hft_font_entries() -> Result<Vec<HftFontEntry>, String> {
    collect_hft_font_entries_from(&desktop_hft_font_dirs())
}

pub fn read_desktop_hft_font(path: &Path) -> Result<Vec<u8>, String> {
    read_hft_font_from_roots(path, &desktop_hft_font_dirs())
}

fn hft_font_dir_candidates(
    configured: Option<OsString>,
    home: Option<OsString>,
    use_macos_default: bool,
) -> Vec<PathBuf> {
    if let Some(configured) = configured.filter(|value| !value.is_empty()) {
        return vec![PathBuf::from(configured)];
    }

    if use_macos_default {
        if let Some(home) = home.filter(|value| !value.is_empty()) {
            return vec![PathBuf::from(home).join("Downloads/ENGGTI")];
        }
    }

    Vec::new()
}

fn collect_hft_font_entries_from(roots: &[PathBuf]) -> Result<Vec<HftFontEntry>, String> {
    let metadata = hft_metadata_index()?;
    let mut seen_paths = BTreeSet::new();
    let mut entries = Vec::new();

    for root in roots {
        let Some(root) = normalize_existing_path(root) else {
            continue;
        };
        let children = fs::read_dir(&root).map_err(|error| {
            format!(
                "HFT 글꼴 디렉터리를 읽을 수 없습니다: {} ({})",
                root.display(),
                error
            )
        })?;

        for child in children {
            let child = child
                .map_err(|error| format!("HFT 글꼴 디렉터리 항목을 읽을 수 없습니다: {}", error))?;
            let path = child.path();
            if !has_hft_extension(&path) {
                continue;
            }

            let Some(path) = normalize_existing_path(&path) else {
                continue;
            };
            if !path.starts_with(&root) || !seen_paths.insert(path.clone()) {
                continue;
            }

            let Some(file_name) = normalized_hft_file_name(&path) else {
                continue;
            };
            let Some(info) = metadata.get(&file_name) else {
                // Unknown HFT files are intentionally not assigned a made-up family name.
                continue;
            };
            let file_metadata = fs::metadata(&path).map_err(|error| {
                format!(
                    "HFT 글꼴 메타데이터를 읽을 수 없습니다: {} ({})",
                    path.display(),
                    error
                )
            })?;
            if !file_metadata.is_file() {
                continue;
            }

            entries.push(HftFontEntry {
                family: info.family.to_string(),
                file_name,
                language: info.language.to_string(),
                source_kind: "hancom-hft".to_string(),
                path: path.to_string_lossy().to_string(),
                byte_len: file_metadata.len(),
            });
        }
    }

    entries.sort_by(|left, right| left.file_name.cmp(&right.file_name));
    Ok(entries)
}

fn read_hft_font_from_roots(path: &Path, roots: &[PathBuf]) -> Result<Vec<u8>, String> {
    if !has_hft_extension(path) {
        return Err(format!(
            "지원하지 않는 HFT 글꼴 확장자입니다: {}",
            path.display()
        ));
    }

    let path = normalize_existing_path(path)
        .ok_or_else(|| format!("HFT 글꼴 파일을 찾을 수 없습니다: {}", path.display()))?;
    let allowed_roots = roots
        .iter()
        .filter_map(|root| normalize_existing_path(root))
        .collect::<Vec<_>>();
    if !allowed_roots.iter().any(|root| path.starts_with(root)) {
        return Err(format!(
            "허용된 HFT 글꼴 디렉터리 밖의 파일입니다: {}",
            path.display()
        ));
    }

    let file_name = normalized_hft_file_name(&path)
        .ok_or_else(|| format!("올바른 HFT 글꼴 파일명이 아닙니다: {}", path.display()))?;
    if !hft_metadata_index()?.contains_key(&file_name) {
        return Err(format!("알 수 없는 HFT 글꼴 파일입니다: {}", file_name));
    }

    let file_metadata = fs::metadata(&path).map_err(|error| {
        format!(
            "HFT 글꼴 메타데이터를 읽을 수 없습니다: {} ({})",
            path.display(),
            error
        )
    })?;
    if !file_metadata.is_file() {
        return Err(format!("HFT 글꼴 파일이 아닙니다: {}", path.display()));
    }
    if file_metadata.len() > MAX_HFT_FILE_BYTES {
        return Err(format!(
            "HFT 글꼴 파일이 안전한 읽기 한도를 초과합니다: {} ({} bytes)",
            path.display(),
            file_metadata.len()
        ));
    }

    fs::read(&path).map_err(|error| {
        format!(
            "HFT 글꼴 파일을 읽을 수 없습니다: {} ({})",
            path.display(),
            error
        )
    })
}

fn hft_metadata_index() -> Result<BTreeMap<String, HftMetadataEntry<'static>>, String> {
    let mut metadata = BTreeMap::new();
    for (index, line) in HFT_METADATA.lines().enumerate() {
        let mut columns = line.split('\t');
        let file_name = columns.next().unwrap_or_default().trim();
        let family = columns.next().unwrap_or_default().trim();
        let language = columns.next().unwrap_or_default().trim();
        if file_name.is_empty()
            || family.is_empty()
            || language.is_empty()
            || columns.next().is_some()
        {
            return Err(format!(
                "HFT 메타데이터 {}행 형식이 올바르지 않습니다",
                index + 1
            ));
        }
        let normalized = file_name.to_ascii_uppercase();
        if metadata
            .insert(normalized.clone(), HftMetadataEntry { family, language })
            .is_some()
        {
            return Err(format!(
                "HFT 메타데이터 파일명이 중복되었습니다: {}",
                normalized
            ));
        }
    }
    Ok(metadata)
}

fn normalized_hft_file_name(path: &Path) -> Option<String> {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.to_ascii_uppercase())
}

fn has_hft_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.eq_ignore_ascii_case("hft"))
        .unwrap_or(false)
}

fn normalize_existing_path(path: &Path) -> Option<PathBuf> {
    fs::canonicalize(path).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    #[test]
    fn metadata_covers_all_387_enggti_hft_files_without_duplicates() {
        let metadata = hft_metadata_index().unwrap();
        assert_eq!(metadata.len(), 387);
        assert_eq!(metadata["HCHGSMJ.HFT"].family, "# 신명조");
        assert_eq!(metadata["HCHGSMJ.HFT"].language, "hangul");
        assert_eq!(metadata["HMEPO.HFT"].family, "HCIPoppy");
        assert_eq!(metadata["YJENWDA.HFT"].family, "양재와당");
        assert_eq!(metadata["YJENINI.HFT"].family, "양재이니셜");

        let mut counts = BTreeMap::new();
        for entry in metadata.values() {
            *counts.entry(entry.language).or_insert(0usize) += 1;
        }
        assert_eq!(counts["hangul"], 110);
        assert_eq!(counts["latin"], 172);
        assert_eq!(counts["hanja"], 37);
        assert_eq!(counts["japanese"], 28);
        assert_eq!(counts["other"], 3);
        assert_eq!(counts["symbol"], 35);
        assert_eq!(counts["user"], 2);
    }

    #[test]
    fn configured_hft_dir_overrides_default_enggti_directory() {
        let configured = OsString::from("/opt/hancom/HFT");
        let home = OsString::from("/Users/test");
        assert_eq!(
            hft_font_dir_candidates(Some(configured), Some(home), true),
            vec![PathBuf::from("/opt/hancom/HFT")]
        );
        assert_eq!(
            hft_font_dir_candidates(None, Some(OsString::from("/Users/test")), true),
            vec![PathBuf::from("/Users/test/Downloads/ENGGTI")]
        );
    }

    #[test]
    fn scanner_only_indexes_known_hft_files_and_is_deterministic() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("HCHGSMJ.HFT"), b"hangul").unwrap();
        fs::write(temp.path().join("enbaskvl.hft"), b"latin").unwrap();
        fs::write(temp.path().join("UNKNOWN.HFT"), b"unknown").unwrap();
        fs::write(temp.path().join("notes.txt"), b"not a font").unwrap();

        let entries = collect_hft_font_entries_from(&[temp.path().to_path_buf()]).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].file_name, "ENBASKVL.HFT");
        assert_eq!(entries[0].family, "BaskervilleBT");
        assert_eq!(entries[0].language, "latin");
        assert_eq!(entries[1].file_name, "HCHGSMJ.HFT");
        assert_eq!(entries[1].family, "# 신명조");
        assert_eq!(entries[1].language, "hangul");
        assert!(entries
            .iter()
            .all(|entry| entry.source_kind == "hancom-hft"));
    }

    #[test]
    fn safe_read_rejects_unknown_and_outside_files() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let known = root.path().join("HGGT.HFT");
        let unknown = root.path().join("UNKNOWN.HFT");
        let escaped = outside.path().join("HGGT.HFT");
        fs::write(&known, b"known").unwrap();
        fs::write(&unknown, b"unknown").unwrap();
        fs::write(&escaped, b"outside").unwrap();

        let roots = vec![root.path().to_path_buf()];
        assert_eq!(read_hft_font_from_roots(&known, &roots).unwrap(), b"known");
        assert!(read_hft_font_from_roots(&unknown, &roots).is_err());
        assert!(read_hft_font_from_roots(&escaped, &roots).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn safe_read_rejects_symlink_escape() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let outside_font = outside.path().join("HGGT.HFT");
        fs::write(&outside_font, b"outside").unwrap();
        let link = root.path().join("HGGT.HFT");
        symlink(&outside_font, &link).unwrap();

        assert!(read_hft_font_from_roots(&link, &[root.path().to_path_buf()]).is_err());
    }

    #[test]
    fn metadata_file_names_are_ascii_case_insensitive_and_unique() {
        let metadata = hft_metadata_index().unwrap();
        let mut names = BTreeSet::new();
        for name in metadata.keys() {
            assert!(name.ends_with(".HFT"));
            assert_eq!(name, &name.to_ascii_uppercase());
            assert!(names.insert(name.to_ascii_lowercase()));
        }
    }
}
