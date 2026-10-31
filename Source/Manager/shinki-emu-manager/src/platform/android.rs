//! Android platform services. The Manager runs inside `MainActivity`
//! (a `NativeActivity`); Java-side work goes through JNI calls on the activity
//! object and the static `ShinkiPlatformBridge` helpers.
//!
//! - Data lives in the app-private files directory; resources listed in the
//!   APK asset `bundled.manifest` are extracted to `<data>/bundled/` at start.
//! - Pickers use the Storage Access Framework. Kotlin copies the picked file or
//!   tree into `<data>/imports/<unix_ms>-<id>/` and reports the copied path,
//!   so the rest of the Manager works with plain filesystem paths.
//! - New directories (patch output) are named under `<data>/exports/` without
//!   a dialog: app-private storage has no user-visible save location.
//! - Lifecycle events come from the Slint android-activity poll listener.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    ffi::{CStr, CString, c_void},
    io::Read,
    path::{Path, PathBuf},
    rc::Rc,
    sync::OnceLock,
    time::{SystemTime, UNIX_EPOCH},
};

use jni::{
    Env, EnvUnowned, JavaVM, jni_sig, jni_str,
    objects::{JObject, JString, JValue},
    refs::LoaderContext,
    strings::JNIStr,
};
use slint::android::{
    AndroidApp,
    android_activity::{MainEvent, PollEvent},
};

use super::{FileFilter, PickDone};

const BRIDGE_CLASS: &JNIStr = jni_str!("org/shinkirou/shinkiemu/ShinkiPlatformBridge");
const BUNDLED_MANIFEST: &str = "bundled.manifest";
const MAX_MANIFEST_BYTES: usize = 1024 * 1024;
const MAX_ASSET_BYTES: usize = 32 * 1024 * 1024;
const MAX_SECRET_BYTES: usize = 16 * 1024;
/// `MainActivity.pickDocument` kinds.
const PICK_TREE: i32 = 0;
const PICK_FILE: i32 = 1;

struct AndroidContext {
    app: AndroidApp,
    vm: JavaVM,
}

static CONTEXT: OnceLock<AndroidContext> = OnceLock::new();

/// A picker waiting for `nativeOnPickResult`. `extensions` is `None` for
/// folder pickers and file pickers without filters.
struct PendingPick {
    done: PickDone,
    extensions: Option<Vec<String>>,
}

thread_local! {
    // Slint event-loop thread only: pickers start there and results are
    // delivered there through `slint::invoke_from_event_loop`.
    static PENDING_PICKS: RefCell<HashMap<u64, PendingPick>> = RefCell::new(HashMap::new());
    static NEXT_PICK_ID: Cell<u64> = const { Cell::new(1) };
    static LIFECYCLE_HANDLER: RefCell<Option<Rc<dyn Fn(LifecycleEvent)>>> = RefCell::new(None);
    static LIFECYCLE_STATE: Cell<(bool, bool)> = const { Cell::new((false, false)) };
}

/// Activity lifecycle changes seen by the Slint poll listener.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LifecycleEvent {
    Focused(bool),
    Resumed(bool),
    ConfigChanged,
}

/// Store the activity context, initialize the platform TLS verifier used by
/// reqwest/rustls, and refresh the bundled resources. Call once from
/// `android_main` before Slint starts.
pub fn initialize(app: AndroidApp) -> Result<(), String> {
    // SAFETY: android-activity hands out the process JavaVM pointer.
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) };
    let activity = app.activity_as_ptr();
    vm.attach_current_thread(|env| -> jni::errors::Result<()> {
        // SAFETY: `activity` is the global reference android-activity keeps
        // alive for the lifetime of the process.
        let activity = unsafe { JObject::from_raw(env, activity.cast()) };
        rustls_platform_verifier::android::init_with_env(env, activity).inspect_err(|_| {
            clear_jni_exception(env);
        })
    })
    .map_err(|error| format!("SHINKI_EMU_ANDROID_TLS_VERIFIER: {error}"))?;
    CONTEXT
        .set(AndroidContext { app, vm })
        .map_err(|_| "SHINKI_EMU_ANDROID_CONTEXT_DUPLICATE".to_owned())?;
    extract_bundled_assets()
}

fn context() -> Result<&'static AndroidContext, String> {
    CONTEXT
        .get()
        .ok_or_else(|| "SHINKI_EMU_ANDROID_CONTEXT_MISSING".to_owned())
}

