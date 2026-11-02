use super::*;
use crate::{MusicaVm, MusicaVmEvent, MusicaWaitState};

const NAMES: &[u8] = b".pragma shinki_name_unknown \\u{672a}\\u{77e5}\r\n.pragma shinki_history_name_unknown \\u{3010}\\u{672a}\\u{77e5}\\u{3011}\r\n";

fn exe() -> Vec<u8> {
    let mut bytes = vec![0; 0x121f30];
    for (offset, text) in [(0x112098, "？？？"), (0x121f18, "【？？？】")] {
        let encoded = ScriptEncoding::ShiftJis.encode(text).unwrap();
        bytes[offset..offset + encoded.len()].copy_from_slice(&encoded);
    }
    bytes
}

fn vm(source: &[u8], names: Option<MusicaNameResources>) -> MusicaVm {
    let script = crate::parse_sc_with_encoding(
        source,
        &ScOpcodeCatalog::observed_musica(),
        ScriptEncoding::Gbk,
    )
    .unwrap();
    let mut vm = MusicaVm::new(
        "musica:/scr/test.sc".into(),
        Hash256::from_sha256(source),
        script,
        0,
    )
    .unwrap();
    vm.set_name_resources(names);
    vm
}

fn next_message(vm: &mut MusicaVm) -> MusicaVmEvent {
    for _ in 1..20 {
        let tick = vm.state().fixed_tick + 1;
        if let Some(event @ MusicaVmEvent::Message { .. }) = vm.step(tick).unwrap() {
            return event;
        }
    }
    panic!("message was not reached")
}

#[test]
fn resource_wins_over_read_only_executable_and_missing_resource_uses_original_constants() {
    let root = tempfile::tempdir().unwrap();
    let original = exe();
    std::fs::write(root.path().join("yorino.exe"), &original).unwrap();
    std::fs::create_dir(root.path().join("scripts")).unwrap();
    let fallback = MusicaNameResources::load(root.path(), "scripts")
        .unwrap()
        .unwrap();
    assert_eq!(
        fallback
            .live_name("@Ren", ScriptEncoding::ShiftJis)
            .unwrap()
            .as_deref(),
        Some("？？？")
    );
    assert_eq!(
        ScriptEncoding::ShiftJis
            .encode(&fallback.unknown)
            .unwrap()
            .len(),
        6
    );
    assert_eq!(
        fallback
            .history_name("#Ren", ScriptEncoding::ShiftJis)
            .unwrap()
            .as_deref(),
        Some("【？？？】")
    );
    assert!(fallback.resource_identity.is_none());
    std::fs::write(root.path().join("scripts/shinki-names.sc"), NAMES).unwrap();
    let resource = MusicaNameResources::load(root.path(), "scripts")
        .unwrap()
        .unwrap();
    assert_eq!(
        resource
            .live_name("@Ren", ScriptEncoding::Gbk)
            .unwrap()
            .as_deref(),
        Some("未知")
    );
    assert_eq!(
        resource
            .history_name("#Ren", ScriptEncoding::Gbk)
            .unwrap()
            .as_deref(),
        Some("【未知】")
    );
    assert!(resource.resource_identity.is_some());
    assert_eq!(
        std::fs::read(root.path().join("yorino.exe")).unwrap(),
        original
    );
    // An explicit resource has no dependency on executable contents.
    std::fs::write(root.path().join("yorino.exe"), b"not read").unwrap();
    assert_eq!(
        MusicaNameResources::load(root.path(), "scripts")
            .unwrap()
            .unwrap(),
        resource
    );
}

#[test]
fn yorino_live_and_history_follow_native_first_byte_and_last_equals_rules() {
    let names = MusicaNameResources::parse(NAMES).unwrap();
    for (token, live, history) in [
        ("", None, None),
        ("@Ren", Some("未知"), Some("【未知】")),
        ("#Ren", None, Some("【未知】")),
        ("Ren", Some("Ren"), Some("【Ren】")),
        ("tag=old=恋", Some("恋"), Some("【tag=old=恋】")),
        ("\\u{604b}", Some("恋"), Some("【恋】")),
        (
            "tag= 恋\t\u{3000}",
            Some(" 恋\t\u{3000}"),
            Some("【tag= 恋\t\u{3000}】"),
        ),
        ("甲乙丙丁", Some("甲乙丙"), Some("【甲乙丙丁】")),
        ("abcde恋", Some("abcde"), Some("【abcde恋】")),
        ("@\\u{604b}", Some("未知"), Some("【未知】")),
    ] {
        assert_eq!(
            names
                .live_name(token, ScriptEncoding::Gbk)
                .unwrap()
                .as_deref(),
            live
        );
        let result = names.history_name(token, ScriptEncoding::Gbk).unwrap();
        assert_eq!(result.as_deref(), history);
        if let Some(result) = result {
            let bytes = ScriptEncoding::Gbk.encode(&result).unwrap();
            assert_eq!(ScriptEncoding::Gbk.decode(&bytes).unwrap(), result);
            if !token.is_empty() && !token.starts_with(['@', '#']) {
                assert!(bytes.starts_with(&[0xa1, 0xbe]));
                assert!(bytes.ends_with(&[0xa1, 0xbf]));
            }
        }
    }
}

