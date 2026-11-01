use crate::{
    BACKUP_DIR, GAME_STATE_DIR, PatchError, PatchErrorKind, PatchProgress, STAGING_DIR,
    error::check_cancelled,
    paths::{self, AuthorizedRoot},
    receipt::RECEIPT_FILE,
    transaction::{self, Transaction},
};
use cap_fs_ext::DirExt;
use cap_std::{ambient_authority, fs::Dir};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};

const ABSENT: &str = "absent";
const RECEIPT_DEST: &str = ".shinki/patch-result.json";
const SCHEMA: &str = "shinki.emu.patch_backup.v1";

#[derive(Clone, Serialize, Deserialize)]
struct Entry {
    relpath: String,
    original_sha256: String,
    new_sha256: String,
}

#[derive(Serialize, Deserialize)]
struct Manifest {
    schema: String,
    recipe_id: String,
    created_utc: String,
    entries: Vec<Entry>,
    #[serde(default)]
    created_directories: Vec<String>,
}

fn failure(
    message: &'static str,
    cause: impl std::error::Error + Send + Sync + 'static,
) -> PatchError {
    PatchError::with_source(PatchErrorKind::Publish, message, cause)
}

fn state(game: &Dir) -> Result<Option<Dir>, PatchError> {
    match game.open_dir_nofollow(GAME_STATE_DIR) {
        Ok(dir) => Ok(Some(dir)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(failure("cannot open private game state", error)),
    }
}

fn backup_parent(game: &Dir) -> Result<Option<Dir>, PatchError> {
    let Some(state) = state(game)? else {
        return Ok(None);
    };
    match state.open_dir_nofollow(BACKUP_DIR) {
        Ok(dir) => Ok(Some(dir)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(failure("cannot open private backup directory", error)),
    }
}

fn open_game(path: &Path) -> Result<Dir, PatchError> {
    Dir::open_ambient_dir(path, ambient_authority())
        .map_err(|error| failure("cannot open game directory", error))
}

fn hash(mut file: impl Read, cancelled: &AtomicBool) -> Result<String, PatchError> {
    let mut hasher = Sha256::new();
    let mut bytes = [0u8; 64 * 1024];
    loop {
        check_cancelled(cancelled)?;
        let count = file.read(&mut bytes)?;
        if count == 0 {
            break;
        }
        hasher.update(&bytes[..count]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn list_files(
    root: &AuthorizedRoot,
    cancelled: &AtomicBool,
    skip_state: bool,
    skip_receipt: bool,
) -> Result<(BTreeMap<String, String>, BTreeSet<String>), PatchError> {
    let mut files = BTreeMap::new();
    let mut directories = vec![String::new()];
    let mut found_directories = BTreeSet::new();
    while let Some(directory) = directories.pop() {
        check_cancelled(cancelled)?;
        for entry in root.list(&directory)? {
            if directory.is_empty()
                && ((skip_state && entry.name.eq_ignore_ascii_case(GAME_STATE_DIR))
                    || (skip_receipt && entry.name == RECEIPT_FILE))
            {
                continue;
            }
            let path = if directory.is_empty() {
                entry.name
            } else {
                format!("{directory}/{}", entry.name)
            };
            paths::relative(&path, false)?;
            match entry.kind.as_str() {
                "directory" => {
                    found_directories.insert(path.clone());
                    directories.push(path);
                }
                "file" => {
                    files.insert(path.clone(), hash(root.checked_file(&path)?, cancelled)?);
                }
                _ => {
                    return Err(PatchError::new(
                        PatchErrorKind::Path,
                        "game contains a link or special file",
                    ));
                }
            }
        }
    }
    Ok((files, found_directories))
}

fn source_path(entry: &Entry) -> &str {
    if entry.relpath == RECEIPT_DEST {
        RECEIPT_FILE
    } else {
        &entry.relpath
    }
}

fn move_file(
    from: &AuthorizedRoot,
    from_path: &str,
    to: &AuthorizedRoot,
    to_path: &str,
) -> Result<(), PatchError> {
    let original = paths::relative(from_path, false)?;
    let replacement = paths::relative(to_path, false)?;
    // Both parents are opened one component at a time without following links.
    let from_parent = from.checked_directory(original.parent().unwrap_or(Path::new("")), false)?;
    let to_parent = to.checked_directory(replacement.parent().unwrap_or(Path::new("")), true)?;
    drop(from.checked_file(from_path)?);
    transaction::rename_noreplace(
        &from_parent,
        Path::new(original.file_name().unwrap()),
        &to_parent,
        Path::new(replacement.file_name().unwrap()),
    )
    .map_err(|error| failure("cannot move patch transaction file", error))?;
    Ok(())
}

fn backup_name() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("UTC clock")
        .as_nanos();
    format!(
        "backup-{nanos:032x}-{:08x}-{:08x}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

fn prepare_backup(game: &Dir) -> Result<(Dir, Dir, bool), PatchError> {
    let state = game.open_dir_nofollow(GAME_STATE_DIR)?;
    let created = state
        .symlink_metadata(BACKUP_DIR)
        .is_err_and(|error| error.kind() == io::ErrorKind::NotFound);
    if created {
        transaction::create_private_directory(&state, Path::new(BACKUP_DIR))?;
    }
    let parent = state.open_dir_nofollow(BACKUP_DIR)?;
    for _ in 0..128 {
        let name = backup_name();
        match transaction::create_private_directory(&parent, Path::new(&name)) {
            Ok(directory) => return Ok((parent, directory, created)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(failure("cannot create backup directory", error)),
        }
    }
    Err(PatchError::new(
        PatchErrorKind::Publish,
        "cannot allocate unique backup directory",
    ))
}

fn prune_directory(root: &AuthorizedRoot, path: &str, must_remove: bool) -> Result<(), PatchError> {
    if !root.exists(path)? {
        return Ok(());
    }
    let relative = paths::relative(path, false)?;
    let directory = root.checked_directory(&relative, false)?;
    let empty = directory.entries()?.next().is_none();
    if !empty && must_remove {
        return Err(PatchError::new(
            PatchErrorKind::Changed,
            format!("patch-created directory contains user files: {path}"),
        ));
    }
    if empty {
        #[cfg(windows)]
        {
            let parent =
                root.checked_directory(relative.parent().unwrap_or(Path::new("")), false)?;
            drop(directory);
            let leaf = Path::new(relative.file_name().expect("nonempty validated path"));
            transaction::open_windows_staging(&parent, leaf)?.remove_open_dir()?;
        }
        #[cfg(not(windows))]
        directory.remove_open_dir()?;
    }
    Ok(())
}

fn prune_created(root: &AuthorizedRoot, manifest: &Manifest) -> Result<(), PatchError> {
    for dir in manifest.created_directories.iter().rev() {
        if !manifest
            .entries
            .iter()
            .any(|entry| entry.relpath == *dir && entry.original_sha256 != ABSENT)
        {
            prune_directory(root, dir, false)?;
        }
    }
    Ok(())
}

fn rollback(
    game: &AuthorizedRoot,
    staged: &AuthorizedRoot,
    backup: &AuthorizedRoot,
    manifest: &Manifest,
    moved_old: &[bool],
    placed_new: &[bool],
) -> Result<(), PatchError> {
    for index in (0..manifest.entries.len()).rev() {
        let entry = &manifest.entries[index];
        if placed_new[index] {
            move_file(game, &entry.relpath, staged, source_path(entry))?;
        }
    }
    for index in (0..manifest.entries.len()).rev() {
        let entry = &manifest.entries[index];
        if moved_old[index] {
            if manifest.created_directories.contains(&entry.relpath) {
                prune_directory(game, &entry.relpath, true)?;
            }
            move_file(backup, &entry.relpath, game, &entry.relpath)?;
        }
    }
    prune_created(game, manifest)?;
    Ok(())
}

pub(crate) fn commit(
    transaction: Transaction,
    recipe_id: &str,
    cancelled: &AtomicBool,
    progress: &(impl Fn(PatchProgress) + Send + Sync),
) -> Result<PathBuf, PatchError> {
    commit_inner(transaction, recipe_id, cancelled, progress, None)
}

fn commit_inner(
    transaction: Transaction,
    recipe_id: &str,
    cancelled: &AtomicBool,
    progress: &(impl Fn(PatchProgress) + Send + Sync),
    fail_after: Option<usize>,
) -> Result<PathBuf, PatchError> {
    let source = transaction.source_dir()?;
    let staged = transaction.staging_dir()?;
    let output = transaction::final_dir_path(&source)?;
    let game_root = AuthorizedRoot::from_input_dir(source.try_clone()?);
    let stage_root = AuthorizedRoot::from_input_dir(staged);
    let (originals, original_directories) = list_files(&game_root, cancelled, true, false)?;
    let (replacements, replacement_directories) = list_files(&stage_root, cancelled, false, true)?;
    let mut paths = BTreeSet::new();
    paths.extend(originals.keys().cloned());
    paths.extend(replacements.keys().cloned());
    let mut entries: Vec<Entry> = paths
        .into_iter()
        .filter_map(|path| {
            let old = originals.get(&path).map(String::as_str).unwrap_or(ABSENT);
            let new = replacements
                .get(&path)
                .map(String::as_str)
                .unwrap_or(ABSENT);
            (old != new).then_some(Entry {
                relpath: path,
                original_sha256: old.into(),
                new_sha256: new.into(),
            })
        })
        .collect();
    let old_receipt = match game_root.checked_file(RECEIPT_DEST) {
        Ok(file) => hash(file, cancelled)?,
        Err(error)
            if error.kind() == PatchErrorKind::Io || error.kind() == PatchErrorKind::Path =>
        {
            if source
                .open_dir_nofollow(GAME_STATE_DIR)?
                .symlink_metadata("patch-result.json")
                .is_err_and(|e| e.kind() == io::ErrorKind::NotFound)
            {
                ABSENT.into()
            } else {
                return Err(error);
            }
        }
        Err(error) => return Err(error),
    };
    entries.push(Entry {
        relpath: RECEIPT_DEST.into(),
        original_sha256: old_receipt,
        new_sha256: hash(stage_root.checked_file(RECEIPT_FILE)?, cancelled)?,
    });
    check_cancelled(cancelled)?;
    let created_directories = replacement_directories
        .difference(&original_directories)
        .filter(|dir| {
            replacements
                .keys()
                .any(|path| path.starts_with(&format!("{dir}/")))
        })
        .cloned()
        .collect();
    let (backup_parent, backup, created_backup) = prepare_backup(&source)?;
    let backup_root = AuthorizedRoot::from_input_dir(backup.try_clone()?);
    let manifest = Manifest {
        schema: SCHEMA.into(),
        recipe_id: recipe_id.into(),
        created_utc: time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .map_err(|e| {
                PatchError::with_source(PatchErrorKind::Io, "cannot format UTC backup date", e)
            })?,
        entries,
        created_directories,
    };
    let write_result = (|| -> Result<(), PatchError> {
        let mut file = backup.create("manifest.json")?;
        serde_json::to_writer_pretty(&mut file, &manifest).map_err(|e| {
            PatchError::with_source(PatchErrorKind::Io, "cannot write backup manifest", e)
        })?;
        file.flush()?;
        file.sync_all()?;
        Ok(())
    })();
    if let Err(error) = write_result {
        drop(backup_root);
        transaction::cleanup(backup)?;
        if created_backup {
            backup_parent.remove_open_dir()?;
        }
        return Err(error);
    }
    let mut moved_old = vec![false; manifest.entries.len()];
    let mut placed_new = vec![false; manifest.entries.len()];
    let result = (|| -> Result<(), PatchError> {
        check_cancelled(cancelled)?;
        for (index, entry) in manifest.entries.iter().enumerate() {
            check_cancelled(cancelled)?;
            if entry.original_sha256 != ABSENT {
                if hash(game_root.checked_file(&entry.relpath)?, cancelled)?
                    != entry.original_sha256
                {
                    return Err(PatchError::new(
                        PatchErrorKind::Changed,
                        "game changed during patch preparation",
                    ));
                }
                move_file(&game_root, &entry.relpath, &backup_root, &entry.relpath)?;
                moved_old[index] = true;
            }
        }
        for (index, entry) in manifest.entries.iter().enumerate() {
            check_cancelled(cancelled)?;
            if entry.new_sha256 != ABSENT {
                if original_directories.contains(&entry.relpath) {
                    prune_directory(&game_root, &entry.relpath, true)?;
                }
                move_file(&stage_root, source_path(entry), &game_root, &entry.relpath)?;
                placed_new[index] = true;
            }
            progress(PatchProgress {
                phase: "committing patch".into(),
                completed: (index + 1) as u64,
                total: Some(manifest.entries.len() as u64),
                ..Default::default()
            });
            if fail_after == Some(index + 1) {
                return Err(PatchError::new(
                    PatchErrorKind::Publish,
                    "injected commit failure",
                ));
            }
        }
        Ok(())
    })();
    if let Err(error) = result {
        rollback(
            &game_root,
            &stage_root,
            &backup_root,
            &manifest,
            &moved_old,
            &placed_new,
        )?;
        drop(backup_root);
        transaction::cleanup(backup)?;
        if created_backup {
            backup_parent.remove_open_dir()?;
        } else {
            drop(backup_parent);
        }
        return Err(error.recovered());
    }
    drop(backup_root);
    drop(backup);
    drop(backup_parent);
    Ok(output)
}

fn newest_backup(parent: &Dir) -> Result<Option<(String, Manifest)>, PatchError> {
    let mut newest: Option<(String, Manifest)> = None;
    for entry in parent.entries()? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() || entry.file_type()?.is_symlink() {
            continue;
        }
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| PatchError::new(PatchErrorKind::Path, "invalid backup name"))?;
        paths::relative(&name, false)?;
        let dir = parent.open_dir_nofollow(&name)?;
        let root = AuthorizedRoot::from_input_dir(dir);
        let manifest: Manifest = serde_json::from_reader(root.open_read("manifest.json")?)
            .map_err(|e| {
                PatchError::with_source(PatchErrorKind::Integrity, "invalid backup manifest", e)
            })?;
        if manifest.schema != SCHEMA {
            return Err(PatchError::new(
                PatchErrorKind::Integrity,
                "unsupported backup manifest schema",
            ));
        }
        for record in &manifest.entries {
            paths::relative(&record.relpath, false)?;
            if record
                .relpath
                .split('/')
                .next()
                .is_some_and(|first| first.eq_ignore_ascii_case(GAME_STATE_DIR))
                && record.relpath != RECEIPT_DEST
            {
                return Err(PatchError::new(
                    PatchErrorKind::Integrity,
                    "backup targets private game state",
                ));
            }
            for digest in [&record.original_sha256, &record.new_sha256] {
                if digest != ABSENT
                    && (digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()))
                {
                    return Err(PatchError::new(
                        PatchErrorKind::Integrity,
                        "invalid backup digest",
                    ));
                }
            }
        }
        for dir in &manifest.created_directories {
            paths::relative(dir, false)?;
            if dir
                .split('/')
                .next()
                .is_some_and(|first| first.eq_ignore_ascii_case(GAME_STATE_DIR))
            {
                return Err(PatchError::new(
                    PatchErrorKind::Integrity,
                    "backup directory targets private state",
                ));
            }
        }
        if newest.as_ref().is_none_or(|(old, current)| {
            (manifest.created_utc.as_str(), name.as_str())
                > (current.created_utc.as_str(), old.as_str())
        }) {
            newest = Some((name, manifest));
        }
    }
    Ok(newest)
}

pub fn has_backup(game: &Path) -> Result<bool, PatchError> {
    let game = open_game(game)?;
    let Some(parent) = backup_parent(&game)? else {
        return Ok(false);
    };
    Ok(newest_backup(&parent)?.is_some())
}

pub fn restore_latest(
    game: &Path,
    cancelled: std::sync::Arc<AtomicBool>,
    progress: impl Fn(PatchProgress) + Send + Sync,
) -> Result<(), PatchError> {
    check_cancelled(&cancelled)?;
    let game = open_game(game)?;
    let parent = backup_parent(&game)?
        .ok_or_else(|| PatchError::new(PatchErrorKind::InvalidRequest, "no patch backup exists"))?;
    let (name, manifest) = newest_backup(&parent)?
        .ok_or_else(|| PatchError::new(PatchErrorKind::InvalidRequest, "no patch backup exists"))?;
    #[cfg(windows)]
    let backup = transaction::open_windows_staging(&parent, Path::new(&name))?;
    #[cfg(not(windows))]
    let backup = parent.open_dir_nofollow(&name)?;
    let root = AuthorizedRoot::from_input_dir(game.try_clone()?);
    let backup_root = AuthorizedRoot::from_input_dir(backup.try_clone()?);
    for entry in &manifest.entries {
        check_cancelled(&cancelled)?;
        let current = match root.checked_file(&entry.relpath) {
            Ok(file) => hash(file, &cancelled)?,
            Err(error) => {
                if entry.new_sha256 == ABSENT
                    && manifest.created_directories.contains(&entry.relpath)
                {
                    root.checked_directory(&paths::relative(&entry.relpath, false)?, false)?;
                    ABSENT.into()
                } else if (entry.new_sha256 == ABSENT
                    && manifest.entries.iter().any(|other| {
                        other.new_sha256 != ABSENT
                            && entry.relpath.starts_with(&format!("{}/", other.relpath))
                    }))
                    || !root.exists(&entry.relpath)?
                {
                    ABSENT.into()
                } else {
                    return Err(error);
                }
            }
        };
        if current != entry.new_sha256 {
            return Err(PatchError::new(
                PatchErrorKind::Changed,
                format!("patched file was changed: {}", entry.relpath),
            ));
        }
        if entry.original_sha256 != ABSENT
            && hash(backup_root.checked_file(&entry.relpath)?, &cancelled)? != entry.original_sha256
        {
            return Err(PatchError::new(
                PatchErrorKind::Integrity,
                format!("backup file was changed: {}", entry.relpath),
            ));
        }
    }
    let state = state(&game)?.expect("backup requires private state");
    let created_staging = state
        .symlink_metadata(STAGING_DIR)
        .is_err_and(|error| error.kind() == io::ErrorKind::NotFound);
    let staging_parent = if created_staging {
        transaction::create_private_directory(&state, Path::new(STAGING_DIR))?
    } else {
        state.open_dir_nofollow(STAGING_DIR)?
    };
    let (_, scratch) = transaction::create_staging(&staging_parent)?;
    let scratch_root = AuthorizedRoot::from_input_dir(scratch.try_clone()?);
    let mut moved_new = vec![false; manifest.entries.len()];
    let mut placed_old = vec![false; manifest.entries.len()];
    let result = (|| -> Result<(), PatchError> {
        for (index, entry) in manifest.entries.iter().enumerate() {
            check_cancelled(&cancelled)?;
            if entry.new_sha256 != ABSENT {
                move_file(&root, &entry.relpath, &scratch_root, &entry.relpath)?;
                moved_new[index] = true;
            }
        }
        for (index, entry) in manifest.entries.iter().enumerate() {
            check_cancelled(&cancelled)?;
            if entry.original_sha256 != ABSENT {
                if manifest.created_directories.contains(&entry.relpath) {
                    prune_directory(&root, &entry.relpath, true)?;
                }
                move_file(&backup_root, &entry.relpath, &root, &entry.relpath)?;
                placed_old[index] = true;
            }
            progress(PatchProgress {
                phase: "restoring patch backup".into(),
                completed: (index + 1) as u64,
                total: Some(manifest.entries.len() as u64),
                ..Default::default()
            });
        }
        Ok(())
    })();
    if let Err(error) = result {
        for index in (0..manifest.entries.len()).rev() {
            let entry = &manifest.entries[index];
            if placed_old[index] {
                move_file(&root, &entry.relpath, &backup_root, &entry.relpath)?;
            }
        }
        for index in (0..manifest.entries.len()).rev() {
            let entry = &manifest.entries[index];
            if moved_new[index] {
                if manifest.entries.iter().any(|old| {
                    old.original_sha256 != ABSENT
                        && old.relpath.starts_with(&format!("{}/", entry.relpath))
                }) {
                    prune_directory(&root, &entry.relpath, true)?;
                }
                move_file(&scratch_root, &entry.relpath, &root, &entry.relpath)?;
            }
        }
        drop(scratch_root);
        transaction::cleanup(scratch)?;
        if created_staging {
            staging_parent.remove_open_dir()?;
        }
        return Err(error);
    }
    prune_created(&root, &manifest)?;
    drop(scratch_root);
    transaction::cleanup(scratch)?;
    if created_staging {
        staging_parent.remove_open_dir()?;
    }
    drop(backup_root);
    transaction::cleanup(backup)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PatchMode, PatchRequest};
    use std::sync::atomic::AtomicBool;

    #[test]
    fn partial_commit_failure_moves_all_prior_originals_back() {
        let fixture = tempfile::tempdir().unwrap();
        let game = fixture.path().join("game");
        std::fs::create_dir(&game).unwrap();
        std::fs::write(game.join("assets.bin"), b"original assets").unwrap();
        std::fs::write(game.join("removed.bin"), b"original removed").unwrap();
        let request = PatchRequest {
            script: fixture.path().join("recipe.luau"),
            source: game.clone(),
            patch: game.clone(),
            mode: PatchMode::InPlace,
        };
        let transaction = Transaction::new(&request).unwrap();
        let stage = AuthorizedRoot::from_dir(transaction.open_staging().unwrap());
        stage
            .create("assets.bin")
            .unwrap()
            .write_all(b"updated assets")
            .unwrap();
        stage
            .create("added.bin")
            .unwrap()
            .write_all(b"new content")
            .unwrap();
        stage
            .create(RECEIPT_FILE)
            .unwrap()
            .write_all(b"receipt")
            .unwrap();
        drop(stage);
        let error = commit_inner(
            transaction,
            "fixture",
            &AtomicBool::new(false),
            &|_| {},
            Some(1),
        )
        .unwrap_err();
        assert_eq!(error.kind(), PatchErrorKind::Publish);
        assert_eq!(
            std::fs::read(game.join("assets.bin")).unwrap(),
            b"original assets"
        );
        assert_eq!(
            std::fs::read(game.join("removed.bin")).unwrap(),
            b"original removed"
        );
        assert!(!game.join("added.bin").exists());
        assert!(!game.join(".shinki/patch-result.json").exists());
        assert!(!has_backup(&game).unwrap());
    }
}