/// The activity handle stored by [`initialize`].
pub fn app() -> Result<&'static AndroidApp, String> {
    context().map(|context| &context.app)
}

/// Hide or show the mouse pointer over the activity window
/// (`MainActivity.setPointerHidden`, applied on the UI thread).
pub fn set_pointer_hidden(hidden: bool) -> Result<(), String> {
    call_activity("setPointerHidden", |env, activity| {
        env.call_method(
            activity,
            jni_str!("setPointerHidden"),
            jni_sig!("(Z)V"),
            &[JValue::Bool(hidden)],
        )
        .map(drop)
    })
    .map_err(|error| format!("SHINKI_EMU_ANDROID_POINTER_VISIBILITY: {error}"))
}

/// Register the lifecycle handler, replacing the previous one. Must be called
/// on the Slint event-loop thread, where events are delivered.
pub fn set_lifecycle_handler(handler: Box<dyn Fn(LifecycleEvent)>) {
    let handler: Rc<dyn Fn(LifecycleEvent)> = Rc::from(handler);
    LIFECYCLE_HANDLER.with(|slot| *slot.borrow_mut() = Some(handler));
}

/// `(focused, resumed)` as last reported by the activity.
pub fn lifecycle_state() -> (bool, bool) {
    LIFECYCLE_STATE.with(Cell::get)
}

/// Slint android-activity poll listener: runs on the event-loop thread after
/// Slint has handled `event`.
pub fn on_poll_event(event: &PollEvent<'_>) {
    let PollEvent::Main(event) = event else {
        return;
    };
    let event = match event {
        MainEvent::GainedFocus => LifecycleEvent::Focused(true),
        MainEvent::LostFocus => LifecycleEvent::Focused(false),
        MainEvent::Resume { .. } => LifecycleEvent::Resumed(true),
        MainEvent::Pause => LifecycleEvent::Resumed(false),
        MainEvent::ConfigChanged { .. } => LifecycleEvent::ConfigChanged,
        _ => return,
    };
    LIFECYCLE_STATE.with(|state| {
        let (focused, resumed) = state.get();
        match event {
            LifecycleEvent::Focused(focused) => state.set((focused, resumed)),
            LifecycleEvent::Resumed(resumed) => state.set((focused, resumed)),
            LifecycleEvent::ConfigChanged => {}
        }
    });
    // Clone out of the slot so the handler may replace itself.
    let handler = LIFECYCLE_HANDLER.with(|slot| slot.borrow().clone());
    if let Some(handler) = handler {
        handler(event);
    }
}

pub(super) fn pick_folder(_title: &str, done: PickDone) {
    start_pick(PICK_TREE, None, done);
}