#[test]
fn live_restore_and_restored_backlog_share_name_resources() {
    for (speaker, expected_live, expected_history) in [
        ("@\\u{604b}", Some("未知"), "【未知】"),
        ("#\\u{604b}", None, "【未知】"),
        ("tag=\\u{604b}", Some("恋"), "【tag=恋】"),
    ] {
        let source = format!(
            ".stage front.png 12 34 back.png 5 6\n.panel 1\n.message 1  {speaker} First\n.message 2  {speaker} Second\n.end\n"
        );
        let names = MusicaNameResources::parse(NAMES).unwrap();
        let mut live = vm(source.as_bytes(), Some(names.clone()));
        let MusicaVmEvent::Message { speaker, .. } = next_message(&mut live) else {
            unreachable!()
        };
        assert_eq!(speaker.as_deref(), expected_live);
        let MusicaWaitState::Input { token_id } = live.state().wait.clone().unwrap() else {
            panic!()
        };
        live.resolve_wait(&token_id).unwrap();
        next_message(&mut live);
        let checkpoint = live.native_checkpoint().unwrap();
        let save = crate::eden_save::EdenSave::from_checkpoint(
            &checkpoint,
            crate::eden_save::EdenEdition::English,
            crate::eden_save::EdenSaveEncoding::Gbk,
            0,
            String::new(),
        )
        .unwrap();
        let mut restored = vm(source.as_bytes(), Some(names));
        let MusicaVmEvent::Message { speaker, .. } = restored.restore_eden_save(&save, 1).unwrap()
        else {
            panic!()
        };
        assert_eq!(speaker.as_deref(), expected_live);
        for state in [live.state(), restored.state()] {
            assert_eq!(state.backlog.len(), 2);
            for entry in &state.backlog {
                assert_eq!(entry.speaker.as_deref(), Some(expected_history));
                assert_eq!(
                    entry.speaker_hash,
                    Some(Hash256::from_sha256(expected_history.as_bytes()))
                );
            }
        }
        assert_eq!(restored.state().name_resources, live.state().name_resources);
    }
}

#[test]
fn absent_resource_leaves_other_games_unchanged_and_resource_identity_binds_vm() {
    let root = tempfile::tempdir().unwrap();
    assert!(
        MusicaNameResources::load(root.path(), "")
            .unwrap()
            .is_none()
    );
    let source = b".message 1  @Ren Body\n.end\n";
    let mut original = vm(source, None);
    let MusicaVmEvent::Message { speaker, .. } = next_message(&mut original) else {
        panic!()
    };
    assert_eq!(speaker.as_deref(), Some("Ren"));
    assert_eq!(original.state().backlog[0].speaker.as_deref(), Some("Ren"));
    let mut patched = vm(source, Some(MusicaNameResources::parse(NAMES).unwrap()));
    next_message(&mut patched);
    assert!(patched.install_test_state(original.state(), 2).is_err());
    let fallback = MusicaNameResources::from_exe(&mut std::io::Cursor::new(exe())).unwrap();
    let mut unpatched_yorino = vm(source, Some(fallback));
    assert!(
        unpatched_yorino
            .install_test_state(patched.state(), 2)
            .is_err()
    );
    let MusicaVmEvent::Message { speaker, .. } = next_message(&mut unpatched_yorino) else {
        panic!()
    };
    assert_eq!(speaker.as_deref(), Some("？？？"));
    assert_eq!(
        unpatched_yorino.state().backlog[0].speaker.as_deref(),
        Some("【？？？】")
    );
}

