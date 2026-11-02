use crate::SourceSpan;
use roukaku_core::Hash256;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// Five foreground slots, one background and at most ten stands.
pub(crate) const MUSICA_STAGE_RESOURCE_LIMIT: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MusicaRuntimeState {
    pub script_uri: String,
    pub script_hash: Hash256,
    pub script_encoding: crate::ScriptEncoding,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name_resources: Option<crate::MusicaNameResources>,
    pub pc_line: u32,
    pub launch_mode: MusicaLaunchMode,
    pub variables: BTreeMap<String, i64>,
    pub global_variables: BTreeMap<String, i64>,
    pub wait: Option<MusicaWaitState>,
    pub message: Option<MusicaMessageState>,
    pub backlog: Vec<MusicaBacklogEntry>,
    pub backlog_bytes: u64,
    pub native_edition: crate::eden_save::EdenEdition,
    pub native_message_variables: BTreeMap<String, i64>,
    pub native_message_saveable: bool,
    /// Native CLog history, including records beyond the bounded visual backlog.
    #[serde(skip)]
    pub native_history: Vec<crate::eden_save::EdenHistoryMessage>,
    #[serde(skip)]
    pub native_foreground: String,
    #[serde(skip)]
    pub native_stand_slots: u32,
    #[serde(skip)]
    pub native_novel_page: Vec<crate::eden_save::EdenHistoryLine>,
    pub read_message_identities: Vec<Hash256>,
    pub gallery_unlocks: Vec<Hash256>,
    pub message_loads: Vec<MusicaMessageLoadState>,
    pub choice: Option<MusicaChoiceState>,
    pub stage: Option<MusicaStageCommand>,
    pub transition: MusicaTransitionState,
    pub transition_active: Option<MusicaTransitionActiveState>,
    pub image_animations: BTreeMap<String, MusicaImageAnimationState>,
    pub ani_patches: BTreeMap<String, MusicaAniPatchState>,
    pub effect: Option<MusicaEffectState>,
    pub characters: BTreeMap<u32, MusicaCharacterState>,
    pub firefly: Option<MusicaFireflyState>,
    pub secondary_effect: Option<MusicaSecondaryEffectState>,
    pub wscroll2: Option<MusicaWScroll2State>,
    pub scroll_xf: Option<MusicaScrollXfState>,
    pub linear_scroll: Option<MusicaLinearScrollState>,
    pub axis_scroll: Option<MusicaAxisScrollState>,
    pub screen_shake: Option<MusicaScreenShakeState>,
    pub panel: Option<MusicaPanelState>,
    pub audio: BTreeMap<u32, MusicaAudioState>,
    pub movie: Option<MusicaMovieState>,
    pub system_ui: MusicaSystemUiState,
    pub fixed_tick: u64,
    pub session_seed: u64,
    pub random_state: u64,
    pub instruction_count: u64,
    pub effect_sequence: u64,
    pub terminal: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MusicaWaitState {
    Voice {
        token_id: String,
        stream_id: u32,
        milliseconds: Option<u32>,
    },
    CharacterTransition {
        token_id: String,
        slot_id: u32,
        milliseconds: u32,
    },
    LinearScroll {
        token_id: String,
        milliseconds: u32,
    },
    AxisScroll {
        token_id: String,
        milliseconds: u32,
    },
    Time {
        token_id: String,
        timer_ticks: u32,
        milliseconds: u32,
    },
    Input {
        token_id: String,
    },
    Media {
        token_id: String,
        media_id: String,
    },
    Presentation {
        token_id: String,
        fence_id: String,
    },
    Provider {
        token_id: String,
        request_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MusicaMessageState {
    pub read_identity: Hash256,
    pub auto_advance: bool,
    pub wait_for_voice: bool,
    pub source: SourceSpan,
    pub message_id: i64,
    /// Keep ANI matching independent of the masked or bracketed display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub animation_speaker: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MusicaChoiceState {
    pub source: SourceSpan,
    pub selected_index: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
pub struct MusicaTransitionState {
    pub mode: i32,
    pub resource: Option<String>,
    /// Native `tr_Param`: the transition speed denominator. The native driver
    /// computes `phase = param * elapsed_10ms_ticks / 10` and a mode-0 crossfade
    /// completes when phase reaches 256, i.e. after `25_600 / param` ms.
    pub param: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MusicaTransitionActiveState {
    pub method: i32,
    pub param: u32,
    pub elapsed_ns: u64,
    pub duration_ns: u64,
    /// Snapshot of the stage replaced by the transitioning stage. The renderer
    /// keeps it fully opaque while the new stage fades in on top.
    pub previous: Box<MusicaStageCommand>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MusicaImageAnimationState {
    /// Index of the SQZ frame currently displayed for the PNG resource URI.
    pub frame_index: u32,
    /// Physical SQZ frame count (`2 * header pair count`).
    pub frame_count: u32,
    /// Per-frame interval from the native SQZ header: `1000 / pair_count` ms.
    pub interval_ns: u64,
    pub elapsed_ns: u64,
}

/// Eye/mouth patch state for one character of a stage ANI patch set
/// (`<image>.ani`). Semantics follow the native minori driver: the eye walker
/// steps the fixed 100 Hz sequence tables (adding 50..549 ticks after each
/// fully open eye), the mouth flaps only while the normalized speaker matches
/// the character name and the voice level clears the native 0x320 threshold.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MusicaAniPatchCharacterState {
    pub name: String,
    /// Native default sequence table index (2).
    pub eye_sequence: u8,
    pub eye_index: u8,
    pub eye_countdown: u32,
    /// 0 = 開き目, 1 = 中目, 2 = 閉じ目.
    pub eye_state: u8,
    /// 0 = 閉じ口, 1 = 中口, 2 = 開き口.
    pub mouth_state: u8,
    pub mouth_prev_level: u32,
    pub mouth_last_tick: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MusicaAniPatchState {
    pub characters: Vec<MusicaAniPatchCharacterState>,
    pub elapsed_ns: u64,
    pub tick: u64,
    /// Per-set wrapping LCG state feeding the post-open-eye random extension.
    pub rng: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MusicaEffectState {
    pub kind: MusicaEffectKind,
    pub resources: Vec<Option<String>>,
    pub current_index: u32,
    pub next_index: u32,
    pub alpha_255: u32,
    pub alpha_step: u32,
    pub interval_ms: u32,
    pub elapsed_ns: u64,
    pub visible_current_index: u32,
    pub visible_next_index: u32,
    pub visible_alpha_255: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MusicaPanelState {
    pub mode: u32,
    pub resource_uri: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MusicaEffectKind {
    CrossFade2,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MusicaEffectFrame {
    pub sequence: u64,
    pub current_resource_uri: Option<String>,
    pub next_resource_uri: Option<String>,
    pub alpha_255: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MusicaStageCommand {
    /// Native composite foreground layers. `main` is the first comma slot of the
    /// colon suffix, `second`/`third` follow further commas, `plus` comes after
    /// `+`, and `prefix` is the colon head (drawn above the main foreground).
    pub foreground: MusicaForegroundLayers,
    pub reference_position: Option<[i32; 2]>,
    pub background: Option<MusicaStageLayer>,
    pub stands: Vec<MusicaStandLayer>,
    pub transition: MusicaTransitionState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
pub struct MusicaForegroundLayers {
    pub main: Option<String>,
    pub second: Option<String>,
    pub third: Option<String>,
    pub plus: Option<String>,
    pub prefix: Option<String>,
}

impl MusicaForegroundLayers {
    /// URIs in native draw order: main, second, third, prefix, plus.
    pub fn uris_in_order(&self) -> Vec<&str> {
        [
            &self.main,
            &self.second,
            &self.third,
            &self.prefix,
            &self.plus,
        ]
        .into_iter()
        .flatten()
        .map(String::as_str)
        .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MusicaStageLayer {
    pub resource_uri: String,
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MusicaStandLayer {
    pub resource_uri: String,
    pub position: i32,
    pub resource_parameter: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MusicaAudioState {
    pub bus: String,
    pub resource_uri: String,
    pub looped: bool,
    pub volume_milli: u16,
    pub pan_milli: i16,
    pub playing: bool,
    pub continuation_pts: u64,
}

/// Resource token accepted by the original BGM/SE path. The bracket suffix is
/// family metadata, not part of the archive entry name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MusicaAudioResourceSpec {
    pub resource: String,
    pub volume_percent: u16,
    pub pan_percent: i16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MusicaMovieState {
    pub media_id: String,
    pub resource_uri: String,
    pub width: u32,
    pub height: u32,
    pub skippable: bool,
    pub continuation_pts: u64,
    pub fence_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum MusicaSystemPage {
    #[default]
    None,
    Title,
    Load,
    Save,
    Config,
    Backlog,
    GalleryCg,
    GalleryBgm,
    GalleryReplay,
    Memories,
    GalleryMovie,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MusicaSystemUiState {
    pub page: MusicaSystemPage,
    pub focus_index: u32,
    pub play_mode: MusicaPlayMode,
    pub skip_enabled: bool,
    pub control_enabled: bool,
    pub backlog_cursor: Option<u32>,
    pub pending_save_slot: Option<u32>,
    pub pending_load_slot: Option<u32>,
}

impl Default for MusicaSystemUiState {
    fn default() -> Self {
        Self {
            page: MusicaSystemPage::default(),
            focus_index: 0,
            play_mode: MusicaPlayMode::Normal,
            skip_enabled: true,
            control_enabled: false,
            backlog_cursor: None,
            pending_save_slot: None,
            pending_load_slot: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum MusicaVmEvent {
    Wait(MusicaWaitState),
    Movie(MusicaMovieState),
    Message {
        presentation_sequence: u64,
        capture_sequence: u64,
        audio_commands: Vec<MusicaAudioCommand>,
        text: String,
        speaker: Option<String>,
        wait: MusicaWaitState,
    },
    Audio {
        commands: Vec<MusicaAudioCommand>,
    },
    Stage(MusicaStageCommand),
    Effect(MusicaEffectFrame),
    EffectCleared,
    ScrollXf(MusicaScrollXfFrame),
    WScroll2(MusicaWScroll2Frame),
    Character(MusicaCharacterFrame),
    Firefly(MusicaFireflyFrame),
    FireflyCleared {
        sequence: u64,
    },
    SecondaryEffect(MusicaSecondaryEffectFrame),
    SecondaryEffectCleared {
        sequence: u64,
    },
    LinearScroll(MusicaLinearScrollFrame),
    AxisScroll(MusicaAxisScrollFrame),
    ScreenShake(MusicaScreenShakeFrame),
    Choice,
    Panel {
        sequence: u64,
    },
    Chain {
        target: String,
    },
    Terminal,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MusicaAudioCommand {
    LoadResource {
        sequence: u64,
        stream_id: u32,
        resource_uri: String,
    },
    Play {
        sequence: u64,
        stream_id: u32,
        volume: f32,
        pan: f32,
        repeat: bool,
        fade_in_ms: u32,
    },
    Stop {
        sequence: u64,
        stream_id: u32,
        fade_ms: u32,
    },
    SetParams {
        sequence: u64,
        stream_id: u32,
        volume: f32,
        pan: f32,
        repeat: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MusicaScreenShakeState {
    pub kind: MusicaScreenShakeKind,
    pub amplitude: i32,
    pub interval_ms: u32,
    pub elapsed_ns: u64,
    pub update_index: u64,
    pub offset: [i32; 2],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MusicaScreenShakeKind {
    Random,
    Vertical,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MusicaScreenShakeFrame {
    pub sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MusicaAxisScrollState {
    pub axis: MusicaAxisScrollAxis,
    pub start: i32,
    pub target: i32,
    /// Native scroll speed in tenths of a pixel per millisecond.
    pub speed_tenths: i32,
    pub duration_ms: u32,
    pub elapsed_ns: u64,
    pub current: i32,
    pub completed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MusicaAxisScrollAxis {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MusicaAxisScrollFrame {
    pub sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MusicaLinearScrollState {
    pub start: [i32; 2],
    pub target: [i32; 2],
    pub speed_tenths: u32,
    pub duration_ms: u32,
    pub elapsed_ns: u64,
    pub current: [i32; 2],
    pub completed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MusicaLinearScrollFrame {
    pub sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MusicaScrollXfState {
    pub start_extent: [i32; 2],
    pub end_extent: [i32; 2],
    pub start_offset: [i32; 2],
    pub end_offset: [i32; 2],
    pub duration_ms: u32,
    pub easing: u8,
    pub elapsed_ns: u64,
    pub completed: bool,
    pub visible_extent: [i32; 2],
    pub visible_offset: [i32; 2],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MusicaScrollXfFrame {
    pub sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MusicaWScroll2State {
    pub sync_resource_uri: String,
    pub period_ticks: u32,
    pub speed_tenths: i32,
    pub elapsed_ns: u64,
    pub elapsed_ticks: u64,
    pub foreground_offset: i64,
    pub background_offset: i64,
    pub background_remainder: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MusicaWScroll2Frame {
    pub sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MusicaFireflyState {
    pub resources: [String; 3],
    pub target_count: u32,
    pub duration_ms: u32,
    pub ending: bool,
    pub fade_alpha_256: u16,
    pub fade_elapsed_ns: u64,
    pub particles: Vec<MusicaFireflyParticle>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MusicaFireflyParticle {
    pub control_points: [[i32; 2]; 7],
    pub kind: u8,
    pub elapsed_ns: u64,
    pub lifetime_ns: u64,
    pub position: [i32; 2],
    pub opacity_255: u16,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MusicaSecondaryEffectState {
    pub kind: MusicaSecondaryEffectKind,
    pub resources: [String; 3],
    pub ending: bool,
    pub alpha_256: u16,
    pub fade_elapsed_ns: u64,
    pub motion_elapsed_ns: u64,
    pub particles: Vec<MusicaSnowHParticle>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MusicaSecondaryEffectKind {
    SnowHorizontal,
    SnowVertical,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MusicaSnowHParticle {
    pub fixed_position: [i64; 2],
    pub horizontal_velocity: u32,
    pub vertical_velocity: u32,
    pub vertical_positive: bool,
    pub kind: u8,
    pub position: [i32; 2],
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MusicaFireflyFrame {
    pub sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MusicaSecondaryEffectFrame {
    pub sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MusicaCharacterState {
    /// Original character manager key. The native engine normalizes signed
    /// command ids with `abs` before lookup.
    pub slot_id: u32,
    /// The sign captured by `.char load`. Native CCharLayer stores it on both
    /// sprite nodes and later position commands retain it.
    pub positive_orientation: bool,
    pub resource_uris: Vec<String>,
    /// Native CCharLayer anchor: horizontal center and bottom-relative Y.
    pub anchor_position: [i32; 2],
    pub visible: bool,
    pub opacity_256: u16,
    pub transition: Option<MusicaCharacterTransitionState>,
    /// Native inline `load` prepares a second sprite node and cross-fades it
    /// against the current node before atomically promoting the replacement.
    pub replacement: Option<MusicaCharacterReplacementState>,
    /// A `.char load` prepares this slot for the next `.stage`. Native
    /// `CCharLayerManager` keeps newly prepared layers while retiring older
    /// unmarked layers at the stage boundary.
    pub pending_stage: bool,
    /// Native `CCharLayer` one-shot retention flag. `.char keep` sets it;
    /// scene finalization consumes it when deciding which previous-scene
    /// characters survive into the next scene.
    pub keep_once: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MusicaCharacterTransitionState {
    pub start_opacity_256: u16,
    pub target_opacity_256: u16,
    pub duration_ms: u32,
    pub elapsed_ns: u64,
    pub completed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MusicaCharacterReplacementState {
    pub resource_uri: String,
    pub start_opacity_256: u16,
    pub target_opacity_256: u16,
    pub next_opacity_256: u16,
    pub duration_ms: u32,
    pub elapsed_ns: u64,
    pub completed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MusicaCharacterFrame {
    pub sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MusicaMessageLoadState {
    pub delay_ms: u32,
    pub elapsed_ns: u64,
    pub slot_id: u32,
    pub resource_uri: String,
    pub transition_ms: u32,
    pub opacity_256: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MusicaBacklogEntry {
    /// Native CLog history has no source byte span; the active message does.
    pub source: Option<SourceSpan>,
    pub message_id: i64,
    pub text: String,
    pub speaker: Option<String>,
    pub text_hash: Hash256,
    pub speaker_hash: Option<Hash256>,
    pub voice_hash: Option<Hash256>,
    pub voice: Option<MusicaMessageVoice>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MusicaMessageVoice {
    pub resource_uri: String,
    pub volume_milli: u16,
    pub pan_milli: i16,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MusicaPlayMode {
    #[default]
    Normal,
    Auto,
    Skip,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum MusicaLaunchMode {
    #[default]
    Direct,
    Title,
}
