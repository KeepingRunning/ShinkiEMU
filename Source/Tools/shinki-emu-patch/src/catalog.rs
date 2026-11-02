use crate::{
    PatchError, PatchErrorKind,
    paths::AuthorizedRoot,
    storage::{TaskFile, scratch},
};
use roukaku_byte_source::{ByteRange, DEFAULT_MAX_RANGE_BYTES};
use std::{
    path::Path,
    sync::{Arc, atomic::AtomicBool},
};

/// Read the single v2 patch catalog. Listing never compiles or runs recipes.
pub fn list_recipes(
    metadata: &Path,
) -> Result<Vec<shinki_emu_metadata::PatchCatalogEntry>, PatchError> {
    let root = AuthorizedRoot::open(metadata)?;
    let mut file = TaskFile::new(
        root.open_read("patches-v2.json")?,
        false,
        Arc::new(AtomicBool::new(false)),
    )?;
    let length = file.size()?;
    ByteRange {
        offset: 0,
        len: length,
    }
    .validate(length, DEFAULT_MAX_RANGE_BYTES)
    .map_err(|cause| {
        PatchError::with_source(
            PatchErrorKind::Bounds,
            "catalog exceeds bounded file range",
            cause,
        )
    })?;
    let mut bytes = scratch(length as usize)?;
    file.read(0, &mut bytes)?;
    shinki_emu_metadata::parse_patch_catalog(&bytes)
        .map(|catalog| catalog.entries)
        .map_err(|cause| {
            PatchError::with_source(
                PatchErrorKind::InvalidRequest,
                "invalid patch catalog",
                cause,
            )
        })
}
