//! The original minori save directory, relocated to
//! `<game>/.shinki/musica/minori/<title>/` with the same relative layout and
//! byte formats the original executable uses under `Documents\minori\<title>\`.
//! Compressed files decode to the original plaintext and envelope; only the
//! deflate stream differs because Musica compresses with miniz.

use crate::{
    ScriptEncoding,
    minori_data::{self, FilePrefix, MinoriTitle, Properties},
    scene::{core_error, error},
};
use roukaku_core::Hash256;
use shinki_emu_sdk::runtime::SessionResult;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
};

mod legacy;
mod slots;
mod sysparams;
#[cfg(test)]
mod tests;

const SYSTEM: &str = "system.dat";
const CG_VIEWS: &str = "cgv.dat";

fn read_optional(path: &Path) -> SessionResult<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(cause) if cause.kind() == ErrorKind::NotFound => Ok(None),
        Err(_) => Err(error(
            "SHINKI_EMU_MUSICA_SAVE_READ",
            "native save file cannot be read",
        )),
    }
}

/// Replaces `dir/name` atomically through a sibling temporary file.
fn replace_file(dir: &Path, name: &str, bytes: &[u8]) -> SessionResult<()> {
    let failed = |_| {
        error(
            "SHINKI_EMU_MUSICA_SAVE_WRITE",
            "native save file cannot be written",
        )
    };
    let mut file = tempfile::NamedTempFile::new_in(dir).map_err(failed)?;
    file.write_all(bytes).map_err(failed)?;
    file.as_file().sync_all().map_err(failed)?;
    file.persist(dir.join(name))
        .map_err(|cause| failed(cause.error))?;
    Ok(())
}

struct NativeDirectory {
    title: MinoriTitle,
    game_root: PathBuf,
    root: PathBuf,
    slot_root: PathBuf,
    prefix: FilePrefix,
}

impl NativeDirectory {
    fn open(game_root: &Path, title: MinoriTitle) -> SessionResult<Self> {
        let root = game_root
            .join(".shinki/musica/minori")
            .join(title.directory());
        std::fs::create_dir_all(&root).map_err(|_| {
            error(
                "SHINKI_EMU_MUSICA_SAVE_PATH",
                "native save directory cannot be created",
            )
        })?;
        let prefix = minori_data::resolve_prefix(title, registry_serial, existing_prefix(&root));
        Ok(Self {
            title,
            game_root: game_root.to_path_buf(),
            slot_root: root.clone(),
            root,
            prefix,
        })
    }

    fn system_data(&self) -> SessionResult<Option<Properties>> {
        read_optional(&self.root.join(SYSTEM))?
            .map(|bytes| minori_data::decode_system(&bytes, self.title.system_format()))
            .transpose()
            .map_err(core_error)
    }

    /// Skips the write when the stored table already decodes to `properties`.
    fn write_system_data(&self, properties: &Properties) -> SessionResult<()> {
        if self
            .system_data()
            .is_ok_and(|stored| stored.as_ref() == Some(properties))
        {
            return Ok(());
        }
        let bytes = minori_data::encode_system(properties, self.title.system_format())
            .map_err(core_error)?;
        replace_file(&self.root, SYSTEM, &bytes)
    }

    fn cg_names(&self) -> SessionResult<Vec<Vec<u8>>> {
        read_optional(&self.root.join(CG_VIEWS))?
            .map(|bytes| minori_data::decode_cg_names(&bytes, self.prefix.value))
            .transpose()
            .map_err(core_error)
            .map(Option::unwrap_or_default)
    }

    fn write_cg_names(&self, names: &[Vec<u8>]) -> SessionResult<()> {
        if self.cg_names().is_ok_and(|stored| stored == names) {
            return Ok(());
        }
        let bytes = minori_data::encode_cg_names(names, self.prefix.value).map_err(core_error)?;
        replace_file(&self.root, CG_VIEWS, &bytes)
    }
}

/// The prefix of the first readable compressed original file, in name order.
fn existing_prefix(dir: &Path) -> Option<u32> {
    let mut names: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.file_name().is_some_and(|name| name == CG_VIEWS)
                || path
                    .extension()
                    .is_some_and(|extension| extension == "tbr" || extension == "sav")
        })
        .collect();
    names.sort();
    names
        .into_iter()
        .find_map(|path| minori_data::file_prefix(&std::fs::read(path).ok()?))
}

