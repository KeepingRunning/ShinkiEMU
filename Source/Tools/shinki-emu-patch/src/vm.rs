use crate::{
    OutputProfile, PatchError, PatchErrorKind, PatchProgress, PatchRequest, buffer,
    error::check_cancelled,
    paths::AuthorizedRoot,
    storage::{self, TaskFile},
    transaction::Transaction,
};
use mlua::{
    Buffer, Function, Lua, LuaOptions, LuaSerdeExt, RegistryKey, StdLib, Table, UserData,
    UserDataMethods, Value,
};
use parking_lot::Mutex;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, atomic::AtomicBool},
};

#[derive(Serialize)]
pub(crate) struct InputRecord {
    pub root: String,
    pub path: String,
    pub sha256: String,
}

#[derive(Serialize)]
pub(crate) struct ScriptRecord {
    pub path: String,
    pub sha256: String,
}

pub(crate) struct Execution {
    pub id: String,
    pub version: u64,
    pub profile: OutputProfile,
    pub inputs: Vec<InputRecord>,
    pub scripts: Vec<ScriptRecord>,
}

// Userdata must also support mlua's `send` feature when Artemis is embedded.
type FileHandle = Arc<Mutex<TaskFile>>;

struct Source {
    file: FileHandle,
    sha256: String,
}

impl UserData for Source {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("size", |_, this, ()| {
            this.file.lock().size().map_err(mlua::Error::external)
        });
        methods.add_method("read", |lua, this, (offset, length): (Value, Value)| {
            let offset = buffer::integer(offset)?;
            let length = buffer::integer(length)?;
            buffer::read(lua, &mut this.file.lock(), offset, length)
        });
        methods.add_method("sha256", |_, this, ()| {
            this.file.lock().size().map_err(mlua::Error::external)?;
            Ok(this.sha256.clone())
        });
    }
}

struct Output(FileHandle);

impl UserData for Output {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("size", |_, this, ()| {
            this.0.lock().size().map_err(mlua::Error::external)
        });
        methods.add_method("read", |lua, this, (offset, length): (Value, Value)| {
            buffer::read(
                lua,
                &mut this.0.lock(),
                buffer::integer(offset)?,
                buffer::integer(length)?,
            )
        });
        methods.add_method("write", |lua, this, (offset, bytes): (Value, Buffer)| {
            let offset = buffer::integer(offset)?;
            buffer::input(lua, bytes, |bytes| this.0.lock().write(offset, bytes))
        });
        methods.add_method("truncate", |_, this, length: Value| {
            this.0
                .lock()
                .truncate(buffer::integer(length)?)
                .map_err(mlua::Error::external)
        });
        methods.add_method("close", |_, this, ()| {
            this.0.lock().close().map_err(mlua::Error::external)
        });
    }
}

struct Files {
    package: Option<AuthorizedRoot>,
    source: AuthorizedRoot,
    patch: AuthorizedRoot,
    output: AuthorizedRoot,
    cancelled: Arc<AtomicBool>,
    inputs: Mutex<BTreeMap<(String, String), String>>,
    outputs: Mutex<Vec<FileHandle>>,
}

impl Files {
    fn input_root(&self, root: &str) -> Result<&AuthorizedRoot, PatchError> {
        match root {
            "source" => Ok(&self.source),
            "patch" => Ok(&self.patch),
            "package" => self.package.as_ref().ok_or_else(|| {
                PatchError::new(
                    PatchErrorKind::InvalidRequest,
                    "package resources require a ZIP patch",
                )
            }),
            _ => Err(PatchError::new(
                PatchErrorKind::InvalidRequest,
                "input root must be source, patch or package",
            )),
        }
    }

    fn open_input(&self, root: &str, relative: &str) -> Result<TaskFile, PatchError> {
        check_cancelled(&self.cancelled)?;
        TaskFile::new(
            self.input_root(root)?.open_read(relative)?,
            false,
            self.cancelled.clone(),
        )
    }

    fn record(&self, root: &str, path: &str, sha256: String) -> Result<(), PatchError> {
        let mut inputs = self.inputs.lock();
        let key = (root.to_owned(), path.to_owned());
        if let Some(previous) = inputs.get(&key) {
            if previous != &sha256 {
                return Err(PatchError::new(
                    PatchErrorKind::Integrity,
                    "input changed between reads",
                ));
            }
        } else {
            inputs.insert(key, sha256);
        }
        Ok(())
    }

