//! Content lives in the app's own folder, not in AppData.
//!
//! `content_root` resolves to:
//!   * `<project>/content/` during debug builds, or
//!   * a `content/` folder next to the executable in release builds.
//! `PS3XMB_CONTENT` is a debug-build-only override.
//!
//! Under it: `Games/`, `Music/`, `Photos/`, `Videos/`, `Themes/`, and a
//! `data/` folder for `settings.json`, `users.json`, `psn.json`.

use std::fs;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use tauri::AppHandle;

fn ensure(p: PathBuf) -> PathBuf {
    let _ = fs::create_dir_all(&p);
    p
}

pub(crate) fn path_key(path: &Path) -> String {
    let path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let key = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) { key.to_lowercase() } else { key }
}

pub(crate) fn deduplicate_paths(paths: &mut Vec<String>) -> bool {
    let mut seen = HashSet::new();
    let original_len = paths.len();
    paths.retain(|path| seen.insert(path_key(Path::new(path))));
    paths.len() != original_len
}

fn dev_project_root(exe_dir: &Path) -> Option<PathBuf> {
    let target_dir = exe_dir.parent()?;
    if target_dir.file_name()? != "target" {
        return None;
    }

    let src_tauri_dir = target_dir.parent()?;
    if src_tauri_dir.file_name()? != "src-tauri" {
        return None;
    }

    src_tauri_dir.parent().map(Path::to_path_buf)
}

fn content_root_for(exe_dir: &Path, development_build: bool) -> PathBuf {
    if development_build {
        if let Some(project) = dev_project_root(exe_dir) {
            return project.join("content");
        }
    }
    exe_dir.join("content")
}

fn settings_portable_root(settings: &serde_json::Value, current_root: &Path) -> Option<PathBuf> {
    let expected = [
        ("libraryPaths", "Games"),
        ("musicPaths", "Music"),
        ("photoPaths", "Photos"),
        ("videoPaths", "Videos"),
        ("subtitlePaths", "Subtitles"),
    ];
    let current_key = path_key(current_root);
    let mut candidates: HashMap<String, (PathBuf, usize)> = HashMap::new();
    for (field, folder) in expected {
        let Some(paths) = settings.get(field).and_then(serde_json::Value::as_array) else {
            continue;
        };
        for value in paths.iter().filter_map(serde_json::Value::as_str) {
            let path = Path::new(value);
            if !path.file_name().map(|name| name.eq_ignore_ascii_case(folder)).unwrap_or(false) {
                continue;
            }
            let Some(content) = path.parent() else { continue };
            if !content.file_name().map(|name| name.eq_ignore_ascii_case("content")).unwrap_or(false) {
                continue;
            }
            let Some(old_root) = content.parent() else { continue };
            let key = path_key(old_root);
            if key == current_key || old_root.starts_with(current_root) { continue; }
            candidates.entry(key)
                .and_modify(|(_, count)| *count += 1)
                .or_insert_with(|| (old_root.to_path_buf(), 1));
        }
    }

    candidates.into_values()
        .filter(|(_, count)| *count >= 2)
        .max_by_key(|(_, count)| *count)
        .map(|(root, _)| root)
}

fn normalize_managed_path(value: &str, install_root: &Path, folder: &str) -> Option<String> {
    let path = Path::new(value);
    let relative = path.strip_prefix(install_root).ok()?;
    let mut components = relative.components();
    if !components.next()?.as_os_str().to_string_lossy().eq_ignore_ascii_case("content") {
        return None;
    }
    let mut content_levels = 1;
    while components.as_path().components().next()
        .map(|component| component.as_os_str().to_string_lossy().eq_ignore_ascii_case("content"))
        .unwrap_or(false)
    {
        components.next();
        content_levels += 1;
    }
    if content_levels < 2
        || !components.next()?.as_os_str().to_string_lossy().eq_ignore_ascii_case(folder)
    {
        return None;
    }
    let mut normalized = install_root.join("content").join(folder);
    for component in components {
        normalized.push(component.as_os_str());
    }
    Some(normalized.display().to_string())
}

