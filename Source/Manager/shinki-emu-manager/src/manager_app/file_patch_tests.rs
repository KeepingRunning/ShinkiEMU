use super::*;
use crate::test_native::{self, Fixture};

fn settle(
    app: &mut ShinkiEmuManagerController,
    ready: impl Fn(&ShinkiEmuManagerController) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ready(app) {
        app.poll_platform().unwrap();
        assert!(
            Instant::now() < deadline,
            "public fixture operation did not finish"
        );
        std::thread::yield_now();
    }
}

fn rejected_import(app: &mut ShinkiEmuManagerController) -> String {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Err(error) = app.poll_platform() {
            assert!(!app.startup_busy());
            return error;
        }
        assert!(app.startup_busy(), "invalid import unexpectedly succeeded");
        assert!(Instant::now() < deadline, "import rejection did not finish");
        std::thread::yield_now();
    }
}

fn controller(root: &Path) -> (ShinkiEmuManagerController, PathBuf, String) {
    controller_with_capabilities(root, shinki_emu_abi::CAP_CPU_SURFACE)
}

fn controller_with_capabilities(
    root: &Path,
    capabilities: u64,
) -> (ShinkiEmuManagerController, PathBuf, String) {
    let source = root.join("source");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("match"), b"public marker").unwrap();
    fs::write(source.join("payload.bin"), b"original public payload").unwrap();
    fs::create_dir(source.join("save")).unwrap();
    fs::write(source.join("save/slot.bin"), b"original public save").unwrap();
    let source = fs::canonicalize(source).unwrap();
    let mut app = ShinkiEmuManagerController::open_library(
        root.join("manager"),
        FrameMailbox::new(),
        Vec::new(),
    )
    .unwrap();
    app.set_host_wake(Arc::new(|| {}));
    app.prepare_library().unwrap();
    settle(&mut app, |app| !app.startup_busy());
    app.registry
        .register_provider(move || {
            let mut fixture = Fixture::new("patch-fixture", "fixture", "fixture");
            fixture.marker = Some("match");
            fixture.config = true;
            fixture.capabilities = capabilities;
            test_native::install(0, fixture)
        })
        .unwrap();
    settle(&mut app, |app| {
        app.registry.descriptor("patch-fixture").is_some()
    });
    app.add_game_directory(&source).unwrap();
    settle(&mut app, |app| !app.startup_busy());
    let id = path_id(&source);
    let mut input = default_vn_preset();
    input.deadzone = GamepadDeadzone::High;
    app.library
        .set_game_settings(
            &id,
            &shinki_emu_manager_core::GameSettings {
                emulator_id: Some("patch-fixture".into()),
                input_mapping: Some(input),
                ..Default::default()
            },
        )
        .unwrap();
    let schema = app.registry.configuration("patch-fixture").unwrap();
    app.library
        .save_family_configuration(
            "patch-fixture",
            &id,
            &schema,
            &[shinki_emu_manager_core::model::ConfigEntry {
                id: "launch_mode".into(),
                value: shinki_emu_manager_core::model::ConfigValue::Enum("direct".into()),
            }],
        )
        .unwrap();
    let play = app.library.start_play_session(&id, 1000).unwrap();
    app.library
        .end_play_session(&play, 2000, PlaySessionEndReason::Leave)
        .unwrap();
    app.select_case(&id).unwrap();
    app.select_patch_game(&id).unwrap();
    (app, source, id)
}

fn recipe(path: &Path, body: &str, configuration: &str) {
    fs::write(
        path,
        format!(
            r#"return {{id="manager-public",version=1,title="公开 Manager 夹具",apply=function(ctx)
        {body}
        ctx.copy("source","match","match")
        ctx.copy("source","payload.bin","payload.bin")
        return {{plugin_id="patch-fixture",configuration={configuration}}}
    end}}"#
        ),
    )
    .unwrap();
    release_manifest(path);
}

