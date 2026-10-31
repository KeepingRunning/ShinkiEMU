//! Explicit trusted Luau file conversion, separate from runtime text extensions.
mod buffer;
mod catalog;
mod codecs;
mod data;
mod error;
mod inplace;
mod paths;
mod pickle;
mod receipt;
mod storage;
mod transaction;
mod vm;
pub use catalog::{RecipeListing, list_recipes};
pub use error::{PatchError, PatchErrorKind};
pub use receipt::read_output_profile;

pub(crate) const STAGING_PREFIX: &str = ".shinki-patch-staging-";
pub const GAME_STATE_DIR: &str = ".shinki";
pub const BACKUP_DIR: &str = "backup";
pub const PATCH_DIR: &str = "patch";
pub const STAGING_DIR: &str = "staging";
pub use inplace::{has_backup, restore_latest};

/// The private transaction namespace is never a discoverable game/output root.
pub fn is_staging_path(path: &std::path::Path) -> bool {
    path.components().any(|component| {
        component.as_os_str().to_str().is_some_and(|name| {
            name.eq_ignore_ascii_case(GAME_STATE_DIR)
                || name
                    .get(..STAGING_PREFIX.len())
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case(STAGING_PREFIX))
        })
    })
}

use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

pub struct PatchRequest {
    pub script: PathBuf,
    pub source: PathBuf,
    pub patch: PathBuf,
    pub mode: PatchMode,
}

#[derive(Clone, Debug)]
pub enum PatchMode {
    NewDirectory { output: PathBuf },
    InPlace,
}

pub(crate) const PROFILE_CAPABILITIES: u64 =
    shinki_emu_abi::CAP_NATIVE_SCRIPT_IDENTITY | shinki_emu_abi::CAP_CONFIGURED_FONT_PRIORITY;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputProfile {
    pub plugin_id: String,
    pub configuration: BTreeMap<String, String>,
    /// Semantic capabilities required from the loaded core's native descriptor.
    #[serde(
        default,
        skip_serializing_if = "no_required_capabilities",
        deserialize_with = "required_capabilities"
    )]
    pub required_capabilities: u64,
}

fn no_required_capabilities(required: &u64) -> bool {
    *required == 0
}

fn required_capabilities<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
    let required = u64::deserialize(deserializer)?;
    if required & !PROFILE_CAPABILITIES != 0 {
        return Err(serde::de::Error::custom("unknown required core capability"));
    }
    Ok(required)
}

#[derive(Clone)]
pub struct PatchProgress {
    pub phase: String,
    pub completed: u64,
    pub total: Option<u64>,
}

pub struct PatchResult {
    pub output: PathBuf,
    pub profile: OutputProfile,
}

pub fn apply(
    request: PatchRequest,
    cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    progress: impl Fn(PatchProgress) + Send + Sync,
) -> Result<PatchResult, PatchError> {
    error::check_cancelled(&cancelled)?;
    let transaction = transaction::Transaction::new(&request)?;
    progress(PatchProgress {
        phase: "applying recipe".into(),
        completed: 0,
        total: None,
    });
    let execution = vm::execute(&request, &transaction, cancelled.clone(), &progress)?;
    let root = paths::AuthorizedRoot::from_dir(transaction.open_staging()?);
    receipt::write(&root, &execution, cancelled.clone(), &progress)?;
    drop(root);
    let output = match request.mode {
        PatchMode::NewDirectory { .. } => transaction.publish(&cancelled)?,
        PatchMode::InPlace => inplace::commit(transaction, &execution.id, &cancelled, &progress)?,
    };
    progress(PatchProgress {
        phase: "complete".into(),
        completed: 1,
        total: Some(1),
    });
    Ok(PatchResult {
        output,
        profile: execution.profile,
    })
}
