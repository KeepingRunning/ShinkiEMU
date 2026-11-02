use super::*;
impl MusicaSession {
    pub(super) fn cancel_text(&mut self) -> SessionResult<()> {
        if let Some(service) = &self.replacement {
            service.invalidate();
        }
        self.pending = None;
        self.hook_failed = false;
        self.choice_text = None;
        Ok(())
    }

    pub(super) fn semantic_event(
        &mut self,
        event: Option<MusicaVmEvent>,
    ) -> SessionResult<Option<MusicaVmEvent>> {
        if self.pending.is_none() {
            let enabled = self.replacement.is_some();
            if !enabled
                || !matches!(
                    event,
                    Some(MusicaVmEvent::Message { .. } | MusicaVmEvent::Choice)
                )
            {
                return Ok(event);
            }
            self.generation = self.generation.checked_add(1).ok_or_else(|| {
                error(
                    "SHINKI_EMU_MUSICA_TEXT_SEQUENCE",
                    "text sequence overflowed",
                )
            })?;
            self.pending = Some(PendingText {
                id: self.generation,
                event: event.unwrap(),
                request: None,
            });
        }
        if self.pending.as_ref().unwrap().request.is_none() {
            let pending = self.pending.as_ref().unwrap();
            let labels;
            let source = match &pending.event {
                MusicaVmEvent::Message { text, speaker, .. } => {
                    let mut source = vec![(shinki_emu_abi::TEXT_DIALOGUE, text.as_str())];
                    if let Some(speaker) = speaker {
                        source.push((shinki_emu_abi::TEXT_SPEAKER, speaker.as_str()));
                    }
                    source
                }
                MusicaVmEvent::Choice => {
                    labels = self
                        .vm
                        .choice_display()
                        .map_err(vm_error)?
                        .ok_or_else(|| error("SHINKI_EMU_MUSICA_CHOICE", "choice is unavailable"))?
                        .0;
                    labels
                        .iter()
                        .map(|text| (shinki_emu_abi::TEXT_CHOICE, text.as_str()))
                        .collect()
                }
                _ => unreachable!(),
            };
            let request = match self
                .replacement
                .as_ref()
                .unwrap()
                .submit(pending.id, &source)
            {
                Ok(request) => request,
                Err(cause) => {
                    self.hook_failed = true;
                    return Err(cause);
                }
            };
            self.pending.as_mut().unwrap().request = Some(request);
        }
        let pending = self.pending.as_mut().unwrap();
        let result = self
            .replacement
            .as_ref()
            .unwrap()
            .poll(pending.request.as_mut().unwrap());
        let mut transformed = match result {
            Ok(Some(result)) => result.into_iter(),
            Ok(None) => return Ok(None),
            Err(cause) => {
                pending.request = None;
                self.hook_failed = true;
                return Err(cause);
            }
        };
        let mut event = self.pending.take().unwrap().event;
        match &mut event {
            MusicaVmEvent::Message { text, speaker, .. } => {
                *text = transformed.next().unwrap();
                if speaker.is_some() {
                    *speaker = transformed.next();
                }
                self.choice_text = None;
            }
            MusicaVmEvent::Choice => self.choice_text = Some(transformed.collect()),
            _ => unreachable!(),
        }
        self.hook_failed = false;
        Ok(Some(event))
    }

    pub(super) fn skip_translation(&mut self) -> SessionResult<bool> {
        let Some(pending) = self.pending.take() else {
            return Ok(false);
        };
        self.replacement.as_ref().unwrap().skip(pending.id);
        self.hook_failed = false;
        self.input_pending = false;
        self.apply_event(Some(pending.event))
    }

    pub(super) fn choice_display(&self) -> SessionResult<Option<(Vec<String>, u32)>> {
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| matches!(pending.event, MusicaVmEvent::Choice))
        {
            return Ok(None);
        }
        let Some((labels, index)) = self.vm.choice_display().map_err(vm_error)? else {
            return Ok(None);
        };
        Ok(Some((self.choice_text.clone().unwrap_or(labels), index)))
    }

    pub(super) fn message(&mut self, text: String, speaker: Option<String>) -> SessionResult<()> {
        if text.len() > MAX_TEXT_BYTES
            || speaker.as_ref().is_some_and(|s| s.len() > MAX_SYMBOL_BYTES)
        {
            return Err(error(
                "SHINKI_EMU_MUSICA_TEXT_BOUND",
                "message exceeds supported bounds",
            ));
        }
        self.message = Some((text, speaker));
        Ok(())
    }
    pub(super) fn poll_voice_duration(&mut self) -> SessionResult<()> {
        let Some(MusicaWaitState::Voice {
            stream_id,
            milliseconds: None,
            ..
        }) = self.vm.state().wait.as_ref()
        else {
            self.voice_duration = None;
            return Ok(());
        };
        if self.voice_duration.is_none() {
            self.voice_duration = Some((self.audio.duration(*stream_id)?, Instant::now()));
        }
        let (reply, started) = self.voice_duration.as_ref().unwrap();
        match reply.try_recv() {
            Ok(result) => {
                self.vm.set_voice_duration(result?).map_err(vm_error)?;
                self.voice_duration = None;
                self.wait_ns = 0;
            }
            Err(std::sync::mpsc::TryRecvError::Empty)
                if started.elapsed() < Duration::from_secs(15) => {}
            _ => {
                return Err(error(
                    "SHINKI_EMU_MUSICA_AUDIO_DURATION",
                    "voice duration request failed or timed out",
                ));
            }
        }
        Ok(())
    }
}
