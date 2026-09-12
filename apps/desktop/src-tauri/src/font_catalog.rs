use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};
use tauri::{AppHandle, Manager};
use unicode_normalization::UnicodeNormalization;
use usvg::fontdb::{self, Source};

/// Keep the desktop font bridge on the same caller-configured search path as
/// rhwp's native/SVG/PDF renderers (`renderer::font_paths::FONT_PATH_ENV`).
///
/// This is intentionally a directory of standard font files, not raw Hancom
/// HFT.  For full native/CanvasKit/PDF parity, the HFT converter materializes
/// one standard face per logical family/style/weight (merging legacy language
/// fragments) in a private runtime cache and points this variable at that cache
/// instead of teaching every renderer the HFT format.
const RHWP_FONT_PATH_ENV: &str = "RHWP_FONT_PATH";
/// Private HOP cache root registered only after the in-process HFT converter
/// succeeds.  Do not derive this trust bit from an externally mutable env var:
/// `hft-derived` is what permits restricted Hancom family authoring.
static HFT_DERIVED_CACHE_DIR: OnceLock<RwLock<Option<PathBuf>>> = OnceLock::new();
static HFT_DERIVED_FONT_PATHS: OnceLock<RwLock<BTreeSet<PathBuf>>> = OnceLock::new();

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LocalFontEntry {
    pub family: String,
    pub post_script_name: String,
    pub style: String,
    pub weight: u16,
    pub source_kind: String,
    pub path: Option<String>,
    pub aliases: Vec<String>,
}

pub fn desktop_extra_font_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    #[cfg(target_os = "macos")]
    {
        if let Some(home_dir) = env_path("HOME") {
            dirs.push(home_dir.join("Library/Fonts"));
        }
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(home_dir) = env_path("HOME") {
            dirs.push(home_dir.join(".local/share/fonts"));
            dirs.push(home_dir.join(".fonts"));
        }

        let wsl_windows_fonts = PathBuf::from("/mnt/c/Windows/Fonts");
        if wsl_windows_fonts.is_dir() {
            dirs.push(wsl_windows_fonts);
        }
    }

    #[cfg(windows)]
    {
        if let Some(local_app_data) = env_path("LOCALAPPDATA") {
            dirs.extend(windows_user_font_dirs(&local_app_data));
        }
    }

    dedupe_existing_dirs(dirs)
}

/// Font directories visible to the desktop webview/local-font bridge.
///
/// Native rhwp renderers already consume `RHWP_FONT_PATH` themselves.  The
/// webview did not, so a caller-provided (or future HFT-derived) SFNT cache was
/// usable by native/PDF output but invisible to CSS FontFace and CanvasKit.
/// Keep the OS discovery roots and append the same configured directories here
/// so all rendering surfaces can resolve the exact same font bytes.
fn desktop_catalog_font_dirs() -> Vec<PathBuf> {
    let mut dirs = desktop_extra_font_dirs();
    dirs.extend(configured_font_dirs());
    dedupe_existing_dirs(dirs)
}

fn configured_font_dirs() -> Vec<PathBuf> {
    let Some(raw) = std::env::var_os(RHWP_FONT_PATH_ENV) else {
        return Vec::new();
    };
    dedupe_existing_dirs(std::env::split_paths(&raw).collect())
}

fn hft_derived_cache_dirs() -> Vec<PathBuf> {
    let cache = HFT_DERIVED_CACHE_DIR
        .get_or_init(|| RwLock::new(None))
        .read()
        .ok()
        .and_then(|guard| guard.clone());
    dedupe_existing_dirs(cache.into_iter().collect())
}

