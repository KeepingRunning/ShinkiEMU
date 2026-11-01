use super::*;
use crate::CoreError;
use crate::eden_save::{EdenHistoryMessage, EdenSave};
fn invalid() -> CoreError {
    CoreError::invalid(
        "SHINKI_EMU_EDEN_SAVE_STATE",
        "native eden state cannot be represented by this VM",
    )
}

impl MusicaVm {
    /// Load the saved script and all referenced history from the mounted game.
    pub fn from_eden_save(
        save: &EdenSave,
        archive: &crate::MusicaMountedVfs,
        session_seed: u64,
        config: crate::MusicaConfigState,
    ) -> Result<(Self, MusicaVmEvent), CoreError> {
        let checkpoint = save.checkpoint()?;
        let encoding = match save.encoding {
            crate::eden_save::EdenSaveEncoding::Gbk => crate::ScriptEncoding::Gbk,
            crate::eden_save::EdenSaveEncoding::ShiftJis => crate::ScriptEncoding::ShiftJis,
            crate::eden_save::EdenSaveEncoding::Windows1252 => return Err(invalid()),
        };
        let uri = format!("musica:/scr/{}", checkpoint.script);
        let loaded =
            crate::script_loader::load_script(archive, &uri, encoding).map_err(script_error)?;
        let mut vm =
            Self::new(uri, loaded.hash, loaded.script, session_seed).map_err(runtime_error)?;
        vm.set_config(config).map_err(runtime_error)?;
        let event = vm.restore_eden_save(save, 1)?;
        Ok((vm, event))
    }

    /// Prepare the entire restored state before replacing the active VM.
    /// Historical CLog text is self-contained; only the active PC binds to source.
    pub fn restore_eden_save(
        &mut self,
        save: &EdenSave,
        next_fixed_tick: u64,
    ) -> Result<MusicaVmEvent, CoreError> {
        let checkpoint = save.checkpoint()?;
        if self.state.script_uri != format!("musica:/scr/{}", checkpoint.script)
            || next_fixed_tick == 0
        {
            return Err(invalid());
        }
        let position = save.validate_message_position(&checkpoint.script, &self.script)?;
        let mut candidate = Self::new(
            self.state.script_uri.clone(),
            self.state.script_hash,
            self.script.clone(),
            self.state.session_seed,
        )
        .map_err(runtime_error)?;
        candidate
            .set_config(self.config.clone())
            .map_err(runtime_error)?;
        candidate.state.launch_mode = self.state.launch_mode;
        candidate.replace_global_variables(self.state.global_variables.clone());
        candidate.state.native_edition = save.edition;
        candidate.state.variables = checkpoint.script_variables.clone();
        candidate.state.native_message_variables = checkpoint.script_variables.clone();
        candidate.state.native_message_saveable = true;
        candidate.state.transition = MusicaTransitionState {
            mode: 0,
            resource: None,
            param: checkpoint.transition_ticks,
        };
        candidate.state.stage = Some(MusicaStageCommand {
            foreground: stage::parse_foreground_spec(&checkpoint.foreground)
                .map_err(runtime_error)?,
            reference_position: Some(checkpoint.foreground_position),
            background: (!checkpoint.background.is_empty()).then(|| MusicaStageLayer {
                resource_uri: format!("musica:/bg/{}", checkpoint.background),
                x: checkpoint.background_position[0],
                y: checkpoint.background_position[1],
            }),
            stands: checkpoint
                .stands
                .iter()
                .map(|stand| MusicaStandLayer {
                    resource_uri: format!("musica:/st/{}", stand.resource),
                    position: stand.position,
                    resource_parameter: stand.offset,
                })
                .collect(),
            transition: candidate.state.transition.clone(),
        });
        candidate.state.native_foreground = checkpoint.foreground.clone();
        candidate.state.native_stand_slots =
            u32::try_from(checkpoint.stands.len()).map_err(|_| invalid())?;
        candidate.state.panel = match checkpoint.panel_mode {
            0 => None,
            1..=3 => Some(MusicaPanelState {
                mode: checkpoint.panel_mode,
                resource_uri: format!(
                    "musica:/sys/{}",
                    if checkpoint.panel_resource.is_empty() {
                        if checkpoint.panel_mode == 1 {
                            "msgPanel.png"
                        } else {
                            "fullPanel.png"
                        }
                    } else {
                        &checkpoint.panel_resource
                    }
                ),
            }),
            _ => return Err(invalid()),
        };
        if !checkpoint.bgm.is_empty() {
            let spec = parse_audio_resource_spec(&checkpoint.bgm).map_err(runtime_error)?;
            validate_audio_relative_path(&spec.resource).map_err(runtime_error)?;
            if spec.volume_percent != 100 {
                return Err(invalid());
            }
            candidate.state.audio.insert(
                0,
                MusicaAudioState {
                    bus: "bgm".into(),
                    resource_uri: format!("musica:/bgm/{}", spec.resource),
                    looped: true,
                    volume_milli: checkpoint.bgm_volume * 10,
                    pan_milli: spec.pan_percent * 10,
                    playing: true,
                    continuation_pts: 0,
                },
            );
        }
        for record in &checkpoint.history[..checkpoint.history.len() - 1] {
            append_native_history(&mut candidate.state, record, false)?;
        }
        let last = checkpoint.history.last().ok_or_else(invalid)?;
        append_native_history(&mut candidate.state, last, true)?;
        let command = checked_command(last, &self.script)?;
        candidate.state.pc_line = position.next_line_index;
        candidate.state.instruction_count = 1;
        let event = execute_message(
            command,
            &mut candidate.state,
            &self.config.voice_preferences(),
            self.config.message_speed_auto_play,
        )
        .map_err(runtime_error)?
        .ok_or_else(invalid)?;
        // Native save has no clock for delayed character loads or auto-advance.
        if !candidate.state.message_loads.is_empty() {
            return Err(invalid());
        }
        candidate.state.native_history = checkpoint.history.clone();
        candidate.state.native_novel_page = last.page.clone();
        candidate.state.fixed_tick = next_fixed_tick - 1;
        validate_stage_state(candidate.state.stage.as_ref()).map_err(runtime_error)?;
        backlog::validate_state(&candidate.state).map_err(runtime_error)?;
        self.state = candidate.state;
        self.read_flags.resume(&self.state);
        self.config_edit = None;
        Ok(event)
    }
}

