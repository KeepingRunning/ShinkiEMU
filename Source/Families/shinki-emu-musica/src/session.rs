mod backlog;
mod config;
mod gallery;
mod lifecycle;
mod native;
mod tick;
pub(crate) mod types;
use lifecycle::vm_error;
mod message;
mod movie;
mod persistence;
mod save_pages;
mod title;
use crate::{
    MusicaMountedVfs, MusicaVm, MusicaVmEvent, MusicaWaitState,
    audio::Audio,
    provider::SessionLease,
    scene::{Scene, error},
};
use shinki_emu_sdk::runtime::*;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
struct PendingText {
    id: u64,
    event: MusicaVmEvent,
    request: Option<crate::native::text::PendingReplacement>,
}
pub(crate) struct MusicaSession {
    info: FrameInfo,
    #[cfg(test)]
    published_frame: FramePublisher,
    force_frame: bool,
    archive: Arc<MusicaMountedVfs>,
    vm: MusicaVm,
    scene: Scene,
    audio: Audio,
    movie: Option<crate::movie::Movie>,
    replacement: Option<crate::native::text::TextBridge>,
    hook_failed: bool,
    choice_text: Option<Vec<String>>,
    pending: Option<PendingText>,
    voice_duration: Option<(std::sync::mpsc::Receiver<SessionResult<u32>>, Instant)>,
    generation: u64,
    _lease: SessionLease,
    message: Option<(String, Option<String>)>,
    phase: u128,
    wait_ns: u64,
    input_pending: bool,
    control_keys: u8,
    pointer: Option<(f32, f32)>,
    config_pointer_down: bool,
    pending_fullscreen: Option<bool>,
    reset_host_clock: bool,
    finished: bool,
    poisoned: bool,
    suspended: bool,
    focused: bool,
    visible: bool,
    progress_in_background: bool,
    primary_encoding: crate::ScriptEncoding,
    gameplay_frame: Option<Arc<[u8]>>,
    save_cards: Vec<(u32, crate::storage::SaveCard)>,
    /// `sysparams.txt` `quickSaveFileNumber`: the next quick save slot.
    quick_cursor: u32,
    /// `sysparams.txt` `autoSaveFileNumber`: the next auto save slot.
    auto_cursor: u32,
    persisted_globals: std::collections::BTreeMap<String, i64>,
    native: crate::native_directory::NativeFiles,
    entry_uri: String,
    title_focus: Option<u32>,
    load_from_title: bool,
    last_quick_save_pc_line: Option<u32>,
}
impl MusicaSession {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        info: FrameInfo,
        _session_epoch: u64,
        archive: Arc<MusicaMountedVfs>,
        mut vm: MusicaVm,
        scene: Scene,
        audio: Audio,
        replacement: Option<crate::native::text::TextBridge>,
        lease: SessionLease,
        focused: bool,
        visible: bool,
        progress_in_background: bool,
        primary_encoding: crate::ScriptEncoding,
        native: crate::native_directory::NativeFiles,
    ) -> Self {
        let persisted_globals = vm.state().global_variables.clone();
        let entry_uri = vm.state().script_uri.clone();
        let pending_fullscreen = vm.config().fullscreen.then_some(true);
        let quick_cursor = native.quick_cursor();
        let auto_cursor = native.auto_cursor();
        vm.set_native_auto_cycle(native.auto_save_cycle());
        if let Some(edition) = native.edition() {
            vm.set_native_edition(edition);
        }
        Self {
            persisted_globals,
            native,
            entry_uri,
            title_focus: None,
            load_from_title: false,
            info,
            #[cfg(test)]
            published_frame: FramePublisher::new(_session_epoch)
                .expect("validated native session epoch"),
            force_frame: false,
            archive,
            vm,
            scene,
            audio,
            movie: None,
            replacement,
            hook_failed: false,
            choice_text: None,
            _lease: lease,
            pending: None,
            voice_duration: None,
            generation: 0,
            message: None,
            phase: 0,
            wait_ns: 0,
            input_pending: false,
            control_keys: 0,
            pointer: None,
            config_pointer_down: false,
            pending_fullscreen,
            reset_host_clock: false,
            finished: false,
            poisoned: false,
            suspended: false,
            focused,
            visible,
            progress_in_background,
            primary_encoding,
            gameplay_frame: None,
            save_cards: Vec::new(),
            quick_cursor,
            auto_cursor,
            last_quick_save_pc_line: None,
        }
    }

    fn paused(&self) -> bool {
        self.suspended || !self.visible || (!self.focused && !self.progress_in_background)
    }
    fn clear_input(&mut self) {
        self.control_keys = 0;
        self.input_pending = false;
        self.pointer = None;
        self.config_pointer_down = false;
        self.vm.set_control_pressed(false);
    }
    fn update_pause(&self) -> SessionResult<()> {
        self.audio.suspend(self.paused())?;
        tracing::debug!(
            event = "shinki.emu.musica.window.pause",
            focused = self.focused,
            suspended = self.suspended,
            paused = self.paused()
        );
        Ok(())
    }

    fn advance_inner(
        &mut self,
        elapsed_ns: u64,
        events: &[RuntimeEvent],
    ) -> SessionResult<AdvanceResponse> {
        validate_events(events)?;
        let mut focused = self.focused;
        let mut visible = self.visible;
        let mut suspended = self.suspended;
        let mut was_paused = suspended || !visible || (!focused && !self.progress_in_background);
        let mut discard_elapsed = false;
        let mut resumed_from_pause = false;
        for event in events {
            match event {
                RuntimeEvent::WindowFocused { focused: value } => {
                    focused = *value;
                }
                RuntimeEvent::WindowVisibility { visible: value } => {
                    visible = *value;
                }
                RuntimeEvent::WindowSuspended { suspended: value } => {
                    suspended = *value;
                }
                _ => {}
            }
            if matches!(
                event,
                RuntimeEvent::WindowFocused { .. }
                    | RuntimeEvent::WindowVisibility { .. }
                    | RuntimeEvent::WindowSuspended { .. }
            ) {
                let paused = suspended || !visible || (!focused && !self.progress_in_background);
                if !was_paused && paused {
                    discard_elapsed = true;
                } else if was_paused && !paused {
                    resumed_from_pause = true;
                }
                was_paused = paused;
            }
        }
        let lifecycle_paused = suspended || !visible || (!focused && !self.progress_in_background);
        let elapsed_ns = if discard_elapsed || lifecycle_paused {
            0
        } else if resumed_from_pause {
            if elapsed_ns > 1_000_000_000 {
                0
            } else {
                elapsed_ns
            }
        } else {
            elapsed_ns
        };
        if elapsed_ns > 1_000_000_000 {
            tracing::warn!(
                event = "shinki.emu.musica.elapsed_rejected",
                diagnostic_code = "SHINKI_EMU_MUSICA_ELAPSED",
                elapsed_ns,
                phase = self.phase,
                wait_ns = self.wait_ns,
                suspended = self.suspended,
                focused = self.focused,
                progress_in_background = self.progress_in_background
            );
            return Err(error(
                "SHINKI_EMU_MUSICA_ELAPSED",
                "elapsed interval exceeds the bounded catch-up window",
            ));
        }
        self.audio.check()?;
        let mut save = false;
        let mut load = false;
        let mut choice_dirty = false;
        for event in events {
            if self.config_event(event)? {
                choice_dirty = true;
                continue;
            }
            if self.gallery_event(event)? {
                choice_dirty = true;
                continue;
            }
            if self.title_event(event)? {
                choice_dirty = true;
                continue;
            }
            if self.save_page_event(event)? {
                choice_dirty = true;
                continue;
            }
            if self.backlog_event(event)? {
                choice_dirty = true;
                continue;
            }
            if self.pending.is_some()
                && matches!(
                    event,
                    RuntimeEvent::Key {
                        code: KeyCode::Enter
                            | KeyCode::Space
                            | KeyCode::ControlLeft
                            | KeyCode::ControlRight,
                        state: KeyState::Pressed,
                        ..
                    } | RuntimeEvent::PointerButton {
                        button: PointerButton::Primary,
                        state: KeyState::Pressed
                    }
                )
            {
                choice_dirty |= self.skip_translation()?;
                continue;
            }
            match event {
                RuntimeEvent::PointerMove { x, y } => {
                    self.pointer = Some((*x, *y));
                    if let Some((labels, _)) = self.vm.choice_display().map_err(vm_error)?
                        && let Some(index) = crate::text_renderer::choice_at(labels.len(), *x, *y)
                    {
                        choice_dirty |= self.vm.focus_choice(index).map_err(vm_error)?;
                    }
                }
                RuntimeEvent::PointerButton {
                    button: PointerButton::Primary,
                    state: KeyState::Pressed,
                } if self.vm.state().choice.is_some() => {
                    let (labels, _) =
                        self.vm.choice_display().map_err(vm_error)?.ok_or_else(|| {
                            error("SHINKI_EMU_MUSICA_CHOICE", "choice is unavailable")
                        })?;
                    if let Some(index) = self
                        .pointer
                        .and_then(|(x, y)| crate::text_renderer::choice_at(labels.len(), x, y))
                    {
                        choice_dirty |= self.vm.focus_choice(index).map_err(vm_error)?;
                        self.input_pending = true;
                    }
                }
                RuntimeEvent::Key {
                    code: KeyCode::ArrowUp | KeyCode::ArrowDown,
                    state: KeyState::Pressed,
                    ..
                } if self.vm.state().choice.is_some() => {
                    let direction = if matches!(
                        event,
                        RuntimeEvent::Key {
                            code: KeyCode::ArrowUp,
                            ..
                        }
                    ) {
                        -1
                    } else {
                        1
                    };
                    self.vm.move_choice(direction).map_err(vm_error)?;
                    choice_dirty = true;
                }
                RuntimeEvent::Key {
                    code: KeyCode::A | KeyCode::S,
                    state: KeyState::Pressed,
                    ..
                } if !self.finished => {
                    let mode = if matches!(
                        event,
                        RuntimeEvent::Key {
                            code: KeyCode::S,
                            ..
                        }
                    ) {
                        crate::MusicaPlayMode::Skip
                    } else {
                        crate::MusicaPlayMode::Auto
                    };
                    if self.vm.toggle_play_mode(mode).map_err(vm_error)? {
                        self.wait_ns = 0;
                    }
                    self.input_pending = false;
                    choice_dirty = true;
                }
                RuntimeEvent::Key {
                    code: KeyCode::F5,
                    state: KeyState::Pressed,
                    ..
                } => save = !self.vm.save_load_prohibited(),
                RuntimeEvent::Key {
                    code: KeyCode::F9,
                    state: KeyState::Pressed,
                    ..
                } => load = !self.vm.save_load_prohibited(),
                RuntimeEvent::Key {
                    code: KeyCode::Enter | KeyCode::Space,
                    state: KeyState::Pressed,
                    ..
                }
                | RuntimeEvent::PointerButton {
                    button: PointerButton::Primary,
                    state: KeyState::Pressed,
                } => {
                    if matches!(self.vm.state().wait, Some(MusicaWaitState::Input { .. }))
                        && self.pending.is_none()
                    {
                        self.input_pending = true;
                    }
                }
                RuntimeEvent::Key {
                    code: KeyCode::ControlLeft | KeyCode::ControlRight,
                    state,
                    ..
                } => {
                    let mask = if matches!(
                        event,
                        RuntimeEvent::Key {
                            code: KeyCode::ControlLeft,
                            ..
                        }
                    ) {
                        1
                    } else {
                        2
                    };
                    if *state == KeyState::Pressed {
                        self.control_keys |= mask;
                    } else {
                        self.control_keys &= !mask;
                    }
                    self.vm.set_control_pressed(self.control_keys != 0);
                }
                RuntimeEvent::WindowFocused { focused } => {
                    let changed = self.focused != *focused;
                    self.focused = *focused;
                    if !focused {
                        self.clear_input();
                    }
                    if changed && !self.progress_in_background {
                        self.reset_host_clock = true;
                    }
                    self.update_pause()?;
                }
                RuntimeEvent::WindowVisibility { visible } => {
                    let changed = self.visible != *visible;
                    self.visible = *visible;
                    if !visible {
                        self.clear_input();
                    }
                    if changed {
                        self.reset_host_clock = true;
                    }
                    self.update_pause()?;
                }
                RuntimeEvent::WindowSuspended { suspended } => {
                    let changed = self.suspended != *suspended;
                    if *suspended {
                        self.clear_input();
                    }
                    self.suspended = *suspended;
                    if changed {
                        self.reset_host_clock = true;
                    }
                    self.update_pause()?;
                }
                _ => {}
            }
        }
        if save && load {
            return Err(error(
                "SHINKI_EMU_MUSICA_SAVE_CONFLICT",
                "save and load cannot be requested together",
            ));
        }
        if load {
            self.quick_load()?;
        }
        let mut dirty = choice_dirty;
        dirty |= self.advance_movie(elapsed_ns)?;
        if !self.paused()
            && self.movie.is_none()
            && !self.finished
            && self.vm.state().system_ui.page == crate::MusicaSystemPage::None
            && !self.hook_failed
        {
            if self.vm.config().animation {
                dirty |= self
                    .vm
                    .advance_axis_scroll_clock(elapsed_ns)
                    .map_err(vm_error)?
                    .is_some();
                dirty |= self
                    .vm
                    .advance_linear_scroll_clock(elapsed_ns)
                    .map_err(vm_error)?
                    .is_some();
                dirty |= self
                    .vm
                    .advance_scroll_xf_clock(elapsed_ns)
                    .map_err(vm_error)?
                    .is_some();
                dirty |= self
                    .vm
                    .advance_wscroll2_clock(elapsed_ns)
                    .map_err(vm_error)?
                    .is_some();
                let transition_dirty = self.vm.state().transition_active.is_some();
                self.vm
                    .advance_transition_clock(elapsed_ns)
                    .map_err(vm_error)?;
                dirty |= transition_dirty;
                dirty |= self
                    .vm
                    .advance_image_animation_clock(elapsed_ns)
                    .map_err(vm_error)?;
                dirty |= self.advance_ani_patches(elapsed_ns)?;
                dirty |= self
                    .vm
                    .advance_firefly_clock(elapsed_ns)
                    .map_err(vm_error)?
                    .is_some();
                dirty |= self
                    .vm
                    .advance_character_clock(elapsed_ns)
                    .map_err(vm_error)?
                    .is_some();
                if self.vm.config().screen_effect {
                    dirty |= self
                        .vm
                        .advance_effect_clock(elapsed_ns)
                        .map_err(vm_error)?
                        .is_some();
                    dirty |= self
                        .vm
                        .advance_screen_shake_clock(elapsed_ns)
                        .map_err(vm_error)?
                        .is_some();
                    dirty |= self
                        .vm
                        .advance_secondary_effect_clock(elapsed_ns)
                        .map_err(vm_error)?
                        .is_some();
                }
            }
            if !self.vm.config().animation {
                self.vm.force_complete_transition().map_err(vm_error)?;
            }
            dirty |= self
                .vm
                .advance_message_load_clock(elapsed_ns, self.vm.config().animation)
                .map_err(vm_error)?
                .is_some();
            self.phase += u128::from(elapsed_ns) * 60;
            while self.phase >= 1_000_000_000
                && !self.finished
                && self.vm.state().system_ui.page == crate::MusicaSystemPage::None
                && !self.hook_failed
            {
                self.phase -= 1_000_000_000;
                dirty |= self.tick()?;
            }
        }
        if dirty && self.vm.state().system_ui.page == crate::MusicaSystemPage::Config {
            self.scene.render_config(
                self.vm.config_for_presentation().map_err(vm_error)?,
                self.pointer,
            )?;
        } else if dirty && self.vm.state().system_ui.page == crate::MusicaSystemPage::Title {
            self.scene
                .render_title(self.vm.title_variant(), self.title_focus)?;
        } else if dirty && crate::runtime::gallery::count(&self.vm.state().system_ui.page).is_some()
        {
            self.scene.render_gallery(
                &self.vm.state().system_ui.page,
                self.vm.state().system_ui.focus_index,
            )?;
        } else if dirty && self.is_save_page() {
            self.scene.render_save_page(
                self.vm.state().system_ui.page.clone(),
                self.vm.state().system_ui.focus_index,
                &self.save_cards,
            )?;
        } else if dirty && self.movie.is_none() {
            let choices = self.choice_display()?;
            self.scene.render(
                self.vm.state(),
                self.message.as_ref(),
                choices
                    .as_ref()
                    .map(|(labels, index)| (labels.as_slice(), *index)),
            )?;
        }
        if save {
            self.quick_save()?;
        }
        self.persist_globals()?;
        Ok(AdvanceResponse {
            // Backpressure pauses the native VM; do not replay that wait as elapsed game time.
            reset_clock: std::mem::take(&mut self.reset_host_clock)
                | self.audio.take_backpressure(),
            status: if self.finished {
                RuntimeStatus::Finished
            } else if self.vm.state().wait.is_some() {
                RuntimeStatus::Waiting
            } else {
                RuntimeStatus::Running
            },
            window_command: self
                .pending_fullscreen
                .take()
                .map(WindowCommand::SetFullscreen),
        })
    }
}
#[cfg(test)]
#[path = "session/tests.rs"]
mod tests;

impl MusicaSession {
    /// Samples the voice level and normalized speaker, then advances every
    /// attached ANI patch set at the native 100 Hz rate.
    fn advance_ani_patches(&mut self, delta_ns: u64) -> SessionResult<bool> {
        if self.vm.state().ani_patches.is_empty() {
            return Ok(false);
        }
        let voice = self
            .audio
            .voice_level()
            .ok()
            .and_then(|receiver| {
                receiver
                    .recv_timeout(std::time::Duration::from_millis(5))
                    .ok()
            })
            .transpose()?
            .map(|level| (true, level))
            .unwrap_or((false, 0));
        let speaker = self
            .message
            .as_ref()
            .and_then(|(_, speaker)| speaker.as_deref());
        self.vm
            .advance_ani_patch_clock(delta_ns, speaker, Some(voice))
            .map_err(vm_error)
    }
}
