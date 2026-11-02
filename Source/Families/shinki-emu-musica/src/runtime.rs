mod backlog;
mod config;
mod config_audio;
mod control;
mod eden;
mod effects;
mod errors;
pub(crate) mod gallery;
mod model;
mod native_save;
pub(crate) mod particles;
mod playback;
pub(crate) mod progress;
mod read_state;
mod save_pages;
mod scroll;
pub(crate) mod scroll_xf;
pub(crate) mod shake;
mod stage;
pub(crate) mod title;
mod wscroll2;
use effects::*;
pub use errors::MusicaRuntimeError;
pub use model::*;
pub(crate) use read_state::ReadBitmap;
use stage::*;
mod audio_commands;
mod character;
mod message;
mod movie;
use message::execute_message;
mod choices;
use audio_commands::*;
use choices::execute_select;
use std::collections::{BTreeMap, BTreeSet};

use roukaku_core::Hash256;

use crate::{ScCommand, ScControlFlow, ScLineKind, ScOperand, ScScript};

/// Reproduces the bounded part of the original audio resource parser:
/// `resource[volume,pan]`, with volume clamped to 0..100 and pan to -100..100.
/// A missing closing bracket leaves the default metadata intact, as observed in
/// the original handler. URI resolution remains a separate fail-closed step.
pub fn parse_audio_resource_spec(
    token: &str,
) -> Result<MusicaAudioResourceSpec, MusicaRuntimeError> {
    if token.is_empty() || token.len() > 4 * 1024 || token.contains('\0') {
        return Err(MusicaRuntimeError::AudioResource);
    }
    let Some(open) = token.find('[') else {
        return Ok(MusicaAudioResourceSpec {
            resource: token.to_owned(),
            volume_percent: 100,
            pan_percent: 0,
        });
    };
    let resource = &token[..open];
    if resource.is_empty() {
        return Err(MusicaRuntimeError::AudioResource);
    }
    let Some(relative_close) = token[open + 1..].find(']') else {
        return Ok(MusicaAudioResourceSpec {
            resource: resource.to_owned(),
            volume_percent: 100,
            pan_percent: 0,
        });
    };
    let close = open + 1 + relative_close;
    let metadata = &token[open + 1..close];
    let (volume, pan) = metadata
        .split_once(',')
        .map_or((metadata, ""), |(volume, pan)| (volume, pan));
    let volume = parse_c_decimal_prefix(volume).unwrap_or(100).clamp(0, 100);
    let pan = parse_c_decimal_prefix(pan).unwrap_or(0).clamp(-100, 100);
    Ok(MusicaAudioResourceSpec {
        resource: resource.to_owned(),
        volume_percent: volume as u16,
        pan_percent: pan as i16,
    })
}

fn parse_c_decimal_prefix(value: &str) -> Option<i32> {
    let bytes = value.as_bytes();
    let mut end = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    let digit_start = end;
    while bytes.get(end).is_some_and(u8::is_ascii_digit) {
        end += 1;
    }
    if end == digit_start {
        return None;
    }
    value[..end].parse().ok()
}

pub struct MusicaVm {
    script: ScScript,
    labels: BTreeMap<String, u32>,
    state: MusicaRuntimeState,
    control_pressed: bool,
    config: crate::MusicaConfigState,
    config_edit: Option<config::ConfigEdit>,
    read_flags: read_state::ReadFlags,
    native_auto_cycle: u64,
    native_auto_count: u64,
    native_auto_pending: Option<crate::eden_save::EdenCheckpoint>,
}

impl MusicaVm {
    pub(crate) fn set_name_resources(&mut self, names: Option<crate::MusicaNameResources>) {
        self.state.name_resources = names;
    }

    pub(crate) fn set_voice_preferences(
        &mut self,
        value: crate::voice_preferences::VoicePreferences,
    ) {
        self.config.backlog_voice_playback = value.backlog_voice_playback;
        self.config.character_voice_enabled = value.character_voice_enabled;
    }
    #[cfg(test)]
    pub(crate) fn voice_preferences(&self) -> crate::voice_preferences::VoicePreferences {
        self.config.voice_preferences()
    }

