mod adapter;
mod config_model;
mod covers;
pub mod fonts;
pub mod i18n;
mod list_model;
mod notices;

slint::include_modules!();

pub use adapter::SlintManagerAdapter;
pub use notices::{ErrorText, describe_error, report_error};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ManagerViewModel {
    pub app: AppViewModel,
    pub network: NetworkViewModel,
    pub library: LibraryViewModel,
    pub selection: SelectionViewModel,
    pub game_settings: GameSettingsViewModel,
    pub patch: PatchViewModel,
    pub cores: CoresViewModel,
    pub notices: NoticesViewModel,
    pub settings: SettingsViewModel,
    pub filters: FiltersViewModel,
    pub session: SessionViewModel,
    pub onboarding: OnboardingViewModel,
    pub resale_notice: ResaleNoticeViewModel,
    pub store: StoreViewModel,
}

/// State of the Rust-rendered bilingual anti-resale notice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResaleNoticeViewModel {
    pub checked: bool,
    pub remaining_seconds: i32,
    pub acknowledged: bool,
}

impl Default for ResaleNoticeViewModel {
    fn default() -> Self {
        Self {
            checked: false,
            remaining_seconds: 8,
            acknowledged: false,
        }
    }
}

/// Logical image coordinates and the surrounding scroll viewport.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct NoticeSlotRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub clip: Option<NoticeClipRect>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct NoticeClipRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AppViewModel {
    pub version: String,
    pub build: String,
    pub appearance: AppearanceViewModel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppearanceViewModel {
    pub theme_mode: String,
    pub density: String,
    pub cover_size: String,
    pub view_mode: String,
    pub reduce_motion: bool,
    pub cover_badges: bool,
    pub language: String,
}