    fn output_handle(&self, path: &str, edit: bool) -> Result<Output, PatchError> {
        check_cancelled(&self.cancelled)?;
        let file = if edit {
            self.output.edit(path)?
        } else {
            self.output.create(path)?
        };
        let file = Arc::new(Mutex::new(TaskFile::new(
            file,
            true,
            self.cancelled.clone(),
        )?));
        self.outputs.lock().push(file.clone());
        Ok(Output(file))
    }

    fn close_outputs(&self) -> Result<(), PatchError> {
        for file in self.outputs.lock().iter() {
            file.lock().close()?;
        }
        Ok(())
    }
}

struct Scripts {
    root: AuthorizedRoot,
    cancelled: Arc<AtomicBool>,
    hashes: Mutex<BTreeMap<String, String>>,
    cache: Mutex<BTreeMap<String, RegistryKey>>,
    loading: Mutex<BTreeSet<String>>,
}

impl Scripts {
    fn load(&self, lua: &Lua, path: &str) -> mlua::Result<Value> {
        crate::paths::relative(path, false).map_err(mlua::Error::external)?;
        if !std::path::Path::new(path)
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("luau"))
        {
            return Err(mlua::Error::external(PatchError::new(
                PatchErrorKind::Path,
                "modules must be Luau source files",
            )));
        }
        check_cancelled(&self.cancelled).map_err(mlua::Error::external)?;
        if let Some(key) = self.cache.lock().get(path) {
            return lua.registry_value(key);
        }
        if !self.loading.lock().insert(path.to_owned()) {
            return Err(mlua::Error::external(PatchError::new(
                PatchErrorKind::Script,
                "cyclic script module load",
            )));
        }
        let result = (|| {
            let mut source =
                TaskFile::new(self.root.open_read(path)?, false, self.cancelled.clone())?;
            let length = usize::try_from(source.size()?).map_err(|cause| {
                PatchError::with_source(PatchErrorKind::Bounds, "script length overflow", cause)
            })?;
            buffer::bounded(length)?;
            let mut bytes = storage::scratch(length)?;
            let mut hash = Sha256::new();
            for (index, chunk) in bytes
                .chunks_mut(roukaku_byte_source::AUDIT_CHUNK_BYTES)
                .enumerate()
            {
                source.read(
                    (index * roukaku_byte_source::AUDIT_CHUNK_BYTES) as u64,
                    chunk,
                )?;
                hash.update(&*chunk);
            }
            self.hashes
                .lock()
                .insert(path.to_owned(), hex::encode(hash.finalize()));
            let value = lua
                .load(&bytes)
                .set_name(format!("@{path}"))
                .set_mode(mlua::chunk::ChunkMode::Text)
                .eval::<Value>()?;
            let key = lua.create_registry_value(value.clone())?;
            self.cache.lock().insert(path.to_owned(), key);
            Ok(value)
        })();
        self.loading.lock().remove(path);
        result
    }
}