    pub fn new(
        script_uri: String,
        script_hash: Hash256,
        script: ScScript,
        session_seed: u64,
    ) -> Result<Self, MusicaRuntimeError> {
        let labels = build_labels(&script)?;
        let state = MusicaRuntimeState {
            script_uri,
            script_hash,
            script_encoding: script.encoding,
            name_resources: None,
            pc_line: 0,
            launch_mode: MusicaLaunchMode::Direct,
            variables: BTreeMap::new(),
            global_variables: BTreeMap::new(),
            wait: None,
            message: None,
            message_loads: Vec::new(),
            read_message_identities: Vec::new(),
            gallery_unlocks: Vec::new(),
            backlog: Vec::new(),
            backlog_bytes: 0,
            native_edition: crate::eden_save::EdenEdition::English,
            native_message_variables: BTreeMap::new(),
            native_message_saveable: false,
            native_history: Vec::new(),
            native_foreground: String::new(),
            native_stand_slots: 0,
            native_novel_page: Vec::new(),
            choice: None,
            stage: None,
            transition: MusicaTransitionState::default(),
            transition_active: None,
            image_animations: BTreeMap::new(),
            ani_patches: BTreeMap::new(),
            effect: None,
            screen_shake: None,
            axis_scroll: None,
            linear_scroll: None,
            scroll_xf: None,
            wscroll2: None,
            characters: BTreeMap::new(),
            firefly: None,
            secondary_effect: None,
            panel: None,
            audio: BTreeMap::new(),
            movie: None,
            system_ui: MusicaSystemUiState::default(),
            fixed_tick: 0,
            session_seed,
            random_state: session_seed,
            instruction_count: 0,
            effect_sequence: 0,
            terminal: false,
        };
        Ok(Self {
            control_pressed: false,
            config: Default::default(),
            config_edit: None,
            read_flags: read_state::ReadFlags::new(&state.script_uri),
            native_auto_cycle: 0,
            native_auto_count: 0,
            native_auto_pending: None,
            script,
            labels,
            state,
        })
    }

    pub fn state(&self) -> &MusicaRuntimeState {
        &self.state
    }

    #[cfg(test)]
    pub(crate) fn checked_test_state(
        state: &MusicaRuntimeState,
    ) -> Result<MusicaRuntimeState, MusicaRuntimeError> {
        validate_stage_state(state.stage.as_ref())?;
        validate_transition_and_animations(state)?;
        save_pages::validate_state(state)?;
        movie::validate_movie_state(state)?;
        scroll::validate_state(state)?;
        if let Some(shake) = &state.screen_shake {
            shake::validate_screen_shake_state(shake)?;
        }
        Ok(state.clone())
    }

