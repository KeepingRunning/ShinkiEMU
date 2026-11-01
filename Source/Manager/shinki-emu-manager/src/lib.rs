pub mod effects;
mod gamepad;
mod host;
pub mod modal_service;
pub mod platform;
pub mod resale_notice;
pub mod startup;
pub mod static_cores;
pub mod window_service;

extern crate self as shinki_emu_manager;

#[cfg(target_env = "ohos")]
pub use host::StageImageUpdate;
pub use host::{
    CloseDecision, ConfigPathPick, FilterApply, HostError, HostExit, HostWake, KeyCaptureState,
    ManagerController, ShinkiUnderlayRenderer, SnackbarOutcome, StageTextureUpdate,
    WgpuFrameContext, close_requested, run_manager, run_manager_with_initial_state,
};

// Android (NativeActivity) and OHOS (NAPI) load the Manager as a cdylib; the
// application modules and their entrypoints come from the shared binary root.
#[cfg(any(target_os = "android", target_env = "ohos"))]
include!("main.rs");
