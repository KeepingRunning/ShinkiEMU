//! Desktop platform services: native dialogs through `rfd`, the per-user data
//! directory, and in-place process restart.
use std::path::PathBuf;

use super::{FileFilter, PickDone};

pub(super) fn pick_folder(title: &str, done: PickDone) {
    if let Some(path) = rfd::FileDialog::new().set_title(title).pick_folder() {
        done(Ok(path));
    }
}

pub(super) fn pick_file(title: &str, filters: &[FileFilter<'_>], done: PickDone) {
    let mut dialog = rfd::FileDialog::new().set_title(title);
    for filter in filters {
        dialog = dialog.add_filter(filter.label, filter.extensions);
    }
    if let Some(path) = dialog.pick_file() {
        done(Ok(path));
    }
}

/// Select the parent with the native folder UI, then propose an unused child.
/// The patch transaction creates the child only when conversion succeeds.
pub(super) fn pick_new_directory(title: &str, default_name: &str, done: PickDone) {
    if let Some(parent) = rfd::FileDialog::new().set_title(title).pick_folder() {
        done(new_directory_in(&parent, default_name));
    }
}

fn new_directory_in(parent: &std::path::Path, default_name: &str) -> Result<PathBuf, String> {
    let metadata =
        std::fs::metadata(parent).map_err(|error| format!("SHINKI_EMU_PATCH_PATH: {error}"))?;
    if !metadata.is_dir() {
        return Err("SHINKI_EMU_PATCH_PATH: output location must be a directory".into());
    }
    let mut path = parent.join(default_name);
    let mut suffix = 0u64;
    loop {
        match std::fs::symlink_metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(path),
            Err(error) => return Err(format!("SHINKI_EMU_PATCH_PATH: {error}")),
            Ok(_) => {
                suffix = suffix.checked_add(1).ok_or("SHINKI_EMU_PATCH_BOUNDS")?;
                path = parent.join(format!("{default_name}-{suffix}"));
            }
        }
    }
}

pub(super) fn report_startup_error(message: &str) {
    rfd::MessageDialog::new()
        .set_title(shinki_emu_manager_ui_slint::tr!("ShinkiEMU 启动失败"))
        .set_description(message)
        .set_level(rfd::MessageLevel::Error)
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
}

pub(super) fn default_data_dir() -> Result<PathBuf, String> {
    directories::ProjectDirs::from("dev", "RoukakuEngine", "ShinkiEMU")
        .map(|dirs| dirs.data_dir().to_path_buf())
        .ok_or_else(|| "SHINKI_EMU_DATA_DIRECTORY_UNAVAILABLE".into())
}

pub(super) fn program_file() -> Result<PathBuf, String> {
    std::env::current_exe().map_err(|error| format!("SHINKI_EMU_EXECUTABLE_LOCATION: {error}"))
}

pub(super) fn bundled_resource_dir() -> Option<PathBuf> {
    let parent = program_file().ok()?.parent()?.to_path_buf();
    #[cfg(target_os = "macos")]
    if parent.file_name().is_some_and(|name| name == "MacOS") {
        return Some(parent.parent()?.join("Resources"));
    }
    Some(parent)
}

pub(super) fn restart() -> Result<(), String> {
    std::process::Command::new(program_file()?)
        .args(std::env::args_os().skip(1))
        .spawn()
        .map(drop)
        .map_err(|error| format!("SHINKI_EMU_RESTART_FAILED: {error}"))
}

/// Open a file or directory in the platform file manager without a shell.
pub(super) fn open_path(path: &std::path::Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let program = "explorer.exe";
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let program = "xdg-open";
    std::process::Command::new(program)
        .arg(path)
        .spawn()
        .map(drop)
        .map_err(|error| format!("SHINKI_EMU_OPEN_PATH_FAILED: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn empty_output_location_proposes_child_without_creating_it() {
        let parent = tempfile::tempdir().unwrap();
        let output = new_directory_in(parent.path(), "localized-output").unwrap();
        assert_eq!(output, parent.path().join("localized-output"));
        assert!(!output.exists());
        assert_eq!(fs::read_dir(parent.path()).unwrap().count(), 0);
    }

    #[test]
    fn occupied_output_names_are_preserved_and_skipped() {
        let parent = tempfile::tempdir().unwrap();
        fs::create_dir(parent.path().join("localized-output")).unwrap();
        fs::write(parent.path().join("localized-output-1"), b"user file").unwrap();
        fs::create_dir(parent.path().join("localized-output-2")).unwrap();
        fs::write(parent.path().join("localized-output-2/keep"), b"user data").unwrap();
        let output = new_directory_in(parent.path(), "localized-output").unwrap();
        assert_eq!(output, parent.path().join("localized-output-3"));
        assert!(!output.exists());
        assert_eq!(fs::read_dir(parent.path()).unwrap().count(), 3);
        assert_eq!(
            fs::read(parent.path().join("localized-output-1")).unwrap(),
            b"user file"
        );
        assert_eq!(
            fs::read(parent.path().join("localized-output-2/keep")).unwrap(),
            b"user data"
        );
    }

    #[test]
    fn file_cannot_be_used_as_output_location() {
        let parent = tempfile::tempdir().unwrap();
        let file = parent.path().join("file");
        fs::write(&file, b"user file").unwrap();
        assert!(new_directory_in(&file, "localized-output").is_err());
        assert_eq!(fs::read(file).unwrap(), b"user file");
    }
}