    pub fn replace_script(
        &mut self,
        script_uri: String,
        script_hash: Hash256,
        script: ScScript,
        label: Option<&str>,
    ) -> Result<(), MusicaRuntimeError> {
        let labels = build_labels(&script)?;
        let pc = match label {
            Some(label) => *labels.get(label).ok_or(MusicaRuntimeError::Label)?,
            None => 0,
        };
        self.read_flags.switch_script(&script_uri);
        self.state.script_encoding = script.encoding;
        self.script = script;
        self.labels = labels;
        self.state.script_uri = script_uri;
        self.state.script_hash = script_hash;
        self.state.pc_line = pc;
        self.state.variables.clear();
        self.state.wait = None;
        self.state.message = None;
        self.state.message_loads.clear();
        self.state.choice = None;
        self.state.movie = None;
        self.state.screen_shake = None;
        self.state.axis_scroll = None;
        self.state.linear_scroll = None;
        self.state.wscroll2 = None;
        self.state.firefly = None;
        self.state.secondary_effect = None;
        self.state.transition_active = None;
        self.state.image_animations.clear();
        self.state.ani_patches.clear();
        self.state.terminal = false;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn install_test_state(
        &mut self,
        restored: &MusicaRuntimeState,
        next_fixed_tick: u64,
    ) -> Result<(), MusicaRuntimeError> {
        if restored.script_uri != self.state.script_uri
            || restored.script_hash != self.state.script_hash
            || restored.script_encoding != self.script.encoding
            || restored.name_resources != self.state.name_resources
            || restored.session_seed != self.state.session_seed
            || restored.pc_line as usize > self.script.lines.len()
        {
            return Err(MusicaRuntimeError::State);
        }
        choices::validate_choice(&self.script, restored)?;
        save_pages::validate_state(restored)?;
        movie::validate_movie_state(restored)?;
        validate_stage_state(restored.stage.as_ref())?;
        validate_transition_and_animations(restored)?;
        scroll::validate_state(restored)?;
        if let Some(shake) = &restored.screen_shake {
            shake::validate_screen_shake_state(shake)?;
        }
        let restored_tick = next_fixed_tick
            .checked_sub(1)
            .ok_or(MusicaRuntimeError::State)?;
        self.state = restored.clone();
        self.config_edit = None;
        self.state.fixed_tick = restored_tick;
        self.read_flags.resume(&self.state);
        Ok(())
    }

    pub fn resolve_wait(&mut self, token_id: &str) -> Result<(), MusicaRuntimeError> {
        let current = self
            .state
            .wait
            .as_ref()
            .ok_or(MusicaRuntimeError::Waiting)?;
        let expected = match current {
            MusicaWaitState::Voice { token_id, .. }
            | MusicaWaitState::CharacterTransition { token_id, .. }
            | MusicaWaitState::LinearScroll { token_id, .. }
            | MusicaWaitState::AxisScroll { token_id, .. }
            | MusicaWaitState::Time { token_id, .. }
            | MusicaWaitState::Input { token_id }
            | MusicaWaitState::Media { token_id, .. }
            | MusicaWaitState::Presentation { token_id, .. }
            | MusicaWaitState::Provider { token_id, .. } => token_id,
        };
        if expected != token_id {
            return Err(MusicaRuntimeError::Waiting);
        }
        if !self.config.animation {
            match self.state.wait.clone() {
                Some(MusicaWaitState::LinearScroll { milliseconds, .. }) => {
                    self.advance_linear_scroll_clock(u64::from(milliseconds) * 1_000_000)?;
                }
                Some(MusicaWaitState::AxisScroll { milliseconds, .. }) => {
                    self.advance_axis_scroll_clock(u64::from(milliseconds) * 1_000_000)?;
                }
                _ => {}
            }
        }
        if matches!(self.state.wait, Some(MusicaWaitState::Media { .. })) {
            save_pages::validate_state(&self.state)?;
            movie::validate_movie_state(&self.state)?;
            self.state.movie = None;
        }
        if token_id.starts_with("musica.message.") {
            message::finish_message_loads(&mut self.state)?;
        }
        if matches!(
            self.state.wait,
            Some(MusicaWaitState::CharacterTransition { .. })
        ) {
            character::complete_character_transition_state(&mut self.state)?;
        }
        self.state.wait = None;
        Ok(())
    }

    pub fn advance_waiting_tick(&mut self, fixed_tick: u64) -> Result<(), MusicaRuntimeError> {
        if self.state.wait.is_none() || fixed_tick == 0 || fixed_tick != self.state.fixed_tick + 1 {
            return Err(MusicaRuntimeError::State);
        }
        self.state.fixed_tick = fixed_tick;
        Ok(())
    }

    pub fn advance_effect_clock(
        &mut self,
        delta_ns: u64,
    ) -> Result<Option<MusicaEffectFrame>, MusicaRuntimeError> {
        let Some(effect) = self.state.effect.as_mut() else {
            return Ok(None);
        };
        effect.elapsed_ns = effect
            .elapsed_ns
            .checked_add(delta_ns)
            .ok_or(MusicaRuntimeError::Overflow)?;
        let interval_ns = u64::from(effect.interval_ms)
            .checked_mul(1_000_000)
            .ok_or(MusicaRuntimeError::Overflow)?;
        if effect.elapsed_ns < interval_ns {
            return Ok(None);
        }
        // The original handler performs at most one update per render call and
        // resets its time origin to the current clock value.
        effect.elapsed_ns = 0;
        if effect.alpha_255 >= 255 {
            effect.current_index = effect.next_index;
            effect.next_index = next_effect_index(effect.next_index, effect.resources.len())?;
            effect.alpha_255 = 0;
        }
        let mut frame = effect_frame(effect)?;
        effect.visible_current_index = effect.current_index;
        effect.visible_next_index = effect.next_index;
        effect.visible_alpha_255 = frame.alpha_255;
        effect.alpha_255 = effect
            .alpha_255
            .checked_add(effect.alpha_step)
            .ok_or(MusicaRuntimeError::Overflow)?;
        next_effect_sequence(&mut self.state)?;
        frame.sequence = self.state.effect_sequence;
        Ok(Some(frame))
    }

    /// Advances the active stage transition. Returns true while the crossfade
    /// is still running (the frame must be re-rendered).
    pub fn advance_transition_clock(&mut self, delta_ns: u64) -> Result<bool, MusicaRuntimeError> {
        let Some(transition) = self.state.transition_active.as_mut() else {
            return Ok(false);
        };
        transition.elapsed_ns = transition
            .elapsed_ns
            .checked_add(delta_ns)
            .ok_or(MusicaRuntimeError::Overflow)?;
        if transition.elapsed_ns >= transition.duration_ns {
            self.state.transition_active = None;
        }
        Ok(self.state.transition_active.is_some())
    }

    /// Advances every attached SQZ image animation by the elapsed time and
    /// returns true when at least one displayed frame changed.
    pub fn advance_image_animation_clock(
        &mut self,
        delta_ns: u64,
    ) -> Result<bool, MusicaRuntimeError> {
        let mut changed = false;
        for animation in self.state.image_animations.values_mut() {
            animation.elapsed_ns = animation
                .elapsed_ns
                .checked_add(delta_ns)
                .ok_or(MusicaRuntimeError::Overflow)?;
            let frames = animation.elapsed_ns / animation.interval_ns;
            animation.elapsed_ns %= animation.interval_ns;
            if frames != 0 {
                animation.frame_index =
                    ((u64::from(animation.frame_index) + frames % u64::from(animation.frame_count))
                        % u64::from(animation.frame_count)) as u32;
                changed = true;
            }
        }
        Ok(changed)
    }

    /// Replaces the SQZ animation table for the stage layers the session just
    /// resolved. Entries keyed by the PNG resource URI carry the native SQZ
    /// header timing (`1000 / pair_count` ms per frame, `2 * pair_count`
    /// frames, looping).
    pub fn attach_image_animations(
        &mut self,
        animations: BTreeMap<String, MusicaImageAnimationState>,
    ) -> Result<(), MusicaRuntimeError> {
        validate_image_animations(&animations)?;
        self.state.image_animations = animations;
        Ok(())
    }

    pub fn transition_progress(&self) -> Option<f32> {
        let transition = self.state.transition_active.as_ref()?;
        if transition.duration_ns == 0 {
            return None;
        }
        let progress = transition.elapsed_ns.min(transition.duration_ns) as f64
            / transition.duration_ns as f64;
        Some(progress as f32)
    }

    /// Native skip path (sub_45D9F0): when animations are disabled the active
    /// stage transition jumps straight to its completed state.
    pub fn force_complete_transition(&mut self) -> Result<(), MusicaRuntimeError> {
        self.state.transition_active = None;
        Ok(())
    }

    /// Advances every attached ANI patch set by the elapsed time at the native
    /// 100 Hz rate. `speaker` is the normalized message speaker and
    /// `voice` carries (playing, level-permille) of the voice stream; the
    /// mouth only flaps while both match the character. Returns true when a
    /// displayed patch may have changed.
    pub fn advance_ani_patch_clock(
        &mut self,
        delta_ns: u64,
        speaker: Option<&str>,
        voice: Option<(bool, u32)>,
    ) -> Result<bool, MusicaRuntimeError> {
        if voice.is_some_and(|(_, level)| level > 1000) {
            return Err(MusicaRuntimeError::State);
        }
        let speaker = if self.state.name_resources.is_some() {
            self.state.message.as_ref().and_then(|message| message.animation_speaker.as_deref())
        } else {
            speaker
        };
        let mut changed = false;
        for state in self.state.ani_patches.values_mut() {
            state.elapsed_ns = state
                .elapsed_ns
                .checked_add(delta_ns)
                .ok_or(MusicaRuntimeError::Overflow)?;
            while state.elapsed_ns >= 10_000_000 {
                state.elapsed_ns -= 10_000_000;
                state.tick = state
                    .tick
                    .checked_add(1)
                    .ok_or(MusicaRuntimeError::Overflow)?;
                let tick = state.tick;
                for character in &mut state.characters {
                    if character.eye_countdown > 0 {
                        character.eye_countdown -= 1;
                    }
                    if character.eye_countdown == 0 {
                        loop {
                            let table = MUSICA_EYE_SEQUENCES
                                .get(usize::from(character.eye_sequence))
                                .ok_or(MusicaRuntimeError::State)?;
                            let entry = table
                                .get(usize::from(character.eye_index))
                                .ok_or(MusicaRuntimeError::State)?;
                            if entry.0 < 0 {
                                character.eye_index = 0;
                                continue;
                            }
                            character.eye_state = entry.0 as u8;
                            character.eye_countdown = u32::from(entry.1);
                            if entry.0 == 0 && character.eye_sequence < 5 {
                                // 64-bit LCG; wrapping steps are the native
                                // modulo behaviour, not an overflow.
                                state.rng = state
                                    .rng
                                    .wrapping_mul(6364136223846793005)
                                    .wrapping_add(1442695040888963407);
                                let roll = (state.rng >> 33) as u32 % MUSICA_EYE_OPEN_RAND_MOD;
                                character.eye_countdown += MUSICA_EYE_OPEN_RAND_BASE + roll;
                            }
                            character.eye_index = (character.eye_index + 1) % table.len() as u8;
                            changed = true;
                            break;
                        }
                    }
                    if character.mouth_last_tick == tick {
                        continue;
                    }
                    character.mouth_last_tick = tick;
                    let speaking =
                        speaker.is_some_and(|name| !name.is_empty() && name == character.name);
                    let (playing, level) = voice.unwrap_or((false, 0));
                    let next = if speaking && playing && level >= MUSICA_MOUTH_LEVEL_THRESHOLD {
                        if level > character.mouth_prev_level {
                            Some(if character.mouth_state != 0 { 2 } else { 1 })
                        } else if character.mouth_state == 1
                            && level < character.mouth_prev_level / 2
                        {
                            Some(0)
                        } else {
                            None
                        }
                    } else if character.mouth_state != 2 {
                        Some(0)
                    } else {
                        None
                    };
                    character.mouth_prev_level = level;
                    if let Some(value) = next {
                        character.mouth_state = value;
                        changed = true;
                    }
                }
            }
        }
        Ok(changed)
    }

    /// Registers the ANI patch sets the session resolved for stage layers.
    /// Each entry maps a stage PNG URI to its per-character patch state; the
    /// eye walker starts armed at the head of the native default sequence.
    pub fn attach_ani_patches(
        &mut self,
        patches: BTreeMap<String, (Vec<String>, u64)>,
    ) -> Result<(), MusicaRuntimeError> {
        let mut map = BTreeMap::new();
        for (uri, (names, seed)) in patches {
            let mut characters = Vec::new();
            for name in names {
                if name.is_empty() || name.len() > 64 {
                    return Err(MusicaRuntimeError::State);
                }
                characters.push(MusicaAniPatchCharacterState {
                    name,
                    eye_sequence: 2,
                    eye_index: 0,
                    eye_countdown: 0,
                    eye_state: 0,
                    mouth_state: 0,
                    mouth_prev_level: 0,
                    mouth_last_tick: 0,
                });
            }
            if characters.is_empty() || characters.len() > 10 {
                return Err(MusicaRuntimeError::State);
            }
            map.insert(
                uri,
                MusicaAniPatchState {
                    characters,
                    elapsed_ns: 0,
                    tick: 0,
                    rng: seed,
                },
            );
        }
        validate_ani_patches(&map)?;
        self.state.ani_patches = map;
        Ok(())
    }

    pub fn step(&mut self, fixed_tick: u64) -> Result<Option<MusicaVmEvent>, MusicaRuntimeError> {
        if fixed_tick == 0 || fixed_tick != self.state.fixed_tick + 1 {
            return Err(MusicaRuntimeError::State);
        }
        if self.state.wait.is_some() {
            return Err(MusicaRuntimeError::Waiting);
        }
        if self.state.terminal {
            return Ok(Some(MusicaVmEvent::Terminal));
        }
        self.state.fixed_tick = fixed_tick;
        let mut visited = BTreeSet::new();
        let mut operations = 0u32;
        loop {
            operations += 1;
            if operations > 10_000 {
                return Err(MusicaRuntimeError::NonYieldingCycle);
            }
            let control_state = (
                self.state.pc_line,
                self.state.variables.clone(),
                self.state.global_variables.clone(),
            );
            if !visited.insert(control_state) {
                return Err(MusicaRuntimeError::NonYieldingCycle);
            }
            let line_index = self.state.pc_line as usize;
            let line = self
                .script
                .lines
                .get(line_index)
                .ok_or(MusicaRuntimeError::ProgramCounter)?;
            self.state.pc_line = self
                .state
                .pc_line
                .checked_add(1)
                .ok_or(MusicaRuntimeError::Overflow)?;
            if line
                .language_guard
                .is_some_and(|language| language != self.script.encoding.language())
            {
                continue;
            }
            let ScLineKind::Command { command } = &line.kind else {
                continue;
            };
            self.state.instruction_count = self
                .state
                .instruction_count
                .checked_add(1)
                .ok_or(MusicaRuntimeError::Overflow)?;
            self.native_auto_count = self.native_auto_count.saturating_add(1);
            let periodic = command.opcode == "message"
                && self.native_auto_cycle > 0
                && self.native_auto_count >= self.native_auto_cycle;
            let explicit = command.opcode == "movie"
                || (command.opcode == "select"
                    && matches!(&command.control_flow, ScControlFlow::Choice { targets } if targets.len() <= 4));
            if periodic || explicit {
                if !native_save::autosave_prohibited(&self.state)
                    && !self.state.native_history.is_empty()
                {
                    self.native_auto_pending = Some(native_save::checkpoint(&self.state, false)?);
                    self.native_auto_count = 0;
                }
                if periodic {
                    self.native_auto_count = 0;
                }
            }
            let message_read = self.read_flags.is_read(&self.state);
            let event = execute_control(
                command,
                &self.labels,
                &mut self.state,
                self.control_pressed,
                message_read,
                &self.config.voice_preferences(),
                self.config.message_speed_auto_play,
            )
            .inspect_err(|cause| {
                tracing::error!(
                    event = "shinki.emu.musica.command.failed",
                    code = cause.diagnostic_code(),
                    ordinal = command.ordinal,
                    line = line_index,
                    offset = command.span.offset,
                    operand_count = command.operands.len(),
                    script_hash = self.state.script_hash.to_hex().as_str(),
                    tick = fixed_tick,
                );
            })?;
            match command.opcode.as_str() {
                "message" => {
                    let id = self.state.message.as_ref().map_or(-1, |m| m.message_id);
                    self.read_flags.message_started(id);
                }
                "chain" | "end" => self.read_flags.finish_message(),
                _ => {}
            }
            if let Some(event) = event {
                return Ok(Some(event));
            }
        }
    }
}

fn execute_control(
    command: &ScCommand,
    labels: &BTreeMap<String, u32>,
    state: &mut MusicaRuntimeState,
    control_pressed: bool,
    message_read: bool,
    voice_preferences: &crate::voice_preferences::VoicePreferences,
    auto_delay_units: u8,
) -> Result<Option<MusicaVmEvent>, MusicaRuntimeError> {
    match command.opcode.as_str() {
        "label" => Ok(None),
        "pragma" => control::execute_pragma(command, state),
        "set" | "setglobal" => {
            let (key, value) = evaluate_assignment(&command.operands, state)?;
            if command.opcode == "set" {
                state.variables.insert(key.to_owned(), value);
            } else {
                state.global_variables.insert(key.to_owned(), value);
                progress::record(state, key, value);
            }
            Ok(None)
        }
        "goto" => {
            let target = branch_target(&command.control_flow)?;
            if let Some(pc) = labels.get(target) {
                state.pc_line = *pc;
            }
            Ok(None)
        }
        "if" => {
            let [
                ScOperand::Symbol { value: key },
                ScOperand::Operator { value: operator },
                ScOperand::Integer { value },
                ScOperand::Symbol { value: target },
            ] = command.operands.as_slice()
            else {
                return Err(MusicaRuntimeError::Operand);
            };
            let current = state
                .variables
                .get(key)
                .or_else(|| state.global_variables.get(key))
                .copied()
                .unwrap_or(0);
            if compare(current, operator, *value)? {
                state.pc_line = *labels.get(target).ok_or(MusicaRuntimeError::Label)?;
            }
            Ok(None)
        }
        "wait" => {
            // Native parse reads the first operand; a single trailing empty
            // operand from a trailing space in the source is ignored.
            let [ScOperand::Integer { value }, ..] = command.operands.as_slice() else {
                return Err(MusicaRuntimeError::Operand);
            };
            let timer_ticks = u32::try_from(*value).map_err(|_| MusicaRuntimeError::Operand)?;
            let milliseconds = timer_ticks
                .checked_mul(10)
                .ok_or(MusicaRuntimeError::Overflow)?;
            if control::fast_forward_active(state, control_pressed, message_read) {
                return Ok(None);
            }
            let token_id = format!("musica.wait.{}", state.instruction_count);
            let wait = MusicaWaitState::Time {
                token_id,
                timer_ticks,
                milliseconds,
            };
            state.wait = Some(wait.clone());
            Ok(Some(MusicaVmEvent::Wait(wait)))
        }
        "message" => execute_message(command, state, voice_preferences, auto_delay_units),
        "movie" => movie::execute_movie(command, state),
        "deletevar" => {
            let [ScOperand::Symbol { value: key }] = command.operands.as_slice() else {
                return Err(MusicaRuntimeError::Operand);
            };
            state.variables.remove(key);
            state.global_variables.remove(key);
            Ok(None)
        }
        "playbgm" => execute_play_bgm(command, state, 0, "bgm"),
        "select" => execute_select(command, &mut *state),
        "playbgm2" => execute_play_bgm(command, state, 5, "bgm2"),
        "playse" => execute_play_se(command, state, 1, "se"),
        "playse2" => execute_play_se(command, state, 2, "se2"),
        "playse3" => execute_play_se(command, state, 3, "se3"),
        "playse4" => execute_play_se(command, state, 6, "se4"),
        "playvoice" => execute_play_voice(command, state),
        "transition" => execute_transition(command, state),
        "stage" => execute_stage(command, state),
        "hscroll" => scroll::execute_axis_scroll(command, state, MusicaAxisScrollAxis::Horizontal),
        "vscroll" => scroll::execute_axis_scroll(command, state, MusicaAxisScrollAxis::Vertical),
        "scrollxf" => scroll_xf::execute_scroll_xf(command, state),
        "scroll" => scroll::execute_linear_scroll(command, state),
        "endscroll" => scroll::execute_end_scroll(command, state),
        "shakescreen" => shake::execute_screen_shake(command, state),
        "char" => character::execute_character(command, state),
        "effect2" => particles::execute_secondary_effect(command, state),
        "effect" => execute_effect(command, state),
        "panel" => execute_panel(command, state),
        "chain" => {
            let ScControlFlow::Chain { target } = &command.control_flow else {
                return Err(MusicaRuntimeError::Operand);
            };
            validate_chain_target(target)?;
            Ok(Some(MusicaVmEvent::Chain {
                target: target.clone(),
            }))
        }
        "end" => {
            if state.launch_mode == MusicaLaunchMode::Title {
                title::return_to_title(state);
            } else {
                state.terminal = true;
            }
            Ok(Some(MusicaVmEvent::Terminal))
        }
        _ => Err(MusicaRuntimeError::UnsupportedOpcode {
            opcode: command.opcode.clone(),
            ordinal: command.ordinal,
        }),
    }
}

fn validate_chain_target(target: &str) -> Result<(), MusicaRuntimeError> {
    crate::script::chain_target_parts(target)
        .map(|_| ())
        .ok_or(MusicaRuntimeError::ChainTarget)
}

fn evaluate_assignment<'a>(
    operands: &'a [ScOperand],
    state: &MusicaRuntimeState,
) -> Result<(&'a str, i64), MusicaRuntimeError> {
    match operands {
        [
            ScOperand::Symbol { value: key },
            ScOperand::Operator { value: assign },
            rhs,
        ] if assign == "=" => Ok((key, resolve_integer(rhs, state)?)),
        [
            ScOperand::Symbol { value: key },
            ScOperand::Operator { value: assign },
            left,
            ScOperand::Operator { value: operator },
            right,
        ] if assign == "=" => {
            let left = resolve_integer(left, state)?;
            let right = resolve_integer(right, state)?;
            let value = match operator.as_str() {
                "|" => left | right,
                "&" => left & right,
                "+" => left
                    .checked_add(right)
                    .ok_or(MusicaRuntimeError::Overflow)?,
                "-" => left
                    .checked_sub(right)
                    .ok_or(MusicaRuntimeError::Overflow)?,
                "*" => left
                    .checked_mul(right)
                    .ok_or(MusicaRuntimeError::Overflow)?,
                "/" if right != 0 => left
                    .checked_div(right)
                    .ok_or(MusicaRuntimeError::Overflow)?,
                "%" if right != 0 => left
                    .checked_rem(right)
                    .ok_or(MusicaRuntimeError::Overflow)?,
                _ => return Err(MusicaRuntimeError::Operand),
            };
            Ok((key, value))
        }
        _ => Err(MusicaRuntimeError::Operand),
    }
}

