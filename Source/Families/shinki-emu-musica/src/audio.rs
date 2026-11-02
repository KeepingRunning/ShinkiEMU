use crate::{
    MusicaAudioCommand, MusicaMountedVfs,
    scene::{error, read_asset},
};
#[cfg(test)]
use kira::sound::PlaybackState;
use kira::{
    AudioManager, AudioManagerSettings, Decibels, Panning, Tween,
    backend::{Backend, Renderer},
    sound::static_sound::{StaticSoundData, StaticSoundHandle},
};
use serde::{Deserialize, Serialize};
#[cfg(not(target_env = "ohos"))]
use shinki_emu_sdk::NativeAudioOutput;
use shinki_emu_sdk::runtime::{AudioSinkBox, PcmFormat, PcmFormatSpec};
use shinki_emu_sdk::runtime::{SessionError, SessionResult};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, SyncSender, TrySendError, sync_channel},
    },
    thread::{self, JoinHandle},
    time::Duration,
};
mod fade;
mod mixer;
mod preferences;
pub(crate) use preferences::AudioPreferences;
#[cfg(feature = "ffmpeg-vcpkg")]
mod movie;
use fade::FadeSnapshot;
use mixer::Mixer;
#[cfg(feature = "ffmpeg-vcpkg")]
pub(crate) use movie::MoviePcm;

