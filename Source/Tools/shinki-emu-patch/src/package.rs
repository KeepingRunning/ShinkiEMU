use crate::{PatchError, PatchErrorKind, paths};
use std::{
    collections::BTreeSet,
    fs, io,
    path::{Path, PathBuf},
};

pub(crate) struct PreparedScript {
    pub path: PathBuf,
    _package: Option<tempfile::TempDir>,
}
impl PreparedScript {
    pub(crate) fn package_root(&self) -> Option<&Path> {
        self._package.as_ref().map(|directory| directory.path())
    }
}

/// A local ZIP has one root patch.luau and relative modules/resources. Archive
/// entries cannot select ambient paths, links, devices, or duplicate spellings.
pub(crate) fn prepare(path: &Path) -> Result<PreparedScript, PatchError> {
    if !path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
    {
        return Ok(PreparedScript {
            path: path.to_owned(),
            _package: None,
        });
    }
    let file = fs::File::open(path)?;
    if file.metadata()?.len() > 64 * 1024 * 1024 {
        return Err(invalid("compressed patch package exceeds 64MiB"));
    }
    let mut archive = zip::ZipArchive::new(file).map_err(|_| invalid("invalid patch ZIP"))?;
    if archive.len() > 4096 {
        return Err(invalid("patch ZIP has too many entries"));
    }
    let directory = tempfile::tempdir()?;
    let mut names = BTreeSet::new();
    let mut total = 0u64;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|_| invalid("cannot read patch ZIP entry"))?;
        let filename = entry
            .name()
            .map_err(|_| invalid("invalid patch ZIP filename"))?;
        let name = filename.trim_end_matches('/');
        let relative = paths::relative(name, false)?;
        if !names.insert(name.to_lowercase()) {
            return Err(invalid("duplicate patch ZIP path"));
        }
        drop(filename);
        if let Some(mode) = entry.unix_mode() {
            let kind = mode & 0o170000;
            if !matches!(kind, 0 | 0o040000 | 0o100000) {
                return Err(invalid("patch ZIP links and special files are forbidden"));
            }
        }
        total = total
            .checked_add(entry.size())
            .filter(|total| *total <= 1024 * 1024 * 1024)
            .ok_or_else(|| invalid("expanded patch package exceeds 1GiB"))?;
        let destination = directory.path().join(relative);
        if entry.is_dir() {
            fs::create_dir_all(destination)?;
            continue;
        }
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)?;
        let expected = entry.size();
        let actual = io::copy(&mut entry, &mut output)?;
        if actual != expected {
            return Err(invalid("patch ZIP entry size differs"));
        }
    }
    let script = directory.path().join("patch.luau");
    if !script.is_file() {
        return Err(invalid("patch ZIP requires root patch.luau"));
    }
    Ok(PreparedScript {
        path: script,
        _package: Some(directory),
    })
}
fn invalid(message: &str) -> PatchError {
    PatchError::new(PatchErrorKind::InvalidRequest, message)
}