pub(super) fn pick_file(_title: &str, filters: &[FileFilter<'_>], done: PickDone) {
    // SAF offers MIME types, not extensions; the picker shows everything and
    // the copied file is checked against the filters.
    let extensions = (!filters.is_empty()).then(|| {
        filters
            .iter()
            .flat_map(|filter| {
                filter
                    .extensions
                    .iter()
                    .map(|extension| (*extension).to_owned())
            })
            .collect()
    });
    start_pick(PICK_FILE, extensions, done);
}

pub(super) fn pick_new_directory(_title: &str, default_name: &str, done: PickDone) {
    let result = default_data_dir().and_then(|data| {
        if !paths::is_valid_name(default_name) {
            return Err("SHINKI_EMU_ANDROID_PICK_NAME".to_owned());
        }
        Ok(data
            .join("exports")
            .join(format!("{default_name}-{}", unix_ms())))
    });
    done(result);
}

const ANDROID_LOG_ERROR: std::os::raw::c_int = 6;

#[link(name = "log")]
unsafe extern "C" {
    fn __android_log_write(
        priority: std::os::raw::c_int,
        tag: *const std::os::raw::c_char,
        text: *const std::os::raw::c_char,
    ) -> std::os::raw::c_int;
}

/// Write the startup failure to logcat; the activity has no UI to show it in.
pub(super) fn report_startup_error(message: &str) {
    let text = CString::new(message.replace('\0', " ")).unwrap_or_default();
    // SAFETY: both pointers are NUL-terminated strings that outlive the call.
    unsafe {
        __android_log_write(ANDROID_LOG_ERROR, c"ShinkiEMU".as_ptr(), text.as_ptr());
    }
}

pub(super) fn default_data_dir() -> Result<PathBuf, String> {
    context()?
        .app
        .internal_data_path()
        .ok_or_else(|| "SHINKI_EMU_DATA_DIRECTORY_UNAVAILABLE".to_owned())
}

pub(super) fn program_file() -> Result<PathBuf, String> {
    // Any function in this library resolves to the loaded .so.
    let address = program_file as *const () as *const c_void;
    let mut info = std::mem::MaybeUninit::<libc::Dl_info>::zeroed();
    // SAFETY: dladdr fills `info` and only reads `address`.
    let found = unsafe { libc::dladdr(address, info.as_mut_ptr()) };
    // SAFETY: zero-initialized and possibly filled by dladdr.
    let info = unsafe { info.assume_init() };
    if found == 0 || info.dli_fname.is_null() {
        return Err("SHINKI_EMU_EXECUTABLE_LOCATION: dladdr found no library".to_owned());
    }
    // SAFETY: dladdr returns a NUL-terminated path owned by the loader.
    let name = unsafe { CStr::from_ptr(info.dli_fname) };
    name.to_str()
        .map(PathBuf::from)
        .map_err(|_| "SHINKI_EMU_EXECUTABLE_LOCATION: library path is not UTF-8".to_owned())
}

pub(super) fn bundled_resource_dir() -> Option<PathBuf> {
    Some(default_data_dir().ok()?.join("bundled"))
}

/// Relaunch the activity in a fresh process (`MainActivity.restartApp`).
pub(super) fn restart() -> Result<(), String> {
    call_activity("restartApp", |env, activity| {
        env.call_method(activity, jni_str!("restartApp"), jni_sig!("()V"), &[])
            .map(drop)
    })
    .map_err(|error| format!("SHINKI_EMU_RESTART_FAILED: {error}"))
}

pub(super) fn open_path(_path: &Path) -> Result<(), String> {
    Err(
        "SHINKI_EMU_OPEN_PATH_UNSUPPORTED: Android has no file manager for app-private storage"
            .into(),
    )
}

fn start_pick(kind: i32, extensions: Option<Vec<String>>, done: PickDone) {
    let id = NEXT_PICK_ID.with(|next| {
        let id = next.get();
        next.set(id + 1);
        id
    });
    let destination = match default_data_dir() {
        Ok(data) => data.join("imports").join(format!("{}-{id}", unix_ms())),
        Err(error) => return done(Err(error)),
    };
    let Some(destination) = destination.to_str().map(str::to_owned) else {
        return done(Err("SHINKI_EMU_ANDROID_PICK_DESTINATION".to_owned()));
    };
    PENDING_PICKS.with(|pending| {
        pending
            .borrow_mut()
            .insert(id, PendingPick { done, extensions })
    });
    let started = call_activity("pickDocument", |env, activity| {
        let destination = env.new_string(&destination)?;
        env.call_method(
            activity,
            jni_str!("pickDocument"),
            jni_sig!("(JILjava/lang/String;)V"),
            &[
                JValue::Long(id as i64),
                JValue::Int(kind),
                JValue::Object(&destination),
            ],
        )
        .map(drop)
    });
    if let Err(error) = started {
        let pick = PENDING_PICKS.with(|pending| pending.borrow_mut().remove(&id));
        if let Some(pick) = pick {
            (pick.done)(Err(format!("SHINKI_EMU_ANDROID_PICK_REQUEST: {error}")));
        }
    }
}

/// Complete a picker on the event-loop thread. Cancelled pickers drop `done`.
fn finish_pick(id: u64, path: Option<String>, error: Option<String>) {
    let Some(pick) = PENDING_PICKS.with(|pending| pending.borrow_mut().remove(&id)) else {
        tracing::warn!(
            event = "shinki.emu.android.pick_result_unknown",
            request_id = id,
            diagnostic_code = "SHINKI_EMU_ANDROID_PICK_UNKNOWN"
        );
        return;
    };
    let result = match (path, error) {
        (_, Some(error)) => Err(format!("SHINKI_EMU_ANDROID_PICK_COPY: {error}")),
        (Some(path), None) => {
            let path = PathBuf::from(path);
            match &pick.extensions {
                Some(extensions) => {
                    let extensions: Vec<&str> = extensions.iter().map(String::as_str).collect();
                    let filters = [FileFilter {
                        label: "",
                        extensions: &extensions,
                    }];
                    if super::matches_filters(&path, &filters) {
                        Ok(path)
                    } else {
                        // Kotlin copied the file into its own import directory.
                        if let Some(import) = path.parent() {
                            let _ = std::fs::remove_dir_all(import);
                        }
                        Err("SHINKI_EMU_ANDROID_PICK_TYPE".to_owned())
                    }
                }
                None => Ok(path),
            }
        }
        (None, None) => return,
    };
    (pick.done)(result);
}

fn extract_bundled_assets() -> Result<(), String> {
    let target = bundled_resource_dir().ok_or("SHINKI_EMU_DATA_DIRECTORY_UNAVAILABLE")?;
    match std::fs::remove_dir_all(&target) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("SHINKI_EMU_ANDROID_BUNDLED_RESET: {error}")),
    }
    let manifest = read_asset(BUNDLED_MANIFEST, MAX_MANIFEST_BYTES)?;
    let manifest = String::from_utf8(manifest)
        .map_err(|_| "SHINKI_EMU_ANDROID_BUNDLED_MANIFEST: not UTF-8".to_owned())?;
    for entry in paths::manifest_entries(&manifest)? {
        let destination = target.join(entry);
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("SHINKI_EMU_ANDROID_BUNDLED_WRITE: {entry}: {error}"))?;
        }
        let name =
            CString::new(entry).map_err(|_| format!("SHINKI_EMU_ANDROID_ASSET_PATH: {entry}"))?;
        let asset = context()?
            .app
            .asset_manager()
            .open(&name)
            .ok_or_else(|| format!("SHINKI_EMU_ANDROID_ASSET_MISSING: {entry}"))?;
        if asset.length() > MAX_ASSET_BYTES {
            return Err(format!("SHINKI_EMU_ANDROID_ASSET_BOUNDS: {entry}"));
        }
        let mut output = std::fs::File::create(&destination)
            .map_err(|error| format!("SHINKI_EMU_ANDROID_BUNDLED_WRITE: {entry}: {error}"))?;
        let copied = std::io::copy(&mut asset.take(MAX_ASSET_BYTES as u64 + 1), &mut output)
            .map_err(|error| format!("SHINKI_EMU_ANDROID_ASSET_READ: {entry}: {error}"))?;
        if copied > MAX_ASSET_BYTES as u64 {
            return Err(format!("SHINKI_EMU_ANDROID_ASSET_BOUNDS: {entry}"));
        }
    }
    Ok(())
}

