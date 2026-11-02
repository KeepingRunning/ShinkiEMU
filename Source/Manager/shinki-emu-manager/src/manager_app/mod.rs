use crate::{
    core_package, metadata_cache, metadata_runtime, platform_secret, stage_renderer,
    text_service::TextReplacementBridge,
};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    env, fs,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use metadata_runtime::{MetadataCommand, MetadataCommandKind, MetadataPayload, MetadataRuntime};
use platform_secret::ManagerSecretStore;
use shinki_emu_manager::modal_service::ModalBridge;
use shinki_emu_manager::{HostExit, HostWake, ManagerController, run_manager_with_initial_state};
use shinki_emu_manager_core::model::{
    FamilyEvent, FamilyStatus, KeyCode, KeyModifiers, KeyState, ProbeRequest, WindowState,
};
use shinki_emu_manager_core::{
    FamilyPluginDescriptor, FamilyProbeCandidate, FamilyProbeSelection, FamilyProviderRegistry,
    GameRecord, GameRelease, GamepadDeadzone, GamepadInput, Library, NativeOpenRequest,
    NativeSession, PlaySessionEndReason, default_vn_preset, input_key_code as key_code,
};
use shinki_emu_manager_ui_slint::{
    AppearanceViewModel, ConfigFieldViewModel, GamepadBindingViewModel, ManagerViewModel,
};
use shinki_emu_metadata::{
    BangumiPlayStatus, BangumiPlayUpdate, CompatibilityEntry, CoverAsset,
    DEFAULT_METADATA_BASE_URL, MetadataProviderId, MetadataRecord, MetadataRelease, MetadataSearchQuery,
    plain_description, unique_exact_match,
};
use shinki_emu_translation::{OpenAiApi, SecretResolver, TranslationProfile, TranslationProvider};
use stage_renderer::{FrameMailbox, ManagerStageRenderer};

const MAX_SCAN_DIRECTORIES: usize = 4_096;
const MAX_SCAN_DEPTH: usize = 5;

mod background;
mod connection_test;
mod controller;
mod cores;
#[cfg(not(any(target_os = "android", target_env = "ohos")))]
mod extensions;
mod family_config;
mod file_patch;
mod patch_download;
mod filters;
mod input;
mod library;
mod metadata;
mod metadata_actions;
mod metadata_auto_link;
#[cfg(test)]
mod metadata_auto_link_tests;
mod metadata_sync;
#[cfg(test)]
mod metadata_tests;
mod model;
mod navigation;
mod owner;
mod owner_worker;
mod preferences;
mod probe_cache;
#[cfg(test)]
mod probe_state_tests;
mod probe_worker;
mod render_extent;
mod runtime;
mod session;
#[cfg(test)]
mod session_finish_tests;
mod snackbar;
mod self_update;
mod store;
mod support;
mod translation;
mod view_fields;
#[cfg(not(any(target_os = "android", target_env = "ohos")))]
pub(super) fn run_extension_command(command: crate::ExtensionCommand) -> Result<(), String> {
    extensions::run(command)
}
use session::ActiveFamilySession;
use support::*;

enum MetadataStatus {
    Message(String),
    Error(String),
}

impl MetadataStatus {
    fn text(&self) -> &str {
        match self {
            Self::Message(message) | Self::Error(message) => message,
        }
    }
}

struct PendingMetadata {
    game_id: Option<String>,
    core: Option<String>,
}

struct ShinkiEmuManagerController {
    library: Library,
    notice_reading: shinki_emu_manager::resale_notice::Reading,
    registry: owner::FamilyOwner,
    candidates: BTreeMap<String, FamilyProbeCandidate>,
    startup_state: background::StartupState,
    probe_choices: BTreeMap<String, Vec<FamilyProbeCandidate>>,
    selected_case_id: Option<String>,
    search_query: String,
    diagnostic: String,
    /// The diagnostic text last written by `report_retired_configuration`; new
    /// retired-path notices append while `diagnostic` still equals it.
    retired_diagnostic: Option<String>,
    diagnostic_severity: &'static str,
    plugin_errors: BTreeMap<String, String>,
    data_dir: PathBuf,
    extensions: Option<crate::text_service::ExtensionRuntime>,
    game_roots: Vec<PathBuf>,
    mailbox: FrameMailbox,
    active: Option<owner::ActiveSessionView>,
    active_play_session: Option<String>,
    pending_events: Vec<FamilyEvent>,
    window_state: Option<WindowState>,
    host_wake: Option<HostWake>,
    physical_keys: Vec<KeyCode>,
    library_sort: String,
    compatibility_filter: String,
    metadata_catalog: Option<metadata_cache::MetadataCatalog>,
    metadata_sync: metadata_sync::SyncState,
    pinned_releases: BTreeMap<String, GameRelease>,
    filter_settings: shinki_emu_manager_core::FilterSettings,
    input_mapping: shinki_emu_manager_core::InputMapping,
    appearance: shinki_emu_manager_core::AppearanceSettings,
    session_mapping: Option<shinki_emu_manager_core::InputMapping>,
    defaults_plugin: Option<String>,
    metadata: MetadataRuntime,
    metadata_sequence: u64,
    metadata_status: BTreeMap<String, MetadataStatus>,
    pending_metadata: BTreeMap<String, PendingMetadata>,
    match_candidates: BTreeMap<String, (String, MetadataRecord)>,
    /// Games waiting for an automatic VNDB search, submitted one at a time.
    auto_link_queue: VecDeque<String>,
    auto_link: Option<metadata_auto_link::AutoLinkRequest>,
    /// Games whose automatic search already ran; persisted, never retried.
    auto_link_attempted: BTreeSet<String>,
    network_allowed: bool,
    metadata_tokens: BTreeMap<String, String>,
    metadata_tokens_loaded: bool,
    sensitive_covers: bool,
    bangumi_play_status: String,
    bangumi_rating: i32,
    bangumi_note: String,
    releases: BTreeMap<String, Vec<MetadataRelease>>,
    translation_enabled: bool,
    translation_models: Vec<String>,
    translation_status: String,
    connection_test: Option<connection_test::ConnectionTest>,
    held_controls: BTreeSet<String>,
    key_modifiers: KeyModifiers,
    pointer_position: (f32, f32),
    action_message: String,
    action_serial: u32,
    patch_job: Option<file_patch::PatchJob>,
    patch_download: Option<patch_download::PatchDownloadJob>,
    patch_import: Option<file_patch::PendingImport>,
    patch_state: shinki_emu_manager_ui_slint::PatchStateViewModel,
    patch_game_id: Option<String>,
    patch_query: String,
    patch_sequence: u64,
    snackbar: snackbar::Snackbar,
    prepare_scan_pending: Option<background::PendingScan>,
    prepare_progress: Option<owner::PrepareProgress>,
    scan_cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    last_excluded: Option<String>,
    key_capture: Option<input::KeyCapture>,
    store: store::StoreRuntime,
    self_update: self_update::State,
}