pub(crate) fn execute(
    request: &PatchRequest,
    transaction: &Transaction,
    cancelled: Arc<AtomicBool>,
    progress: &(impl Fn(PatchProgress) + Send + Sync),
    downloads: Option<&[shinki_emu_metadata::PatchDownload]>,
    package_root: Option<&std::path::Path>,
) -> Result<Execution, PatchError> {
    check_cancelled(&cancelled)?;
    let script = std::fs::canonicalize(&request.script).map_err(|cause| {
        PatchError::with_source(PatchErrorKind::Path, "cannot open entry script", cause)
    })?;
    let script_parent = script
        .parent()
        .ok_or_else(|| PatchError::new(PatchErrorKind::Path, "entry script requires a parent"))?;
    let entry = script
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            PatchError::new(PatchErrorKind::Path, "entry script name must be Unicode")
        })?;
    let lua = Lua::new_with(
        StdLib::TABLE
            | StdLib::STRING
            | StdLib::MATH
            | StdLib::UTF8
            | StdLib::BIT
            | StdLib::BUFFER
            | StdLib::COROUTINE,
        LuaOptions::default(),
    )
    .map_err(lua_error)?;
    lua.set_memory_limit(256 * 1024 * 1024).map_err(lua_error)?;
    // Scripts have no content logging or alternative file/module loading path.
    for name in [
        "print", "warn", "require", "dofile", "loadfile", "os", "io", "package", "debug",
    ] {
        lua.globals().set(name, Value::Nil).map_err(lua_error)?;
    }
    let interrupt = cancelled.clone();
    lua.set_interrupt(move |_| {
        check_cancelled(&interrupt).map_err(mlua::Error::external)?;
        Ok(mlua::VmState::Continue)
    });
    let files = Arc::new(Files {
        source: AuthorizedRoot::from_input_dir(transaction.open_source()?),
        patch: AuthorizedRoot::from_input_dir(transaction.open_patch()?),
        output: AuthorizedRoot::from_dir(transaction.open_staging()?),
        package: package_root.map(AuthorizedRoot::open).transpose()?,
        cancelled: cancelled.clone(),
        inputs: Mutex::default(),
        outputs: Mutex::default(),
    });
    let scripts = Arc::new(Scripts {
        root: AuthorizedRoot::open(script_parent)?,
        cancelled: cancelled.clone(),
        hashes: Mutex::default(),
        cache: Mutex::default(),
        loading: Mutex::default(),
    });
    let (id, version, profile) = lua
        .scope(|scope| {
            let ctx = context(&lua, files.clone(), scripts.clone())?;
            let cancellation = cancelled.clone();
            ctx.set(
                "progress",
                scope.create_function(
                    move |_, (phase, completed, total): (String, Value, Value)| {
                        check_cancelled(&cancellation).map_err(mlua::Error::external)?;
                        let completed = buffer::integer(completed)?;
                        let total = if matches!(total, Value::Nil) {
                            None
                        } else {
                            Some(buffer::integer(total)?)
                        };
                        progress(PatchProgress {
                            phase,
                            completed,
                            total,
                            ..Default::default()
                        });
                        Ok(())
                    },
                )?,
            )?;
            crate::codecs::install(&lua, &ctx, cancelled.clone())?;
            crate::data::install(&lua, &ctx, cancelled.clone())?;
            crate::pickle::install(&lua, &ctx, cancelled.clone())?;
            let native_files = files.clone();
            let staging = transaction.open_staging().map_err(mlua::Error::external)?;
            let staging_path =
                crate::transaction::final_dir_path(&staging).map_err(mlua::Error::external)?;
            ctx.set(
                "musica_embed",
                scope.create_function(
                    move |lua,
                          (profile, translations, resources, destination): (
                        String,
                        Value,
                        Value,
                        String,
                    )| {
                        native_files
                            .close_outputs()
                            .map_err(mlua::Error::external)?;
                        let mut writer = buffer::BoundedWriter::new();
                        serde_json::to_writer(&mut writer, &translations).map_err(|error| {
                            mlua::Error::external(PatchError::with_source(
                                PatchErrorKind::Bounds,
                                "invalid or oversized native translations",
                                error,
                            ))
                        })?;
                        let translations = writer.finish();
                        let resources = lua.from_value(resources)?;
                        crate::musica::embed(crate::musica::MusicaEmbedConfig {
                            root: &staging_path,
                            output: &native_files.output,
                            profile: &profile,
                            translations: &translations,
                            resources: &resources,
                            destination: &destination,
                            cancelled: &native_files.cancelled,
                            progress,
                        })
                        .map_err(mlua::Error::external)
                    },
                )?,
            )?;
            let downloaded_files = files.clone();
            let game = transaction.source_dir().map_err(mlua::Error::external)?;
            ctx.set(
                "download",
                scope.create_function(move |_, (url, sha256, size): (String, String, Value)| {
                    let size = buffer::integer(size)?;
                    crate::download::validate_request(&url, &sha256, size)
                        .map_err(mlua::Error::external)?;
                    if downloads.is_some_and(|declared| {
                        !declared.iter().any(|item| {
                            item.url == url && item.sha256 == sha256 && item.size == size
                        })
                    }) {
                        return Err(mlua::Error::external(PatchError::new(
                            PatchErrorKind::InvalidRequest,
                            format!("undeclared download: {url}"),
                        )));
                    }
                    let downloaded = crate::download::download(
                        &game,
                        &url,
                        &sha256,
                        size,
                        &downloaded_files.cancelled,
                        progress,
                    )
                    .map_err(mlua::Error::external)?;
                    downloaded_files
                        .record(
                            "download",
                            &format!("{}.bin", downloaded.sha256),
                            downloaded.sha256.clone(),
                        )
                        .map_err(mlua::Error::external)?;
                    Ok(Source {
                        file: Arc::new(Mutex::new(
                            TaskFile::new(
                                downloaded.file,
                                false,
                                downloaded_files.cancelled.clone(),
                            )
                            .map_err(mlua::Error::external)?,
                        )),
                        sha256: downloaded.sha256,
                    })
                })?,
            )?;
            ctx.set(
                "check",
                scope.create_function(
                    move |_, (label, passed, detail): (String, bool, Option<String>)| {
                        let detail = detail.unwrap_or_default();
                        if label.len() > 256 || detail.len() > 2048 {
                            return Err(mlua::Error::external(PatchError::new(
                                PatchErrorKind::Bounds,
                                "check detail exceeds its bound",
                            )));
                        }
                        progress(PatchProgress::check(
                            label.clone(),
                            Some(passed),
                            detail.clone(),
                        ));
                        if passed {
                            Ok(())
                        } else {
                            Err(mlua::Error::external(PatchError::new(
                                PatchErrorKind::UnsupportedInput,
                                if detail.is_empty() { label } else { detail },
                            )))
                        }
                    },
                )?,
            )?;
            let recipe = match scripts.load(&lua, entry)? {
                Value::Table(table) => table,
                _ => {
                    return Err(mlua::Error::external(PatchError::new(
                        PatchErrorKind::Script,
                        "entry script must return a recipe table",
                    )));
                }
            };
            let id = text(recipe.get::<Value>("id")?)?;
            let version = buffer::integer(recipe.get::<Value>("version")?)?;
            let title = text(recipe.get::<Value>("title")?)?;
            if id.trim().is_empty() || id.contains('\0') || title.trim().is_empty() || version == 0
            {
                return Err(mlua::Error::external(PatchError::new(
                    PatchErrorKind::Script,
                    "invalid recipe metadata",
                )));
            }
            let apply: Function = recipe.get("apply")?;
            let result: Value = apply.call(ctx)?;
            let profile = profile(result)?;
            validate_profile(&profile).map_err(mlua::Error::external)?;
            files.close_outputs().map_err(mlua::Error::external)?;
            check_cancelled(&cancelled).map_err(mlua::Error::external)?;
            Ok((id, version, profile))
        })
        .map_err(lua_error)?;
    // Scope invalidates borrowed progress callbacks; releasing the VM also
    // drops script-held userdata/closures before the transaction is published.
    drop(lua);
    let inputs = std::mem::take(&mut *files.inputs.lock())
        .into_iter()
        .map(|((root, path), sha256)| InputRecord { root, path, sha256 })
        .collect();
    let scripts = std::mem::take(&mut *scripts.hashes.lock())
        .into_iter()
        .map(|(path, sha256)| ScriptRecord { path, sha256 })
        .collect();
    Ok(Execution {
        id,
        version,
        profile,
        inputs,
        scripts,
    })
}