/// Publish a converter-produced standard-font cache to every HOP/rhwp render
/// surface without discarding a caller supplied RHWP_FONT_PATH.
///
/// The converter owns cache creation/validation.  This function only installs
/// an already-existing directory and marks it as the sole trusted source for
/// `hft-derived` authoring entries.
pub fn install_hft_derived_font_cache(
    cache_dir: &Path,
    derived_paths: &[PathBuf],
) -> Result<PathBuf, String> {
    let cache_dir = normalize_existing_path(cache_dir).ok_or_else(|| {
        format!(
            "HFT derived font cache directory를 찾을 수 없습니다: {}",
            cache_dir.display()
        )
    })?;
    if !cache_dir.is_dir() {
        return Err(format!(
            "HFT derived font cache 경로가 디렉터리가 아닙니다: {}",
            cache_dir.display()
        ));
    }

    let mut verified_paths = BTreeSet::new();
    for path in derived_paths {
        let path = normalize_existing_path(path).ok_or_else(|| {
            format!(
                "HFT derived font 파일을 찾을 수 없습니다: {}",
                path.display()
            )
        })?;
        if !path_is_within_root(&path, &cache_dir) || !has_supported_font_extension(&path) {
            return Err(format!(
                "HFT derived font 파일이 private cache 밖에 있습니다: {}",
                path.display()
            ));
        }
        verified_paths.insert(path);
    }

    let mut rhwp_paths = std::env::var_os(RHWP_FONT_PATH_ENV)
        .map(|raw| std::env::split_paths(&raw).collect::<Vec<_>>())
        .unwrap_or_default();
    if !rhwp_paths.iter().any(|path| {
        normalize_existing_path(path)
            .as_ref()
            .is_some_and(|path| path == &cache_dir)
    }) {
        rhwp_paths.push(cache_dir.clone());
    }
    let joined = std::env::join_paths(rhwp_paths)
        .map_err(|error| format!("RHWP_FONT_PATH 구성 실패: {error}"))?;
    std::env::set_var(RHWP_FONT_PATH_ENV, joined);
    *HFT_DERIVED_CACHE_DIR
        .get_or_init(|| RwLock::new(None))
        .write()
        .map_err(|_| "HFT derived cache 상태 잠금이 손상되었습니다".to_string())? =
        Some(cache_dir.clone());
    *HFT_DERIVED_FONT_PATHS
        .get_or_init(|| RwLock::new(BTreeSet::new()))
        .write()
        .map_err(|_| "HFT derived font 상태 잠금이 손상되었습니다".to_string())? = verified_paths;
    Ok(cache_dir)
}

pub fn pdf_font_dirs(app: &AppHandle) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(resource_dir) = app.path().resource_dir() {
        dirs.push(resource_dir.join("fonts/pdf"));
    }
    dirs.extend(desktop_extra_font_dirs());
    // Searchable PDF builds its own fontdb from this explicit list, so merely
    // setting RHWP_FONT_PATH for the native renderer is insufficient.
    dirs.extend(configured_font_dirs());
    dedupe_existing_dirs(dirs)
}

pub fn collect_desktop_local_font_entries() -> Vec<LocalFontEntry> {
    collect_local_font_entries(&desktop_catalog_font_dirs())
}

/// Normalized exact aliases -> canonical SFNT family name for faces backed by
/// the verified private HFT cache.  PDF/native consumers can use the canonical
/// value when a document spells a face as e.g. `HCI Poppy` while the derived
/// name table contains `HCIPoppy`.
pub fn hft_derived_font_family_map() -> BTreeMap<String, String> {
    let mut aliases = BTreeMap::new();
    for entry in collect_local_font_entries(&hft_derived_cache_dirs())
        .into_iter()
        .filter(|entry| entry.source_kind == "hft-derived")
    {
        for name in std::iter::once(&entry.family)
            .chain(std::iter::once(&entry.post_script_name))
            .chain(entry.aliases.iter())
        {
            let key = font_family_alias_key(name);
            if !key.is_empty() {
                aliases.entry(key).or_insert_with(|| entry.family.clone());
            }
        }
    }
    aliases
}

fn hft_derived_font_paths() -> BTreeSet<PathBuf> {
    HFT_DERIVED_FONT_PATHS
        .get_or_init(|| RwLock::new(BTreeSet::new()))
        .read()
        .map(|guard| guard.clone())
        .unwrap_or_default()
}