fn normalize_settings_paths(settings: &mut serde_json::Value, install_root: &Path) -> bool {
    let fields = [
        ("libraryPaths", "Games"),
        ("musicPaths", "Music"),
        ("photoPaths", "Photos"),
        ("videoPaths", "Videos"),
        ("subtitlePaths", "Subtitles"),
    ];
    let mut changed = false;
    for (field, folder) in fields {
        let Some(paths) = settings.get_mut(field).and_then(serde_json::Value::as_array_mut) else {
            continue;
        };
        for index in 0..paths.len() {
            let Some(path) = paths[index].as_str().map(str::to_string) else { continue };
            if let Some(normalized) = normalize_managed_path(&path, install_root, folder) {
                paths[index] = serde_json::Value::String(normalized);
                changed = true;
            }
        }
        let mut seen = HashSet::new();
        let len = paths.len();
        paths.retain(|value| {
            value.as_str().map(|path| seen.insert(path_key(Path::new(path)))).unwrap_or(true)
        });
        changed |= paths.len() != len;
    }
    changed
}

fn rebase_path(value: &str, old_root: &Path, new_root: &Path) -> Option<String> {
    let value = value.replace('\\', "/");
    let old = old_root.to_string_lossy().replace('\\', "/");
    let prefix = value.get(..old.len())?;
    if !prefix.eq_ignore_ascii_case(&old) {
        return None;
    }
    let suffix = value.get(old.len()..)?;
    if !suffix.is_empty() && !suffix.starts_with('/') {
        return None;
    }
    let suffix = suffix.replace('/', &std::path::MAIN_SEPARATOR.to_string());
    Some(format!("{}{}", new_root.display(), suffix))
}

fn rebase_json_paths(value: &mut serde_json::Value, old_root: &Path, new_root: &Path) -> bool {
    match value {
        serde_json::Value::String(path) => {
            if let Some(rebased) = rebase_path(path, old_root, new_root) {
                *path = rebased;
                true
            } else {
                false
            }
        }
        serde_json::Value::Array(items) => {
            items.iter_mut().fold(false, |changed, item| {
                rebase_json_paths(item, old_root, new_root) || changed
            })
        }
        serde_json::Value::Object(fields) => {
            fields.values_mut().fold(false, |changed, item| {
                rebase_json_paths(item, old_root, new_root) || changed
            })
        }
        _ => false,
    }
}

fn rebase_json_file(file: &Path, old_root: &Path, new_root: &Path) -> Result<bool, String> {
    if !file.is_file() { return Ok(false); }
    let text = fs::read_to_string(file).map_err(|e| format!("reading {}: {e}", file.display()))?;
    let mut value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| format!("parsing {}: {e}", file.display()))?;
    if !rebase_json_paths(&mut value, old_root, new_root) { return Ok(false); }
    let text = serde_json::to_string_pretty(&value)
        .map_err(|e| format!("serializing {}: {e}", file.display()))?;
    fs::write(file, text).map_err(|e| format!("writing {}: {e}", file.display()))?;
    Ok(true)
}

/// Rebase paths saved under an older location of this portable folder.
/// External game and media paths are left untouched.
pub fn relocate_portable_data(app: &AppHandle) -> Result<bool, String> {
    let content = content_root(app);
    let root = content.parent().unwrap_or(&content);
    let settings_file = settings_file(app);
    if !settings_file.is_file() { return Ok(false); }
    let text = fs::read_to_string(&settings_file)
        .map_err(|e| format!("reading {}: {e}", settings_file.display()))?;
    let mut settings: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| format!("parsing {}: {e}", settings_file.display()))?;
    let old_root = settings_portable_root(&settings, root);
    let mut changed = false;
    if let Some(old_root) = old_root {
        changed |= rebase_json_paths(&mut settings, &old_root, root);
        let serialized = serde_json::to_string_pretty(&settings)
            .map_err(|e| format!("serializing {}: {e}", settings_file.display()))?;
        fs::write(&settings_file, serialized)
            .map_err(|e| format!("writing {}: {e}", settings_file.display()))?;
        changed |= rebase_json_file(&game_sources_file(app), &old_root, root)?;
    }
    changed |= normalize_settings_paths(&mut settings, root);
    if changed {
        let serialized = serde_json::to_string_pretty(&settings)
            .map_err(|e| format!("serializing {}: {e}", settings_file.display()))?;
        fs::write(&settings_file, serialized)
            .map_err(|e| format!("writing {}: {e}", settings_file.display()))?;
    }
    Ok(changed)
}