fn release_manifest(path: &Path) {
    fs::write(path.with_extension("patch.json"), serde_json::to_vec(&serde_json::json!([{
        "release_id":"r1","vn_id":"v17","title":{"zh-CN":"公开版本","en":"Public edition"},
        "edition":"download","game_version":null,
        "fingerprints":[{"path":"match","size":13,"sha256":crate::metadata_cache::sha256_hex(b"public marker")}]
    }])).unwrap()).unwrap();
}

#[test]
fn selected_output_path_is_previewed_then_created_only_by_successful_conversion() {
    let root = tempfile::tempdir().unwrap();
    let (mut app, source, _) = controller(root.path());
    let script = root.path().join("convert.luau");
    recipe(&script, "", "{launch_mode=\"title\"}");
    let parent = root.path().join("chosen-output-location");
    fs::create_dir(&parent).unwrap();
    let output = parent.join("localized-output");
    app.set_patch_script(&script).unwrap();
    app.set_patch_directory("output", &output).unwrap();
    assert_eq!(app.patch_state.output_directory, output.to_str().unwrap());
    assert_eq!(fs::read_dir(&parent).unwrap().count(), 0);
    app.start_patch().unwrap();
    settle(&mut app, |app| !app.patch_busy() && !app.startup_busy());
    assert!(
        app.patch_state.error.is_empty(),
        "{}",
        app.patch_state.error
    );
    assert_eq!(fs::read_dir(&parent).unwrap().count(), 1);
    assert_eq!(
        fs::read(output.join("payload.bin")).unwrap(),
        b"original public payload"
    );
    assert_eq!(
        fs::read(source.join("payload.bin")).unwrap(),
        b"original public payload"
    );
    assert!(output.join("shinki-patch-result.json").is_file());
}

#[test]
fn occupied_or_source_contained_output_is_rejected_without_mutation() {
    let root = tempfile::tempdir().unwrap();
    let (mut app, source, _) = controller(root.path());
    let script = root.path().join("convert.luau");
    recipe(&script, "", "{launch_mode=\"title\"}");
    let empty = root.path().join("occupied-empty");
    fs::create_dir(&empty).unwrap();
    let nonempty = root.path().join("occupied-data");
    fs::create_dir(&nonempty).unwrap();
    fs::write(nonempty.join("keep"), b"user data").unwrap();
    let file = root.path().join("occupied-file");
    fs::write(&file, b"user file").unwrap();
    let contained = source.join("localized-output");
    for output in [&empty, &nonempty, &file, &source, &contained] {
        app.start_file_patch(&script, Path::new(""), output)
            .unwrap();
        settle(&mut app, |app| !app.patch_busy());
        assert!(!app.patch_state.error.is_empty(), "{output:?}");
        assert_eq!(
            fs::read(source.join("payload.bin")).unwrap(),
            b"original public payload"
        );
        assert_eq!(
            fs::read(source.join("save/slot.bin")).unwrap(),
            b"original public save"
        );
        assert_eq!(fs::read_dir(&empty).unwrap().count(), 0);
        assert_eq!(fs::read(nonempty.join("keep")).unwrap(), b"user data");
        assert_eq!(fs::read(&file).unwrap(), b"user file");
        assert!(!contained.exists());
    }
    assert_eq!(app.library.list_games().unwrap().len(), 1);
}