pub fn read_desktop_local_font(path: &Path) -> Result<Vec<u8>, String> {
    let path = normalize_existing_path(path)
        .ok_or_else(|| format!("로컬 폰트 파일을 찾을 수 없습니다: {}", path.display()))?;

    if !has_supported_font_extension(&path) {
        return Err(format!(
            "지원하지 않는 로컬 폰트 확장자입니다: {}",
            path.display()
        ));
    }

    let allowed_roots = desktop_catalog_font_dirs();
    if !allowed_roots
        .iter()
        .any(|root| path_is_within_root(&path, root))
    {
        return Err(format!(
            "지원된 로컬 폰트 디렉터리 밖의 파일입니다: {}",
            path.display()
        ));
    }

    fs::read(&path).map_err(|error| {
        format!(
            "로컬 폰트 파일을 읽을 수 없습니다: {} ({})",
            path.display(),
            error
        )
    })
}

pub fn create_font_database(extra_font_dirs: &[PathBuf]) -> fontdb::Database {
    let mut fontdb = fontdb::Database::new();
    fontdb.load_system_fonts();

    for dir in extra_font_dirs {
        if dir.is_dir() {
            fontdb.load_fonts_dir(dir);
        }
    }

    fontdb
}

pub fn collect_local_font_entries(extra_font_dirs: &[PathBuf]) -> Vec<LocalFontEntry> {
    let fontdb = create_font_database(extra_font_dirs);
    let file_backed_dirs = extra_font_dirs
        .iter()
        .filter_map(|dir| normalize_existing_path(dir))
        .collect::<Vec<_>>();
    let hft_derived_dirs = hft_derived_cache_dirs();
    let hft_derived_paths = hft_derived_font_paths();

    let mut seen = BTreeSet::new();
    let mut entries = Vec::new();

    for face in fontdb.faces() {
        let path = source_path(&face.source);
        let source_kind = classify_source(
            path.as_deref(),
            &file_backed_dirs,
            &hft_derived_dirs,
            &hft_derived_paths,
        );
        let style = style_name(face.style);

        let mut families = BTreeSet::new();
        for (family, _) in &face.families {
            let family = family.trim();
            if !family.is_empty() {
                families.insert(family.to_string());
            }
        }

        for family in families {
            let key = (
                family.clone(),
                face.post_script_name.clone(),
                style,
                face.weight.0,
                source_kind,
                path.clone(),
            );
            if !seen.insert(key) {
                continue;
            }

            entries.push(LocalFontEntry {
                aliases: if source_kind == "hft-derived" {
                    hft_exact_family_aliases(&family)
                } else {
                    Vec::new()
                },
                family,
                post_script_name: face.post_script_name.clone(),
                style: style.to_string(),
                weight: face.weight.0,
                source_kind: source_kind.to_string(),
                path: path.clone(),
            });
        }
    }

    entries.sort_by(|left, right| {
        left.family
            .cmp(&right.family)
            .then(left.weight.cmp(&right.weight))
            .then(left.style.cmp(&right.style))
            .then(left.post_script_name.cmp(&right.post_script_name))
    });
    entries
}

/// Names that are already treated by the native renderer as the same Hancom
/// face, not general FontMap fallback substitutions.  Keep this deliberately
/// narrow: only a converter-trusted HFT-derived entry receives these aliases.
fn hft_exact_family_aliases(family: &str) -> Vec<String> {
    let aliases: &[&str] = match family {
        "한양신명조" => &["HY신명조", "HYSinMyeongJo-Medium"],
        "한양중고딕" => &["HY중고딕", "HYGothic-Medium"],
        "한양견고딕" => &["HY견고딕", "HYGothic-Extra"],
        "한양그래픽" => &["HY그래픽", "HYGraphic-Medium"],
        "한양견명조" => &["HY견명조", "HYMyeongJo-Extra"],
        _ => &[],
    };
    aliases.iter().map(|alias| (*alias).to_string()).collect()
}

fn source_path(source: &Source) -> Option<String> {
    match source {
        Source::File(path) | Source::SharedFile(path, _) => {
            Some(path.to_string_lossy().to_string())
        }
        Source::Binary(_) => None,
    }
}

