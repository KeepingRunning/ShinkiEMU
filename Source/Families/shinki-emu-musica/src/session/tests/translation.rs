use super::*;
use crate::native::text::{ServiceContext, TextBridge};
use shinki_emu_abi as abi;
use std::{ffi::c_void, sync::atomic::AtomicU64};

type SubmittedItems = Vec<(u64, u32, String)>;

#[derive(Default)]
struct Service {
    mode: AtomicUsize,
    calls: AtomicUsize,
    acknowledgements: AtomicUsize,
    invalidations: AtomicUsize,
    epoch: AtomicU64,
    ids: parking_lot::Mutex<Vec<u64>>,
    kinds: parking_lot::Mutex<Vec<Vec<u32>>>,
    inputs: parking_lot::Mutex<Vec<Vec<String>>>,
    pending: parking_lot::Mutex<Option<(u64, SubmittedItems)>>,
    cancellations: AtomicUsize,
}
impl Service {
    fn bridge(&self) -> TextBridge {
        let context = (self as *const Self).cast_mut().cast();
        TextBridge(
            Arc::new(ServiceContext {
                session: 1,
                epoch: AtomicU64::new(1),
                cancelled: AtomicBool::new(false),
                waiting: AtomicBool::new(false),
                text: Some(abi::TextService {
                    context,
                    request: None,
                    acknowledge: Some(acknowledge),
                    invalidate: Some(invalidate),
                }),
                window: None,
                host_cancel: None,
            }),
            abi::AsyncTextServiceV1 {
                version: 1,
                size: std::mem::size_of::<abi::AsyncTextServiceV1>() as u32,
                context,
                submit: Some(submit),
                poll: Some(poll),
                cancel: Some(cancel),
                acknowledge: Some(acknowledge),
                invalidate: Some(invalidate),
            },
        )
    }
}
unsafe extern "C" fn submit(
    context: *mut c_void,
    request: *const abi::TextRequest,
    _: *mut abi::Error,
) -> abi::Status {
    let service = unsafe { &*context.cast::<Service>() };
    let request = unsafe { &*request };
    let mut pending = service.pending.lock();
    if pending
        .as_ref()
        .is_some_and(|(id, _)| *id == request.request_id)
    {
        return abi::OK;
    }
    let items = unsafe { std::slice::from_raw_parts(request.items, request.item_count) };
    let owned: Vec<_> = items
        .iter()
        .map(|item| {
            (
                item.id,
                item.kind,
                unsafe { item.text.text(1024 * 1024) }.unwrap().to_owned(),
            )
        })
        .collect();
    service.calls.fetch_add(1, Ordering::AcqRel);
    service.ids.lock().push(request.request_id);
    service
        .kinds
        .lock()
        .push(owned.iter().map(|(_, kind, _)| *kind).collect());
    service
        .inputs
        .lock()
        .push(owned.iter().map(|(_, _, text)| text.clone()).collect());
    *pending = Some((request.request_id, owned));
    abi::OK
}
unsafe extern "C" fn poll(
    context: *mut c_void,
    _: u64,
    _: u64,
    _: u64,
    output: *mut abi::TextResult,
    error: *mut abi::Error,
) -> abi::Status {
    let service = unsafe { &*context.cast::<Service>() };
    let result = unsafe { &mut *output };
    let mut pending = service.pending.lock();
    match service.mode.load(Ordering::Acquire) {
        0 => {
            pending.take();
            unsafe { (*error).set("TEST_TRANSLATION_FAILED", "host service error") };
            return abi::CORE_ERROR;
        }
        2 => return abi::PENDING,
        _ => {}
    }
    let Some((_, items)) = pending.as_ref() else {
        return abi::CANCELLED;
    };
    let replacement = match service.mode.load(Ordering::Acquire) {
        3 => "\u{10ffff}".to_owned(),
        4 => "T".repeat(2048),
        _ => "Translated".to_owned(),
    };
    result.byte_count = replacement.len() * items.len();
    result.item_count = items.len();
    if result.byte_capacity < result.byte_count || result.item_capacity < result.item_count {
        return abi::CAPACITY;
    }
    for (index, item) in items.iter().enumerate() {
        unsafe {
            std::ptr::copy_nonoverlapping(
                replacement.as_ptr(),
                result.bytes.add(index * replacement.len()),
                replacement.len(),
            );
            result.items.add(index).write(abi::TextRange {
                id: item.0,
                offset: index * replacement.len(),
                len: replacement.len(),
            });
        }
    }
    abi::OK
}
unsafe extern "C" fn cancel(context: *mut c_void, _: u64, _: u64, _: u64) -> abi::Status {
    unsafe { &*context.cast::<Service>() }
        .cancellations
        .fetch_add(1, Ordering::AcqRel);
    // Keep the delayed result to exercise a host that publishes after cancel.
    abi::OK
}
unsafe extern "C" fn acknowledge(context: *mut c_void, _: u64, _: u64, _: u64) -> abi::Status {
    unsafe { &*context.cast::<Service>() }
        .acknowledgements
        .fetch_add(1, Ordering::AcqRel);
    abi::OK
}
unsafe extern "C" fn invalidate(context: *mut c_void, _: u64, epoch: u64) {
    let service = unsafe { &*context.cast::<Service>() };
    service.epoch.store(epoch, Ordering::Release);
    service.invalidations.fetch_add(1, Ordering::AcqRel);
}

