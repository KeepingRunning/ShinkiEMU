//! UIKit file-provider authorization and app-sandbox resources. The winit
//! backend owns UIApplication on the main thread; no desktop processes run here.
use super::{FileFilter, PickDone};
use std::path::{Path, PathBuf};

fn sandbox() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "SHINKI_EMU_IOS_SANDBOX_HOME_MISSING".into())
}
fn bookmarks() -> Result<PathBuf, String> {
    Ok(sandbox()?.join("Library/Application Support/org.shinkirou.shinkiemu/document-bookmarks"))
}
fn pick(title: &str, folder: bool, done: PickDone) {
    let directory = match bookmarks() {
        Ok(path) => path,
        Err(error) => {
            done(Err(error));
            return;
        }
    };
    roukaku_platform::ios::pick_document(
        title,
        folder,
        &directory,
        Box::new(move |result| match result {
            Ok(Some(path)) => done(Ok(path)),
            Ok(None) => {} // Cancellation follows the platform PickDone contract.
            Err(error) => done(Err(error.to_string())),
        }),
    );
}
pub(super) fn pick_folder(title: &str, done: PickDone) {
    pick(title, true, done);
}
pub(super) fn pick_file(title: &str, filters: &[FileFilter<'_>], done: PickDone) {
    let extensions: Vec<String> = filters
        .iter()
        .flat_map(|filter| {
            filter
                .extensions
                .iter()
                .map(|extension| (*extension).to_owned())
        })
        .collect();
    pick(
        title,
        false,
        Box::new(move |result| {
            done(result.and_then(|path| {
                if extensions.is_empty()
                    || path
                        .extension()
                        .and_then(|value| value.to_str())
                        .is_some_and(|value| {
                            extensions
                                .iter()
                                .any(|allowed| allowed.eq_ignore_ascii_case(value))
                        })
                {
                    Ok(path)
                } else {
                    Err("SHINKI_EMU_IOS_FILE_TYPE_NOT_ALLOWED".into())
                }
            }));
        }),
    );
}
pub(super) fn pick_new_directory(title: &str, default_name: &str, done: PickDone) {
    if default_name.is_empty()
        || default_name.contains(['/', '\\', ':'])
        || matches!(default_name, "." | "..")
    {
        done(Err("SHINKI_EMU_IOS_INVALID_DIRECTORY_NAME".into()));
        return;
    }
    let name = default_name.to_owned();
    pick(
        title,
        true,
        Box::new(move |result| {
            done(result.and_then(|parent| {
                let path = parent.join(name);
                if path.exists() {
                    Err("SHINKI_EMU_IOS_DIRECTORY_ALREADY_EXISTS".into())
                } else {
                    Ok(path)
                }
            }))
        }),
    );
}
pub(super) fn default_data_dir() -> Result<PathBuf, String> {
    let directory = sandbox()?.join("Library/Application Support/org.shinkirou.shinkiemu");
    roukaku_platform::ios::activate_audio().map_err(|error| error.to_string())?;
    roukaku_platform::ios::restore_document_access(&bookmarks()?)
        .map_err(|error| error.to_string())?;
    Ok(directory)
}
pub(super) fn program_file() -> Result<PathBuf, String> {
    std::env::current_exe().map_err(|error| format!("SHINKI_EMU_IOS_EXECUTABLE_LOCATION: {error}"))
}
pub(super) fn bundled_resource_dir() -> Option<PathBuf> {
    Some(program_file().ok()?.parent()?.to_owned())
}
pub(super) fn report_startup_error(message: &str) {
    tracing::error!(event = "shinki.emu.ios.startup_failed", error = message);
    eprintln!("ShinkiEMU: {message}");
}
pub(super) fn restart() -> Result<(), String> {
    Err("SHINKI_EMU_IOS_RESTART: close and reopen the app using the system app switcher; iOS does not permit process spawning".into())
}
pub(super) fn open_path(path: &Path) -> Result<(), String> {
    roukaku_platform::ios::open_document(path).map_err(|error| error.to_string())
}

pub(super) fn open_url(url: &str) -> Result<(), String> {
    use objc2_foundation::{MainThreadMarker, NSDictionary, NSString, NSURL};
    use objc2_ui_kit::UIApplication;
    let mtm = MainThreadMarker::new().ok_or("SHINKI_EMU_IOS_URL_MAIN_THREAD")?;
    let url = NSURL::URLWithString(&NSString::from_str(url)).ok_or("SHINKI_EMU_MANAGER_UPDATE_URL")?;
    let application = UIApplication::sharedApplication(mtm);
    if !application.canOpenURL(&url) {
        return Err("SHINKI_EMU_OPEN_URL_FAILED: no browser".into());
    }
    // SAFETY: an empty dictionary has no incorrectly typed option values.
    unsafe { application.openURL_options_completionHandler(&url, &NSDictionary::new(), None) };
    Ok(())
}