fn resolve_integer(
    operand: &ScOperand,
    state: &MusicaRuntimeState,
) -> Result<i64, MusicaRuntimeError> {
    match operand {
        ScOperand::Integer { value } => Ok(*value),
        ScOperand::Symbol { value } => Ok(state
            .variables
            .get(value)
            .or_else(|| state.global_variables.get(value))
            .copied()
            .unwrap_or(0)),
        _ => Err(MusicaRuntimeError::Operand),
    }
}

fn compare(left: i64, operator: &str, right: i64) -> Result<bool, MusicaRuntimeError> {
    Ok(match operator {
        "==" => left == right,
        "!=" => left != right,
        "<" => left < right,
        "<=" => left <= right,
        ">" => left > right,
        ">=" => left >= right,
        _ => return Err(MusicaRuntimeError::Operand),
    })
}

fn branch_target(control_flow: &ScControlFlow) -> Result<&str, MusicaRuntimeError> {
    match control_flow {
        ScControlFlow::Jump { target } | ScControlFlow::ConditionalJump { target } => Ok(target),
        _ => Err(MusicaRuntimeError::Operand),
    }
}

fn build_labels(script: &ScScript) -> Result<BTreeMap<String, u32>, MusicaRuntimeError> {
    if script.schema != crate::MUSICA_SCRIPT_IR_SCHEMA {
        return Err(MusicaRuntimeError::State);
    }
    let mut labels = BTreeMap::new();
    for (line_index, line) in script.lines.iter().enumerate() {
        let ScLineKind::Command { command } = &line.kind else {
            continue;
        };
        if command.encoding != script.encoding {
            return Err(MusicaRuntimeError::State);
        }
        if line
            .language_guard
            .is_some_and(|language| language != script.encoding.language())
        {
            continue;
        }
        if let ScControlFlow::Label { id } = &command.control_flow {
            let target = u32::try_from(line_index + 1).map_err(|_| MusicaRuntimeError::Overflow)?;
            if labels.insert(id.clone(), target).is_some() {
                return Err(MusicaRuntimeError::Label);
            }
        }
    }
    Ok(labels)
}