#[test]
fn semantic_hook_failure_retries_same_event_without_reexecuting_vm() {
    let _session = PROVIDER_SESSION.lock();
    let root = tempfile::tempdir().unwrap();
    fixture::game(
        root.path(),
        b".message 1  Speaker Hello\r\n.select First:done Second:done\r\n.label done\r\n.end\r\n",
    );
    let service = Box::<Service>::default();
    let mut provider = fixture::provider();
    let (_, mut session) = provider
        .open_native_session(
            request(root.path(), Sink::default()),
            shinki_emu_abi::Extent::default(),
            Some(service.bridge()),
        )
        .unwrap();
    let failure = session.advance(16_666_667, &[]).unwrap_err();
    assert_eq!(failure.code(), "TEST_TRANSLATION_FAILED");
    assert!(session.hook_failed && session.pending.is_some());
    assert!(!session.poisoned);
    assert!(session.message.is_none());
    let pc = session.vm.state().pc_line;
    let instructions = session.vm.state().instruction_count;
    let request_id = session.pending.as_ref().unwrap().id;
    service.mode.store(1, Ordering::Release);
    session.native_retry().unwrap();
    assert_eq!(session.vm.state().pc_line, pc);
    assert_eq!(session.vm.state().instruction_count, instructions);
    assert_eq!(
        session.message,
        Some(("Translated".into(), Some("Translated".into())))
    );
    assert_eq!(*service.ids.lock(), vec![request_id, request_id]);
    assert_eq!(
        service.kinds.lock()[0],
        vec![abi::TEXT_DIALOGUE, abi::TEXT_SPEAKER]
    );
    session.advance(0, &[key(KeyCode::F5)]).unwrap();
    session.advance(16_666_667, &[key(KeyCode::Enter)]).unwrap();
    assert_eq!(
        service.kinds.lock().last().unwrap(),
        &vec![abi::TEXT_CHOICE, abi::TEXT_CHOICE]
    );
    assert_eq!(
        session.choice_display().unwrap().unwrap().0,
        vec!["Translated", "Translated"]
    );
    let epoch = session
        .replacement
        .as_ref()
        .unwrap()
        .0
        .epoch
        .load(Ordering::Acquire);
    session.advance(0, &[key(KeyCode::F9)]).unwrap();
    assert!(
        session
            .replacement
            .as_ref()
            .unwrap()
            .0
            .epoch
            .load(Ordering::Acquire)
            > epoch
    );
    assert!(session.choice_text.is_none());
    assert_eq!(session.message.as_ref().unwrap().0, "Translated");
    Box::new(session).close().unwrap();
}

#[test]
fn independent_invalidation_rejects_a_late_success() {
    for ending in [false, true] {
        let service = Arc::new(Service::default());
        service.mode.store(2, Ordering::Release);
        let bridge = service.bridge();
        let mut pending = bridge
            .submit(7, &[(abi::TEXT_DIALOGUE, "Original")])
            .unwrap();
        assert!(bridge.poll(&mut pending).unwrap().is_none());
        if ending {
            bridge.0.cancel();
        } else {
            bridge.invalidate();
        }
        service.mode.store(1, Ordering::Release);
        assert_eq!(
            bridge.poll(&mut pending).unwrap_err().code(),
            "SHINKI_EMU_MUSICA_TEXT_CANCELLED"
        );
        assert_eq!(service.acknowledgements.load(Ordering::Acquire), 0);
        assert_eq!(bridge.0.cancelled.load(Ordering::Acquire), ending);
    }
}

#[test]
fn hook_glyph_error_is_a_renderer_failure_not_a_retryable_service_error() {
    let _session = PROVIDER_SESSION.lock();
    let root = tempfile::tempdir().unwrap();
    fixture::game(root.path(), b".message 1   Hello\r\n.end\r\n");
    let service = Box::<Service>::default();
    service.mode.store(3, Ordering::Release);
    let (_, mut session) = fixture::provider()
        .open_native_session(
            request(root.path(), Sink::default()),
            shinki_emu_abi::Extent::default(),
            Some(service.bridge()),
        )
        .unwrap();
    assert_eq!(
        session.advance(16_666_667, &[]).unwrap_err().code(),
        "ROUKAKU_TEXT_GLYPH_MISSING"
    );
    assert!(session.poisoned);
    assert!(session.pending.is_none());
    Box::new(session).close().unwrap();
}