pub(crate) const FORMAT: PcmFormatSpec = PcmFormatSpec {
    sample_rate: 48000,
    channels: 2,
    format: PcmFormat::F32,
};
const MAX_FRAMES: usize = 32 * 1024 * 1024;
pub(crate) const COMMAND_CAPACITY: usize = 64;
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct SoundSnapshot {
    pub id: u32,
    pub uri: String,
    pub position: f64,
    pub volume: f32,
    pub pan: f32,
    pub repeat: bool,
    pub playing: bool,
    pub fade: Option<FadeSnapshot>,
}
enum Command {
    Apply(Vec<MusicaAudioCommand>),
    #[cfg(test)]
    Snapshot(SyncSender<SessionResult<Vec<SoundSnapshot>>>),
    Restore(Vec<SoundSnapshot>, SyncSender<SessionResult<()>>),
    Suspend(bool),
    Preferences(AudioPreferences),
    Duration(u32, SyncSender<SessionResult<u32>>),
    VoiceLevel(SyncSender<SessionResult<u32>>),
}
pub(crate) struct Audio {
    #[cfg(feature = "ffmpeg-vcpkg")]
    movie: Arc<Mutex<Option<Arc<MoviePcm>>>>,
    commands: SyncSender<Command>,
    sink: Option<Arc<AudioSinkBox>>,
    stop: Arc<AtomicBool>,
    backpressured: AtomicBool,
    failure: Arc<Mutex<Option<SessionError>>>,
    worker: Option<JoinHandle<()>>,
}
impl Audio {
    pub fn start(
        archive: Arc<MusicaMountedVfs>,
        sink: Option<AudioSinkBox>,
    ) -> SessionResult<Self> {
        if let Some(sink) = &sink {
            sink.configure(FORMAT)?;
        }
        let sink = sink.map(Arc::new);
        let stop = Arc::new(AtomicBool::new(false));
        let failure = Arc::new(Mutex::new(None));
        let (tx, rx) = sync_channel(COMMAND_CAPACITY);
        let (ready_tx, ready_rx) = sync_channel(1);
        let (s, c, f) = (sink.clone(), stop.clone(), failure.clone());
        #[cfg(feature = "ffmpeg-vcpkg")]
        let movie = Arc::new(Mutex::new(None));
        #[cfg(feature = "ffmpeg-vcpkg")]
        let movie_worker = movie.clone();
        let worker = thread::Builder::new()
            .name("musica-audio".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run(
                        archive,
                        s.clone(),
                        c.clone(),
                        &rx,
                        &ready_tx,
                        #[cfg(feature = "ffmpeg-vcpkg")]
                        movie_worker,
                    )
                }));
                let error = match result {
                    Ok(Ok(())) => None,
                    Ok(Err(e)) => Some(e),
                    Err(_) => Some(error(
                        "SHINKI_EMU_MUSICA_AUDIO_PANIC",
                        "audio worker panicked",
                    )),
                };
                if let Some(cause) = &error {
                    let _ = ready_tx.try_send(Err(cause.clone()));
                }
                if error.is_some()
                    && let Ok(mut slot) = f.lock()
                    && slot.is_none()
                {
                    *slot = error;
                }
            })
            .map_err(|_| {
                error(
                    "SHINKI_EMU_MUSICA_AUDIO_THREAD",
                    "audio worker could not start",
                )
            })?;
        if let Err(cause) = ready_rx.recv().unwrap_or_else(|_| {
            Err(error(
                "SHINKI_EMU_MUSICA_AUDIO_START",
                "audio worker exited during initialization",
            ))
        }) {
            stop.store(true, Ordering::Release);
            let _ = worker.join();
            return Err(cause);
        }
        Ok(Self {
            #[cfg(feature = "ffmpeg-vcpkg")]
            movie,
            commands: tx,
            sink,
            stop,
            backpressured: AtomicBool::new(false),
            failure,
            worker: Some(worker),
        })
    }
    pub fn check(&self) -> SessionResult<()> {
        self.failure
            .lock()
            .map_err(|_| {
                error(
                    "SHINKI_EMU_MUSICA_AUDIO_STATE",
                    "audio worker state poisoned",
                )
            })?
            .clone()
            .map_or(Ok(()), Err)
    }
    fn send(&self, command: Command) -> SessionResult<()> {
        self.check()?;
        if self.stop.load(Ordering::Acquire) {
            return Err(cancelled_error());
        }
        match self.commands.try_send(command) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(command)) => {
                self.backpressured.store(true, Ordering::Release);
                self.commands.send(command).map_err(|_| self.unavailable())
            }
            Err(TrySendError::Disconnected(_)) => Err(self.unavailable()),
        }
    }
    fn unavailable(&self) -> SessionError {
        self.check().err().unwrap_or_else(|| {
            if self.stop.load(Ordering::Acquire) {
                cancelled_error()
            } else {
                error(
                    "SHINKI_EMU_MUSICA_AUDIO_QUEUE",
                    "audio command queue is closed",
                )
            }
        })
    }
    pub(crate) fn take_backpressure(&self) -> bool {
        self.backpressured.swap(false, Ordering::AcqRel)
    }
    pub fn apply(&self, commands: Vec<MusicaAudioCommand>) -> SessionResult<()> {
        if commands.is_empty() {
            self.check()
        } else {
            self.send(Command::Apply(commands))
        }
    }
    pub fn set_preferences(&self, preferences: AudioPreferences) -> SessionResult<()> {
        preferences.gains()?;
        self.send(Command::Preferences(preferences))
    }
    pub fn suspend(&self, value: bool) -> SessionResult<()> {
        self.send(Command::Suspend(value))
    }
    pub(crate) fn cancellation_flag(&self) -> Arc<AtomicBool> {
        self.stop.clone()
    }
    pub fn duration(&self, stream: u32) -> SessionResult<Receiver<SessionResult<u32>>> {
        let (tx, rx) = sync_channel(1);
        self.send(Command::Duration(stream, tx))?;
        Ok(rx)
    }

    /// Voice stream (id 4) peak level in per-mille with the native 300 ms
    /// half-life decay, sampled from the decoded frames at the playhead and
    /// scaled by the current stream volume.
    pub fn voice_level(&self) -> SessionResult<Receiver<SessionResult<u32>>> {
        let (tx, rx) = sync_channel(1);
        self.send(Command::VoiceLevel(tx))?;
        Ok(rx)
    }
    #[cfg(test)]
    pub fn snapshot(&self) -> SessionResult<Vec<SoundSnapshot>> {
        let (tx, rx) = sync_channel(1);
        self.send(Command::Snapshot(tx))?;
        rx.recv_timeout(Duration::from_secs(5)).map_err(|_| {
            if let Err(cause) = self.check() {
                return cause;
            }
            if self.stop.load(Ordering::Acquire) {
                return cancelled_error();
            }
            self.fail_request(error(
                "SHINKI_EMU_MUSICA_AUDIO_SNAPSHOT",
                "audio snapshot did not complete",
            ))
        })?
    }
    pub fn restore(&self, snapshot: Vec<SoundSnapshot>) -> SessionResult<()> {
        let (tx, rx) = sync_channel(1);
        self.send(Command::Restore(snapshot, tx))?;
        rx.recv_timeout(Duration::from_secs(15)).map_err(|_| {
            if let Err(cause) = self.check() {
                return cause;
            }
            if self.stop.load(Ordering::Acquire) {
                return cancelled_error();
            }
            self.fail_request(error(
                "SHINKI_EMU_MUSICA_AUDIO_RESTORE",
                "audio restore did not complete",
            ))
        })?
    }
    fn fail_request(&self, failure: SessionError) -> SessionError {
        self.stop.store(true, Ordering::Release);
        if let Ok(mut slot) = self.failure.lock()
            && slot.is_none()
        {
            *slot = Some(failure.clone());
        }
        if let Some(sink) = &self.sink
            && let Err(cancel_error) = sink.cancel()
        {
            return cancel_error;
        }
        failure
    }
    pub fn shutdown(&mut self) -> SessionResult<()> {
        if self.worker.is_none() {
            return self.check();
        }
        self.stop.store(true, Ordering::Release);
        let cancel = self.sink.as_ref().map(|sink| sink.cancel()).transpose();
        let joined = self
            .worker
            .take()
            .map(|w| w.join())
            .transpose()
            .map_err(|_| {
                error(
                    "SHINKI_EMU_MUSICA_AUDIO_JOIN",
                    "audio worker panicked during shutdown",
                )
            });
        cancel?;
        joined?;
        self.check()
    }
}
impl Drop for Audio {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}
/// The PCM destination: the SDK device backend, or a host-provided sink.
enum Output {
    #[cfg(not(target_env = "ohos"))]
    Device(Box<NativeAudioOutput>),
    Sink(Arc<AudioSinkBox>),
}
impl Output {
    fn open(sink: Option<Arc<AudioSinkBox>>, stop: &Arc<AtomicBool>) -> SessionResult<Self> {
        match sink {
            Some(sink) => Ok(Self::Sink(sink)),
            #[cfg(not(target_env = "ohos"))]
            None => Ok(Self::Device(Box::new(NativeAudioOutput::open(
                stop.clone(),
            )?))),
            #[cfg(target_env = "ohos")]
            None => {
                let _ = stop;
                Err(error(
                    "SHINKI_EMU_MUSICA_AUDIO_OUTPUT",
                    "this platform has no SDK device audio; the host must install an audio output",
                ))
            }
        }
    }
    fn write(&mut self, samples: Vec<f32>, stop: &AtomicBool) -> SessionResult<Option<Vec<f32>>> {
        match self {
            #[cfg(not(target_env = "ohos"))]
            Self::Device(device) => device.write(samples, stop),
            Self::Sink(sink) => {
                let _ = stop;
                sink.write(samples)
            }
        }
    }
    fn finish(self) -> SessionResult<()> {
        match self {
            #[cfg(not(target_env = "ohos"))]
            Self::Device(device) => device.check(),
            Self::Sink(_) => Ok(()),
        }
    }
}
fn run(
    archive: Arc<MusicaMountedVfs>,
    sink: Option<Arc<AudioSinkBox>>,
    stop: Arc<AtomicBool>,
    commands: &Receiver<Command>,
    ready: &SyncSender<SessionResult<()>>,
    #[cfg(feature = "ffmpeg-vcpkg")] movie: Arc<Mutex<Option<Arc<MoviePcm>>>>,
) -> SessionResult<()> {
    let mut mixer = Mixer::new()?;
    let mut output = Output::open(sink, &stop)?;
    let _ = ready.send(Ok(()));
    let mut samples = vec![0.0; 512 * 2];
    let mut suspended = false;
    'running: while !stop.load(Ordering::Acquire) {
        for command in commands.try_iter().take(COMMAND_CAPACITY) {
            if stop.load(Ordering::Acquire) {
                break 'running;
            }
            match command {
                Command::Apply(list) => {
                    for command in list {
                        if stop.load(Ordering::Acquire) {
                            break 'running;
                        }
                        if mixer.apply(command, &archive, &stop)?.is_none() {
                            break 'running;
                        }
                    }
                }
                #[cfg(test)]
                Command::Snapshot(reply) => {
                    let _ = reply.send(Ok(mixer.snapshot()));
                }
                Command::Restore(snapshot, reply) => {
                    match mixer.restore(snapshot, &archive, &stop) {
                        Ok(Some(())) => {
                            let _ = reply.send(Ok(()));
                        }
                        Ok(None) => {
                            let _ = reply.send(Err(cancelled_error()));
                            break 'running;
                        }
                        Err(cause) => {
                            let _ = reply.send(Err(cause.clone()));
                            return Err(cause);
                        }
                    }
                }
                Command::Suspend(value) => suspended = value,
                Command::Preferences(value) => mixer.set_preferences(value)?,
                Command::Duration(stream, reply) => {
                    let result = mixer.duration_ms(stream);
                    let _ = reply.send(result.clone());
                    result?;
                }
                Command::VoiceLevel(reply) => {
                    let level = mixer.voice_level_milli();
                    let _ = reply.send(Ok(level));
                }
            }
        }
        samples.fill(0.0);
        if !suspended {
            mixer.render(&mut samples);
        }
        #[cfg(feature = "ffmpeg-vcpkg")]
        let movie_mix = if !suspended {
            let track = movie
                .lock()
                .map_err(|_| {
                    error(
                        "SHINKI_EMU_MUSICA_AUDIO_STATE",
                        "movie audio state poisoned",
                    )
                })?
                .clone();
            track
                .map(|track| track.mix(&mut samples).map(|position| (track, position)))
                .transpose()?
        } else {
            None
        };
        let next = output.write(samples, &stop)?;
        match next {
            Some(recycled) => {
                samples = recycled;
                #[cfg(feature = "ffmpeg-vcpkg")]
                if let Some((track, position)) = movie_mix {
                    track.accept(position);
                }
            }
            None if stop.load(Ordering::Acquire) => break,
            None => {
                return Err(error(
                    "SHINKI_EMU_MUSICA_AUDIO_CLOSED",
                    "audio output closed unexpectedly",
                ));
            }
        }
    }
    output.finish()
}
fn cancelled_error() -> SessionError {
    error("SHINKI_EMU_AUDIO_CANCELLED", "audio operation cancelled")
}
fn validate_params(volume: f32, pan: f32) -> SessionResult<()> {
    if !volume.is_finite()
        || !(0.0..=1.0).contains(&volume)
        || !pan.is_finite()
        || !(-1.0..=1.0).contains(&pan)
    {
        Err(error(
            "SHINKI_EMU_MUSICA_AUDIO_PARAMS",
            "audio volume or pan is invalid",
        ))
    } else {
        Ok(())
    }
}
fn db(volume: f32) -> Decibels {
    if volume <= 0.0 {
        Decibels::SILENCE
    } else {
        Decibels((20.0 * volume.log10()).max(Decibels::SILENCE.0))
    }
}
fn tween(ms: u32) -> Tween {
    Tween {
        duration: Duration::from_millis(ms.into()),
        ..Default::default()
    }
}
fn immediate() -> Tween {
    tween(0)
}