impl Default for AppearanceViewModel {
    fn default() -> Self {
        Self {
            theme_mode: "system".into(),
            density: "comfortable".into(),
            cover_size: "medium".into(),
            view_mode: "grid".into(),
            reduce_motion: false,
            cover_badges: true,
            language: "system".into(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChoiceViewModel {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NetworkViewModel {
    pub allowed: bool,
    pub syncing: bool,
    pub sync_status: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LibraryViewModel {
    pub games: Vec<GameCardViewModel>,
    pub query: String,
    pub sort: String,
    pub compat_filter: String,
    pub prepare_state: String,
    pub prepare_label: String,
    pub prepare_detail: String,
    /// `None` means indeterminate progress.
    pub prepare_progress: Option<f32>,
    pub can_cancel_prepare: bool,
    pub selected_id: String,
}

impl Default for LibraryViewModel {
    fn default() -> Self {
        Self {
            games: Vec::new(),
            query: String::new(),
            sort: "title".into(),
            compat_filter: "all".into(),
            prepare_state: "unprepared".into(),
            prepare_label: tr!("准备").into(),
            prepare_detail: String::new(),
            prepare_progress: None,
            can_cancel_prepare: false,
            selected_id: String::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GameCardViewModel {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    /// Tells installed copies of one VNDB work apart: the pinned release
    /// title, else the install directory name. Empty for a work installed once.
    pub version: String,
    pub cover_uri: String,
    pub ready: bool,
    pub problem: String,
    pub play_time: String,
    pub last_played: String,
    pub compat: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SelectionViewModel {
    pub game: SelectedGameViewModel,
    pub emulator_choices: Vec<ChoiceViewModel>,
    pub compat: CompatViewModel,
    pub releases: Vec<ChoiceViewModel>,
    pub pinned_release: String,
    /// Other installed copies linked to the same VNDB work: `(game id, version label)`.
    pub other_versions: Vec<ChoiceViewModel>,
    pub history: Vec<PlaySessionViewModel>,
    pub bangumi: BangumiViewModel,
    pub bangumi_summary: String,
    pub metadata_busy: bool,
    pub metadata_status: String,
    /// Request state, independent of the localized status message.
    pub metadata_error: bool,
    pub matches: Vec<MatchViewModel>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SelectedGameViewModel {
    pub id: String,
    pub title: String,
    pub engine: String,
    pub emulator: String,
    pub emulator_id: String,
    pub launchable: bool,
    pub problem: String,
    pub play_time: String,
    pub last_played: String,
    pub developer: String,
    pub release_date: String,
    pub platforms: String,
    pub description: String,
    pub tags: String,
    pub aliases: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompatViewModel {
    pub status: String,
    pub notes: String,
    pub updated: String,
    pub release_id: String,
    pub emulator_label: String,
    pub report_url: String,
    pub hint: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlaySessionViewModel {
    pub started: String,
    pub duration: String,
    pub ended_by: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MatchViewModel {
    pub id: String,
    pub provider: String,
    pub title: String,
    pub aliases: String,
    pub release_date: String,
    pub developer: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BangumiViewModel {
    pub status: String,
    pub rating: i32,
    pub note: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConfigFieldViewModel {
    pub key: String,
    pub label: String,
    pub description: String,
    pub group: String,
    /// `bool | integer | float | string | enum | file | directory`
    pub kind: String,
    pub value: String,
    pub display_value: String,
    pub options: Vec<String>,
    pub option_values: Vec<String>,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    pub unit: String,
    pub advanced: bool,
    pub overridden: bool,
    pub error: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GamepadBindingViewModel {
    pub id: String,
    pub label: String,
    pub key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderViewModel {
    pub supported: bool,
    pub options: Vec<ChoiceViewModel>,
    pub mode: String,
    pub custom_width: i32,
    pub custom_height: i32,
    pub min: String,
    pub max: String,
    pub note: String,
    pub error: String,
}

impl Default for RenderViewModel {
    fn default() -> Self {
        Self {
            supported: false,
            options: Vec::new(),
            mode: "default".into(),
            custom_width: 0,
            custom_height: 0,
            min: String::new(),
            max: String::new(),
            note: String::new(),
            error: String::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecipeViewModel {
    pub id: String,
    pub title: String,
    pub recommended: bool,
}

/// Conversion job state; `step` is 0 before a run, 4 while running or failed, 5 once imported.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PatchStateViewModel {
    pub step: i32,
    pub running: bool,
    pub phase: String,
    pub completed: u64,
    pub total: Option<u64>,
    pub output: String,
    pub error: String,
    pub can_start: bool,
    pub script: String,
    pub patch_directory: String,
    pub output_directory: String,
    pub in_place: bool,
    pub has_backup: bool,
    pub downloading: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PatchGameViewModel {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub cover_uri: String,
    pub linked: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PatchViewModel {
    pub games: Vec<PatchGameViewModel>,
    pub query: String,
    /// Empty `id` when no game is selected.
    pub game: PatchGameViewModel,
    pub recipes: Vec<RecipeViewModel>,
    pub artifacts: Vec<RecipeViewModel>,
    pub hint: String,
    pub state: PatchStateViewModel,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct GameSettingsViewModel {
    pub game_id: String,
    pub title: String,
    pub emulator_choices: Vec<ChoiceViewModel>,
    pub emulator_id: String,
    pub render: RenderViewModel,
    pub fields: Vec<ConfigFieldViewModel>,
    pub recommended_available: bool,
    pub recommended_summary: String,
    pub input_override: bool,
    pub bindings: Vec<GamepadBindingViewModel>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CoresViewModel {
    pub items: Vec<CoreViewModel>,
    pub restart_required: bool,
    pub updates_available: i32,
    pub default_fields: Vec<ConfigFieldViewModel>,
    pub default_plugin: String,
    pub default_render: RenderViewModel,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CoreViewModel {
    pub id: String,
    pub name: String,
    pub engine: String,
    pub version: String,
    /// `loaded | failed | pending-install | pending-update | pending-removal | available`
    pub state: String,
    /// `bundled | user | online`
    pub source: String,
    pub message: String,
    pub capabilities: Vec<String>,
    pub removable: bool,
    pub update_version: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct NoticesViewModel {
    pub items: Vec<NoticeViewModel>,
    pub snackbar: SnackbarViewModel,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NoticeViewModel {
    pub id: String,
    pub severity: String,
    pub title: String,
    pub detail: String,
    pub code: String,
    pub core: String,
}

/// A new `serial` shows `text` again even when it repeats.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SnackbarViewModel {
    pub serial: u32,
    pub text: String,
    pub action: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TranslationFormViewModel {
    pub provider: String,
    pub base_url: String,
    pub openai_api: String,
    pub model: String,
    pub language: String,
    pub timeout_s: i32,
    pub custom_prompt: String,
    pub context_segments: i32,
    pub context_chars: i32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExcludedViewModel {
    pub id: String,
    pub title: String,
    pub location: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsViewModel {
    pub translation: TranslationFormViewModel,
    pub translation_enabled: bool,
    pub secret_saved: bool,
    pub models: Vec<String>,
    pub translation_status: String,
    pub sensitive_covers: bool,
    pub bangumi_token_saved: bool,
    pub compat_summary: String,
    pub gamepad_enabled: bool,
    pub deadzone: String,
    pub bindings: Vec<GamepadBindingViewModel>,
    pub roots: Vec<String>,
    pub excluded: Vec<ExcludedViewModel>,
}

impl Default for SettingsViewModel {
    fn default() -> Self {
        Self {
            translation: TranslationFormViewModel::default(),
            translation_enabled: false,
            secret_saved: false,
            models: Vec::new(),
            translation_status: String::new(),
            sensitive_covers: false,
            bangumi_token_saved: false,
            compat_summary: String::new(),
            gamepad_enabled: true,
            deadzone: "medium".into(),
            bindings: Vec::new(),
            roots: Vec::new(),
            excluded: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FiltersViewModel {
    pub preset: String,
    pub fields: Vec<ConfigFieldViewModel>,
}

impl Default for FiltersViewModel {
    fn default() -> Self {
        Self {
            preset: "none".into(),
            fields: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionViewModel {
    /// `opening | running`
    pub phase: String,
    pub launch_title: String,
    /// Library card whose cached cover becomes `Session.launch-cover`.
    pub launch_game_id: String,
    pub actions: Vec<EngineActionViewModel>,
    pub action_message: String,
    pub action_serial: u32,
}

impl Default for SessionViewModel {
    fn default() -> Self {
        Self {
            phase: "opening".into(),
            launch_title: String::new(),
            launch_game_id: String::new(),
            actions: Vec::new(),
            action_message: String::new(),
            action_serial: 0,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EngineActionViewModel {
    pub control: i32,
    pub label: String,
    pub icon: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OnboardingViewModel {
    pub roots: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum StoreLoginState {
    #[default]
    SignedOut,
    Connecting,
    EmailCode,
    MobileCode,
    MobileConfirmation,
    QrCode,
    SignedIn,
    Error,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum StoreLoadState {
    #[default]
    Idle,
    Loading,
    Ready,
    Error,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum StoreTransferState {
    #[default]
    Idle,
    Preparing,
    Downloading,
    Verifying,
    Cancelling,
    Cancelled,
    Complete,
    Error,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct StoreViewModel {
    pub providers: Vec<ChoiceViewModel>,
    pub provider_id: String,
    pub provider_name: String,
    pub can_qr: bool,
    pub can_credentials: bool,
    /// The provider signs in on its own pages in the embedded webview.
    pub can_web: bool,
    pub has_branches: bool,
    pub has_depots: bool,
    pub can_verify: bool,
    pub login_state: StoreLoginState,
    pub account_name: String,
    pub can_import_local: bool,
    pub qr_url: String,
    pub guard_hint: String,
    pub status: String,
    pub error: String,
    pub library_state: StoreLoadState,
    pub games: Vec<StoreGameViewModel>,
    pub query: String,
    pub selected_title_id: String,
    pub depots: Vec<StoreDepotViewModel>,
    pub branches: Vec<ChoiceViewModel>,
    pub selected_branch: String,
    pub branch_password: String,
    pub library_root: String,
    pub transfer: StoreTransferViewModel,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StoreGameViewModel {
    pub title_id: String,
    pub name: String,
    pub size_bytes: u64,
    pub platforms: Vec<String>,
    pub languages: Vec<String>,
    pub installed: bool,
    pub update_available: bool,
    pub update_unknown: bool,
    pub unavailable_reason: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StoreDepotViewModel {
    pub depot_id: String,
    pub name: String,
    pub platform: String,
    pub language: String,
    pub size_bytes: u64,
    pub selected: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct StoreTransferViewModel {
    pub state: StoreTransferState,
    pub title_id: String,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub files_done: u64,
    pub files_total: u64,
    pub speed_bytes_per_second: u64,
    pub eta_seconds: u64,
    pub progress: Option<f32>,
    pub status: String,
    pub error: String,
    pub notice: String,
    pub destination: String,
}

/// Secret-bearing actions deliberately do not implement Debug.
pub enum StoreAction {
    Open,
    SelectProvider {
        id: String,
    },
    /// Open the provider's sign-in page in the embedded webview. The host
    /// owns the webview, so it handles this and `CancelWebLogin` itself.
    WebLogin,
    CancelWebLogin,
    OpenDestination,
    Login {
        account: String,
        password: String,
    },
    QrLogin,
    ImportLocal,
    SubmitGuard {
        code: String,
    },
    Logout,
    RefreshLibrary,
    Search {
        query: String,
    },
    SelectGame {
        title_id: String,
    },
    SelectDepot {
        depot_id: String,
        selected: bool,
    },
    SelectBranch {
        branch: String,
        password: String,
    },
    ChooseLibraryRoot,
    SetLibraryRoot {
        path: String,
    },
    Download,
    Cancel,
    CheckUpdates,
    Verify,
}