fn read_asset(path: &str, max_bytes: usize) -> Result<Vec<u8>, String> {
    let name = CString::new(path).map_err(|_| format!("SHINKI_EMU_ANDROID_ASSET_PATH: {path}"))?;
    let mut asset = context()?
        .app
        .asset_manager()
        .open(&name)
        .ok_or_else(|| format!("SHINKI_EMU_ANDROID_ASSET_MISSING: {path}"))?;
    let length = asset.length();
    if length > max_bytes {
        return Err(format!("SHINKI_EMU_ANDROID_ASSET_BOUNDS: {path}"));
    }
    let mut bytes = Vec::with_capacity(length);
    asset
        .by_ref()
        .take(max_bytes as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("SHINKI_EMU_ANDROID_ASSET_READ: {path}: {error}"))?;
    if bytes.len() > max_bytes {
        return Err(format!("SHINKI_EMU_ANDROID_ASSET_BOUNDS: {path}"));
    }
    Ok(bytes)
}

fn unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis())
        .unwrap_or_default()
}

/// Run `call` with the activity object on an attached thread; clears a
/// pending Java exception so later JNI calls stay valid.
fn call_activity<T>(
    method: &str,
    call: impl FnOnce(&mut Env<'_>, &JObject<'_>) -> jni::errors::Result<T>,
) -> Result<T, String> {
    let context = context()?;
    let activity = context.app.activity_as_ptr();
    context
        .vm
        .attach_current_thread(|env| -> jni::errors::Result<T> {
            // SAFETY: global activity reference owned by android-activity.
            let activity = unsafe { JObject::from_raw(env, activity.cast()) };
            call(env, &activity).inspect_err(|_| clear_jni_exception(env))
        })
        .map_err(|error| format!("MainActivity.{method}: {error}"))
}

fn clear_jni_exception(env: &mut Env<'_>) {
    if env.exception_check() {
        env.exception_describe();
        env.exception_clear();
    }
}

