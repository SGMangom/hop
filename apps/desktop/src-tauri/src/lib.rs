mod app_quit;
mod commands;
mod font_catalog;
mod hft_catalog;
mod hft_outline_codec;
pub(crate) mod hft_sfnt_cache;
#[cfg(target_os = "linux")]
mod linux_runtime;
#[cfg(target_os = "macos")]
mod macos_recent_documents;
#[cfg(target_os = "macos")]
mod menu;
mod pdf_export;
mod pdf_font_fallbacks;
mod pending_open;
mod recent_documents;
mod state;
#[cfg(any(target_os = "macos", windows, target_os = "linux"))]
mod updates;
mod windows;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use std::{env, ffi::OsStr};
#[cfg(target_os = "macos")]
use tauri::RunEvent;
use tauri::{AppHandle, Emitter, Manager};

use commands::{
    cancel_app_quit, check_external_modification, clear_recent_documents, close_document,
    commit_staged_document_save, commit_staged_hwp_save, create_document, create_editor_window,
    desktop_platform, destroy_current_window, export_pdf, export_pdf_from_document_path,
    export_pdf_from_hwp_path, list_hft_fonts, list_local_fonts, list_recent_documents,
    mark_document_dirty, mutate_document, note_finder_recent_document, open_document_tracking,
    prepare_document_open, prepare_staged_document_pdf_export, prepare_staged_document_save,
    prepare_staged_hwp_pdf_export, prepare_staged_hwp_save, print_webview, query_document,
    read_hft_font, read_local_font, record_recent_document, render_document_preview,
    render_page_svg, reveal_in_folder, take_pending_open_paths,
};
use state::AppState;
use updates::{get_update_state, restart_to_apply_update, start_update_install};

