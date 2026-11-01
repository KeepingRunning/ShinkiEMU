mod core_package;
mod manager_app;
mod metadata_cache;
mod metadata_runtime;
#[cfg(not(any(target_os = "android", target_env = "ohos")))]
mod patch_cli;
mod platform_secret;
mod stage_renderer;
#[cfg(test)]
#[path = "../../shinki-emu-manager-core/src/test_native.rs"]
mod test_native;
mod text_service;

#[cfg(not(any(target_os = "android", target_env = "ohos")))]
use clap::Parser;
#[cfg(not(any(target_os = "android", target_env = "ohos")))]
use std::process::ExitCode;

// Native VM boot may hold large temporaries. Reserve once per session owner.
const FAMILY_OWNER_STACK_BYTES: usize = 8 * 1024 * 1024;

// The command line is desktop-only: the Android activity has no arguments.
#[cfg(not(any(target_os = "android", target_env = "ohos")))]
#[derive(Parser)]
#[command(version)]
struct Arguments {
    #[command(subcommand)]
    command: Option<Command>,
}

#[cfg(not(any(target_os = "android", target_env = "ohos")))]
#[derive(clap::Subcommand)]
enum Command {
    /// Run a core and capture frames without creating a window.
    Headless(shinki_emu_headless::Arguments),
    /// Render the Manager UI with synthetic data and no OS window.
    HeadlessUi(shinki_emu_headless::ui::Arguments),
    /// Apply trusted offline file patches without starting the GUI or a core.
    Patch {
        #[command(subcommand)]
        command: patch_cli::Command,
    },
    #[command(flatten)]
    Extension(ExtensionCommand),
}

#[cfg(not(any(target_os = "android", target_env = "ohos")))]
#[derive(clap::Subcommand)]
pub(crate) enum ExtensionCommand {
    /// Install a trusted local Luau text extension, initially disabled.
    #[command(name = "extension-install")]
    Install {
        id: String,
        source: std::path::PathBuf,
    },
    #[command(name = "extension-enable")]
    Enable {
        id: String,
        #[arg(action = clap::ArgAction::Set)]
        enabled: bool,
    },
    #[command(name = "extension-configure")]
    Configure {
        id: String,
        configuration: std::path::PathBuf,
    },
    #[command(name = "extension-list")]
    List,
}

#[cfg(not(any(target_os = "android", target_env = "ohos")))]
fn main() -> ExitCode {
    let arguments = match Arguments::try_parse() {
        Ok(arguments) => arguments,
        Err(error) => {
            #[cfg(target_os = "windows")]
            attach_cli_console();
            let success = !error.use_stderr();
            let _ = error.print();
            return ExitCode::from(if success { 0 } else { 1 });
        }
    };
    if let Some(command) = arguments.command {
        #[cfg(target_os = "windows")]
        attach_cli_console();
        return match command {
            Command::Patch { command } => patch_cli::run(command),
            Command::Headless(arguments) => shinki_emu_headless::run_cli(
                arguments,
                shinki_emu_manager::static_cores::STATIC_CORES,
            ),
            Command::HeadlessUi(arguments) => shinki_emu_headless::ui::run_cli(arguments),
            Command::Extension(command) => match manager_app::run_extension_command(command) {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("{error}");
                    ExitCode::FAILURE
                }
            },
        };
    }
    shinki_emu_manager_ui_slint::i18n::select_system();
    match manager_app::run_application() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            shinki_emu_manager::platform::report_startup_error(&error.to_string());
            ExitCode::FAILURE
        }
    }
}

/// NativeActivity entry, called on the activity's native thread. Slint's
/// android-activity backend must be installed before `run_application`
/// selects the wgpu-30 renderer; its poll listener forwards lifecycle events.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: slint::android::AndroidApp) {
    use shinki_emu_manager::platform;
    let result = platform::android::initialize(app.clone())
        .map_err(Box::<dyn std::error::Error>::from)
        .and_then(|()| {
            slint::android::init_with_event_listener(app, platform::android::on_poll_event)
                .map_err(Box::<dyn std::error::Error>::from)
        })
        .and_then(|()| manager_app::run_application());
    if let Err(error) = result {
        tracing::error!(
            event = "shinki.emu.android.startup_failed",
            diagnostic_code = "SHINKI_EMU_ANDROID_STARTUP",
            error = %error
        );
        platform::report_startup_error(&error.to_string());
    }
}

/// Target of the ArkTS shell's NAPI `start(filesDir)`, called once per
/// process. The Manager runs on its own thread; the ArkTS UI thread only
/// forwards XComponent and lifecycle events.
#[cfg(target_env = "ohos")]
#[unsafe(no_mangle)]
unsafe extern "C" fn shinki_ohos_start(files: *const std::ffi::c_char) {
    // Slint layouts and Luau extensions recurse deeply; match desktop main threads.
    const UI_THREAD_STACK_BYTES: usize = 8 * 1024 * 1024;
    unsafe {
        shinki_emu_manager::platform::ohos::start(
            files,
            UI_THREAD_STACK_BYTES,
            manager_app::run_application,
        )
    }
}

#[cfg(target_os = "windows")]
fn attach_cli_console() {
    // The GUI subsystem does not inherit a console. Attach only for CLI paths,
    // never allocate a window; redirected standard handles remain redirected.
    let _ = unsafe {
        windows::Win32::System::Console::AttachConsole(
            windows::Win32::System::Console::ATTACH_PARENT_PROCESS,
        )
    };
}