#[test]
fn completion_uses_output_identity_after_selection_changes_without_copying_source_state() {
    let root = tempfile::tempdir().unwrap();
    let (mut app, source, source_id) = controller(root.path());
    let source_record = app.library.game(&source_id).unwrap();
    let source_settings = app.library.game_settings(&source_id).unwrap();
    let other = root.path().join("other");
    fs::create_dir(&other).unwrap();
    fs::write(other.join("match"), b"other marker").unwrap();
    app.add_game_directory(&other).unwrap();
    settle(&mut app, |app| !app.startup_busy());
    let other_id = path_id(&fs::canonicalize(other).unwrap());
    let other_settings = app.library.game_settings(&other_id).unwrap();
    app.select_case(&source_id).unwrap();
    let script = root.path().join("convert.luau");
    recipe(&script, "", "{launch_mode=\"title\"}");
    let output = root.path().join("independent");
    app.start_file_patch(&script, Path::new(""), &output)
        .unwrap();
    app.select_case(&other_id).unwrap();
    settle(&mut app, |app| !app.patch_busy() && !app.startup_busy());
    let output_id = path_id(&fs::canonicalize(&output).unwrap());
    assert_eq!(app.selected_case_id.as_deref(), Some(other_id.as_str()));
    assert_eq!(app.library.list_games().unwrap().len(), 3);
    assert_eq!(app.library.game(&source_id).unwrap(), source_record);
    assert_eq!(
        app.library.game_settings(&source_id).unwrap(),
        source_settings
    );
    assert_eq!(
        app.library.game_settings(&other_id).unwrap(),
        other_settings
    );
    let schema = app.registry.configuration("patch-fixture").unwrap();
    let values = app
        .library
        .family_configuration("patch-fixture", &output_id, &schema)
        .unwrap()
        .values;
    assert_eq!(
        values[0].value,
        shinki_emu_manager_core::model::ConfigValue::Enum("title".into())
    );
    assert!(
        app.library
            .game_settings(&output_id)
            .unwrap()
            .unwrap()
            .input_mapping
            .is_none()
    );
    let snapshot = app.library.library_snapshot().unwrap();
    assert_eq!(snapshot.play_stats[&source_id].total_duration_ms, 1000);
    assert_eq!(snapshot.play_stats[&output_id].total_duration_ms, 0);
    assert_eq!(
        fs::read(output.join("payload.bin")).unwrap(),
        b"original public payload"
    );
    assert!(!output.join("save").exists());
    assert_eq!(
        fs::read(source.join("save/slot.bin")).unwrap(),
        b"original public save"
    );
    assert!(
        app.patch_state.error.is_empty(),
        "{}",
        app.patch_state.error
    );
}

#[test]
fn in_place_patch_and_restore_reprobe_the_same_library_game() {
    let root = tempfile::tempdir().unwrap();
    let (mut app, source, game_id) = controller(root.path());
    let script = root.path().join("in-place.luau");
    recipe(
        &script,
        r#"local added=ctx.create("added.bin"); added:write(0,buffer.fromstring("new")); added:close()"#,
        "{launch_mode=\"title\"}",
    );
    app.set_patch_mode(true).unwrap();
    app.start_file_patch(&script, Path::new(""), Path::new(""))
        .unwrap();
    settle(&mut app, |app| !app.patch_busy() && !app.startup_busy());
    assert!(
        app.patch_state.error.is_empty(),
        "{}",
        app.patch_state.error
    );
    assert_eq!(app.library.list_games().unwrap().len(), 1);
    assert!(app.library.game(&game_id).is_ok());
    assert_eq!(fs::read(source.join("added.bin")).unwrap(), b"new");
    assert!(!source.join("save/slot.bin").exists());
    assert!(source.join(".shinki/patch-result.json").is_file());
    assert!(app.patch_state_view().has_backup);
    app.restore_patch().unwrap();
    settle(&mut app, |app| !app.patch_busy() && !app.startup_busy());
    assert!(
        app.patch_state.error.is_empty(),
        "{}",
        app.patch_state.error
    );
    assert_eq!(app.library.list_games().unwrap().len(), 1);
    assert_eq!(
        fs::read(source.join("save/slot.bin")).unwrap(),
        b"original public save"
    );
    assert!(!source.join("added.bin").exists());
    assert!(!source.join(".shinki/patch-result.json").exists());
    assert!(!app.patch_state_view().has_backup);
}