fn classify_source(
    path: Option<&str>,
    file_backed_dirs: &[PathBuf],
    hft_derived_dirs: &[PathBuf],
    hft_derived_paths: &BTreeSet<PathBuf>,
) -> &'static str {
    let Some(path) = path else {
        return "system-installed";
    };
    let normalized_path = normalize_existing_path(Path::new(path));
    let path = normalized_path
        .as_deref()
        .unwrap_or_else(|| Path::new(path));
    if hft_derived_dirs.iter().any(|dir| path.starts_with(dir)) && hft_derived_paths.contains(path)
    {
        "hft-derived"
    } else if file_backed_dirs.iter().any(|dir| path.starts_with(dir)) {
        "file-backed"
    } else {
        "system-installed"
    }
}

fn style_name(style: fontdb::Style) -> &'static str {
    match style {
        fontdb::Style::Normal => "normal",
        fontdb::Style::Italic => "italic",
        fontdb::Style::Oblique => "oblique",
    }
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from)
}

#[cfg(any(windows, test))]
fn windows_user_font_dirs(local_app_data: &Path) -> Vec<PathBuf> {
    vec![local_app_data.join("Microsoft/Windows/Fonts")]
}

fn dedupe_existing_dirs(dirs: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = BTreeSet::new();
    let mut deduped = Vec::new();

    for dir in dirs {
        let Some(normalized_dir) = normalize_existing_path(&dir) else {
            continue;
        };
        if seen.insert(normalized_dir.clone()) {
            deduped.push(normalized_dir);
        }
    }

    deduped
}

fn normalize_existing_path(path: &Path) -> Option<PathBuf> {
    fs::canonicalize(path).ok()
}

fn path_is_within_root(path: &Path, root: &Path) -> bool {
    let Some(normalized_path) = normalize_existing_path(path) else {
        return false;
    };
    let Some(normalized_root) = normalize_existing_path(root) else {
        return false;
    };
    normalized_path.starts_with(&normalized_root)
}

fn has_supported_font_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "ttf" | "otf" | "ttc" | "otc" | "woff" | "woff2"
            )
        })
        .unwrap_or(false)
}