impl Drop for ShinkiEmuManagerController {
    fn drop(&mut self) {
        if let Some(mut job) = self.patch_job.take() {
            job.cancel();
            if let Err(error) = job.join() {
                tracing::error!(event = "shinki.emu.manager.patch_shutdown_failed", diagnostic_code = %error);
            }
        }
        if let Err(error) = self.close_active(PlaySessionEndReason::Shutdown) {
            tracing::error!(event = "shinki.emu.manager.shutdown_failed", diagnostic_code = %error);
        }
        if let Err(error) = self.registry.shutdown() {
            tracing::error!(event = "shinki.emu.manager.owner_shutdown_failed", diagnostic_code = %error);
        }
        if let Err(error) = self.finish_play_session(PlaySessionEndReason::Shutdown) {
            tracing::error!(event = "shinki.emu.manager.play_shutdown_failed", diagnostic_code = %error);
        }
    }
}

fn initialize_network_crypto() {
    static INITIALIZE: std::sync::Once = std::sync::Once::new();
    INITIALIZE.call_once(|| {
        // reqwest 0.13/platform-verifier uses AWS-LC. VNDB's vn-core still
        // enables reqwest 0.12's ring backend, so feature inference is ambiguous.
        if rustls::crypto::CryptoProvider::get_default().is_none() {
            let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
        }
    });
}

pub(super) fn run_application() -> Result<(), Box<dyn std::error::Error>> {
    initialize_network_crypto();
    shinki_emu_manager::startup::begin();
    let data_dir = platform_data_dir()?;
    fs::create_dir_all(&data_dir)?;
    let filter = match std::env::var("RUST_LOG") {
        Ok(filter) => filter,
        Err(std::env::VarError::NotPresent) => "info".to_owned(),
        Err(error) => return Err(error.into()),
    };
    let mut observability = roukaku_observability::HostObservabilityConfig::for_cli(filter);
    observability.role = roukaku_observability::HostRole::Manager;
    observability.console = false;
    observability.log_dir = Some(data_dir.join("diagnostics"));
    let _observability = std::sync::Arc::new(roukaku_observability::init_host(observability)?);
    shinki_emu_manager::startup::mark("process_entry");
    let filter_guard = std::sync::Arc::downgrade(&_observability);
    shinki_emu_manager_core::set_diagnostic_enabled(move |level, target| {
        filter_guard
            .upgrade()
            .is_some_and(|guard| guard.external_enabled(level, target))
    });
    let emit_guard = std::sync::Arc::downgrade(&_observability);
    shinki_emu_manager_core::set_diagnostic_emit(move |record| {
        if let Some(guard) = emit_guard.upgrade()
            && let Err(error) =
                guard.record_external(record.level, &record.target, &record.event, record.fields)
        {
            tracing::error!(event = "shinki.emu.diagnostics.record_failed", error = %error);
        }
    });
    let flush_guard = std::sync::Arc::downgrade(&_observability);
    shinki_emu_manager_core::set_diagnostic_flush(move || {
        if let Some(guard) = flush_guard.upgrade()
            && let Err(error) = guard.flush()
        {
            tracing::error!(event = "shinki.emu.diagnostics.flush_failed", error = %error);
        }
    });
    let result = (|| {
        let mailbox = FrameMailbox::new();
        let controller = ShinkiEmuManagerController::open(mailbox.clone())?;
        shinki_emu_manager::startup::mark("cached_controller_ready");
        let exit =
            run_manager_with_initial_state(controller, ManagerStageRenderer::new(mailbox), false)?;
        if matches!(exit, HostExit::Restart) {
            shinki_emu_manager::platform::restart()?;
        }
        Ok(())
    })();
    if let Err(error) = &result {
        tracing::error!(
            event = "shinki.emu.manager.fatal",
            diagnostic_code = "SHINKI_EMU_MANAGER_STARTUP_FAILED",
            error = %error
        );
    }
    result
}