#[test]
fn patch_output_inherits_source_work_but_not_its_release() {
    let root = tempfile::tempdir().unwrap();
    let (mut app, _source, source_id) = controller(root.path());
    app.library
        .set_external_identity(&shinki_emu_manager_core::ExternalIdentityRecord {
            game_id: source_id.clone(),
            provider: "vndb".into(),
            remote_id: "v17".into(),
            provenance: "manual".into(),
            linked_at_unix_ms: 1,
        })
        .unwrap();
    let snapshot = shinki_emu_manager_core::MetadataSnapshotRecord {
        game_id: source_id.clone(),
        provider: "vndb".into(),
        remote_id: "v17".into(),
        metadata_json: serde_json::to_string(&MetadataRecord {
            provider: MetadataProviderId::Vndb,
            remote_id: "v17".into(),
            title: "公开作品".into(),
            description: None,
            alternate_titles: Vec::new(),
            developers: Vec::new(),
            tags: Vec::new(),
            release_date: None,
            platforms: Vec::new(),
            engine: None,
            cover: None,
            sensitive: false,
        })
        .unwrap(),
        fetched_at_unix_ms: 2,
        state: shinki_emu_manager_core::MetadataSnapshotState::Fresh,
    };
    app.library.set_metadata_snapshot(&snapshot).unwrap();
    app.library
        .set_game_release(
            &source_id,
            Some(&shinki_emu_manager_core::GameRelease {
                release_id: "r5".into(),
                title: "原版".into(),
            }),
        )
        .unwrap();
    let covers = app.data_dir.join("covers");
    fs::create_dir_all(&covers).unwrap();
    fs::write(covers.join(format!("{source_id}.png")), b"public cover").unwrap();
    let script = root.path().join("convert.luau");
    recipe(&script, "", "{launch_mode=\"title\"}");
    let output = root.path().join("translated");
    app.start_file_patch(&script, Path::new(""), &output)
        .unwrap();
    settle(&mut app, |app| !app.patch_busy() && !app.startup_busy());
    assert!(
        app.patch_state.error.is_empty(),
        "{}",
        app.patch_state.error
    );
    let output_id = path_id(&fs::canonicalize(&output).unwrap());
    let identities = app.library.external_identities(&output_id).unwrap();
    assert_eq!(identities.len(), 1);
    assert_eq!(identities[0].remote_id, "v17");
    assert_eq!(identities[0].provenance, "inherited");
    let inherited = app
        .library
        .metadata_snapshot(&output_id, "vndb")
        .unwrap()
        .unwrap();
    assert_eq!(inherited.metadata_json, snapshot.metadata_json);
    assert_eq!(app.library.game_release(&output_id).unwrap(), None);
    assert_eq!(
        app.library
            .game_release(&source_id)
            .unwrap()
            .unwrap()
            .release_id,
        "r5"
    );
    assert_eq!(
        fs::read(covers.join(format!("{output_id}.png"))).unwrap(),
        b"public cover"
    );
}

#[test]
fn running_patch_blocks_launch_scan_and_second_job_but_cancel_joins_and_allows_restart() {
    let root = tempfile::tempdir().unwrap();
    let (mut app, source, source_id) = controller(root.path());
    let script = root.path().join("cancel.luau");
    recipe(
        &script,
        "ctx.create('partial'):write(0,buffer.fromstring('partial')); ctx.progress('public waiting',0,nil); while true do end",
        "{launch_mode=\"title\"}",
    );
    let parent = root.path().join("chosen-output-location");
    fs::create_dir(&parent).unwrap();
    let output = parent.join("localized-output");
    app.start_file_patch(&script, Path::new(""), &output)
        .unwrap();
    settle(&mut app, |app| app.patch_state.phase == "public waiting");
    assert!(!app.model().unwrap().selection.game.launchable);
    assert!(
        app.start_file_patch(&script, Path::new(""), &output)
            .is_err()
    );
    assert!(app.prepare_library().is_err());
    assert!(app.add_game_directory(&source).is_err());
    assert!(app.launch(&source_id).is_err());
    assert!(app.active.is_none());
    app.cancel_file_patch().unwrap();
    settle(&mut app, |app| app.patch_job.is_none());
    assert!(!output.exists());
    assert!(parent.is_dir());
    assert_eq!(fs::read_dir(&parent).unwrap().count(), 0);
    assert!(app.patch_state_view().can_start);
    recipe(&script, "", "{launch_mode=\"title\"}");
    app.start_file_patch(&script, Path::new(""), &output)
        .unwrap();
    settle(&mut app, |app| !app.patch_busy() && !app.startup_busy());
    assert_eq!(
        fs::read(output.join("payload.bin")).unwrap(),
        b"original public payload"
    );
    assert_eq!(
        fs::read(source.join("save/slot.bin")).unwrap(),
        b"original public save"
    );
    assert!(
        app.patch_state.error.is_empty(),
        "{}",
        app.patch_state.error
    );
}