pub fn content_root(_app: &AppHandle) -> PathBuf {
    if cfg!(debug_assertions) {
        if let Ok(p) = std::env::var("PS3XMB_CONTENT") {
            return ensure(PathBuf::from(p));
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            return ensure(content_root_for(dir, cfg!(debug_assertions)));
        }
    }
    if cfg!(debug_assertions) {
        return ensure(PathBuf::from("content"));
    }
    ensure(std::env::temp_dir().join("MaXMB").join("content"))
}

#[cfg(test)]
mod tests {
    use super::dev_project_root;
    use std::path::{Path, PathBuf};

    #[test]
    fn detects_standard_dev_profile_directory() {
        let exe_dir = Path::new(r"C:\MaXMB\src-tauri\target\release");
        assert_eq!(
            dev_project_root(exe_dir),
            Some(PathBuf::from(r"C:\MaXMB"))
        );
    }

    #[test]
    fn does_not_treat_portable_folder_under_target_as_dev_build() {
        let exe_dir = Path::new(r"C:\MaXMB\src-tauri\target\release\portable");
        assert_eq!(
            super::content_root_for(exe_dir, false),
            PathBuf::from(r"C:\MaXMB\src-tauri\target\release\portable\content")
        );
    }

    #[test]
    fn release_build_in_target_directory_does_not_use_project_content() {
        let exe_dir = Path::new(r"C:\MaXMB\src-tauri\target\release");
        assert_eq!(
            super::content_root_for(exe_dir, false),
            PathBuf::from(r"C:\MaXMB\src-tauri\target\release\content")
        );
    }

    #[test]
    fn deduplicates_windows_paths_with_different_separators_and_case() {
        let mut paths = vec![
            r"C:\MaXMB-Portable\content\Games".to_string(),
            "c:/maxmb-portable/content/games".to_string(),
        ];
        assert!(super::deduplicate_paths(&mut paths));
        assert_eq!(paths.len(), 1);
    }

    #[test]
    fn detects_old_portable_root_from_multiple_app_library_paths() {
        let settings = serde_json::json!({
            "libraryPaths": [r"C:\Old\MaXMB\content\Games"],
            "musicPaths": [r"C:\Old\MaXMB\content\Music"],
            "photoPaths": [r"E:\Photos"]
        });
        assert_eq!(
            super::settings_portable_root(&settings, Path::new(r"E:\CrossMediaBar\content")),
            Some(PathBuf::from(r"C:\Old\MaXMB"))
        );
    }

    #[test]
    fn portable_path_rebasing_keeps_external_paths_unchanged() {
        let old = Path::new(r"C:\Old\MaXMB");
        let new = Path::new(r"E:\CrossMediaBar");
        assert_eq!(
            super::rebase_path(r"C:\Old\MaXMB\content\Games\Game", old, new),
            Some(r"E:\CrossMediaBar\content\Games\Game".to_string())
        );
        assert_eq!(
            super::rebase_path(r"E:\Games\Game", old, new),
            None
        );
    }

    #[test]
    fn portable_path_rebasing_does_not_panic_on_unrelated_unicode_strings() {
        let old = Path::new(r"C:\Old\MaXMB");
        let new = Path::new(r"E:\CrossMediaBar");
        assert_eq!(super::rebase_path("abcdefghijkéZ", old, new), None);
    }