#[cfg(test)]
pub(crate) fn validate_transition_and_animations(
    state: &MusicaRuntimeState,
) -> Result<(), MusicaRuntimeError> {
    if let Some(resource) = &state.transition.resource {
        effects::validate_scene_filename(resource)?;
    }
    if let Some(transition) = &state.transition_active {
        let stage = state.stage.as_ref().ok_or(MusicaRuntimeError::State)?;
        validate_stage_state(Some(&transition.previous))?;
        let phase_max = match transition.method {
            0 => 256u64,
            1 | 18 => {
                if stage.transition.resource.is_none() {
                    return Err(MusicaRuntimeError::State);
                }
                if transition.method == 1 { 512 } else { 256 }
            }
            _ => return Err(MusicaRuntimeError::State),
        };
        if transition.param == 0
            || transition.duration_ns == 0
            || transition.method != stage.transition.mode
            || transition.param != stage.transition.param
        {
            return Err(MusicaRuntimeError::State);
        }
        let expected_ns = phase_max * 100_000_000 / u64::from(transition.param);
        if transition.duration_ns != expected_ns || transition.elapsed_ns >= transition.duration_ns
        {
            return Err(MusicaRuntimeError::State);
        }
    }
    validate_image_animations(&state.image_animations)?;
    validate_ani_patches(&state.ani_patches)
}