#[test]
fn unsupported_profile_keeps_completed_output_and_does_not_bind_candidate_or_modify_source() {
    let root = tempfile::tempdir().unwrap();
    let (mut app, _, source_id) = controller(root.path());
    let before = app.library.game_settings(&source_id).unwrap();
    let script = root.path().join("unsupported.luau");
    recipe(&script, "", "{unsupported_field=\"value\"}");
    let output = root.path().join("completed-but-not-imported");
    app.start_file_patch(&script, Path::new(""), &output)
        .unwrap();
    settle(&mut app, |app| !app.patch_busy() && !app.startup_busy());
    assert_eq!(
        fs::read(output.join("payload.bin")).unwrap(),
        b"original public payload"
    );
    assert!(output.join("shinki-patch-result.json").is_file());
    let output_id = path_id(&fs::canonicalize(output).unwrap());
    assert!(app.patch_state.error.contains("unknown field"));
    assert!(!app.candidates.contains_key(&output_id));
    assert!(!app.probe_choices.contains_key(&output_id));
    assert!(app.library.game_settings(&output_id).unwrap().is_none());
    assert_eq!(app.library.game_settings(&source_id).unwrap(), before);
    assert_eq!(app.selected_case_id.as_deref(), Some(source_id.as_str()));
}

#[test]
fn explicit_receipt_import_reads_only_data_and_private_staging_is_never_discovered() {
    let root = tempfile::tempdir().unwrap();
    let (mut app, _, source_id) = controller(root.path());
    let before = app.library.game_settings(&source_id).unwrap();
    let output = root.path().join("manual-output");
    fs::create_dir(&output).unwrap();
    fs::write(output.join("match"), b"public manual fixture").unwrap();
    fs::write(
        output.join("must-not-run.luau"),
        b"this is deliberately not Lua",
    )
    .unwrap();
    let receipt = r#"{"schema":"shinki.emu.file_patch.v1","scripts":[{"path":"must-not-run.luau","sha256":"not-executed"}],"profile":{"plugin_id":"patch-fixture","configuration":{"launch_mode":"title"}}}"#;
    fs::write(output.join("shinki-patch-result.json"), receipt).unwrap();
    app.add_game_directory(&output).unwrap();
    settle(&mut app, |app| !app.startup_busy());
    let output_id = path_id(&fs::canonicalize(output).unwrap());
    let schema = app.registry.configuration("patch-fixture").unwrap();
    assert_eq!(
        app.library
            .family_configuration("patch-fixture", &output_id, &schema)
            .unwrap()
            .values[0]
            .value,
        shinki_emu_manager_core::model::ConfigValue::Enum("title".into())
    );
    let staging = root.path().join(".shinki-patch-staging-public");
    fs::create_dir(&staging).unwrap();
    fs::write(staging.join("match"), b"unfinished").unwrap();
    fs::write(staging.join("shinki-patch-result.json"), receipt).unwrap();
    app.add_game_directory(root.path()).unwrap();
    settle(&mut app, |app| !app.startup_busy());
    assert!(
        !app.library
            .list_games()
            .unwrap()
            .iter()
            .any(|game| shinki_emu_patch::is_staging_path(Path::new(&game.location)))
    );
    assert_eq!(app.library.game_settings(&source_id).unwrap(), before);
}