#[test]
fn capacity_retry_keeps_request_identity_and_acknowledges_once() {
    let service = Box::<Service>::default();
    service.mode.store(4, Ordering::Release);
    let bridge = service.bridge();
    let mut pending = bridge
        .submit(17, &[(abi::TEXT_DIALOGUE, "Source")])
        .unwrap();
    let result = bridge.poll(&mut pending).unwrap().unwrap();
    assert_eq!(result, vec!["T".repeat(2048)]);
    assert_eq!(*service.ids.lock(), vec![17]);
    assert_eq!(service.acknowledgements.load(Ordering::Acquire), 1);
}

#[test]
fn delayed_translation_keeps_frames_audio_and_input_alive_and_drops_skip_and_load_results() {
    let _session = PROVIDER_SESSION.lock();
    let root = tempfile::tempdir().unwrap();
    fixture::game(root.path(), b".stage BG.png 0 0 * 0 0\r\n.playBGM tone.ogg[50,-100]\r\n.shakescreen V 4 30\r\n.message 1   First\r\n.message 2   Second\r\n.end\r\n");
    let service = Box::<Service>::default();
    service.mode.store(2, Ordering::Release);
    let sink = Sink::default();
    let (_, mut session) = fixture::provider()
        .open_native_session(
            request(root.path(), sink.clone()),
            abi::Extent::default(),
            Some(service.bridge()),
        )
        .unwrap();
    for _ in 0..10 {
        session.advance(16_666_667, &[]).unwrap();
        if session.pending.is_some() {
            break;
        }
    }
    assert!(session.pending.is_some());
    let instructions = session.vm.state().instruction_count;
    let mut frame = Capture(Vec::new());
    frame.read(session.acquire_frame(None).unwrap()).unwrap();
    let first_frame = frame.0.clone();
    until(|| sink.nonzero.load(Ordering::Acquire));
    let samples = sink.left.load(Ordering::Acquire);
    session.advance(30_000_000, &[]).unwrap();
    frame.read(session.acquire_frame(None).unwrap()).unwrap();
    assert!(
        frame.0 != first_frame,
        "animation must keep changing the pending frame"
    );
    until(|| sink.left.load(Ordering::Acquire) > samples);
    assert_eq!(session.vm.state().instruction_count, instructions);
    assert_eq!(service.calls.load(Ordering::Acquire), 1);
    session.advance(0, &[key(KeyCode::Enter)]).unwrap();
    assert!(session.pending.is_none());
    assert_eq!(session.message.as_ref().unwrap().0, "First");
    assert_eq!(service.cancellations.load(Ordering::Acquire), 1);
    service.mode.store(1, Ordering::Release);
    session.advance(16_666_667, &[]).unwrap();
    assert_eq!(session.message.as_ref().unwrap().0, "First");
    assert_eq!(service.acknowledgements.load(Ordering::Acquire), 0);
    session.advance(0, &[key(KeyCode::F5)]).unwrap();
    service.mode.store(2, Ordering::Release);
    session.advance(16_666_667, &[key(KeyCode::Enter)]).unwrap();
    let mut stale = session.pending.as_mut().unwrap().request.take().unwrap();
    let bridge = session.replacement.as_ref().unwrap().clone();
    session.advance(0, &[key(KeyCode::F9)]).unwrap();
    service.mode.store(1, Ordering::Release);
    assert_eq!(
        bridge.poll(&mut stale).unwrap_err().code(),
        "SHINKI_EMU_MUSICA_TEXT_CANCELLED"
    );
    session.advance(16_666_667, &[]).unwrap();
    assert!(session.pending.is_none());
    assert_eq!(session.message.as_ref().unwrap().0, "Translated");
    assert_eq!(service.acknowledgements.load(Ordering::Acquire), 1);
    Box::new(session).close().unwrap();
}

#[test]
fn host_startup_cancellation_is_checked_without_a_session_handle() {
    unsafe extern "C" fn cancelled(context: *mut c_void, _: u64) -> u32 {
        u32::from(unsafe { &*context.cast::<AtomicBool>() }.load(Ordering::Acquire))
    }
    let service = Box::<Service>::default();
    let stop = AtomicBool::new(true);
    let mut bridge = service.bridge();
    Arc::get_mut(&mut bridge.0).unwrap().host_cancel = Some(abi::CancellationService {
        context: (&stop as *const AtomicBool).cast_mut().cast(),
        is_cancelled: Some(cancelled),
    });
    assert_eq!(
        bridge.check_cancelled().unwrap_err().code(),
        "SHINKI_EMU_MUSICA_CANCELLED"
    );
    assert_eq!(service.calls.load(Ordering::Acquire), 0);
    assert_eq!(service.invalidations.load(Ordering::Acquire), 1);
}