#[test]
fn mounted_resources_bind_script_and_game_identity_without_changing_script_bytes() {
    let root = tempfile::tempdir().unwrap();
    let source = b".message 1  @Ren Body\n.end\n";
    crate::test_fixture::game(root.path(), source);
    let baseline = crate::mount_musica(root.path(), &root.path().join("key.toml")).unwrap();
    let baseline_script =
        crate::script_loader::load_script(&baseline, "musica:/scr/test.sc", ScriptEncoding::Gbk)
            .unwrap();
    let baseline_identity = crate::resources::game_identity(&baseline, None, None).unwrap();
    std::fs::create_dir(root.path().join("scripts")).unwrap();
    std::fs::write(root.path().join("scripts/shinki-names.sc"), NAMES).unwrap();
    let (patched, _) = crate::mount_musica_with_resources(
        root.path(),
        &root.path().join("key.toml"),
        &crate::MusicaResourcePaths {
            script_directory: "scripts".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let patched_script =
        crate::script_loader::load_script(&patched, "musica:/scr/test.sc", ScriptEncoding::Gbk)
            .unwrap();
    assert_ne!(baseline_script.hash, patched_script.hash);
    assert_eq!(
        crate::encode_sc(&baseline_script.script).unwrap(),
        crate::encode_sc(&patched_script.script).unwrap()
    );
    assert_ne!(
        baseline_identity,
        crate::resources::game_identity(&patched, None, None).unwrap()
    );
}

#[test]
fn malformed_present_resource_does_not_silently_use_executable() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("yorino.exe"), exe()).unwrap();
    std::fs::create_dir(root.path().join("scripts")).unwrap();
    for bytes in [
        b".pragma shinki_name_unknown \\u{41}\n".as_slice(),
        b".pragma shinki_name_unknown \\u{0}\n",
    ] {
        std::fs::write(root.path().join("scripts/shinki-names.sc"), bytes).unwrap();
        assert!(MusicaNameResources::load(root.path(), "scripts").is_err());
    }
}

#[test]
fn embedded_translation_retains_authored_name_style_for_live_and_history() {
    for (token, translated, expected_live, expected_history) in [
        ("@Ren", "恋", Some("未知"), "【未知】"),
        ("#Ren", "恋", None, "【未知】"),
        ("Ren", "恋", Some("恋"), "【恋】"),
        ("Ren", "@恋", Some("未知"), "【未知】"),
        ("Ren", "#恋", None, "【未知】"),
    ] {
        let source = format!(".message 1  {token} Body\n.end\n");
        let document = serde_json::json!({
            "version": 2,
            "source_encoding": "gbk",
            "hash_collision_policy": "first_record",
            "names": [{
                "source_hash": crate::musica_display_cstr_hash(token.as_bytes()),
                "source_sha256": Hash256::from_sha256(token.as_bytes()).to_hex(),
                "text": translated
            }],
            "dialogue": []
        });
        let table =
            crate::MusicaTranslationTable::from_json(&serde_json::to_vec(&document).unwrap())
                .unwrap();
        let (embedded, _) =
            crate::embed_sc_localization(source.as_bytes(), &table, ScriptEncoding::Gbk).unwrap();
        let mut machine = vm(&embedded, Some(MusicaNameResources::parse(NAMES).unwrap()));
        let MusicaVmEvent::Message { speaker, .. } = next_message(&mut machine) else {
            panic!()
        };
        assert_eq!(speaker.as_deref(), expected_live);
        assert_eq!(
            machine.state().backlog[0].speaker.as_deref(),
            Some(expected_history)
        );
        let mut markerless = vm(&embedded, None);
        let MusicaVmEvent::Message { speaker, .. } = next_message(&mut markerless) else {
            panic!()
        };
        assert_eq!(speaker.as_deref(), Some("恋"));
    }
}

#[test]
fn masked_live_names_keep_existing_animation_speaker_matching() {
    let source = b".message 1  @\\u{604b} Body\n.end\n";
    let mut machine = vm(source, Some(MusicaNameResources::parse(NAMES).unwrap()));
    let MusicaVmEvent::Message { speaker, .. } = next_message(&mut machine) else {
        panic!()
    };
    assert_eq!(speaker.as_deref(), Some("未知"));
    machine
        .attach_ani_patches(std::collections::BTreeMap::from([(
            "musica:/bg/back.png".into(),
            (vec!["恋".into()], 0),
        )]))
        .unwrap();
    machine
        .advance_ani_patch_clock(10_000_000, speaker.as_deref(), Some((true, 1000)))
        .unwrap();
    let character = &machine.state().ani_patches["musica:/bg/back.png"].characters[0];
    assert_eq!(character.mouth_state, 1);
}