#[test]
fn native_profile_import_requires_every_loaded_core_capability_before_binding() {
    let required = shinki_emu_abi::CAP_NATIVE_SCRIPT_IDENTITY
        | shinki_emu_abi::CAP_CONFIGURED_FONT_PRIORITY
        | shinki_emu_abi::CAP_NATIVE_EMBEDDED_TEXT
        | shinki_emu_abi::CAP_NATIVE_NAME_RESOURCES;
    for available in [
        0,
        shinki_emu_abi::CAP_NATIVE_SCRIPT_IDENTITY,
        shinki_emu_abi::CAP_CONFIGURED_FONT_PRIORITY,
        shinki_emu_abi::CAP_NATIVE_EMBEDDED_TEXT,
        shinki_emu_abi::CAP_NATIVE_NAME_RESOURCES,
        shinki_emu_abi::CAP_NATIVE_EMBEDDED_TEXT | shinki_emu_abi::CAP_NATIVE_NAME_RESOURCES,
        shinki_emu_abi::CAP_NATIVE_SCRIPT_IDENTITY | shinki_emu_abi::CAP_CONFIGURED_FONT_PRIORITY,
        required,
    ] {
        let root = tempfile::tempdir().unwrap();
        let (mut app, _, source_id) =
            controller_with_capabilities(root.path(), shinki_emu_abi::CAP_CPU_SURFACE | available);
        let before = app.library.game_settings(&source_id).unwrap();
        let output = root.path().join("native-output");
        fs::create_dir(&output).unwrap();
        fs::write(output.join("match"), b"public native fixture").unwrap();
        fs::write(
            output.join("shinki-patch-result.json"),
            serde_json::to_vec(&serde_json::json!({
                "schema": "shinki.emu.file_patch.v1",
                "profile": {
                    "plugin_id": "patch-fixture",
                    "required_capabilities": required,
                    "configuration": {"launch_mode": "title"}
                }
            }))
            .unwrap(),
        )
        .unwrap();
        app.add_game_directory(&output).unwrap();
        if available == required {
            settle(&mut app, |app| !app.startup_busy());
        } else {
            assert!(rejected_import(&mut app).contains("SHINKI_EMU_CORE_UPDATE_REQUIRED"));
        }
        let output_id = path_id(&fs::canonicalize(&output).unwrap());
        if available == required {
            assert!(app.candidates.contains_key(&output_id));
            let schema = app.registry.configuration("patch-fixture").unwrap();
            assert_eq!(
                app.library
                    .family_configuration("patch-fixture", &output_id, &schema)
                    .unwrap()
                    .values[0]
                    .value,
                shinki_emu_manager_core::model::ConfigValue::Enum("title".into())
            );
        } else {
            assert!(!app.candidates.contains_key(&output_id));
            assert!(!app.probe_choices.contains_key(&output_id));
            assert!(app.library.game_settings(&output_id).unwrap().is_none());
            assert!(app.library.game(&output_id).unwrap().is_none());
            assert_eq!(app.library.list_games().unwrap().len(), 1);
            assert!(app.diagnostic.contains("SHINKI_EMU_CORE_UPDATE_REQUIRED"));
        }
        assert_eq!(app.library.game_settings(&source_id).unwrap(), before);
        assert_eq!(
            fs::read(output.join("match")).unwrap(),
            b"public native fixture"
        );
    }
}