fn embed_native_script(root: &std::path::Path, source: &[u8]) {
    let record = |source: &[u8], text: &str| {
        serde_json::json!({
            "source_hash": crate::musica_display_hash(source),
            "source_sha256": roukaku_core::Hash256::from_sha256(source).to_hex(), "text": text,
        })
    };
    let table = crate::MusicaTranslationTable::from_json(
        &serde_json::to_vec(&serde_json::json!({
            "version": 2, "source_encoding": "gbk", "hash_collision_policy": "first_record",
            "names": [record(b"Speaker", "Native speaker"), record(b"First", "Native choice")],
            "dialogue": [record(b"Hello", "Native dialogue")],
        }))
        .unwrap(),
    )
    .unwrap();
    let (embedded, _) =
        crate::embed_sc_localization(source, &table, crate::ScriptEncoding::Gbk).unwrap();
    fixture::assets(root, "scr", &[("test.sc", &embedded)]);
}

#[test]
fn embedded_display_precedes_hook_and_retry_keeps_vm_and_hook_identity() {
    let _session = PROVIDER_SESSION.lock();
    let root = tempfile::tempdir().unwrap();
    fixture::game(
        root.path(),
        b".message 1  Speaker Hello\r\n.select First:done Second:done\r\n.label done\r\n.end\r\n",
    );
    embed_native_script(
        root.path(),
        b".message 1  Speaker Hello\r\n.select First:done Second:done\r\n.label done\r\n.end\r\n",
    );
    let service = Box::<Service>::default();
    let (_, mut session) = fixture::provider()
        .open_native_session(
            request(root.path(), Sink::default()),
            shinki_emu_abi::Extent::default(),
            Some(service.bridge()),
        )
        .unwrap();
    assert_eq!(
        session.advance(16_666_667, &[]).unwrap_err().code(),
        "TEST_TRANSLATION_FAILED"
    );
    let original = &session.vm.state().backlog[0];
    assert_eq!(original.text, "Native dialogue");
    assert_eq!(original.speaker.as_deref(), Some("Native speaker"));
    assert_eq!(
        service.inputs.lock()[0],
        vec!["Native dialogue", "Native speaker"]
    );
    let instructions = session.vm.state().instruction_count;
    service.mode.store(1, Ordering::Release);
    session.native_retry().unwrap();
    assert_eq!(session.vm.state().instruction_count, instructions);
    {
        let ids = service.ids.lock();
        assert_eq!(ids[0], ids[1]);
    }
    assert_eq!(
        service.inputs.lock()[1],
        vec!["Native dialogue", "Native speaker"]
    );
    session.advance(16_666_667, &[key(KeyCode::Enter)]).unwrap();
    assert_eq!(
        service.inputs.lock().last().unwrap(),
        &vec!["Native choice", "Second"]
    );
    assert_eq!(
        session.vm.choice_display().unwrap().unwrap().0,
        vec!["Native choice", "Second"]
    );
    Box::new(session).close().unwrap();
}

#[test]
fn embedded_native_text_survives_save_restore_without_tab_overlay_state() {
    let _session = PROVIDER_SESSION.lock();
    let root = tempfile::tempdir().unwrap();
    fixture::game(
        root.path(),
        b".message 1  Speaker Hello\r\n.message 2   Unmatched\r\n.end\r\n",
    );
    embed_native_script(
        root.path(),
        b".message 1  Speaker Hello\r\n.message 2   Unmatched\r\n.end\r\n",
    );
    let (_, mut session) = fixture::provider()
        .open_native_session(
            request(root.path(), Sink::default()),
            shinki_emu_abi::Extent::default(),
            None,
        )
        .unwrap();
    session.advance(16_666_667, &[]).unwrap();
    assert_eq!(session.message.as_ref().unwrap().0, "Native dialogue");
    assert_eq!(
        session.message.as_ref().unwrap().1.as_deref(),
        Some("Native speaker")
    );
    let expected = session.message.clone();
    session.advance(0, &[key(KeyCode::Tab)]).unwrap();
    let mut released = key(KeyCode::Tab);
    if let RuntimeEvent::Key { state, .. } = &mut released {
        *state = KeyState::Released;
    }
    session.advance(0, &[released]).unwrap();
    assert_eq!(session.message, expected);
    session.advance(0, &[key(KeyCode::F5)]).unwrap();
    session.advance(0, &[key(KeyCode::F9)]).unwrap();
    assert_eq!(session.message, expected);
    session.advance(16_666_667, &[key(KeyCode::Enter)]).unwrap();
    assert_eq!(session.message.as_ref().unwrap().0, "Unmatched");
    Box::new(session).close().unwrap();
}