fn text(value: Value) -> mlua::Result<String> {
    let Value::String(value) = value else {
        return Err(mlua::Error::external(PatchError::new(
            PatchErrorKind::Script,
            "expected a string",
        )));
    };
    buffer::bounded(value.as_bytes().len()).map_err(mlua::Error::external)?;
    Ok(value.to_str()?.to_owned())
}

fn profile(value: Value) -> mlua::Result<OutputProfile> {
    let Value::Table(table) = value else {
        return Err(mlua::Error::external(PatchError::new(
            PatchErrorKind::Script,
            "expected an output profile table",
        )));
    };
    for item in table.clone().pairs::<Value, Value>() {
        let (key, _) = item?;
        if !matches!(
            text(key)?.as_str(),
            "plugin_id" | "configuration" | "required_capabilities"
        ) {
            return Err(mlua::Error::external(PatchError::new(
                PatchErrorKind::Script,
                "unknown output profile field",
            )));
        }
    }
    let plugin_id = text(table.raw_get::<Value>("plugin_id")?)?;
    let required_capabilities = match table.raw_get::<Value>("required_capabilities")? {
        Value::Nil => 0,
        Value::Integer(value) if value >= 0 => value as u64,
        Value::Number(value)
            if value.is_finite()
                && value.fract() == 0.0
                && value >= 0.0
                && value <= crate::PROFILE_CAPABILITIES as f64 =>
        {
            value as u64
        }
        _ => {
            return Err(mlua::Error::external(PatchError::new(
                PatchErrorKind::Script,
                "required capabilities must be an unsigned integer",
            )));
        }
    };
    let Value::Table(values) = table.raw_get::<Value>("configuration")? else {
        return Err(mlua::Error::external(PatchError::new(
            PatchErrorKind::Script,
            "profile configuration must be a table",
        )));
    };
    let mut configuration = BTreeMap::new();
    let mut bytes = plugin_id.len();
    for item in values.pairs::<Value, Value>() {
        let (key, value) = item?;
        let key = text(key)?;
        let value = text(value)?;
        bytes = bytes
            .checked_add(key.len())
            .and_then(|n| n.checked_add(value.len()))
            .and_then(|n| n.checked_add(256))
            .ok_or_else(|| {
                mlua::Error::external(PatchError::new(
                    PatchErrorKind::Memory,
                    "profile allocation overflow",
                ))
            })?;
        buffer::bounded(bytes).map_err(mlua::Error::external)?;
        configuration.insert(key, value);
    }
    Ok(OutputProfile {
        plugin_id,
        configuration,
        required_capabilities,
    })
}