#[test]
fn automatic_import_rejects_removed_receipt_requirements_before_library_mutation() {
    let root = tempfile::tempdir().unwrap();
    let (mut app, _, source_id) = controller(root.path());
    let before = app.library.game(&source_id).unwrap();
    let script = root.path().join("native.luau");
    fs::write(&script, r#"return {id="native",version=1,title="Native",apply=function(ctx)
        ctx.copy("source","match","match")
        return {plugin_id="patch-fixture",required_capabilities=192,configuration={launch_mode="title"}}
    end}"#).unwrap();
    release_manifest(&script);
    let output = root.path().join("native-output");
    app.start_file_patch(&script, Path::new(""), &output)
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !app
        .patch_job
        .as_ref()
        .unwrap()
        .thread
        .as_ref()
        .unwrap()
        .is_finished()
    {
        assert!(
            Instant::now() < deadline,
            "conversion worker did not finish"
        );
        std::thread::yield_now();
    }
    let receipt = output.join("shinki-patch-result.json");
    let mut document: serde_json::Value =
        serde_json::from_slice(&fs::read(&receipt).unwrap()).unwrap();
    assert_eq!(document["profile"]["required_capabilities"], 192);
    document["profile"]
        .as_object_mut()
        .unwrap()
        .remove("required_capabilities");
    fs::write(receipt, serde_json::to_vec(&document).unwrap()).unwrap();
    settle(&mut app, |app| !app.patch_busy() && !app.startup_busy());
    assert!(app.patch_state.error.contains("SHINKI_EMU_PATCH_INTEGRITY"));
    let output_id = path_id(&fs::canonicalize(&output).unwrap());
    assert!(app.library.game(&output_id).unwrap().is_none());
    assert_eq!(app.library.list_games().unwrap().len(), 1);
    assert_eq!(app.library.game(&source_id).unwrap(), before);
    assert!(!app.candidates.contains_key(&output_id));
    assert!(output.join("match").is_file());
}

#[test]
fn native_profile_receipt_blocks_launch_after_reopening_with_an_older_core() {
    let root = tempfile::tempdir().unwrap();
    let required =
        shinki_emu_abi::CAP_NATIVE_SCRIPT_IDENTITY | shinki_emu_abi::CAP_CONFIGURED_FONT_PRIORITY;
    let (mut app, _, _) =
        controller_with_capabilities(root.path(), shinki_emu_abi::CAP_CPU_SURFACE | required);
    let output = root.path().join("native-output");
    fs::create_dir(&output).unwrap();
    fs::write(output.join("match"), b"public native fixture").unwrap();
    fs::write(
        output.join("shinki-patch-result.json"),
        serde_json::to_vec(&serde_json::json!({
            "schema": "shinki.emu.file_patch.v1",
            "profile": {
                "plugin_id": "patch-fixture",
                "required_capabilities": required,
                "configuration": {"launch_mode": "title"}
            }
        }))
        .unwrap(),
    )
    .unwrap();
    app.add_game_directory(&output).unwrap();
    settle(&mut app, |app| !app.startup_busy());
    let id = path_id(&fs::canonicalize(&output).unwrap());
    assert!(app.candidates.contains_key(&id));
    drop(app);
    let mut reopened = ShinkiEmuManagerController::open_library(
        root.path().join("manager"),
        FrameMailbox::new(),
        Vec::new(),
    )
    .unwrap();
    reopened.set_host_wake(Arc::new(|| {}));
    reopened.prepare_library().unwrap();
    settle(&mut reopened, |app| !app.startup_busy());
    reopened
        .registry
        .register_provider(|| {
            let mut fixture = Fixture::new("patch-fixture", "fixture", "fixture");
            fixture.marker = Some("match");
            fixture.config = true;
            test_native::install(0, fixture)
        })
        .unwrap();
    settle(&mut reopened, |app| {
        app.registry.descriptor("patch-fixture").is_some()
    });
    reopened.prepare_library().unwrap();
    settle(&mut reopened, |app| !app.startup_busy());
    assert!(
        reopened
            .launch(&id)
            .unwrap_err()
            .contains("SHINKI_EMU_CORE_UPDATE_REQUIRED")
    );
    assert!(reopened.active.is_none());
}