pub(crate) fn font_family_alias_key(value: &str) -> String {
    value
        .trim_matches(|ch: char| ch == '\'' || ch == '"' || ch == '\0')
        .nfc()
        .filter(|ch| !ch.is_whitespace() && *ch != '_' && *ch != '-')
        .flat_map(char::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::sync::Mutex;
    use usvg::fontdb::{FaceInfo, Stretch, Weight, ID};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvRestore {
        name: &'static str,
        value: Option<OsString>,
    }

    impl EnvRestore {
        fn capture(name: &'static str) -> Self {
            Self {
                name,
                value: std::env::var_os(name),
            }
        }
    }

    impl Drop for EnvRestore {
        fn drop(&mut self) {
            if let Some(value) = &self.value {
                std::env::set_var(self.name, value);
            } else {
                std::env::remove_var(self.name);
            }
        }
    }

    #[test]
    fn collect_local_font_entries_keeps_localized_family_aliases() {
        let faces = vec![FaceInfo {
            id: ID::dummy(),
            source: Source::File(PathBuf::from("/Library/Fonts/Test.ttf")),
            index: 0,
            families: vec![
                (
                    "Malgun Gothic".to_string(),
                    fontdb::Language::English_UnitedStates,
                ),
                (
                    "맑은 고딕".to_string(),
                    fontdb::Language::English_UnitedStates,
                ),
            ],
            post_script_name: "MalgunGothicRegular".to_string(),
            style: fontdb::Style::Normal,
            weight: Weight::NORMAL,
            stretch: Stretch::Normal,
            monospaced: false,
        }];

        let mut seen = BTreeSet::new();
        let mut entries = Vec::new();
        for face in &faces {
            let path = source_path(&face.source);
            for (family, _) in &face.families {
                let key = (
                    family.clone(),
                    face.post_script_name.clone(),
                    style_name(face.style),
                    face.weight.0,
                    "system-installed",
                    path.clone(),
                );
                if seen.insert(key) {
                    entries.push(LocalFontEntry {
                        family: family.clone(),
                        post_script_name: face.post_script_name.clone(),
                        style: style_name(face.style).to_string(),
                        weight: face.weight.0,
                        source_kind: "system-installed".to_string(),
                        path: path.clone(),
                        aliases: Vec::new(),
                    });
                }
            }
        }

        let families = entries
            .into_iter()
            .map(|entry| entry.family)
            .collect::<Vec<_>>();
        assert_eq!(
            families,
            vec!["Malgun Gothic".to_string(), "맑은 고딕".to_string()]
        );
    }

    #[test]
    fn classify_source_marks_extra_dirs_as_file_backed() {
        let extra_dir = PathBuf::from("/opt/hancom/Shared/TTF");
        assert_eq!(
            classify_source(
                Some("/opt/hancom/Shared/TTF/HYHeadLine.ttf"),
                &[extra_dir],
                &[],
                &BTreeSet::new(),
            ),
            "file-backed"
        );
        assert_eq!(
            classify_source(
                Some("/System/Library/Fonts/Supplemental/Apple SD Gothic Neo.ttc"),
                &[],
                &[],
                &BTreeSet::new(),
            ),
            "system-installed"
        );
    }

    #[test]
    fn install_hft_cache_preserves_existing_rhwp_path_and_marks_only_cache_as_derived() {
        let _guard = ENV_LOCK.lock().unwrap();
        let _restore_rhwp = EnvRestore::capture(RHWP_FONT_PATH_ENV);
        let temp = tempfile::tempdir().unwrap();
        let existing = temp.path().join("existing-fonts");
        let cache = temp.path().join("derived-hft-fonts");
        fs::create_dir_all(&existing).unwrap();
        fs::create_dir_all(&cache).unwrap();
        fs::write(existing.join("Ordinary.ttf"), b"test").unwrap();
        fs::write(cache.join("HancomDerived.ttf"), b"test").unwrap();
        fs::write(cache.join("Untrusted.ttf"), b"test").unwrap();
        std::env::set_var(RHWP_FONT_PATH_ENV, &existing);

        let derived_path = cache.join("HancomDerived.ttf");
        let installed =
            install_hft_derived_font_cache(&cache, std::slice::from_ref(&derived_path)).unwrap();

        assert_eq!(installed, fs::canonicalize(&cache).unwrap());
        assert_eq!(
            configured_font_dirs(),
            vec![
                fs::canonicalize(&existing).unwrap(),
                fs::canonicalize(&cache).unwrap(),
            ]
        );
        assert_eq!(
            hft_derived_cache_dirs(),
            vec![fs::canonicalize(&cache).unwrap()]
        );
        assert_eq!(
            classify_source(
                cache.join("HancomDerived.ttf").to_str(),
                &[
                    fs::canonicalize(&existing).unwrap(),
                    fs::canonicalize(&cache).unwrap()
                ],
                &hft_derived_cache_dirs(),
                &hft_derived_font_paths(),
            ),
            "hft-derived"
        );
        assert_eq!(
            classify_source(
                existing.join("Ordinary.ttf").to_str(),
                &[
                    fs::canonicalize(&existing).unwrap(),
                    fs::canonicalize(&cache).unwrap()
                ],
                &hft_derived_cache_dirs(),
                &hft_derived_font_paths(),
            ),
            "file-backed"
        );
        assert_eq!(
            classify_source(
                cache.join("Untrusted.ttf").to_str(),
                &[
                    fs::canonicalize(&existing).unwrap(),
                    fs::canonicalize(&cache).unwrap()
                ],
                &hft_derived_cache_dirs(),
                &hft_derived_font_paths(),
            ),
            "file-backed"
        );
    }

    #[test]
    fn font_family_alias_key_matches_hwp_hft_spacing_variants() {
        assert_eq!(
            font_family_alias_key("HCI Poppy"),
            font_family_alias_key("HCIPoppy")
        );
        assert_eq!(
            font_family_alias_key("Baskerville BT"),
            font_family_alias_key("BaskervilleBT")
        );
        assert_eq!(font_family_alias_key("HY_Head-Line"), "hyheadline");
        assert_eq!(
            font_family_alias_key("휴먼명조"),
            font_family_alias_key("휴먼명조")
        );
    }

    #[test]
    fn exact_hft_aliases_cover_native_hanyang_face_names_without_general_fallbacks() {
        assert_eq!(
            hft_exact_family_aliases("한양신명조"),
            vec!["HY신명조".to_string(), "HYSinMyeongJo-Medium".to_string()]
        );
        assert_eq!(
            hft_exact_family_aliases("한양중고딕"),
            vec!["HY중고딕".to_string(), "HYGothic-Medium".to_string()]
        );
        assert!(hft_exact_family_aliases("명조").is_empty());
    }

    #[test]
    fn windows_user_font_dirs_use_local_app_data_root() {
        let root = PathBuf::from("C:/Users/test/AppData/Local");
        assert_eq!(
            windows_user_font_dirs(&root),
            vec![PathBuf::from(
                "C:/Users/test/AppData/Local/Microsoft/Windows/Fonts"
            )]
        );
    }

    #[test]
    fn path_is_within_root_rejects_escape_paths() {
        let temp = tempfile::tempdir().unwrap();
        let fonts_root = temp.path().join("fonts");
        let outside_root = temp.path().join("outside");
        fs::create_dir_all(&fonts_root).unwrap();
        fs::create_dir_all(&outside_root).unwrap();

        let allowed_font = fonts_root.join("test.ttf");
        let outside_font = outside_root.join("test.ttf");
        fs::write(&allowed_font, b"font").unwrap();
        fs::write(&outside_font, b"font").unwrap();

        assert!(path_is_within_root(&allowed_font, &fonts_root));
        assert!(!path_is_within_root(&outside_font, &fonts_root));
        assert!(has_supported_font_extension(&allowed_font));
        assert!(!has_supported_font_extension(
            &fonts_root.join("legacy.HFT")
        ));
        assert!(!has_supported_font_extension(
            &outside_root.join("notes.txt")
        ));
    }

    #[test]
    fn configured_font_dirs_follow_rhwp_font_path_and_filter_missing_entries() {
        let _guard = ENV_LOCK.lock().unwrap();
        let _restore = EnvRestore::capture(RHWP_FONT_PATH_ENV);
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("font-cache-a");
        let second = temp.path().join("font-cache-b");
        let missing = temp.path().join("missing");
        fs::create_dir_all(&first).unwrap();
        fs::create_dir_all(&second).unwrap();

        let joined = std::env::join_paths([&first, &missing, &second, &first]).unwrap();
        std::env::set_var(RHWP_FONT_PATH_ENV, joined);

        assert_eq!(
            configured_font_dirs(),
            vec![
                fs::canonicalize(first).unwrap(),
                fs::canonicalize(second).unwrap()
            ],
        );
    }

    #[test]
    fn read_desktop_local_font_accepts_standard_font_cache_from_rhwp_font_path() {
        let _guard = ENV_LOCK.lock().unwrap();
        let _restore = EnvRestore::capture(RHWP_FONT_PATH_ENV);
        let temp = tempfile::tempdir().unwrap();
        let cache = temp.path().join("derived-font-cache");
        fs::create_dir_all(&cache).unwrap();
        let derived = cache.join("DerivedFromHft.ttf");
        fs::write(&derived, b"synthetic-test-font-bytes").unwrap();
        std::env::set_var(RHWP_FONT_PATH_ENV, &cache);

        assert_eq!(
            read_desktop_local_font(&derived).unwrap(),
            b"synthetic-test-font-bytes"
        );
    }

    #[test]
    fn collect_local_font_entries_loads_real_sfnt_from_derived_cache_dir() {
        let temp = tempfile::tempdir().unwrap();
        let cache = temp.path().join("derived-font-cache");
        fs::create_dir_all(&cache).unwrap();
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../assets/fonts/computer-modern/source/cmex10.ttf");
        let cached_font = cache.join("Cmex10.ttf");
        fs::copy(source, &cached_font).unwrap();

        let entries = collect_local_font_entries(&[cache]);
        let entry = entries
            .iter()
            .find(|entry| entry.post_script_name == "Cmex10" && entry.source_kind == "file-backed")
            .expect("derived-cache SFNT face should be catalogued");

        assert_eq!(entry.family, "cmex10");
        assert_eq!(
            entry.path.as_deref(),
            Some(cached_font.to_string_lossy().as_ref())
        );
    }
}
