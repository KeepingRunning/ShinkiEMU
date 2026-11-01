//! Windowless native ABI host. Input messages reuse the pinned Roukaku protocol.
pub mod capture;
mod files;
mod script;
#[cfg(feature = "manager-ui")]
pub mod ui;

use clap::Args;
use roukaku_headless_protocol::TICK_DURATION_NS;
use serde::Serialize;
use shinki_emu_abi as abi;
use shinki_emu_manager_core::{
    LoadedFamilyPlugin, NativeCancellation, NativeOpenRequest, NativeSession, model::*,
    no_host_services,
};
use std::{
    path::PathBuf,
    process::ExitCode,
    time::{Duration, Instant},
};

pub type Result<T, E = Error> = std::result::Result<T, E>;
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Run(String),
    #[error("assertion failed: {0}")]
    Assertion(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Image(#[from] image::ImageError),
    #[error(transparent)]
    Core(#[from] FamilyError),
}
impl Error {
    pub fn exit_code(&self) -> u8 {
        if matches!(self, Self::Assertion(_)) {
            3
        } else {
            1
        }
    }
}

#[derive(Debug, Clone, Args)]
pub struct Arguments {
    /// Core ZIP, unpacked core directory, library path, or embedded inventory ID.
    #[arg(long)]
    pub core: String,
    /// Source game. Copied into an isolated temporary directory before open.
    #[arg(long)]
    pub game_dir: PathBuf,
    #[arg(long, default_value_t = 60)]
    pub frames: u64,
    /// Minimum presentation timeline length in milliseconds (60 Hz host samples).
    #[arg(long)]
    pub duration_ms: Option<u64>,
    #[arg(long)]
    pub output: PathBuf,
    /// Capture host presentation sample numbers, starting at 1.
    #[arg(long, value_delimiter = ',')]
    pub capture_frame: Vec<u64>,
    #[arg(long, value_delimiter = ',')]
    pub capture_ms: Vec<u64>,
    /// Directory containing identically named checkpoint PNG files.
    #[arg(long)]
    pub baseline: Option<PathBuf>,
    /// Exact canonical pixel SHA-256 required for the final frame.
    #[arg(long)]
    pub baseline_hash: Option<String>,
    #[arg(long, default_value_t = 0)]
    pub max_channel_delta: u8,
    /// Fraction of pixels allowed to exceed max-channel-delta.
    #[arg(long, default_value_t = 0.0)]
    pub changed_pixel_ratio: f64,
    /// JSON array of roukaku.user_input_sequence.v1 InputMessage values.
    #[arg(long)]
    pub script: Option<PathBuf>,
    /// JSON array of Manager ConfigEntry values.
    #[arg(long)]
    pub configuration: Option<PathBuf>,
    #[arg(long, default_value_t = 30000)]
    pub startup_timeout_ms: u64,
}

#[derive(Debug, Serialize)]
pub struct RunReport {
    pub schema: &'static str,
    pub core: String,
    pub abi: &'static str,
    pub clock: &'static str,
    pub presented_samples: u64,
    pub acquired_frames: u64,
    pub consumed_input_messages: usize,
    pub final_pixel_hash: String,
    pub captures: Vec<capture::Capture>,
}

pub fn run_cli(arguments: Arguments, queries: &[(&str, abi::Query)]) -> ExitCode {
    if let Err(error) = files::prepare_output(
        &arguments.game_dir,
        &arguments.output,
        arguments.baseline.as_deref(),
    ) {
        eprintln!("{error}");
        return ExitCode::from(error.exit_code());
    }
    if std::env::var_os("SHINKI_HEADLESS_ISOLATED_CHILD").is_none() {
        return match isolated_process() {
            Ok(status) => {
                let code = status
                    .code()
                    .and_then(|code| u8::try_from(code).ok())
                    .unwrap_or(1);
                let failure = arguments.output.join("failure.json");
                if !status.success() && !failure.exists() {
                    let diagnostic = serde_json::json!({"error":format!("native child terminated: {status}"),"exit_code":code});
                    if let Err(error) = std::fs::write(
                        failure,
                        serde_json::to_vec_pretty(&diagnostic).expect("serializable failure"),
                    ) {
                        eprintln!("cannot write failure report: {error}");
                    }
                }
                ExitCode::from(code)
            }
            Err(error) => {
                eprintln!("{error}");
                ExitCode::FAILURE
            }
        };
    }
    // Match Manager's native owner stack without starting Slint or an event loop.
    let core = arguments.core.clone();
    let output = arguments.output.clone();
    let query = queries
        .iter()
        .find(|(id, _)| *id == core)
        .map(|(_, query)| *query);
    let result = std::thread::Builder::new()
        .name("shinki-headless-owner".into())
        .stack_size(8 * 1024 * 1024)
        .spawn(move || run(arguments, query));
    match result {
        Ok(thread) => match thread.join() {
            Ok(Ok(report)) => {
                println!(
                    "{}",
                    serde_json::to_string(&report).expect("serializable report")
                );
                ExitCode::SUCCESS
            }
            Ok(Err(error)) => {
                eprintln!("{error}");
                if let Err(write_error) = std::fs::create_dir_all(&output).and_then(|()| std::fs::write(output.join("failure.json"),
                    serde_json::to_vec_pretty(&serde_json::json!({"error":error.to_string(),"exit_code":error.exit_code()})).expect("serializable failure"))) {
                    eprintln!("cannot write failure report: {write_error}");
                }
                ExitCode::from(error.exit_code())
            }
            Err(_) => {
                eprintln!("headless owner panicked");
                ExitCode::FAILURE
            }
        },
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn isolated_process() -> Result<std::process::ExitStatus> {
    let root = tempfile::Builder::new()
        .prefix("shinki-headless-process-")
        .tempdir()?;
    let mut command = std::process::Command::new(std::env::current_exe()?);
    command.args(std::env::args_os().skip(1));
    command.env("SHINKI_HEADLESS_ISOLATED_CHILD", "1");
    // Native producers often hold large VM boot temporaries too. Set their
    // default stack before std caches RUST_MIN_STACK in the isolated process.
    let native_stack = std::env::var("RUST_MIN_STACK")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(8 * 1024 * 1024)
        .max(8 * 1024 * 1024);
    command.env("RUST_MIN_STACK", native_stack.to_string());
    for variable in [
        "APPDATA",
        "LOCALAPPDATA",
        "USERPROFILE",
        "HOME",
        "XDG_DATA_HOME",
        "XDG_CONFIG_HOME",
        "XDG_CACHE_HOME",
        "TEMP",
        "TMP",
    ] {
        let path = root.path().join(variable);
        std::fs::create_dir(&path)?;
        command.env(variable, path);
    }
    // Environment is set on the child before any core thread starts. No process-global
    // environment mutation races with native workers, and all managed storage is temporary.
    Ok(command.status()?)
}

/// `query` must be a linked, process-lifetime core entrypoint, as in Manager's inventory.
/// Dynamic cores use exactly the same LoadedFamilyPlugin path as Manager.
fn run(arguments: Arguments, query: Option<abi::Query>) -> Result<RunReport> {
    if arguments.frames == 0
        || arguments.startup_timeout_ms == 0
        || !(0.0..=1.0).contains(&arguments.changed_pixel_ratio)
    {
        return Err(Error::Run(
            "frames/timeout must be positive; changed-pixel-ratio must be in 0..=1".into(),
        ));
    }
    let messages = script::load(arguments.script.as_deref())?;
    let ticks = arguments
        .frames
        .max(ms_ticks(arguments.duration_ms.unwrap_or(0))?)
        .max(script::end_tick(&messages)?)
        .max(1);
    let mut captures = std::collections::BTreeSet::new();
    for frame in &arguments.capture_frame {
        if *frame == 0 || *frame > ticks {
            return Err(Error::Run("capture-frame outside run timeline".into()));
        }
        captures.insert(*frame);
    }
    for ms in &arguments.capture_ms {
        let frame = ms_ticks(*ms)?.max(1);
        if frame > ticks {
            return Err(Error::Run("capture-ms outside run timeline".into()));
        }
        captures.insert(frame);
    }
    captures.insert(ticks);
    files::prepare_output(
        &arguments.game_dir,
        &arguments.output,
        arguments.baseline.as_deref(),
    )?;
    // Never copy a game into its own descendant, and never expose the source to a core.
    let private = tempfile::Builder::new()
        .prefix("shinki-headless-")
        .tempdir()?;
    let game = private.path().join("game");
    files::copy_game(&arguments.game_dir, &game)?;
    let extracted = private.path().join("core");
    let mut plugin = if let Some(query) = query {
        // Safety: this function only receives entries from the host's linked inventory.
        unsafe { LoadedFamilyPlugin::from_query(query) }
    } else {
        let library = files::library(&PathBuf::from(&arguments.core), &extracted)?;
        LoadedFamilyPlugin::load(library)
    }
    .map_err(|error| Error::Run(error.to_string()))?;
    let game_path = game
        .to_str()
        .ok_or_else(|| Error::Run("game path must be UTF-8".into()))?
        .to_owned();
    if plugin
        .probe(ProbeRequest {
            game_path: game_path.clone(),
        })?
        .is_none()
    {
        return Err(Error::Run("core probe did not recognize the game".into()));
    }
    let mut configuration = arguments
        .configuration
        .as_ref()
        .map(|path| -> Result<Vec<ConfigEntry>> {
            Ok(serde_json::from_slice(&std::fs::read(path)?)?)
        })
        .transpose()?
        .unwrap_or_default();
    if plugin
        .configuration()
        .iter()
        .any(|field| field.id == "save_root" && matches!(field.kind, ConfigKind::String { .. }))
    {
        let saves = private.path().join("saves");
        std::fs::create_dir(&saves)?;
        configuration.retain(|entry| entry.id != "save_root");
        configuration.push(ConfigEntry {
            id: "save_root".into(),
            value: ConfigValue::String(
                saves
                    .to_str()
                    .ok_or_else(|| Error::Run("save path must be UTF-8".into()))?
                    .into(),
            ),
        });
    }
    let epoch = allocate_session_epoch()?;
    let cancellation = NativeCancellation::new(epoch);
    let mut host = no_host_services(&cancellation);
    host.window = files::window_service();
    let mut session = plugin.open(NativeOpenRequest {
        game_path,
        session_epoch: epoch,
        configuration,
        initial_window: WindowState {
            width: 1280,
            height: 720,
            scale_factor: 1.0,
            focused: true,
            visible: true,
            suspended: false,
        },
        host,
        text_async: None,
        cancellation,
        render_extent: None,
    })?;
    let result = execute(&mut session, &arguments, ticks, &captures, &messages);
    let closed = session.close();
    drop(session);
    let shutdown = plugin.shutdown();
    let report = result?;
    closed?;
    shutdown?;
    std::fs::write(
        arguments.output.join("run.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(report)
}

pub(crate) fn ms_ticks(ms: u64) -> Result<u64> {
    ms.checked_mul(1_000_000)
        .map(|ns| ns.div_ceil(TICK_DURATION_NS))
        .ok_or_else(|| Error::Run("timeline overflows".into()))
}

struct Timeline<'a> {
    session: &'a mut NativeSession,
    origin: Instant,
    tick: u64,
    latest: Option<FrameLease>,
    acquired: u64,
}
impl Timeline<'_> {
    fn poll(&mut self) -> Result<()> {
        let state = self.session.state()?;
        if state.state == abi::SESSION_FAILED {
            return Err(Error::Run(format!(
                "core failed: {}: {}",
                String::from_utf8_lossy(&state.error.code[..state.error.code_len as usize]),
                String::from_utf8_lossy(&state.error.message[..state.error.message_len as usize])
            )));
        }
        if let FramePoll::Ready(frame) = self
            .session
            .acquire_frame(self.latest.as_ref().map(|frame| frame.identity))?
        {
            self.latest = Some(frame);
            self.acquired += 1;
        }
        Ok(())
    }
    fn frame(&self) -> Result<&FrameLease> {
        self.latest
            .as_ref()
            .ok_or_else(|| Error::Run("core produced no frame".into()))
    }
    fn advance(&mut self, tick: u64) -> Result<()> {
        let ns = tick
            .checked_mul(TICK_DURATION_NS)
            .ok_or_else(|| Error::Run("timeline overflows".into()))?;
        let due = self
            .origin
            .checked_add(Duration::from_nanos(ns))
            .ok_or_else(|| Error::Run("timeline overflows".into()))?;
        while Instant::now() < due {
            self.poll()?;
            std::thread::sleep(
                due.saturating_duration_since(Instant::now())
                    .min(Duration::from_millis(2)),
            );
        }
        self.tick = tick;
        self.poll()
    }
}

fn execute(
    session: &mut NativeSession,
    args: &Arguments,
    ticks: u64,
    captures: &std::collections::BTreeSet<u64>,
    messages: &[roukaku_headless_protocol::InputMessage],
) -> Result<RunReport> {
    let mut timeline = Timeline {
        session,
        origin: Instant::now(),
        tick: 0,
        latest: None,
        acquired: 0,
    };
    let startup = Instant::now();
    while timeline.latest.is_none() {
        timeline.poll()?;
        if startup.elapsed() >= Duration::from_millis(args.startup_timeout_ms) {
            return Err(Error::Run(
                "timed out waiting for the first completed CPU frame".into(),
            ));
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    timeline.origin = Instant::now();
    let mut report = RunReport {
        schema: "shinki.emu.headless_run.v1",
        core: args.core.clone(),
        abi: "shinki.emu.native.c.v3",
        clock: "60hz_host_observation_native_owned_game_time",
        presented_samples: ticks,
        acquired_frames: 0,
        consumed_input_messages: 0,
        final_pixel_hash: String::new(),
        captures: vec![],
    };
    let mut index = 0;
    let mut wait_until = 0;
    for tick in 0..=ticks {
        timeline.advance(tick)?;
        while index < messages.len() && messages[index].tick <= tick && tick >= wait_until {
            let event = &messages[index].event;
            match event {
                roukaku_headless_protocol::PhysicalInput::AdvanceTicks { count } => {
                    wait_until = tick + u64::from(*count)
                }
                roukaku_headless_protocol::PhysicalInput::Checkpoint { id } => {
                    report
                        .captures
                        .push(capture::save(timeline.frame()?, id, tick, args)?);
                }
                roukaku_headless_protocol::PhysicalInput::Await {
                    observation,
                    timeout_ticks,
                    ..
                } => {
                    // Await consumes host samples but leaves capture-frame scheduling intact.
                    if !script::matches(timeline.frame()?, observation)? {
                        let deadline = messages[index].tick + u64::from(*timeout_ticks);
                        if tick >= deadline {
                            return Err(Error::Assertion(format!(
                                "await timed out at sample {tick}"
                            )));
                        }
                        break;
                    }
                }
                _ => {
                    let events = script::events(event, timeline.frame()?)?;
                    timeline.session.input(
                        &events,
                        timeline.latest.as_ref().map(|frame| frame.identity),
                    )?;
                }
            }
            index += 1;
        }
        if captures.contains(&tick) {
            report.captures.push(capture::save(
                timeline.frame()?,
                &format!("frame-{tick:06}"),
                tick,
                args,
            )?);
        }
    }
    if index != messages.len() {
        return Err(Error::Assertion(
            "script did not finish within the requested timeline; increase --frames/--duration-ms"
                .into(),
        ));
    }
    report.acquired_frames = timeline.acquired;
    report.consumed_input_messages = index;
    report.final_pixel_hash = capture::pixel_hash(timeline.frame()?)?;
    if let Some(expected) = &args.baseline_hash {
        if *expected != report.final_pixel_hash {
            return Err(Error::Assertion(format!(
                "final hash {} != {expected}",
                report.final_pixel_hash
            )));
        }
    }
    Ok(report)
}
