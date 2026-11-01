#![cfg(feature = "diagnostic-bridge")]

use parking_lot::Mutex;
use shinki_emu_abi::*;
use std::sync::LazyLock;

static RECEIVED: LazyLock<Mutex<Vec<(String, u64)>>> = LazyLock::new(|| Mutex::new(Vec::new()));

unsafe extern "C" fn emit(_: *mut std::ffi::c_void, raw: *const LogRecord) {
    let record = unsafe { &*raw };
    let event = unsafe { record.event.text(4096) }.unwrap();
    let fields = unsafe { borrowed_array(record.fields, record.field_count, 32) }.unwrap();
    let sequence = fields
        .iter()
        .find(|field| unsafe { field.name.text(128) } == Ok("sequence"))
        .map(|field| field.unsigned)
        .unwrap_or_default();
    RECEIVED.lock().push((event.into(), sequence));
    // This is the static Manager/headless failure path: the host diagnoses an
    // unavailable sink using the very subscriber that delivered this callback.
    // Both events must remain blocked until the outer callback has returned.
    tracing::error!(event = "shinki.emu.diagnostics.sink_unavailable");
    tracing::warn!(event = "host.diagnostic.callback_finished");
}

unsafe extern "C" fn flush(_: *mut std::ffi::c_void) {}

fn core_event(sequence: u64) {
    tracing::info!(event = "core.session.open", sequence);
}

#[test]
fn static_host_tracing_reentry_forwards_each_original_event_once() {
    unsafe {
        diagnostic_bridge::install(Diagnostics {
            header: Header::of::<Diagnostics>(),
            context: std::ptr::null_mut(),
            emit: Some(emit),
            flush: Some(flush),
        })
        .unwrap();
    }

    core_event(1);
    core_event(2);
    std::thread::spawn(|| core_event(3)).join().unwrap();

    assert_eq!(
        *RECEIVED.lock(),
        vec![
            ("core.session.open".into(), 1),
            ("core.session.open".into(), 2),
            ("core.session.open".into(), 3),
        ]
    );
}
