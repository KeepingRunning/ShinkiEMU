//! Offline native-reader bridge. All native reads use the private staging tree,
//! never an ambient game path supplied by a recipe. Writes retain capability checks.
use crate::{
    PatchError, PatchErrorKind, PatchProgress, buffer, error::check_cancelled,
    paths::AuthorizedRoot,
};
use shinki_emu_musica::{MusicaResourcePaths, MusicaTranslationTable, ScriptEncoding};
use std::{io::Write, path::Path, sync::atomic::AtomicBool};

#[allow(clippy::too_many_arguments)]
pub(crate) fn embed(
    root: &Path,
    output: &AuthorizedRoot,
    profile: &str,
    translations: &[u8],
    resources: &MusicaResourcePaths,
    destination: &str,
    cancelled: &AtomicBool,
    progress: &(impl Fn(PatchProgress) + Send + Sync),
) -> Result<usize, PatchError> {
    crate::paths::relative(profile, false)?;
    crate::paths::relative(destination, false)?;
    let table = MusicaTranslationTable::from_json(translations).map_err(native_error)?;
    let (archive, _) =
        shinki_emu_musica::mount_musica_with_resources(root, Path::new(profile), resources)
            .map_err(native_error)?;
    let scripts: Vec<_> = archive
        .manifest()
        .entries
        .iter()
        .filter(|entry| {
            entry.uri.starts_with("musica:/scr/") && entry.uri.to_ascii_lowercase().ends_with(".sc")
        })
        .collect();
    if scripts.is_empty() {
        return Err(PatchError::new(
            PatchErrorKind::UnsupportedInput,
            "native script archive is empty",
        ));
    }
    output.mkdir(destination)?;
    let mut replaced = 0;
    for (index, entry) in scripts.iter().enumerate() {
        check_cancelled(cancelled)?;
        let name = entry
            .uri
            .strip_prefix("musica:/scr/")
            .expect("filtered native script URI");
        crate::paths::relative(name, false)?;
        // Musica's native loose-SC consumer is flat; reject rather than silently
        // generating an unconsumed nested script.
        if name.contains('/') {
            return Err(PatchError::new(
                PatchErrorKind::UnsupportedInput,
                "native loose scripts require flat names",
            ));
        }
        let source =
            shinki_emu_musica::read_expanded_script(&archive, &entry.uri).map_err(|_| {
                PatchError::new(PatchErrorKind::Codec, "native script cannot be decoded")
            })?;
        buffer::bounded(source.len())?;
        let (bytes, hits) =
            shinki_emu_musica::embed_sc_localization(&source, &table, ScriptEncoding::Gbk)
                .map_err(native_error)?;
        buffer::bounded(bytes.len())?;
        let path = format!("{destination}/{name}");
        let mut file = output.create(&path)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        replaced += hits;
        progress(PatchProgress {
            phase: "Embedding native Musica scripts".into(),
            completed: index as u64 + 1,
            total: Some(scripts.len() as u64),
            ..Default::default()
        });
    }
    if replaced == 0 {
        return Err(PatchError::new(
            PatchErrorKind::UnsupportedInput,
            "translation has no native script display endpoints",
        ));
    }
    Ok(replaced)
}

fn native_error(error: shinki_emu_musica::CoreError) -> PatchError {
    PatchError::with_source(
        PatchErrorKind::Codec,
        "native Musica conversion failed",
        error,
    )
}