fn validate_profile(profile: &OutputProfile) -> Result<(), PatchError> {
    if profile.plugin_id.trim().is_empty()
        || profile.plugin_id.contains('\0')
        || profile.required_capabilities & !crate::PROFILE_CAPABILITIES != 0
        || profile
            .configuration
            .keys()
            .any(|key| key.is_empty() || key.contains('\0'))
    {
        return Err(PatchError::new(
            PatchErrorKind::Script,
            "invalid output profile",
        ));
    }
    Ok(())
}

fn context(lua: &Lua, files: Arc<Files>, scripts: Arc<Scripts>) -> mlua::Result<Table> {
    let ctx = lua.create_table()?;
    ctx.set(
        "musica_profile",
        lua.create_function(|lua, bytes: Buffer| {
            let profile = buffer::input(lua, bytes, |bytes| {
                shinki_emu_musica_keys::parse_key_file(bytes).map_err(|error| {
                    let kind = if error.code == "SHINKI_EMU_MUSICA_PRIVATE_ROLES" {
                        PatchErrorKind::Integrity
                    } else {
                        PatchErrorKind::UnsupportedInput
                    };
                    PatchError::with_source(kind, "invalid private Musica key.toml", error)
                })
            })?;
            lua.to_value_with(
                &profile,
                mlua::serde::SerializeOptions::new().serialize_none_to_null(false),
            )
        })?,
    )?;
    let state = files.clone();
    ctx.set(
        "open",
        lua.create_function(move |_, (root, path): (String, String)| {
            let mut file = state
                .open_input(&root, &path)
                .map_err(mlua::Error::external)?;
            let sha256 = file.sha256().map_err(mlua::Error::external)?;
            state
                .record(&root, &path, sha256.clone())
                .map_err(mlua::Error::external)?;
            Ok(Source {
                file: Arc::new(Mutex::new(file)),
                sha256,
            })
        })?,
    )?;
    let state = files.clone();
    ctx.set(
        "exists",
        lua.create_function(move |_, (root, path): (String, String)| {
            check_cancelled(&state.cancelled).map_err(mlua::Error::external)?;
            state
                .input_root(&root)
                .and_then(|root| root.exists(&path))
                .map_err(mlua::Error::external)
        })?,
    )?;
    let state = files.clone();
    ctx.set(
        "list",
        lua.create_function(move |lua, (root, path): (String, String)| {
            check_cancelled(&state.cancelled).map_err(mlua::Error::external)?;
            let list = state
                .input_root(&root)
                .and_then(|root| root.list(&path))
                .map_err(mlua::Error::external)?;
            for entry in &list {
                storage::exact(entry.size).map_err(mlua::Error::external)?;
            }
            lua.to_value(&list)
        })?,
    )?;
    let state = files.clone();
    ctx.set(
        "create",
        lua.create_function(move |_, path: String| {
            state
                .output_handle(&path, false)
                .map_err(mlua::Error::external)
        })?,
    )?;
    let state = files.clone();
    ctx.set(
        "edit",
        lua.create_function(move |_, path: String| {
            state
                .output_handle(&path, true)
                .map_err(mlua::Error::external)
        })?,
    )?;
    let state = files.clone();
    ctx.set(
        "copy",
        lua.create_function(move |_, (root, source, output): (String, String, String)| {
            let mut input = state
                .open_input(&root, &source)
                .map_err(mlua::Error::external)?;
            let mut output = TaskFile::new(
                state
                    .output
                    .create(&output)
                    .map_err(mlua::Error::external)?,
                true,
                state.cancelled.clone(),
            )
            .map_err(mlua::Error::external)?;
            let sha256 = storage::copy(&mut input, &mut output).map_err(mlua::Error::external)?;
            output.close().map_err(mlua::Error::external)?;
            state
                .record(&root, &source, sha256)
                .map_err(mlua::Error::external)
        })?,
    )?;
    let state = files.clone();
    ctx.set(
        "mkdir",
        lua.create_function(move |_, path: String| {
            check_cancelled(&state.cancelled).map_err(mlua::Error::external)?;
            state.output.mkdir(&path).map_err(mlua::Error::external)
        })?,
    )?;
    let state = files.clone();
    ctx.set(
        "remove",
        lua.create_function(move |_, path: String| {
            check_cancelled(&state.cancelled).map_err(mlua::Error::external)?;
            state.output.remove(&path).map_err(mlua::Error::external)
        })?,
    )?;
    let state = files.clone();
    ctx.set(
        "rename",
        lua.create_function(move |_, (from, to): (String, String)| {
            check_cancelled(&state.cancelled).map_err(mlua::Error::external)?;
            state
                .output
                .rename(&from, &to)
                .map_err(mlua::Error::external)
        })?,
    )?;
    let cancelled = files.cancelled.clone();
    ctx.set(
        "check_cancelled",
        lua.create_function(move |_, ()| {
            check_cancelled(&cancelled).map_err(mlua::Error::external)
        })?,
    )?;
    ctx.set(
        "load",
        lua.create_function(move |lua, path: String| scripts.load(lua, &path))?,
    )?;
    ctx.set(
        "fail",
        lua.create_function(|_, (code, message): (String, String)| -> mlua::Result<()> {
            let kind = match code.as_str() {
                "INVALID_REQUEST" => PatchErrorKind::InvalidRequest,
                "UNSUPPORTED_INPUT" => PatchErrorKind::UnsupportedInput,
                "OUTPUT_EXISTS" => PatchErrorKind::OutputExists,
                "PATH" => PatchErrorKind::Path,
                "BOUNDS" => PatchErrorKind::Bounds,
                "MEMORY" => PatchErrorKind::Memory,
                "CODEC" => PatchErrorKind::Codec,
                "INTEGRITY" => PatchErrorKind::Integrity,
                "CANCELLED" => PatchErrorKind::Cancelled,
                "SCRIPT" => PatchErrorKind::Script,
                "IO" => PatchErrorKind::Io,
                "PUBLISH" => PatchErrorKind::Publish,
                _ => {
                    return Err(mlua::Error::external(PatchError::new(
                        PatchErrorKind::Script,
                        "unknown failure category",
                    )));
                }
            };
            Err(mlua::Error::external(PatchError::new(kind, message)))
        })?,
    )?;
    Ok(ctx)
}