fn validate_animation_uri(uri: &str) -> Result<(), MusicaRuntimeError> {
    let prefix = if uri.starts_with("musica:/bg/") {
        "musica:/bg/"
    } else {
        "musica:/st/"
    };
    stage::validate_uri(uri, prefix)?;
    if !uri
        .rsplit_once('.')
        .is_some_and(|(_, extension)| extension.eq_ignore_ascii_case("png"))
    {
        return Err(MusicaRuntimeError::State);
    }
    Ok(())
}

fn validate_image_animations(
    animations: &BTreeMap<String, MusicaImageAnimationState>,
) -> Result<(), MusicaRuntimeError> {
    if animations.len() > MUSICA_STAGE_RESOURCE_LIMIT {
        return Err(MusicaRuntimeError::State);
    }
    for (uri, animation) in animations {
        validate_animation_uri(uri)?;
        if animation.frame_count == 0
            || animation.interval_ns == 0
            || animation.frame_index >= animation.frame_count
            || animation.elapsed_ns >= animation.interval_ns
        {
            return Err(MusicaRuntimeError::State);
        }
    }
    Ok(())
}

fn validate_ani_patches(
    patches: &BTreeMap<String, MusicaAniPatchState>,
) -> Result<(), MusicaRuntimeError> {
    if patches.len() > MUSICA_STAGE_RESOURCE_LIMIT {
        return Err(MusicaRuntimeError::State);
    }
    for (uri, patch) in patches {
        validate_animation_uri(uri)?;
        if patch.characters.is_empty()
            || patch.characters.len() > 10
            || patch.elapsed_ns >= 10_000_000
        {
            return Err(MusicaRuntimeError::State);
        }
        for character in &patch.characters {
            let sequence = MUSICA_EYE_SEQUENCES
                .get(usize::from(character.eye_sequence))
                .ok_or(MusicaRuntimeError::State)?;
            let max_countdown = sequence
                .iter()
                .map(|&(eye, ticks)| {
                    u32::from(ticks)
                        + if eye == 0 && character.eye_sequence < 5 {
                            MUSICA_EYE_OPEN_RAND_BASE + MUSICA_EYE_OPEN_RAND_MOD - 1
                        } else {
                            0
                        }
                })
                .max()
                .unwrap_or(0);
            if character.name.is_empty()
                || character.name.len() > 64
                || usize::from(character.eye_index) >= sequence.len()
                || character.eye_countdown > max_countdown
                || character.eye_state > 2
                || character.mouth_state > 2
                || character.mouth_prev_level > 1000
                || character.mouth_last_tick > patch.tick
            {
                return Err(MusicaRuntimeError::State);
            }
        }
    }
    Ok(())
}