fn append_native_history(
    state: &mut MusicaRuntimeState,
    record: &EdenHistoryMessage,
    omit_current: bool,
) -> Result<(), CoreError> {
    if record.page.is_empty() {
        if !omit_current {
            append_native_line(
                state,
                record.message_id,
                &record.voice,
                &record.speaker,
                &record.text,
            )?;
        }
    } else {
        let end = record.page.len() - usize::from(omit_current);
        for line in &record.page[..end] {
            append_native_line(
                state,
                record.message_id,
                &line.voice,
                &line.speaker,
                &line.text,
            )?;
        }
    }
    Ok(())
}

fn append_native_line(
    state: &mut MusicaRuntimeState,
    message_id: i64,
    voice_token: &str,
    speaker: &str,
    text: &str,
) -> Result<(), CoreError> {
    let text = crate::parse_musica_message_markup(text)
        .map_err(|_| invalid())?
        .visible_text;
    let speaker = message::native_speaker_name(speaker).map_err(|_| invalid())?;
    let voice = if voice_token.is_empty() {
        None
    } else {
        let spec = parse_audio_resource_spec(voice_token).map_err(runtime_error)?;
        validate_audio_relative_path(&spec.resource).map_err(runtime_error)?;
        Some(MusicaMessageVoice {
            resource_uri: format!("musica:/voice/{}", spec.resource),
            volume_milli: spec.volume_percent * 10,
            pan_milli: spec.pan_percent * 10,
        })
    };
    backlog::append_backlog_entry(
        state,
        MusicaBacklogEntry {
            source: None,
            message_id,
            text_hash: Hash256::from_sha256(text.as_bytes()),
            speaker_hash: speaker
                .as_ref()
                .map(|name| Hash256::from_sha256(name.as_bytes())),
            voice_hash: voice
                .as_ref()
                .map(|_| Hash256::from_sha256(voice_token.as_bytes())),
            text,
            speaker,
            voice,
        },
    )
    .map_err(runtime_error)
}

fn checked_command<'a>(
    record: &EdenHistoryMessage,
    script: &'a ScScript,
) -> Result<&'a ScCommand, CoreError> {
    let line = script
        .lines
        .get(record.next_line.checked_sub(1).ok_or_else(invalid)? as usize)
        .ok_or_else(invalid)?;
    let ScLineKind::Command { command } = &line.kind else {
        return Err(invalid());
    };
    // CLog also records an empty novel page when `.panel 2` creates it.
    // Its Params retain the preceding script ID, while L0 remains -1.
    let empty_page = record.panel_mode == 2
        && record.message_id == -1
        && record.state_message_id.is_some()
        && record
            .page
            .iter()
            .all(|line| line.voice.is_empty() && line.speaker.is_empty() && line.text.is_empty());
    if empty_page
        && command.opcode == "panel"
        && command
            .tokens()
            .map_err(|_| invalid())?
            .first()
            .is_some_and(|mode| mode == "2")
    {
        return Ok(command);
    }
    if command.opcode != "message"
        || line
            .language_guard
            .is_some_and(|guard| guard != script.encoding.language())
    {
        return Err(CoreError::invalid(
            "SHINKI_EMU_EDEN_SAVE_STATE",
            format!(
                "native history PC {} is not an active message",
                record.next_line
            ),
        ));
    }
    let (voice, speaker, text) = if let Some(line) = record.page.last() {
        (&line.voice, &line.speaker, &line.text)
    } else {
        (&record.voice, &record.speaker, &record.text)
    };
    let tokens = command.tokens().map_err(|_| invalid())?;
    let empty = tokens.len() < 4
        && record.message_id == -1
        && voice.is_empty()
        && speaker.is_empty()
        && text.is_empty();
    if !empty
        && (tokens.len() < 4
            || tokens[0].parse::<i64>().ok() != Some(record.message_id)
            || tokens[1] != *voice
            || tokens[2] != *speaker
            || tokens[3..].join(" ") != *text)
    {
        return Err(CoreError::invalid(
            "SHINKI_EMU_EDEN_SAVE_STATE",
            format!(
                "native history message mismatch at line {} (id={}, voice={}, speaker={}, text={})",
                record.next_line,
                tokens.first().and_then(|token| token.parse::<i64>().ok())
                    == Some(record.message_id),
                tokens.get(1) == Some(voice),
                tokens.get(2) == Some(speaker),
                tokens.len() >= 4 && tokens[3..].join(" ") == *text
            ),
        ));
    }
    Ok(command)
}

fn script_error(cause: shinki_emu_sdk::runtime::SessionError) -> CoreError {
    CoreError::invalid(
        "SHINKI_EMU_EDEN_SAVE_SCRIPT",
        format!(
            "native checkpoint script could not be loaded: {}",
            cause.code()
        ),
    )
}

fn runtime_error(cause: MusicaRuntimeError) -> CoreError {
    CoreError::invalid(
        "SHINKI_EMU_EDEN_SAVE_STATE",
        format!(
            "native VM restoration rejected: {}",
            cause.diagnostic_code()
        ),
    )
}