const HFT_RUNTIME_MANIFEST_FILE: &str = "hft-runtime-cache-v1.json";
const HFT_RUNTIME_MANIFEST_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct HftRuntimeSourceStamp {
    path: String,
    byte_len: u64,
    modified_secs: u64,
    modified_nanos: u32,
    content_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct HftRuntimeDerivedStamp {
    path: String,
    byte_len: u64,
    content_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct HftRuntimeManifest {
    version: u32,
    pipeline_hash: String,
    sources: Vec<HftRuntimeSourceStamp>,
    derived: Vec<HftRuntimeDerivedStamp>,
}

pub fn run() {
    #[cfg(target_os = "linux")]
    linux_runtime::apply_linux_runtime_fixes();

    let app = tauri::Builder::default()
        .enable_macos_default_menu(false)
        .manage(AppState::default())
        .plugin(tauri_plugin_log::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_single_instance::init(|app, args, cwd| {
            let paths = document_paths_from_args(&args, &cwd);
            if paths.is_empty() {
                return;
            }
            #[cfg(target_os = "macos")]
            queue_open_paths(app, paths);
            #[cfg(not(target_os = "macos"))]
            {
                let app = app.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    open_paths_in_new_windows(&app, paths);
                });
            }
        }))
        .setup(|app| {
            // HFT conversion is deliberately fail-soft *and* non-blocking. A
            // cold 387-font conversion can be expensive, so never hold Tauri's
            // setup/window creation path. The runtime publishes only a fully
            // verified cache and emits one refresh event afterwards; until then
            // Studio keeps the existing redistributable fallbacks.
            let hft_app = app.handle().clone();
            tauri::async_runtime::spawn_blocking(move || {
                match initialize_hft_font_runtime(&hft_app) {
                    Ok(face_count) if face_count > 0 => {
                        if let Err(error) = hft_app.emit(
                            "hop-hft-fonts-updated",
                            serde_json::json!({ "faceCount": face_count }),
                        ) {
                            eprintln!("[HOP:HFT] font refresh event failed: {error}");
                        }
                    }
                    Ok(_) => {}
                    Err(error) => {
                        eprintln!("[HOP:HFT] derived font cache unavailable: {error}");
                    }
                }
            });
            #[cfg(target_os = "macos")]
            menu::install(app)?;
            #[cfg(not(target_os = "macos"))]
            app.set_menu(tauri::menu::Menu::new(app)?)?;
            #[cfg(not(target_os = "macos"))]
            queue_open_paths(app.handle(), startup_document_paths());
            if let Some(window) = app.get_webview_window("main") {
                windows::install_editor_window_minimum(&window);
                windows::attach_document_drop_handler(app.handle(), &window);
            }
            #[cfg(any(target_os = "macos", windows, target_os = "linux"))]
            updates::install_startup_update_check(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            create_document,
            create_editor_window,
            close_document,
            mark_document_dirty,
            render_page_svg,
            query_document,
            mutate_document,
            export_pdf,
            export_pdf_from_document_path,
            export_pdf_from_hwp_path,
            print_webview,
            destroy_current_window,
            cancel_app_quit,
            desktop_platform,
            list_hft_fonts,
            list_local_fonts,
            read_hft_font,
            read_local_font,
            prepare_document_open,
            open_document_tracking,
            prepare_staged_hwp_pdf_export,
            prepare_staged_document_pdf_export,
            prepare_staged_hwp_save,
            commit_staged_hwp_save,
            prepare_staged_document_save,
            commit_staged_document_save,
            check_external_modification,
            take_pending_open_paths,
            reveal_in_folder,
            list_recent_documents,
            clear_recent_documents,
            record_recent_document,
            note_finder_recent_document,
            render_document_preview,
            get_update_state,
            start_update_install,
            restart_to_apply_update,
        ])
        .build(tauri::generate_context!())
        .expect("failed to build HOP desktop app");

    app.run(|_app, _event| {
        #[cfg(target_os = "macos")]
        {
            let app = _app;
            let event = _event;

            if let RunEvent::Opened { urls } = &event {
                let paths = urls
                    .clone()
                    .into_iter()
                    .filter_map(|url| url.to_file_path().ok())
                    .filter_map(document_path_from_path)
                    .collect();
                queue_open_paths(app, paths);
            }

            if let Err(error) = app_quit::handle_run_event(app, &event) {
                eprintln!("[quit] 앱 종료 흐름 처리 실패: {}", error);
            }
        }
    });
}

fn initialize_hft_font_runtime(app: &AppHandle) -> Result<usize, String> {
    let source_entries = hft_catalog::collect_desktop_hft_font_entries()?;
    if source_entries.is_empty() {
        return Ok(0);
    }
    let source_stamps = hft_runtime_source_stamps(&source_entries)?;
    let pipeline_hash = hft_runtime_pipeline_hash();
    let app_cache_dir = app
        .path()
        .app_cache_dir()
        .map_err(|error| format!("앱 캐시 디렉터리를 확인할 수 없습니다: {error}"))?;
    let manifest_path = app_cache_dir.join(HFT_RUNTIME_MANIFEST_FILE);

    if let Some(paths) = reusable_hft_runtime_paths(
        &manifest_path,
        &app_cache_dir,
        &source_stamps,
        &pipeline_hash,
    ) {
        let cache_root = common_parent_dir(&paths).ok_or_else(|| {
            "HFT derived font cache manifest의 경로 구성이 올바르지 않습니다".to_string()
        })?;
        prune_stale_hft_sfnts(&cache_root, &paths)?;
        font_catalog::install_hft_derived_font_cache(&cache_root, &paths)?;
        return Ok(paths.len());
    }

    let derived = hft_sfnt_cache::materialize_desktop_hft_sfnt_cache(app)?;
    if derived.is_empty() {
        return Ok(0);
    }

    let paths = derived
        .iter()
        .map(|font| PathBuf::from(&font.derived_path))
        .collect::<Vec<_>>();
    let cache_root = common_parent_dir(&paths).ok_or_else(|| {
        "HFT derived font 파일들이 하나의 private cache에 있지 않습니다".to_string()
    })?;
    prune_stale_hft_sfnts(&cache_root, &paths)?;
    font_catalog::install_hft_derived_font_cache(&cache_root, &paths)?;
    write_hft_runtime_manifest(
        &manifest_path,
        HftRuntimeManifest {
            version: HFT_RUNTIME_MANIFEST_VERSION,
            pipeline_hash,
            sources: source_stamps,
            derived: hft_runtime_derived_stamps(&paths)?,
        },
    )?;
    Ok(paths.len())
}

fn hft_runtime_source_stamps(
    entries: &[hft_catalog::HftFontEntry],
) -> Result<Vec<HftRuntimeSourceStamp>, String> {
    entries
        .iter()
        .map(|entry| {
            let metadata = fs::metadata(&entry.path).map_err(|error| {
                format!("HFT source metadata 확인 실패 ({}): {error}", entry.path)
            })?;
            let modified = metadata
                .modified()
                .map_err(|error| {
                    format!("HFT source 수정시각 확인 실패 ({}): {error}", entry.path)
                })?
                .duration_since(UNIX_EPOCH)
                .map_err(|error| {
                    format!(
                        "HFT source 수정시각이 유효하지 않습니다 ({}): {error}",
                        entry.path
                    )
                })?;
            if metadata.len() != entry.byte_len {
                return Err(format!(
                    "HFT source 크기가 catalog 이후 변경되었습니다: {}",
                    entry.path
                ));
            }
            Ok(HftRuntimeSourceStamp {
                path: entry.path.clone(),
                byte_len: entry.byte_len,
                modified_secs: modified.as_secs(),
                modified_nanos: modified.subsec_nanos(),
                content_hash: sha256_file(Path::new(&entry.path))?,
            })
        })
        .collect()
}

fn hft_runtime_derived_stamps(paths: &[PathBuf]) -> Result<Vec<HftRuntimeDerivedStamp>, String> {
    paths
        .iter()
        .map(|path| {
            let metadata = fs::metadata(path).map_err(|error| {
                format!(
                    "HFT derived font metadata 확인 실패 ({}): {error}",
                    path.display()
                )
            })?;
            if !metadata.is_file() {
                return Err(format!(
                    "HFT derived font 경로가 파일이 아닙니다: {}",
                    path.display()
                ));
            }
            Ok(HftRuntimeDerivedStamp {
                path: path.to_string_lossy().to_string(),
                byte_len: metadata.len(),
                content_hash: sha256_file(path)?,
            })
        })
        .collect()
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path)
        .map_err(|error| format!("SHA-256 입력 파일 열기 실패 ({}): {error}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| {
            format!("SHA-256 입력 파일 읽기 실패 ({}): {error}", path.display())
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let digest = hasher.finalize();
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn hft_runtime_pipeline_hash() -> String {
    let mut hasher = Sha256::new();
    hasher.update(include_bytes!("hft_sfnt_cache.rs"));
    hasher.update([0]);
    hasher.update(include_bytes!("hft_outline_codec.rs"));
    hasher.update([0]);
    hasher.update(include_bytes!("hft_metadata.tsv"));
    let digest = hasher.finalize();
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn reusable_hft_runtime_paths(
    manifest_path: &Path,
    app_cache_dir: &Path,
    expected_sources: &[HftRuntimeSourceStamp],
    expected_pipeline_hash: &str,
) -> Option<Vec<PathBuf>> {
    let raw = fs::read(manifest_path).ok()?;
    let manifest: HftRuntimeManifest = serde_json::from_slice(&raw).ok()?;
    if manifest.version != HFT_RUNTIME_MANIFEST_VERSION
        || manifest.pipeline_hash != expected_pipeline_hash
        || manifest.sources != expected_sources
        || manifest.derived.is_empty()
    {
        return None;
    }

    let app_cache_dir = fs::canonicalize(app_cache_dir).ok()?;
    let manifest_modified = fs::metadata(manifest_path).ok()?.modified().ok()?;
    let paths = manifest
        .derived
        .iter()
        .map(|stamp| {
            let path = PathBuf::from(&stamp.path);
            let metadata = fs::metadata(&path).ok()?;
            if !metadata.is_file() || metadata.len() != stamp.byte_len {
                return None;
            }
            // Derived SFNTs live in HOP's private converter-owned cache. Rehashing
            // every generated face (~1.6 GiB for the current ENGGTI set) made a
            // warm launch spend tens of seconds at 100% CPU. The source HFTs are
            // still SHA-256 checked above. For derived outputs, reject anything
            // modified after the atomically-written manifest and cheaply verify
            // the standard-font signature before trusting the exact face.
            if metadata.modified().ok()? > manifest_modified || !has_sfnt_signature(&path) {
                return None;
            }
            fs::canonicalize(path).ok()
        })
        .collect::<Option<Vec<_>>>()?;
    let cache_root = fs::canonicalize(common_parent_dir(&paths)?).ok()?;
    if !cache_root.starts_with(&app_cache_dir) {
        return None;
    }
    if paths.iter().any(|path| {
        !path.is_file()
            || !path.starts_with(&cache_root)
            || !path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("ttf"))
    }) {
        return None;
    }
    Some(paths)
}

fn has_sfnt_signature(path: &Path) -> bool {
    let Ok(mut file) = fs::File::open(path) else {
        return false;
    };
    let mut signature = [0u8; 4];
    if file.read_exact(&mut signature).is_err() {
        return false;
    }
    matches!(
        &signature,
        b"\x00\x01\x00\x00" | b"OTTO" | b"true" | b"typ1"
    )
}

fn write_hft_runtime_manifest(path: &Path, manifest: HftRuntimeManifest) -> Result<(), String> {
    let parent = path.parent().ok_or_else(|| {
        format!(
            "HFT runtime manifest 상위 경로가 없습니다: {}",
            path.display()
        )
    })?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("HFT runtime manifest 디렉터리 생성 실패: {error}"))?;
    let bytes = serde_json::to_vec(&manifest)
        .map_err(|error| format!("HFT runtime manifest 직렬화 실패: {error}"))?;
    state::atomic_write(path, &bytes)
}

fn common_parent_dir(paths: &[PathBuf]) -> Option<PathBuf> {
    let first = paths.first()?.parent()?.to_path_buf();
    paths
        .iter()
        .all(|path| path.parent() == Some(first.as_path()))
        .then_some(first)
}

/// Source hashes are part of derived filenames.  On an HFT update a new file is
/// therefore created; remove older converter-owned SFNTs before publishing the
/// directory through RHWP_FONT_PATH so fontdb cannot nondeterministically pick
/// a stale face with the same family/style.
fn prune_stale_hft_sfnts(cache_root: &Path, active_paths: &[PathBuf]) -> Result<(), String> {
    let cache_root = fs::canonicalize(cache_root)
        .map_err(|error| format!("HFT cache directory 확인 실패: {error}"))?;
    let active = active_paths
        .iter()
        .map(|path| {
            fs::canonicalize(path).map_err(|error| {
                format!("HFT derived font 확인 실패 ({}): {error}", path.display())
            })
        })
        .collect::<Result<BTreeSet<_>, _>>()?;

    for entry in fs::read_dir(&cache_root)
        .map_err(|error| format!("HFT cache directory 읽기 실패: {error}"))?
    {
        let entry = entry.map_err(|error| format!("HFT cache entry 읽기 실패: {error}"))?;
        let path = entry.path();
        if !path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("ttf"))
        {
            continue;
        }
        let canonical = fs::canonicalize(&path)
            .map_err(|error| format!("HFT cache font 확인 실패 ({}): {error}", path.display()))?;
        if active.contains(&canonical) {
            continue;
        }
        fs::remove_file(&path).map_err(|error| {
            format!(
                "stale HFT cache font 제거 실패 ({}): {error}",
                path.display()
            )
        })?;
    }
    Ok(())
}

fn queue_open_paths(app: &AppHandle, paths: Vec<String>) {
    if paths.is_empty() {
        return;
    }

    app.state::<AppState>()
        .pending_open_paths
        .queue_global(paths.iter().cloned());

    let payload = serde_json::json!({ "paths": paths });
    if let Some(label) = crate::windows::target_window_label(app) {
        let _ = app.emit_to(label, "hop-open-paths", payload);
    } else {
        let _ = app.emit("hop-open-paths", payload);
    }
}

#[cfg(not(target_os = "macos"))]
fn open_paths_in_new_windows(app: &AppHandle, paths: Vec<String>) {
    for path in paths {
        if let Err(error) = open_path_in_new_window(app, path) {
            eprintln!("[open] 새 창 파일 열기 준비 실패: {}", error);
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn open_path_in_new_window(app: &AppHandle, path: String) -> Result<(), String> {
    let label = crate::windows::new_editor_window_label();
    app.state::<AppState>()
        .pending_open_paths
        .queue_for_window(&label, [path]);
    if let Err(error) = crate::windows::create_editor_window_with_label(app, &label) {
        app.state::<AppState>()
            .pending_open_paths
            .discard_for_window(&label);
        return Err(error);
    }
    Ok(())
}

fn document_paths_from_args(args: &[String], cwd: &str) -> Vec<String> {
    let cwd = Path::new(cwd);
    args.iter()
        .skip(1)
        .filter_map(|arg| document_path_from_os_arg(OsStr::new(arg), Some(cwd)))
        .collect()
}

#[cfg(not(target_os = "macos"))]
fn startup_document_paths() -> Vec<String> {
    let cwd = env::current_dir().ok();
    env::args_os()
        .skip(1)
        .filter_map(|arg| document_path_from_os_arg(&arg, cwd.as_deref()))
        .collect()
}

fn document_path_from_os_arg(arg: &OsStr, cwd: Option<&Path>) -> Option<String> {
    if let Some(arg) = arg.to_str() {
        if let Ok(url) = tauri::Url::parse(arg) {
            if let Ok(path) = url.to_file_path() {
                return document_path_from_path(path);
            }
        }
    }

    let path = PathBuf::from(arg);
    let resolved = match cwd {
        Some(cwd) if !path.is_absolute() => cwd.join(path),
        _ => path,
    };
    document_path_from_path(resolved)
}

pub(crate) fn document_path_from_path(path: impl AsRef<Path>) -> Option<String> {
    let path = path.as_ref();
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    if ext != "hwp" && ext != "hwpx" {
        return None;
    }
    Some(path.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_path_from_path_accepts_hwp_and_hwpx_case_insensitively() {
        assert!(document_path_from_path(PathBuf::from("/tmp/doc.hwp")).is_some());
        assert!(document_path_from_path(PathBuf::from("/tmp/doc.HWPX")).is_some());
    }

    #[test]
    fn document_path_from_path_rejects_other_extensions() {
        assert!(document_path_from_path(PathBuf::from("/tmp/doc.pdf")).is_none());
        assert!(document_path_from_path(PathBuf::from("/tmp/doc")).is_none());
    }

    #[test]
    fn document_path_from_arg_resolves_relative_paths_against_cwd() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path();
        let expected = dir.path().join("docs/sample.hwp");

        assert_eq!(
            document_path_from_os_arg(OsStr::new("docs/sample.hwp"), Some(cwd)),
            Some(expected.to_string_lossy().to_string())
        );
    }

    #[test]
    fn document_path_from_arg_accepts_file_urls() {
        let path = std::env::temp_dir().join("sample.hwpx");
        let url = tauri::Url::from_file_path(&path).unwrap().to_string();

        assert_eq!(
            document_path_from_os_arg(OsStr::new(&url), Some(Path::new("/ignored"))),
            Some(path.to_string_lossy().to_string())
        );
    }

    #[test]
    fn document_paths_from_args_skip_executable_and_filter_unsupported_args() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().to_string_lossy();
        let paths = document_paths_from_args(
            &[
                dir.path().join("HOP.exe").to_string_lossy().to_string(),
                "first.hwp".to_string(),
                "notes.txt".to_string(),
                "second.HWPX".to_string(),
            ],
            &cwd,
        );

        assert_eq!(
            paths,
            vec![
                dir.path().join("first.hwp").to_string_lossy().to_string(),
                dir.path().join("second.HWPX").to_string_lossy().to_string()
            ]
        );
    }

    #[test]
    fn startup_like_args_skip_the_executable_path() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path();
        let executable = dir.path().join("sample.hwp");
        let document = dir.path().join("opened.hwpx");
        let args = [executable.as_os_str(), document.as_os_str()];

        let paths = args
            .iter()
            .skip(1)
            .filter_map(|arg| document_path_from_os_arg(arg, Some(cwd)))
            .collect::<Vec<_>>();

        assert_eq!(paths, vec![document.to_string_lossy().to_string()]);
    }

    #[test]
    fn prune_stale_hft_sfnts_keeps_only_active_converter_outputs() {
        let temp = tempfile::tempdir().unwrap();
        let active = temp.path().join("active.ttf");
        let stale = temp.path().join("stale.ttf");
        let unrelated = temp.path().join("keep.txt");
        fs::write(&active, b"active").unwrap();
        fs::write(&stale, b"stale").unwrap();
        fs::write(&unrelated, b"unrelated").unwrap();

        prune_stale_hft_sfnts(temp.path(), std::slice::from_ref(&active)).unwrap();

        assert!(active.is_file());
        assert!(!stale.exists());
        assert!(unrelated.is_file());
    }

    #[test]
    fn hft_runtime_manifest_reuses_only_matching_private_cache_outputs() {
        let temp = tempfile::tempdir().unwrap();
        let app_cache = temp.path().join("app-cache");
        let font_cache = app_cache.join("hft-sfnt-v1");
        fs::create_dir_all(&font_cache).unwrap();
        let derived = font_cache.join("merged.ttf");
        fs::write(&derived, b"\0\x01\0\0derived-sfnt-test").unwrap();
        let manifest_path = app_cache.join(HFT_RUNTIME_MANIFEST_FILE);
        let sources = vec![HftRuntimeSourceStamp {
            path: "/private/source/HGMJ.HFT".to_string(),
            byte_len: 1234,
            modified_secs: 100,
            modified_nanos: 200,
            content_hash: "source-hash".to_string(),
        }];
        let manifest = HftRuntimeManifest {
            version: HFT_RUNTIME_MANIFEST_VERSION,
            pipeline_hash: "pipeline-v1".to_string(),
            sources: sources.clone(),
            derived: vec![HftRuntimeDerivedStamp {
                path: derived.to_string_lossy().to_string(),
                byte_len: fs::metadata(&derived).unwrap().len(),
                content_hash: sha256_file(&derived).unwrap(),
            }],
        };
        write_hft_runtime_manifest(&manifest_path, manifest).unwrap();

        let reused =
            reusable_hft_runtime_paths(&manifest_path, &app_cache, &sources, "pipeline-v1")
                .expect("matching runtime manifest should be reusable");
        assert_eq!(reused, vec![fs::canonicalize(&derived).unwrap()]);

        let mut changed_sources = sources.clone();
        changed_sources[0].modified_nanos += 1;
        assert!(reusable_hft_runtime_paths(
            &manifest_path,
            &app_cache,
            &changed_sources,
            "pipeline-v1",
        )
        .is_none());
        assert!(
            reusable_hft_runtime_paths(&manifest_path, &app_cache, &sources, "pipeline-v2",)
                .is_none()
        );

        fs::write(&derived, b"BAD!derived-sfnt-test").unwrap();
        assert!(
            reusable_hft_runtime_paths(&manifest_path, &app_cache, &sources, "pipeline-v1",)
                .is_none()
        );
    }

    #[test]
    fn hft_runtime_manifest_rejects_derived_paths_outside_app_cache() {
        let temp = tempfile::tempdir().unwrap();
        let app_cache = temp.path().join("app-cache");
        let outside = temp.path().join("outside");
        fs::create_dir_all(&app_cache).unwrap();
        fs::create_dir_all(&outside).unwrap();
        let derived = outside.join("fake.ttf");
        fs::write(&derived, b"\0\x01\0\0not-a-trusted-cache-face").unwrap();
        let manifest_path = app_cache.join(HFT_RUNTIME_MANIFEST_FILE);
        let sources = vec![];
        write_hft_runtime_manifest(
            &manifest_path,
            HftRuntimeManifest {
                version: HFT_RUNTIME_MANIFEST_VERSION,
                pipeline_hash: "pipeline-v1".to_string(),
                sources: sources.clone(),
                derived: vec![HftRuntimeDerivedStamp {
                    path: derived.to_string_lossy().to_string(),
                    byte_len: fs::metadata(&derived).unwrap().len(),
                    content_hash: sha256_file(&derived).unwrap(),
                }],
            },
        )
        .unwrap();

        assert!(
            reusable_hft_runtime_paths(&manifest_path, &app_cache, &sources, "pipeline-v1",)
                .is_none()
        );
    }
}