    #[test]
    fn rebases_nested_json_paths_without_changing_external_paths() {
        let old = Path::new(r"C:\Old\MaXMB");
        let new = Path::new(r"E:\CrossMediaBar");
        let mut value = serde_json::json!({
            "libraryPaths": [r"C:\Old\MaXMB\content\Games", r"E:\Games"],
            "entries": [{ "folder": r"C:\Old\MaXMB\content\Games\A Game" }],
            "external": r"C:\Other\Game.exe"
        });

        assert!(super::rebase_json_paths(&mut value, old, new));
        assert_eq!(
            value["libraryPaths"][0],
            r"E:\CrossMediaBar\content\Games"
        );
        assert_eq!(value["libraryPaths"][1], r"E:\Games");
        assert_eq!(
            value["entries"][0]["folder"],
            r"E:\CrossMediaBar\content\Games\A Game"
        );
        assert_eq!(value["external"], r"C:\Other\Game.exe");
    }

    #[test]
    fn normalizes_duplicate_content_roots_without_touching_external_paths() {
        let root = Path::new(r"E:\CrossMediaBar");
        let mut settings = serde_json::json!({
            "libraryPaths": [
                r"E:\CrossMediaBar\content\content\Games",
                r"E:\CrossMediaBar\content\content\content\Games",
                r"E:\Other\Games"
            ],
            "musicPaths": [
                r"E:\CrossMediaBar\content\content\Music",
                r"E:\CrossMediaBar\content\Music"
            ]
        });
        assert!(super::normalize_settings_paths(&mut settings, root));
        assert_eq!(settings["libraryPaths"], serde_json::json!([
            r"E:\CrossMediaBar\content\Games",
            r"E:\Other\Games"
        ]));
        assert_eq!(settings["musicPaths"], serde_json::json!([
            r"E:\CrossMediaBar\content\Music"
        ]));
        assert!(!super::normalize_settings_paths(&mut settings, root));
    }

    #[test]
    fn does_not_treat_nested_content_paths_as_an_old_install_root() {
        let root = Path::new(r"E:\CrossMediaBar");
        let settings = serde_json::json!({
            "libraryPaths": [
                r"E:\CrossMediaBar\content\content\Games",
                r"E:\CrossMediaBar\content\content\content\Games"
            ],
            "musicPaths": [
                r"E:\CrossMediaBar\content\content\Music",
                r"E:\CrossMediaBar\content\content\content\Music"
            ]
        });
        assert_eq!(super::settings_portable_root(&settings, root), None);
    }

    #[test]
    fn move_tree_keeps_source_when_a_destination_file_conflicts() {
        let root = std::env::temp_dir().join(format!("xmb_move_tree_conflict_{}", std::process::id()));
        let old = root.join("old");
        let new = root.join("new");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(old.join("cache.bin"), b"old").unwrap();
        std::fs::write(new.join("cache.bin"), b"new").unwrap();

        assert!(super::move_tree(&old, &new).is_err());
        assert_eq!(std::fs::read(old.join("cache.bin")).unwrap(), b"old");
        assert_eq!(std::fs::read(new.join("cache.bin")).unwrap(), b"new");
        let _ = std::fs::remove_dir_all(&root);
    }
}

pub fn data_dir(app: &AppHandle) -> PathBuf {
    ensure(content_root(app).join("data"))
}

/// Cache folder for extracted embedded music cover art.
pub fn covers_dir(app: &AppHandle) -> PathBuf { ensure(data_dir(app).join("covers")) }
/// Small copies of photos and song covers (see thumbs.rs): the temporary
/// folder, or the system directory with "Save Cache in System Directory".
pub fn thumbs_dirs(app: &AppHandle) -> crate::thumbs::Dirs {
    crate::thumbs::Dirs { temp: temp_dir(app).join("thumbs"), app: data_dir(app).join("cache").join("thumbs") }
}
/// Videos converted for playback, kept in the system directory.
pub fn video_cache_app_dir(app: &AppHandle) -> PathBuf { ensure(data_dir(app).join("cache").join("video")) }
/// Icons chosen for videos ("Change Icon" in the video player).
pub fn video_icons_dir(app: &AppHandle) -> PathBuf { ensure(data_dir(app).join("video_icons")) }
/// Saved icon shading (the glossy icon images), in the system directory.
pub fn icon_shades_app_file(app: &AppHandle) -> PathBuf { ensure(data_dir(app).join("cache")).join("icon_shades.json") }
/// The same, in the temporary folder (the default).
pub fn icon_shades_temp_file(app: &AppHandle) -> PathBuf { temp_dir(app).join("icon_shades.json") }