#[cfg(windows)]
fn registry_serial(key: &str) -> Option<Vec<u8>> {
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, REG_SZ, RegCloseKey, RegOpenKeyExA,
        RegQueryValueExA,
    };
    let key = std::ffi::CString::new(key).ok()?;
    let value = c"SerialCode";
    let mut handle: HKEY = std::ptr::null_mut();
    // SAFETY: `key` is NUL-terminated and `handle` is a valid out pointer.
    let opened = unsafe {
        RegOpenKeyExA(
            HKEY_LOCAL_MACHINE,
            key.as_ptr().cast(),
            0,
            KEY_READ | KEY_WOW64_32KEY,
            &mut handle,
        )
    };
    if opened != 0 {
        return None;
    }
    let mut kind = 0;
    let mut size = 0u32;
    // SAFETY: `handle` is open; a null data pointer asks only for the size.
    let sized = unsafe {
        RegQueryValueExA(
            handle,
            value.as_ptr().cast(),
            std::ptr::null(),
            &mut kind,
            std::ptr::null_mut(),
            &mut size,
        )
    };
    let mut buffer = vec![0u8; size as usize];
    let read = sized == 0
        && kind == REG_SZ
        && size != 0
        // SAFETY: `buffer` holds `size` writable bytes.
        && unsafe {
            RegQueryValueExA(
                handle,
                value.as_ptr().cast(),
                std::ptr::null(),
                std::ptr::null_mut(),
                buffer.as_mut_ptr(),
                &mut size,
            )
        } == 0;
    // SAFETY: `handle` was opened above and is closed exactly once.
    unsafe { RegCloseKey(handle) };
    read.then(|| {
        buffer.truncate(size as usize);
        buffer
    })
}

/// The original reads `SerialCode` only from the Windows registry; elsewhere
/// `resolve_prefix` falls back to an existing file's prefix, then zero.
#[cfg(not(windows))]
fn registry_serial(_key: &str) -> Option<Vec<u8>> {
    None
}

fn integer(raw: &[u8]) -> Option<i64> {
    std::str::from_utf8(raw).ok()?.parse().ok()
}

fn global(encoding: ScriptEncoding, key: &[u8], value: &[u8]) -> Option<(String, i64)> {
    Some((encoding.decode(key)?.into_owned(), integer(value)?))
}

/// `<stem>.tbr`; the stem comes from an archive script name and must stay a
/// plain file name inside the native directory.
fn read_flags_name(stem: &str) -> SessionResult<String> {
    if stem.is_empty()
        || stem.starts_with('.')
        || stem.chars().any(|c| {
            matches!(
                c,
                '/' | '\\' | ':' | '\0' | '*' | '?' | '"' | '<' | '>' | '|'
            )
        })
    {
        return Err(error(
            "SHINKI_EMU_MUSICA_SAVE_PATH",
            "script name cannot name a read-flag file",
        ));
    }
    Ok(format!("{stem}.tbr"))
}

/// Session view of the title-wide original files: `system.dat` holds every
/// `.setglobal` value and `cgv.dat` the background images already shown.
///
/// Without a recognized executable there is no original directory: the
/// previous JSON progress stays readable, and background persistence is
/// skipped with one warning instead of failing the session.
pub(crate) struct NativeFiles {
    dir: Option<NativeDirectory>,
    encoding: ScriptEncoding,
    system: Properties,
    cg: BTreeSet<Vec<u8>>,
    /// `sysparams.txt`, `None` before the first write.
    settings: Option<Properties>,
    /// `musica-settings.txt`: settings without an original key.
    musica: Properties,
    legacy_globals: BTreeMap<String, i64>,
    legacy_config: Option<crate::MusicaConfigState>,
    legacy_quick_cursor: Option<u32>,
    warned: bool,
}

impl NativeFiles {
    pub(crate) fn open(game_root: &Path, encoding: ScriptEncoding) -> SessionResult<Self> {
        let mut files = Self {
            dir: None,
            encoding,
            system: Properties::new(),
            cg: BTreeSet::new(),
            settings: None,
            musica: Properties::new(),
            legacy_globals: BTreeMap::new(),
            legacy_config: None,
            legacy_quick_cursor: None,
            warned: false,
        };
        let Some(title) = MinoriTitle::detect(game_root) else {
            return Ok(files);
        };
        let dir = NativeDirectory::open(game_root, title)?;
        files.system = dir.system_data()?.unwrap_or_default();
        files.cg = dir.cg_names()?.into_iter().collect();
        files.settings = dir.settings()?;
        files.musica = dir.musica_settings()?;
        files.dir = Some(dir);
        Ok(files)
    }

