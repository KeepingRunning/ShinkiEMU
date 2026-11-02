pub mod effects;
mod gamepad;
mod host;
pub mod modal_service;
pub mod platform;
pub mod resale_notice;
pub mod self_update;
pub mod startup;
pub mod static_cores;
pub mod window_service;

extern crate self as shinki_emu_manager;

/// Product builds inject their release version; local builds use the package version.
pub const fn version() -> &'static str {
    env!("SHINKI_EMU_PRODUCT_VERSION")
}

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
