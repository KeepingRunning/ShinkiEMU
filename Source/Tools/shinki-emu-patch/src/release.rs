use crate::{
    PatchError, PatchErrorKind, PatchProgress, error::check_cancelled, paths::AuthorizedRoot,
    storage::TaskFile,
};
use shinki_emu_metadata::PatchRelease;
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, atomic::AtomicBool},
};

/// Optional local release bindings use `<script-stem>.patch.json`. Without a
/// manifest, local scripts run in the same sandbox and validate their own inputs.
pub fn read_release_bindings(script: &Path) -> Result<Vec<PatchRelease>, PatchError> {
    let path = script.with_extension("patch.json");
    let metadata = std::fs::metadata(&path).map_err(|cause| {
        PatchError::with_source(
            PatchErrorKind::InvalidRequest,
            "patch has no release-binding manifest",
            cause,
        )
    })?;
    if !metadata.is_file() || metadata.len() > 1024 * 1024 {
        return Err(PatchError::new(
            PatchErrorKind::Bounds,
            "release manifest exceeds its bound",
        ));
    }
    let bytes = std::fs::read(path)?;
    let releases: Vec<PatchRelease> = serde_json::from_value(
        shinki_emu_metadata::parse_unique_json(&bytes).map_err(|cause| {
            PatchError::with_source(
                PatchErrorKind::InvalidRequest,
                "invalid release manifest",
                cause,
            )
        })?,
    )
    .map_err(|cause| {
        PatchError::with_source(
            PatchErrorKind::InvalidRequest,
            "invalid release manifest",
            cause,
        )
    })?;
    validate(&releases)?;
    Ok(releases)
}

pub(crate) fn validate(releases: &[PatchRelease]) -> Result<(), PatchError> {
    if releases.is_empty() || releases.len() > 256 {
        return Err(PatchError::new(
            PatchErrorKind::InvalidRequest,
            "patch needs concrete release bindings",
        ));
    }
    for release in releases {
        release.validate().map_err(|cause| {
            PatchError::with_source(
                PatchErrorKind::InvalidRequest,
                "invalid release binding",
                cause,
            )
        })?;
    }
    Ok(())
}

pub(crate) fn identify<'a>(
    root: &AuthorizedRoot,
    releases: &'a [PatchRelease],
    cancelled: &Arc<AtomicBool>,
    progress: &(impl Fn(PatchProgress) + Send + Sync),
) -> Result<&'a PatchRelease, PatchError> {
    let mut cache = BTreeMap::new();
    let mut matched: Option<&PatchRelease> = None;
    let mut reason = String::new();
    for release in releases {
        let mut matches = true;
        for fingerprint in &release.fingerprints {
            check_cancelled(cancelled)?;
            let actual = match cache.entry(fingerprint.path.as_str()) {
                std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                std::collections::btree_map::Entry::Vacant(entry) => {
                    let actual = match root.open_read(&fingerprint.path) {
                        Ok(file) => {
                            let mut file = TaskFile::new(file, false, cancelled.clone())?;
                            let size = file.size()?;
                            let hash = if size == fingerprint.size {
                                Some(file.sha256()?)
                            } else {
                                None
                            };
                            Some((size, hash))
                        }
                        Err(_) => None,
                    };
                    entry.insert(actual)
                }
            };
            if let Some((size, hash)) = actual.as_mut()
                && *size == fingerprint.size
                && hash.is_none()
            {
                let mut file =
                    TaskFile::new(root.open_read(&fingerprint.path)?, false, cancelled.clone())?;
                if file.size()? != *size {
                    return Err(PatchError::new(
                        PatchErrorKind::Changed,
                        "release identity file changed",
                    ));
                }
                *hash = Some(file.sha256()?);
            }
            let failure = match actual {
                None => Some("missing or unreadable file"),
                Some((size, _)) if *size != fingerprint.size => Some("file size differs"),
                Some((_, Some(hash))) if hash == &fingerprint.sha256 => None,
                _ => Some("SHA256 differs"),
            };
            progress(PatchProgress::check(
                format!("{}: {}", release.release_id, fingerprint.path),
                Some(failure.is_none()),
                failure.unwrap_or("size and SHA256 matched").into(),
            ));
            if let Some(failure) = failure {
                reason = format!("{}: {} ({failure})", release.release_id, fingerprint.path);
                matches = false;
                break;
            }
        }
        if matches {
            if matched.is_some_and(|other| other.release_id != release.release_id) {
                return Err(PatchError::new(
                    PatchErrorKind::ReleaseMismatch,
                    "release fingerprints are ambiguous",
                ));
            }
            matched = Some(release);
        }
    }
    matched.ok_or_else(|| PatchError::new(PatchErrorKind::ReleaseMismatch, reason))
}