/// Persistent folder (inside the app's own directory) for decoded song data,
/// used when "Keep Preloaded Songs Data" is on so preloaded songs survive a
/// restart instead of living in the OS temp dir.
pub fn preloaded_songs_dir(app: &AppHandle) -> PathBuf { ensure(data_dir(app).join("preloaded_songs")) }

/// Icons pulled out of shortcuts (game folders that only hold a shortcut).
pub fn shortcut_icons_dir(app: &AppHandle) -> PathBuf { ensure(data_dir(app).join("shortcut_icons")) }
/// Record of the games created from the game source folders.
pub fn game_sources_file(app: &AppHandle) -> PathBuf { data_dir(app).join("game_sources.json") }
/// The temporary place for caches: Windows' own Temp folder (%TEMP%, a
/// "MaXMB" folder in it), cleared when the system turns off with Delete
/// Temporary Files on; the kept caches stay in the system's own directory.
pub fn temp_dir(_app: &AppHandle) -> PathBuf { ensure(std::env::temp_dir().join("MaXMB")) }
/// The temporary folder of older versions (inside the system's own directory):
/// what's in it moves to the Temp folder once (see `move_old_temp`).
pub fn old_temp_dir(app: &AppHandle) -> PathBuf { data_dir(app).join("temp") }
/// Once, at start: the old temporary folder's files go to the Temp folder (moved
/// when it's on the same drive; otherwise left out, they were only temporary).
/// (also the Temp folder's "PS3 XMB" folder, from before the name MaXMB)
pub fn move_old_temp(app: &AppHandle) {
    let new = temp_dir(app);
    for old in [old_temp_dir(app), std::env::temp_dir().join("PS3 XMB")] {
        if old.is_dir() {
            if let Err(error) = move_tree(&old, &new) {
                crate::warn_log!("Could not finish moving temporary files from {}: {error}", old.display());
            }
        }
    }
}
fn move_tree(old: &Path, new: &Path) -> std::io::Result<()> {
    fn mv(from: &Path, to: &Path) -> std::io::Result<()> {
        fs::create_dir_all(to)?;
        for entry in fs::read_dir(from)? {
            let entry = entry?;
            let source = entry.path();
            let target = to.join(entry.file_name());
            if source.is_dir() {
                mv(&source, &target)?;
                fs::remove_dir(&source)?;
            } else {
                if target.exists() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::AlreadyExists,
                        format!("destination already exists: {}", target.display()),
                    ));
                }
                fs::rename(&source, &target)?;
            }
        }
        fs::remove_dir(from)
    }
    mv(old, new)
}
/// The webview's own storage (WebView2 profile), also kept in the app folder.
pub fn webview_dir(app: &AppHandle) -> PathBuf { ensure(data_dir(app).join("webview")) }

pub fn settings_file(app: &AppHandle) -> PathBuf { data_dir(app).join("settings.json") }
pub fn users_file(app: &AppHandle) -> PathBuf { data_dir(app).join("users.json") }
pub fn psn_file(app: &AppHandle) -> PathBuf { data_dir(app).join("psn.json") }