/// Native blink sequences at 100 Hz: (eye state, tick count); the sentinel
/// `(-1, 0)` loops the table. After every fully open eye (state 0, tables 0-4)
/// the native driver adds a random 50..549 tick extension.
pub(crate) const MUSICA_EYE_SEQUENCES: [&[(i16, u16)]; 6] = [
    &[
        (0, 40),
        (1, 40),
        (2, 80),
        (1, 40),
        (0, 400),
        (1, 40),
        (2, 80),
        (1, 40),
        (0, 3600),
        (1, 40),
        (2, 80),
        (1, 40),
        (0, 1200),
        (-1, 0),
    ],
    &[
        (0, 10),
        (1, 20),
        (2, 40),
        (1, 20),
        (0, 200),
        (1, 20),
        (2, 40),
        (1, 20),
        (0, 1800),
        (1, 20),
        (2, 40),
        (1, 20),
        (0, 600),
        (-1, 0),
    ],
    &[
        (0, 10),
        (1, 10),
        (2, 20),
        (1, 10),
        (0, 100),
        (1, 10),
        (2, 20),
        (1, 10),
        (0, 900),
        (1, 10),
        (2, 20),
        (1, 10),
        (0, 300),
        (-1, 0),
    ],
    &[
        (0, 5),
        (1, 5),
        (2, 10),
        (1, 5),
        (0, 100),
        (1, 5),
        (2, 10),
        (1, 5),
        (0, 450),
        (1, 5),
        (2, 10),
        (1, 5),
        (0, 150),
        (-1, 0),
    ],
    &[
        (0, 3),
        (1, 3),
        (2, 7),
        (1, 3),
        (0, 33),
        (1, 3),
        (2, 7),
        (1, 3),
        (0, 225),
        (1, 3),
        (2, 7),
        (1, 3),
        (0, 75),
        (-1, 0),
    ],
    &[(0, 3), (1, 3), (2, 3), (1, 3), (-1, 0)],
];
pub(crate) const MUSICA_EYE_OPEN_RAND_MOD: u32 = 500;
pub(crate) const MUSICA_EYE_OPEN_RAND_BASE: u32 = 50;
/// Native mouth-flap threshold in per-mille of the voice peak.
pub(crate) const MUSICA_MOUTH_LEVEL_THRESHOLD: u32 = 0x320;

#[cfg(test)]
#[path = "runtime/tests.rs"]
mod tests;

fn next_effect_sequence(state: &mut MusicaRuntimeState) -> Result<u64, MusicaRuntimeError> {
    state.effect_sequence = state
        .effect_sequence
        .checked_add(1)
        .ok_or(MusicaRuntimeError::Overflow)?;
    Ok(state.effect_sequence)
}
