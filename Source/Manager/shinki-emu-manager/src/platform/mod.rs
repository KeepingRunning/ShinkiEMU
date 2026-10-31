//! Platform services the Manager host needs beyond Slint: file and folder
//! pickers, the per-user data directory, bundled resources, opening a location
//! in the system file manager, startup error reporting and restart.
//!
//! Desktop uses native dialogs and executable-relative resources. Android and
//! OHOS use app-private directories and native app lifecycle integration.
use std::path::{Path, PathBuf};

#[cfg(target_os = "android")]
pub mod android;
#[cfg(not(any(target_os = "android", target_os = "ios", target_env = "ohos")))]
mod desktop;
#[cfg(target_env = "ohos")]
pub mod ohos;
#[cfg(target_os = "android")]
use android as imp;
#[cfg(not(any(target_os = "android", target_os = "ios", target_env = "ohos")))]
use desktop as imp;
#[cfg(target_env = "ohos")]
use ohos as imp;
#[cfg(target_os = "ios")]
mod ios;
#[cfg(target_os = "ios")]
use ios as imp;

/// A file type offered by a file picker.
#[derive(Clone, Copy, Debug)]
pub struct FileFilter<'a> {
    pub label: &'a str,
    pub extensions: &'a [&'a str],
}

/// Receives a picked path. A cancelled picker never calls it; a picker that
/// failed reports `Err` with a diagnostic code.
pub type PickDone = Box<dyn FnOnce(Result<PathBuf, String>)>;

/// Pick an existing directory.
pub fn pick_folder(title: &str, done: PickDone) {
    imp::pick_folder(title, done);
}

/// Pick an existing file whose extension matches one of `filters`.
pub fn pick_file(title: &str, filters: &[FileFilter<'_>], done: PickDone) {
    imp::pick_file(title, filters, done);
}

/// Choose a location for a new directory without creating it. Desktop selects
/// its parent and proposes an unused `default_name` child; mobile platforms
/// resolve a writable destination through their native/sandbox services.
/// The returned path is a proposal, not a reservation: callers must publish
/// without replacing anything that appeared there after selection.
pub fn pick_new_directory(title: &str, default_name: &str, done: PickDone) {
    imp::pick_new_directory(title, default_name, done);
}

/// Show a startup failure that happened before the Manager window exists.
pub fn report_startup_error(message: &str) {
    imp::report_startup_error(message);
}

/// The platform's per-user application data directory.
pub fn default_data_dir() -> Result<PathBuf, String> {
    imp::default_data_dir()
}

/// The file holding the running Manager code: the executable on desktop, the
/// extracted `libshinki_emu_manager.so` on Android. Its directory holds the
/// bundled native runtime files (for example the DXC compiler).
pub fn program_file() -> Result<PathBuf, String> {
    imp::program_file()
}

/// Directory with the resources shipped next to the Manager (`metadata/`,
/// `THIRD_PARTY_NOTICES.md`).
pub fn bundled_resource_dir() -> Option<PathBuf> {
    imp::bundled_resource_dir()
}

/// Start a new Manager process once this one exits.
pub fn restart() -> Result<(), String> {
    imp::restart()
}

/// Open a file or directory in the platform file manager without a shell.
pub fn open_path(path: &Path) -> Result<(), String> {
    imp::open_path(path)
}

/// Whether `path` has one of the extensions in `filters` (case-insensitive).
pub fn matches_filters(path: &Path, filters: &[FileFilter<'_>]) -> bool {
    filters.is_empty()
        || path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                filters.iter().any(|filter| {
                    filter
                        .extensions
                        .iter()
                        .any(|allowed| allowed.eq_ignore_ascii_case(extension))
                })
            })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_match_extensions_case_insensitively() {
        let filters = [FileFilter {
            label: "Luau",
            extensions: &["luau"],
        }];
        assert!(matches_filters(Path::new("/a/patch.LUAU"), &filters));
        assert!(!matches_filters(Path::new("/a/patch.lua"), &filters));
        assert!(!matches_filters(Path::new("/a/patch"), &filters));
        assert!(matches_filters(Path::new("/a/anything"), &[]));
    }
}
