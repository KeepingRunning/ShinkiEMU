use crate::{Error, Result};
use shinki_emu_abi as abi;
use std::{
    fs,
    path::{Path, PathBuf},
};

pub(crate) fn prepare_output(source: &Path, output: &Path, baseline: Option<&Path>) -> Result<()> {
    let source = fs::canonicalize(source)?;
    let output = resolved_destination(output)?;
    if output.starts_with(&source) {
        return Err(Error::Run(
            "output must be outside the read-only source game".into(),
        ));
    }
    if let Some(baseline) = baseline {
        if fs::canonicalize(baseline)? == output {
            return Err(Error::Run(
                "output and baseline directories must differ".into(),
            ));
        }
    }
    fs::create_dir_all(output)?;
    Ok(())
}
fn resolved_destination(path: &Path) -> Result<PathBuf> {
    let absolute = std::path::absolute(path)?;
    let mut existing = absolute.as_path();
    let mut missing = Vec::new();
    while !existing.exists() {
        missing.push(
            existing
                .file_name()
                .ok_or_else(|| Error::Run("invalid output directory".into()))?
                .to_owned(),
        );
        existing = existing
            .parent()
            .ok_or_else(|| Error::Run("invalid output directory".into()))?;
    }
    let mut resolved = fs::canonicalize(existing)?;
    for part in missing.into_iter().rev() {
        resolved.push(part);
    }
    Ok(resolved)
}

pub(crate) fn copy_game(source: &Path, destination: &Path) -> Result<()> {
    if !source.is_dir() {
        return Err(Error::Run("game-dir must be a directory".into()));
    }
    fs::create_dir(destination)?;
    copy_tree(source, destination)
}
fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let to = destination.join(entry.file_name());
        if kind.is_symlink() {
            return Err(Error::Run(format!(
                "game copy rejects symbolic links: {}",
                entry.path().display()
            )));
        }
        if kind.is_dir() {
            fs::create_dir(&to)?;
            copy_tree(&entry.path(), &to)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), &to)?;
        } else {
            return Err(Error::Run("game copy contains a special file".into()));
        }
    }
    Ok(())
}

pub(crate) fn library(source: &Path, extracted: &Path) -> Result<PathBuf> {
    let root = if source
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
    {
        fs::create_dir(extracted)?;
        let mut archive = zip::ZipArchive::new(fs::File::open(source)?)
            .map_err(|error| Error::Run(error.to_string()))?;
        let mut total = 0u64;
        let mut paths = std::collections::BTreeSet::new();
        if archive.len() > 8192 {
            return Err(Error::Run("core ZIP contains too many files".into()));
        }
        for index in 0..archive.len() {
            let mut entry = archive
                .by_index(index)
                .map_err(|error| Error::Run(error.to_string()))?;
            let path = entry
                .enclosed_name()
                .ok_or_else(|| Error::Run("unsafe core ZIP path".into()))?;
            if entry.name().contains([':', '\\'])
                || entry.is_symlink()
                || !paths.insert(path.to_string_lossy().to_lowercase())
            {
                return Err(Error::Run("unsafe or duplicate core ZIP entry".into()));
            }
            total = total
                .checked_add(entry.size())
                .ok_or_else(|| Error::Run("core ZIP size overflow".into()))?;
            if total > 2 * 1024 * 1024 * 1024 {
                return Err(Error::Run("core ZIP exceeds 2 GiB".into()));
            }
            let output = extracted.join(path);
            if entry.is_dir() {
                fs::create_dir_all(output)?;
            } else {
                fs::create_dir_all(output.parent().expect("extracted child"))?;
                let copied = std::io::copy(&mut entry, &mut fs::File::create(output)?)?;
                if copied != entry.size() {
                    return Err(Error::Run("truncated core ZIP entry".into()));
                }
            }
        }
        extracted
    } else if source.is_file() {
        return Ok(source.to_owned());
    } else {
        source
    };
    let mut libraries = Vec::new();
    find_library(root, &mut libraries)?;
    match <[PathBuf; 1]>::try_from(libraries) {
        Ok([library]) => Ok(library),
        Err(_) => Err(Error::Run(
            "core directory must contain exactly one library for this platform".into(),
        )),
    }
}
fn find_library(root: &Path, output: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            find_library(&entry.path(), output)?;
        } else if entry.file_type()?.is_file()
            && shinki_emu_manager_core::is_family_library_path(&entry.path())
        {
            output.push(entry.path());
        }
    }
    Ok(())
}

pub(crate) fn window_service() -> abi::WindowService {
    abi::WindowService {
        context: std::ptr::null_mut(),
        request: Some(window_request),
        invalidate: Some(invalidate_window),
    }
}
unsafe extern "C" fn invalidate_window(_: *mut std::ffi::c_void, _: u64, _: u64) {}
unsafe extern "C" fn window_request(
    _: *mut std::ffi::c_void,
    request: *const abi::WindowRequest,
    output: *mut abi::WindowResult,
    error: *mut abi::Error,
) -> abi::Status {
    if request.is_null() || output.is_null() {
        return abi::INVALID_ARGUMENT;
    }
    let request = unsafe { &*request };
    // Metadata requests have no display-side effect. Interactive requests require a UI
    // and fail explicitly rather than inventing a dialog response.
    match request.kind {
        abi::WINDOW_TITLE | abi::WINDOW_ICON | abi::WINDOW_POINTER => {
            unsafe { (*output).valid_fields = 0 };
            abi::OK
        }
        _ => {
            if !error.is_null() {
                unsafe {
                    (*error).set(
                        "SHINKI_EMU_HEADLESS_WINDOW_REQUEST",
                        "interactive window request is unavailable in headless mode",
                    )
                };
            }
            abi::UNSUPPORTED
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn archive(path: &Path, names: &[&str]) {
        let mut writer = zip::ZipWriter::new(fs::File::create(path).unwrap());
        for name in names {
            writer
                .start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(b"package path test input").unwrap();
        }
        writer.finish().unwrap();
    }

    #[test]
    fn nested_zip_paths_are_portable_and_parent_traversal_never_escapes() {
        let root = tempfile::tempdir().unwrap();
        let core = format!("shinki_emu_fixture.{}", std::env::consts::DLL_EXTENSION);
        let source = root.path().join("core.zip");
        archive(&source, &[&core, "licenses/rust/example/LICENSE"]);
        let extracted = root.path().join("extracted");
        assert_eq!(library(&source, &extracted).unwrap(), extracted.join(&core));
        assert!(extracted.join("licenses/rust/example/LICENSE").is_file());
        let unsafe_zip = root.path().join("unsafe.zip");
        archive(&unsafe_zip, &["../escaped"]);
        assert!(library(&unsafe_zip, &root.path().join("unsafe")).is_err());
        assert!(!root.path().join("escaped").exists());
    }
}