// mlua without its `send` feature owns VM-local external errors. Snapshot the
// causal chain before dropping the VM; never mark Lua or its errors unsafe Send.
#[derive(Debug)]
struct DetachedError {
    message: String,
    cause: Option<Box<DetachedError>>,
}

impl std::fmt::Display for DetachedError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for DetachedError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.cause
            .as_deref()
            .map(|cause| cause as &(dyn std::error::Error + 'static))
    }
}

pub(crate) fn lua_error(error: mlua::Error) -> PatchError {
    let mut kind = PatchErrorKind::Script;
    for cause in error.chain() {
        if let Some(cause) = cause.downcast_ref::<PatchError>() {
            kind = cause.kind();
            break;
        }
        if let Some(mlua::Error::MemoryError(_)) = cause.downcast_ref::<mlua::Error>() {
            kind = PatchErrorKind::Memory;
        }
    }
    let messages: Vec<String> = error.chain().map(ToString::to_string).collect();
    let mut cause = None;
    for message in messages.into_iter().rev() {
        cause = Some(Box::new(DetachedError { message, cause }));
    }
    let cause = cause.unwrap_or_else(|| {
        Box::new(DetachedError {
            message: error.to_string(),
            cause: None,
        })
    });
    PatchError::with_source(kind, format!("recipe execution failed\n{cause}"), *cause)
}