    /// One-time, no-clobber import of `<game>/.shinki/musica/saves/global-progress.json`
    /// into `system.dat`; the source becomes `global-progress.json.imported`.
    /// An existing `system.dat` is never overwritten and the JSON stays put.
    pub(crate) fn import_legacy_progress(
        &mut self,
        game_root: &Path,
        game: Hash256,
    ) -> SessionResult<()> {
        let saves = game_root.join(".shinki/musica/saves");
        if let Some(dir) = &self.dir
            && dir.system_data()?.is_some()
        {
            return Ok(());
        }
        let Some(unlocks) = legacy::progress(&saves, game)? else {
            return Ok(());
        };
        let globals = legacy::unlocked_globals(&unlocks)?;
        let Some(dir) = &self.dir else {
            self.legacy_globals = globals;
            return Ok(());
        };
        let system: Properties = globals
            .into_iter()
            .map(|(flag, value)| (flag.into_bytes(), value.to_string().into_bytes()))
            .collect();
        dir.write_system_data(&system)?;
        self.system = system;
        legacy::mark_imported(&saves, legacy::PROGRESS)
    }

    /// The `.setglobal` table stored in `system.dat`.
    pub(crate) fn globals(&self) -> BTreeMap<String, i64> {
        if self.dir.is_none() {
            return self.legacy_globals.clone();
        }
        self.system
            .iter()
            .filter_map(|(key, value)| global(self.encoding, key, value))
            .collect()
    }

    /// Replaces the representable entries of `system.dat` with `globals`;
    /// entries Musica cannot decode stay verbatim.
    pub(crate) fn store_globals(&mut self, globals: &BTreeMap<String, i64>) -> SessionResult<()> {
        let Some(dir) = &self.dir else {
            self.warn_unpersisted();
            return Ok(());
        };
        let encoding = self.encoding;
        let mut system: Properties = self
            .system
            .iter()
            .filter(|(key, value)| global(encoding, key, value).is_none())
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        for (name, value) in globals {
            let key = encoding.encode(name).ok_or_else(|| {
                error(
                    "SHINKI_EMU_MUSICA_SAVE_WRITE",
                    "global variable name has no native encoding",
                )
            })?;
            system.insert(key.into_owned(), value.to_string().into_bytes());
        }
        dir.write_system_data(&system)?;
        self.system = system;
        Ok(())
    }

    /// Adds one shown background image (lowercase file name) to `cgv.dat`.
    pub(crate) fn record_cg(&mut self, name: &str) -> SessionResult<()> {
        let Some(dir) = &self.dir else {
            self.warn_unpersisted();
            return Ok(());
        };
        let bytes = self.encoding.encode(name).ok_or_else(|| {
            error(
                "SHINKI_EMU_MUSICA_SAVE_WRITE",
                "image name has no native encoding",
            )
        })?;
        if self.cg.contains(bytes.as_ref()) {
            return Ok(());
        }
        let mut names = self.cg.clone();
        names.insert(bytes.into_owned());
        dir.write_cg_names(&names.iter().cloned().collect::<Vec<_>>())?;
        self.cg = names;
        Ok(())
    }

    /// The stored `<stem>.tbr` read bitmap, `None` before the first write.
    pub(crate) fn read_flags(
        &self,
        stem: &str,
    ) -> SessionResult<Option<crate::runtime::ReadBitmap>> {
        let Some(dir) = &self.dir else {
            return Ok(None);
        };
        read_optional(&dir.root.join(read_flags_name(stem)?))?
            .map(|bytes| minori_data::decode_read_flags(&bytes, dir.prefix.value))
            .transpose()
            .map_err(core_error)
    }

    /// Replaces `<stem>.tbr` with `bits`.
    pub(crate) fn store_read_flags(
        &mut self,
        stem: &str,
        bits: &crate::runtime::ReadBitmap,
    ) -> SessionResult<()> {
        let Some(dir) = &self.dir else {
            self.warn_unpersisted();
            return Ok(());
        };
        let name = read_flags_name(stem)?;
        let bytes = minori_data::encode_read_flags(bits, dir.prefix.value).map_err(core_error)?;
        replace_file(&dir.root, &name, &bytes)
    }

    fn warn_unpersisted(&mut self) {
        if !std::mem::replace(&mut self.warned, true) {
            tracing::warn!(
                event = "shinki.emu.musica.native.unpersisted",
                diagnostic_code = "SHINKI_EMU_MUSICA_SAVE_TITLE"
            );
        }
    }
}