/// Encrypt and persist `secret` under `reference` (AndroidKeyStore AES-GCM).
pub(crate) fn store_secret(reference: &str, secret: &str) -> Result<(), String> {
    paths::validate_secret_reference(reference)?;
    if secret.is_empty() || secret.len() > MAX_SECRET_BYTES {
        return Err("SHINKI_EMU_ANDROID_SECRET_VALUE".to_owned());
    }
    call_bridge_secret(jni_str!("storeSecret"), reference, Some(secret)).map(drop)
}

pub(crate) fn resolve_secret(reference: &str) -> Result<String, String> {
    paths::validate_secret_reference(reference)?;
    let secret = call_bridge_secret(jni_str!("resolveSecret"), reference, None)?;
    if secret.is_empty() || secret.len() > MAX_SECRET_BYTES {
        return Err("SHINKI_EMU_ANDROID_SECRET_VALUE".to_owned());
    }
    Ok(secret)
}

pub(crate) fn delete_secret(reference: &str) -> Result<(), String> {
    paths::validate_secret_reference(reference)?;
    call_bridge_secret(jni_str!("deleteSecret"), reference, None).map(drop)
}

/// Show `url` in the Store sign-in WebView (`MainActivity.openWebLogin`).
pub(crate) fn open_web_login(url: &str) -> Result<(), String> {
    call_activity("openWebLogin", |env, activity| {
        let url = env.new_string(url)?;
        env.call_method(
            activity,
            jni_str!("openWebLogin"),
            jni_sig!("(Ljava/lang/String;)V"),
            &[JValue::Object(&url)],
        )
        .map(drop)
    })
}

/// The page the sign-in WebView shows, or `None` once the user closed it.
pub(crate) fn web_login_page() -> Result<Option<String>, String> {
    call_activity("webLoginPage", |env, activity| {
        let page = env
            .call_method(
                activity,
                jni_str!("webLoginPage"),
                jni_sig!("()Ljava/lang/String;"),
                &[],
            )?
            .l()?;
        nullable_string(env, page)
    })
}

/// The `Cookie` header the sign-in WebView sends to `url`, HttpOnly cookies
/// included; `None` when it has none.
pub(crate) fn web_login_cookies(url: &str) -> Result<Option<String>, String> {
    call_activity("webLoginCookies", |env, activity| {
        let url = env.new_string(url)?;
        let header = env
            .call_method(
                activity,
                jni_str!("webLoginCookies"),
                jni_sig!("(Ljava/lang/String;)Ljava/lang/String;"),
                &[JValue::Object(&url)],
            )?
            .l()?;
        nullable_string(env, header)
    })
}

/// Close the sign-in WebView and wipe its cookies and site data.
pub(crate) fn close_web_login() -> Result<(), String> {
    call_activity("closeWebLogin", |env, activity| {
        env.call_method(activity, jni_str!("closeWebLogin"), jni_sig!("()V"), &[])
            .map(drop)
    })
}

fn nullable_string(env: &mut Env<'_>, object: JObject<'_>) -> jni::errors::Result<Option<String>> {
    if object.as_raw().is_null() {
        return Ok(None);
    }
    env.cast_local::<JString>(object)?
        .try_to_string(env)
        .map(Some)
}

/// `ShinkiPlatformBridge.<method>(Activity, String[, String]): String`.
fn call_bridge_secret(
    method: &'static JNIStr,
    reference: &str,
    secret: Option<&str>,
) -> Result<String, String> {
    call_activity("ShinkiPlatformBridge", |env, activity| {
        // Native threads see only the system class loader; resolve the app
        // class through the activity's loader.
        let bridge = LoaderContext::FromObject(activity).find_class(env, BRIDGE_CLASS, false)?;
        let reference = env.new_string(reference)?;
        let result = match secret {
            Some(secret) => {
                let secret = env.new_string(secret)?;
                env.call_static_method(
                    &bridge,
                    method,
                    jni_sig!("(Landroid/app/Activity;Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;"),
                    &[
                        JValue::Object(activity),
                        JValue::Object(&reference),
                        JValue::Object(&secret),
                    ],
                )
            }
            None => env.call_static_method(
                &bridge,
                method,
                jni_sig!("(Landroid/app/Activity;Ljava/lang/String;)Ljava/lang/String;"),
                &[JValue::Object(activity), JValue::Object(&reference)],
            ),
        }?;
        let object = result.l()?;
        if object.as_raw().is_null() {
            return Ok(String::new());
        }
        env.cast_local::<JString>(object)?.try_to_string(env)
    })
    .map_err(|error| format!("SHINKI_EMU_ANDROID_SECRET_STORE: {error}"))
}