/// Your own PSN avatars, read at startup (any picture, any file name).
pub fn psn_avatars_dir(app: &AppHandle) -> PathBuf { ensure(content_root(app).join("Avatars").join("PSN")) }
pub fn games_dir(app: &AppHandle) -> PathBuf { ensure(content_root(app).join("Games")) }
pub fn music_dir(app: &AppHandle) -> PathBuf { ensure(content_root(app).join("Music")) }
pub fn photos_dir(app: &AppHandle) -> PathBuf { ensure(content_root(app).join("Photos")) }
pub fn videos_dir(app: &AppHandle) -> PathBuf { ensure(content_root(app).join("Videos")) }
pub fn subtitles_dir(app: &AppHandle) -> PathBuf { ensure(content_root(app).join("Subtitles")) }
pub fn themes_dir(app: &AppHandle) -> PathBuf { ensure(content_root(app).join("Themes")) }
/// Icon Themes: one folder each, with the replacement icons and a .json naming it.
pub fn icon_themes_dir(app: &AppHandle) -> PathBuf { ensure(content_root(app).join("Icon")) }
/// Sound Effects packs (a folder each: sounds named like the system's, and a .json with the pack's name).
pub fn sound_effects_dir(app: &AppHandle) -> PathBuf { ensure(content_root(app).join("Sound Effects")) }
/// Where the PS3 themes (.p3t) are unpacked (made again when needed).
pub fn p3t_cache_dir(app: &AppHandle) -> PathBuf { ensure(data_dir(app).join("cache").join("p3t")) }
/// Where Custom Themes (.zip) are unpacked.
pub fn custom_theme_cache_dir(app: &AppHandle) -> PathBuf { ensure(data_dir(app).join("cache").join("custom_themes")) }
/// Where extracted PS3 themes go (Extract Theme). (It was called
/// "ThemeExtraction": a folder by that name is renamed, when there's no
/// "Themes Extracted" yet.)
pub fn theme_extraction_dir(app: &AppHandle) -> PathBuf {
    let dir = content_root(app).join("Themes Extracted");
    let old = content_root(app).join("ThemeExtraction");
    if !dir.exists() && old.is_dir() { let _ = fs::rename(&old, &dir); }
    ensure(dir)
}
/// Fonts the user can pick in Theme Settings > Font (.ttf / .otf / .woff / .woff2).
pub fn fonts_dir(app: &AppHandle) -> PathBuf { ensure(content_root(app).join("Fonts")) }
/// Holds the bundled LibreWolf (content/Browser/librewolf), the managed profile,
/// the generated helper extension, policies.json, and any user-dropped .xpi.
pub fn browser_dir(app: &AppHandle) -> PathBuf { ensure(content_root(app).join("Browser")) }

/// Create the whole content tree on first run.
pub fn ensure_content_tree(app: &AppHandle) {
    games_dir(app); music_dir(app); photos_dir(app); videos_dir(app); subtitles_dir(app); themes_dir(app); icon_themes_dir(app); sound_effects_dir(app);
    browser_dir(app);
    // the old (unused) BrowserScripts folder is removed when it's empty
    let _ = fs::remove_dir(content_root(app).join("BrowserScripts"));
    data_dir(app);
    retire_backgrounds_folder(app);
    // the old ThemeExtraction folder gets its new name at start
    if content_root(app).join("ThemeExtraction").is_dir() { theme_extraction_dir(app); }
    remove_old_controls_txt(app);
}

/// Older versions had a separate Backgrounds folder. Backgrounds now come from
/// Photos / Videos, so move anything left in it into Photos and remove it.
fn retire_backgrounds_folder(app: &AppHandle) {
    let old = content_root(app).join("Backgrounds");
    if !old.is_dir() { return; }
    let photos = photos_dir(app);
    if let Ok(rd) = fs::read_dir(&old) {
        for e in rd.flatten() {
            let mut dest = photos.join(e.file_name());
            let mut n = 1;
            while dest.exists() {
                let stem = e.path().file_stem().map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
                let ext = e.path().extension().map(|x| format!(".{}", x.to_string_lossy())).unwrap_or_default();
                dest = photos.join(format!("{stem} ({n}){ext}"));
                n += 1;
            }
            let _ = fs::rename(e.path(), dest);
        }
    }
    let _ = fs::remove_dir(&old);
}

/// Older versions wrote a CONTROLS.txt next to the content folder; the
/// controls are in the Manual now, so that copy is removed.
fn remove_old_controls_txt(app: &AppHandle) {
    let root = content_root(app);
    let Some(dir) = root.parent() else { return };
    let _ = std::fs::remove_file(dir.join("CONTROLS.txt"));
}
