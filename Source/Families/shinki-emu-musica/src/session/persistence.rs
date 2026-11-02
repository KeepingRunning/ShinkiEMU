use super::*;
use crate::eden_save::EdenSave;

impl MusicaSession {
    /// Writes `system.dat` when a `.setglobal` changed the title-wide table.
    pub(super) fn persist_globals(&mut self) -> SessionResult<()> {
        let globals = &self.vm.state().global_variables;
        if *globals != self.persisted_globals {
            self.native.store_globals(globals)?;
            self.persisted_globals = globals.clone();
        }
        Ok(())
    }
    /// Writes every changed bitmap, then loads the running script's
    /// `<stem>.tbr` once (the original flushes on `.chain` and at teardown).
    pub(crate) fn sync_read_flags(&mut self) -> SessionResult<()> {
        self.flush_read_flags()?;
        let stem = self.vm.read_flags_script().to_owned();
        if !self.vm.has_read_flags(&stem)
            && let Some(bits) = self.native.read_flags(&stem)?
        {
            self.vm.install_read_flags(&stem, &bits);
        }
        Ok(())
    }
    pub(super) fn flush_read_flags(&mut self) -> SessionResult<()> {
        let native = &mut self.native;
        self.vm
            .flush_read_flags(|stem, bits| native.store_read_flags(stem, bits))
    }
    pub(super) fn quick_save(&mut self) -> SessionResult<()> {
        let pc_line = self.vm.state().pc_line;
        if self.last_quick_save_pc_line == Some(pc_line) {
            return Ok(());
        }
        let perseus = self.native.is_perseus()?;
        let slot = if perseus {
            self.quick_cursor
        } else {
            self.quick_cursor.max(1)
        };
        self.save_with_comment(
            slot,
            if perseus {
                "クイックセーブ"
            } else {
                "快速存档"
            },
        )?;
        let next = if perseus {
            (self.quick_cursor + 1) % 10
        } else {
            slot % 9 + 1
        };
        self.store_quick_cursor(next)?;
        self.last_quick_save_pc_line = Some(pc_line);
        tracing::debug!(event = "shinki.emu.musica.quick_save.rotated", slot, next);
        Ok(())
    }
    pub(super) fn quick_load(&mut self) -> SessionResult<()> {
        let slot = if self.native.is_perseus()? {
            (self.quick_cursor + 9) % 10
        } else if self.quick_cursor <= 1 {
            9
        } else {
            self.quick_cursor - 1
        };
        self.load(slot)
    }
    pub(super) fn save(&mut self, slot: u32) -> SessionResult<()> {
        self.save_with_comment(slot, "")
    }
    pub(super) fn auto_save(
        &mut self,
        mut checkpoint: crate::eden_save::EdenCheckpoint,
    ) -> SessionResult<()> {
        let limit = self.native.auto_save_backlog_limit();
        if limit > 0 && checkpoint.history.len() > limit {
            checkpoint.history.drain(..checkpoint.history.len() - limit);
        }
        let perseus = self.native.is_perseus()?;
        let slot = if perseus { 10 + self.auto_cursor } else { 0 };
        self.write_checkpoint(
            slot,
            if perseus {
                "オートセーブ"
            } else {
                "Auto save"
            },
            checkpoint,
        )?;
        if perseus {
            self.store_auto_cursor((self.auto_cursor + 1) % 10)?;
        }
        Ok(())
    }
    fn save_with_comment(&mut self, slot: u32, comment: &str) -> SessionResult<()> {
        if self
            .vm
            .state()
            .global_variables
            .get("prohibitSaveLoad")
            .is_some_and(|value| *value != 0)
        {
            return Err(error(
                "SHINKI_EMU_MUSICA_SAVE_PROHIBITED",
                "script prohibits saving and loading",
            ));
        }
        self.poll_voice_duration()?;
        let checkpoint = self.vm.native_checkpoint().map_err(|_| {
            error(
                "SHINKI_EMU_MUSICA_SAVE_STATE",
                "current VM message cannot be represented by a native slot",
            )
        })?;
        self.write_checkpoint(slot, comment, checkpoint)
    }
    fn write_checkpoint(
        &mut self,
        slot: u32,
        comment: &str,
        checkpoint: crate::eden_save::EdenCheckpoint,
    ) -> SessionResult<()> {
        self.persist_globals()?;
        let (edition, encoding, prefix) = self.native.save_edition()?;
        let save =
            EdenSave::from_checkpoint(&checkpoint, edition, encoding, prefix, comment.into())
                .map_err(crate::scene::core_error)?;
        let pixels = self.gameplay_frame.as_ref().unwrap_or(&self.scene.pixels);
        let thumbnail = crate::storage::SaveCard::capture_thumbnail(
            edition,
            self.info.logical_width,
            self.info.logical_height,
            self.info.width,
            self.info.height,
            pixels,
        )?;
        self.native.write_slot(slot, &save, &thumbnail)?;
        tracing::info!(event = "shinki.emu.musica.save.completed", slot);
        Ok(())
    }
    pub(super) fn load(&mut self, slot: u32) -> SessionResult<()> {
        if self
            .vm
            .state()
            .global_variables
            .get("prohibitSaveLoad")
            .is_some_and(|value| *value != 0)
        {
            return Err(error(
                "SHINKI_EMU_MUSICA_SAVE_PROHIBITED",
                "script prohibits saving and loading",
            ));
        }
        let saved = self.native.slot(slot)?.ok_or_else(|| {
            error(
                "SHINKI_EMU_MUSICA_SAVE_OPEN",
                "native save slot does not exist",
            )
        })?;
        let encoding = match saved.encoding {
            crate::eden_save::EdenSaveEncoding::Gbk => crate::ScriptEncoding::Gbk,
            crate::eden_save::EdenSaveEncoding::ShiftJis => crate::ScriptEncoding::ShiftJis,
            crate::eden_save::EdenSaveEncoding::Windows1252 => {
                return Err(error(
                    "SHINKI_EMU_MUSICA_SAVE_ENCODING",
                    "native save encoding is unsupported",
                ));
            }
        };
        let (mut vm, event) = MusicaVm::from_eden_save(
            &saved,
            &self.archive,
            self.vm.state().session_seed,
            self.vm.config().clone(),
        )
        .map_err(crate::scene::core_error)?;
        vm.set_native_auto_cycle(self.native.auto_save_cycle());
        vm.set_launch_mode(self.vm.state().launch_mode);
        // `system.dat` is title-wide; the slot carries only script variables.
        vm.replace_global_variables(self.vm.state().global_variables.clone());
        let mut scene = Scene::new_scaled(
            self.scene.gpu_instance(),
            self.archive.clone(),
            self.info.logical_width,
            self.info.logical_height,
            self.info.width,
            self.info.height,
            self.scene.texture_overrides(),
            encoding,
        )?;
        scene.set_text_shadow(self.scene.text_shadow());
        scene.set_configured_font(self.scene.configured_font())?;
        #[cfg(feature = "ffmpeg-vcpkg")]
        scene.set_subtitle_catalog(self.scene.subtitle_catalog());
        let message = match &event {
            MusicaVmEvent::Message { text, speaker, .. } => Some((text.clone(), speaker.clone())),
            _ => {
                return Err(error(
                    "SHINKI_EMU_MUSICA_SAVE_STATE",
                    "slot is not a message boundary",
                ));
            }
        };
        scene.render(vm.state(), message.as_ref(), None)?;
        self.cancel_text()?;
        self.generation = self.generation.checked_add(1).ok_or_else(|| {
            error(
                "SHINKI_EMU_MUSICA_TEXT_SEQUENCE",
                "text generation overflowed",
            )
        })?;
        if let Some(movie) = self.movie.take() {
            movie.close(&self.audio)?;
        }
        let sounds = vm
            .state()
            .audio
            .iter()
            .filter(|(id, audio)| **id == 0 && audio.playing)
            .map(|(id, audio)| crate::audio::SoundSnapshot {
                id: *id,
                uri: audio.resource_uri.clone(),
                position: 0.0,
                volume: f32::from(audio.volume_milli) / 1000.0,
                pan: f32::from(audio.pan_milli) / 1000.0,
                repeat: audio.looped,
                playing: true,
                fade: None,
            })
            .collect();
        self.audio.restore(sounds)?;
        vm.take_read_flags_from(&mut self.vm);
        vm.set_control_pressed(self.control_keys != 0);
        self.voice_duration = None;
        self.load_from_title = false;
        self.title_focus = None;
        self.gameplay_frame = None;
        self.save_cards.clear();
        self.vm = vm;
        self.scene = scene;
        if let Some(stage) = self.vm.state().stage.clone() {
            self.attach_stage_animations(&stage)?;
        }
        self.message = message;
        let event = self.semantic_event(Some(event))?;
        self.apply_event(event)?;
        self.wait_ns = 0;
        self.phase = 0;
        self.input_pending = false;
        self.pointer = None;
        self.finished = false;
        self.reset_host_clock = true;
        self.sync_read_flags()?;
        tracing::info!(event = "shinki.emu.musica.load.completed", slot);
        Ok(())
    }
}