#[test]
fn malformed_receipt_requirements_block_import_and_retained_receipt_launch() {
    for required in [
        serde_json::json!(1024),
        serde_json::json!(-1),
        serde_json::json!(1.5),
        serde_json::json!("192"),
    ] {
        let root = tempfile::tempdir().unwrap();
        let (mut app, _, _) = controller(root.path());
        let output = root.path().join("native-output");
        fs::create_dir(&output).unwrap();
        fs::write(output.join("match"), b"public native fixture").unwrap();
        let receipt = output.join("shinki-patch-result.json");
        let mut document = serde_json::json!({
            "schema": "shinki.emu.file_patch.v1",
            "profile": {
                "plugin_id": "patch-fixture",
                "required_capabilities": required,
                "configuration": {"launch_mode": "title"}
            }
        });
        fs::write(&receipt, serde_json::to_vec(&document).unwrap()).unwrap();
        app.add_game_directory(&output).unwrap();
        assert!(rejected_import(&mut app).contains("SHINKI_EMU_PATCH_INVALID_REQUEST"));
        let id = path_id(&fs::canonicalize(&output).unwrap());
        assert!(app.library.game(&id).unwrap().is_none());
        assert_eq!(app.library.list_games().unwrap().len(), 1);
        assert!(!app.candidates.contains_key(&id));
        drop(app);
        let launch_host = root.path().join("launch-host");
        fs::create_dir(&launch_host).unwrap();
        let (mut app, _, _) = controller(&launch_host);

        document["profile"]
            .as_object_mut()
            .unwrap()
            .remove("required_capabilities");
        fs::write(&receipt, serde_json::to_vec(&document).unwrap()).unwrap();
        app.add_game_directory(&output).unwrap();
        settle(&mut app, |app| !app.startup_busy());
        assert!(app.candidates.contains_key(&id));
        document["profile"]["required_capabilities"] = required;
        fs::write(receipt, serde_json::to_vec(&document).unwrap()).unwrap();
        assert!(
            app.launch(&id)
                .unwrap_err()
                .contains("SHINKI_EMU_PATCH_INVALID_REQUEST")
        );
        assert!(app.active.is_none());
    }
}

#[test]
#[ignore = "requires a real conversion receipt and read-only installed beta3/updated RFVP libraries; never opens a game"]
fn native_profile_checks_real_rfvp_descriptors_without_version_string_inference() {
    let output =
        std::env::var_os("SHINKI_RFVP_PROFILE_OUTPUT").expect("SHINKI_RFVP_PROFILE_OUTPUT");
    let profile =
        shinki_emu_patch::read_output_profile(Path::new(&output), &AtomicBool::new(false))
            .unwrap()
            .expect("actual native conversion receipt");
    assert_eq!(profile.plugin_id, "rfvp");
    assert_eq!(
        profile.required_capabilities,
        shinki_emu_abi::CAP_NATIVE_SCRIPT_IDENTITY | shinki_emu_abi::CAP_CONFIGURED_FONT_PRIORITY,
    );
    assert_eq!(profile.configuration["script_encoding"], "gbk");
    assert_eq!(profile.configuration["font_path"], "font/HanaMinA.ttf");
    assert!(!profile.configuration.contains_key("translation_table"));
    for (variable, accepted) in [
        ("SHINKI_RFVP_OLD_CORE", false),
        ("SHINKI_RFVP_NATIVE_CORE", true),
    ] {
        let path = std::env::var_os(variable).expect(variable);
        let loaded = shinki_emu_manager_core::LoadedFamilyPlugin::load(Path::new(&path)).unwrap();
        let descriptor = loaded.manager_descriptor();
        let result = file_patch::require_profile_capabilities(&profile, descriptor);
        assert_eq!(result.is_ok(), accepted, "{variable}: {result:?}");
        if !accepted {
            assert!(
                result
                    .unwrap_err()
                    .contains("SHINKI_EMU_CORE_UPDATE_REQUIRED")
            );
        }
    }
}