/// Kotlin `MainActivity.nativeOnPickResult(requestId: Long, path: String?, error: String?)`.
/// Called from a Java thread once the SAF copy finished, failed (`error`) or
/// the user cancelled (both null).
#[unsafe(no_mangle)]
pub extern "system" fn Java_org_shinkirou_shinkiemu_MainActivity_nativeOnPickResult(
    mut env: EnvUnowned<'_>,
    _activity: JObject<'_>,
    request_id: jni::sys::jlong,
    path: JString<'_>,
    error: JString<'_>,
) {
    let strings = env
        .with_env(
            |env| -> jni::errors::Result<(Option<String>, Option<String>)> {
                Ok((optional_string(env, &path)?, optional_string(env, &error)?))
            },
        )
        .into_outcome();
    let id = request_id as u64;
    let (path, error) = match strings {
        jni::Outcome::Ok(strings) => strings,
        jni::Outcome::Err(_) | jni::Outcome::Panic(_) => (
            None,
            Some("SHINKI_EMU_ANDROID_PICK_RESULT: unreadable JNI strings".to_owned()),
        ),
    };
    if let Err(error) = slint::invoke_from_event_loop(move || finish_pick(id, path, error)) {
        tracing::error!(
            event = "shinki.emu.android.pick_result_dropped",
            request_id = id,
            diagnostic_code = "SHINKI_EMU_ANDROID_PICK_EVENT_LOOP",
            error = %error
        );
    }
}

fn optional_string(env: &Env<'_>, value: &JString<'_>) -> jni::errors::Result<Option<String>> {
    if value.as_raw().is_null() {
        Ok(None)
    } else {
        value.try_to_string(env).map(Some)
    }
}

/// Pure validation helpers, kept free of JNI so they are unit-testable.
mod paths {
    /// One component of a relative path: not empty, `.` or `..`, no
    /// separators or NUL.
    pub(super) fn is_valid_name(name: &str) -> bool {
        !name.is_empty()
            && name.len() <= 255
            && name != "."
            && name != ".."
            && !name.contains(['/', '\\', '\0', ':'])
    }

    /// A relative asset path made of valid components separated by `/`.
    pub(super) fn is_valid_relative_path(path: &str) -> bool {
        !path.is_empty() && path.len() <= 4096 && path.split('/').all(is_valid_name)
    }

    /// Parse `bundled.manifest`: one asset-relative path per line, blank lines
    /// ignored. Any invalid path rejects the whole manifest.
    pub(super) fn manifest_entries(manifest: &str) -> Result<Vec<&str>, String> {
        manifest
            .lines()
            .map(|line| line.trim_end_matches('\r'))
            .filter(|line| !line.is_empty())
            .map(|line| {
                if is_valid_relative_path(line) {
                    Ok(line)
                } else {
                    Err(format!(
                        "SHINKI_EMU_ANDROID_BUNDLED_MANIFEST: invalid path {line:?}"
                    ))
                }
            })
            .collect()
    }

    pub(super) fn validate_secret_reference(reference: &str) -> Result<(), String> {
        if reference.is_empty()
            || reference.len() > 128
            || !reference
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
        {
            return Err("SHINKI_EMU_ANDROID_SECRET_REFERENCE".into());
        }
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn manifest_accepts_relative_paths() {
            let entries =
                manifest_entries("THIRD_PARTY_NOTICES.md\r\nmetadata/a/b.json\n\n").unwrap();
            assert_eq!(entries, ["THIRD_PARTY_NOTICES.md", "metadata/a/b.json"]);
        }

        #[test]
        fn manifest_rejects_escaping_paths() {
            for bad in [
                "/etc/passwd",
                "a/../b",
                "..",
                "a//b",
                "a/",
                "a\\b",
                "c:x",
                "./a",
            ] {
                assert!(manifest_entries(bad).is_err(), "{bad}");
            }
        }

        #[test]
        fn names_reject_separators() {
            assert!(is_valid_name("patched-game"));
            assert!(!is_valid_name(""));
            assert!(!is_valid_name("a/b"));
            assert!(!is_valid_name(".."));
        }

        #[test]
        fn secret_references_are_restricted() {
            assert!(validate_secret_reference("translation.openai-key_1").is_ok());
            assert!(validate_secret_reference("").is_err());
            assert!(validate_secret_reference("a/b").is_err());
            assert!(validate_secret_reference(&"a".repeat(129)).is_err());
        }
    }
}
