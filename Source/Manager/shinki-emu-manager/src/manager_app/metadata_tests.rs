use super::*;

fn controller(root: &Path) -> ShinkiEmuManagerController {
    ShinkiEmuManagerController::open_library(root.into(), FrameMailbox::new(), Vec::new()).unwrap()
}

#[test]
#[cfg(not(target_os = "android"))]
fn damaged_core_keeps_manager_available_and_error_visible() {
    let root = tempfile::tempdir().unwrap();
    let core = root.path().join("cores").join("shinki_emu_broken");
    std::fs::create_dir_all(&core).unwrap();
    std::fs::write(
        core.join(format!(
            "shinki_emu_broken.{}",
            std::env::consts::DLL_EXTENSION
        )),
        b"not a dynamic library",
    )
    .unwrap();
    std::fs::write(
        core.join(format!("avcodec-62.{}", std::env::consts::DLL_EXTENSION)),
        b"dependency bytes",
    )
    .unwrap();

    let mut app = controller(root.path());
    app.prepare_library().unwrap();
    super::probe_state_tests::settle_background(&mut app);
    assert!(app.plugin_errors.contains_key("shinki_emu_broken"));
    assert!(!app.plugin_errors.contains_key("avcodec-62"));
    assert!(app.registry.descriptor("shinki.emu.missing").is_none());
    assert!(
        app.model()
            .unwrap()
            .notices
            .items
            .iter()
            .any(|notice| notice.code.contains("SHINKI_EMU_FAMILY_LOAD"))
    );
}

fn add_game(controller: &mut ShinkiEmuManagerController, id: &str) {
    controller
        .library
        .add_game(&GameRecord {
            game_id: id.into(),
            title: "Imported folder".into(),
            user_title: None,
            location: format!("fixtures/{id}"),
            family_id: None,
            content_fingerprint: None,
            added_at_unix_ms: 1,
        })
        .unwrap();
    controller.selected_case_id = Some(id.into());
}

fn record() -> MetadataRecord {
    MetadataRecord {
        provider: MetadataProviderId::Vndb,
        remote_id: "v1".into(),
        title: "Search result".into(),
        description: None,
        alternate_titles: vec!["Alias".into()],
        developers: vec!["Developer".into()],
        tags: Vec::new(),
        release_date: Some("2026-01-01".into()),
        platforms: Vec::new(),
        engine: None,
        cover: None,
        sensitive: false,
    }
}

#[test]
fn metadata_candidates_cannot_link_another_selected_game() {
    let root = tempfile::tempdir().unwrap();
    let mut app = controller(root.path());
    add_game(&mut app, "first");
    app.apply_metadata_payload(
        Some("first"),
        MetadataProviderId::Vndb,
        MetadataPayload::Search(vec![record()]),
    )
    .unwrap();
    let candidate = app.model().unwrap().selection.matches[0].id.clone();
    add_game(&mut app, "second");
    assert!(app.model().unwrap().selection.matches.is_empty());
    assert_eq!(
        app.accept_match(&candidate).unwrap_err(),
        "SHINKI_EMU_METADATA_MATCH_GAME_MISMATCH"
    );
    assert!(app.pending_metadata.is_empty());
    assert!(app.library.external_identities("first").unwrap().is_empty());
}

#[test]
fn metadata_search_rejects_invalid_input_and_overlapping_requests() {
    let root = tempfile::tempdir().unwrap();
    let mut app = controller(root.path());
    add_game(&mut app, "first");
    assert_eq!(
        app.search_metadata("vndb", "Title").unwrap_err(),
        "SHINKI_EMU_NETWORK_PERMISSION_REQUIRED"
    );
    app.network_allowed = true;
    assert!(app.search_metadata("vndb", "   ").is_err());
    assert!(app.search_metadata("vndb", &"x".repeat(257)).is_err());
    assert!(app.pending_metadata.is_empty());
    app.pending_metadata.insert(
        "existing".into(),
        PendingMetadata {
            game_id: Some("first".into()),
            core: None,
        },
    );
    assert_eq!(
        app.search_metadata("vndb", "Title").unwrap_err(),
        "SHINKI_EMU_METADATA_REQUEST_PENDING"
    );
    assert_eq!(
        app.unlink_identity("vndb").unwrap_err(),
        "SHINKI_EMU_METADATA_REQUEST_PENDING"
    );
    assert!(app.model().unwrap().selection.metadata_busy);
}

#[test]
fn metadata_linked_title_persists_and_unlink_hides_stale_snapshot() {
    let root = tempfile::tempdir().unwrap();
    {
        let mut app = controller(root.path());
        add_game(&mut app, "first");
        app.apply_metadata_payload(
            Some("first"),
            MetadataProviderId::Vndb,
            MetadataPayload::Fetch {
                record: Box::new(record()),
                cover: None,
            },
        )
        .unwrap();
        assert_eq!(app.model().unwrap().selection.game.title, "Search result");
        assert_eq!(app.model().unwrap().library.games[0].title, "Search result");
        app.search_query = "Search result".into();
        assert_eq!(app.model().unwrap().library.games.len(), 1);
    }
    let mut app = controller(root.path());
    app.selected_case_id = Some("first".into());
    assert_eq!(app.model().unwrap().selection.game.title, "Search result");
    app.library
        .set_game_user_title("first", Some("My title"))
        .unwrap();
    assert_eq!(app.model().unwrap().selection.game.title, "My title");
    app.library.set_game_user_title("first", None).unwrap();
    app.unlink_identity("vndb").unwrap();
    assert_eq!(app.model().unwrap().selection.game.title, "Imported folder");
    assert!(
        app.metadata_record("first", MetadataProviderId::Vndb)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        app.refresh_metadata("vndb").unwrap_err(),
        "SHINKI_EMU_METADATA_NOT_LINKED"
    );
}

#[test]
fn network_permission_gates_metadata_and_translation_and_persists() {
    let root = tempfile::tempdir().unwrap();
    {
        let mut app = controller(root.path());
        add_game(&mut app, "first");
        assert!(!app.network_allowed);
        assert_eq!(
            app.search_metadata("bangumi", "Title").unwrap_err(),
            "SHINKI_EMU_NETWORK_PERMISSION_REQUIRED"
        );
        assert!(app.pending_metadata.is_empty());
        let profile = TranslationProfile {
            model: "model".into(),
            ..TranslationProfile::default()
        };
        app.library.set_translation_profile(&profile).unwrap();
        app.library
            .set_app_state(TRANSLATION_ENABLED_KEY, &true)
            .unwrap();
    }
    let mut app = controller(root.path());
    let (runtime, _commands, _completions) = MetadataRuntime::scripted();
    app.metadata = runtime;
    assert!(app.translation_enabled);
    assert!(app.launch_translation_profile().unwrap().is_none());
    assert_eq!(
        app.set_translation_enabled(true).unwrap_err(),
        "SHINKI_EMU_NETWORK_PERMISSION_REQUIRED"
    );
    app.set_network_allowed(true).unwrap();
    assert!(app.launch_translation_profile().unwrap().is_some());
    drop(app);
    let mut app = controller(root.path());
    assert!(app.network_allowed);
    assert!(app.model().unwrap().network.allowed);
}

fn releases(app: &mut ShinkiEmuManagerController, game_id: &str, json: &str) {
    app.apply_metadata_payload(
        Some(game_id),
        MetadataProviderId::Vndb,
        MetadataPayload::Releases(serde_json::from_str(json).unwrap()),
    )
    .unwrap();
}

#[test]
fn release_selection_preserves_localized_details_and_duplicate_title_identity() {
    let root = tempfile::tempdir().unwrap();
    let mut app = controller(root.path());
    add_game(&mut app, "first");
    let title = "樱花飘落的季节、与你重逢的故事 ～初回限定纪念特别发行版本～".repeat(4);
    let payload = serde_json::json!([
        {"release_id":"r10", "title":title, "released":"2026-10-31", "platforms":["win","linux"]},
        {"release_id":"r11", "title":title, "released":"2025-01-01", "platforms":["mac"]}
    ]);
    releases(&mut app, "first", &payload.to_string());
    let selection = app.model().unwrap().selection;
    assert_eq!(selection.releases.len(), 2);
    assert_eq!(selection.releases[0].title, title);
    assert_eq!(selection.releases[0].platforms, ["win", "linux"]);
    assert_eq!(selection.releases[0].released.as_deref(), Some("2026-10-31"));
    assert_eq!(selection.releases[1].title, title);
    assert_eq!(selection.releases[1].platforms, ["mac"]);
    assert_eq!(selection.releases[1].released.as_deref(), Some("2025-01-01"));
    let pinned = app.pin_release("r11").unwrap().selection;
    assert_eq!(pinned.pinned_release, "r11");
    assert_eq!(app.library.game_release("first").unwrap().unwrap().title, title);
    assert_eq!(app.pin_release("r12").unwrap_err(), "SHINKI_EMU_VNDB_RELEASE_INVALID");
    assert_eq!(app.model().unwrap().selection.pinned_release, "r11");
}

#[test]
fn release_selection_keeps_unknown_metadata_empty_and_allows_unpin() {
    let root = tempfile::tempdir().unwrap();
    let mut app = controller(root.path());
    add_game(&mut app, "first");
    releases(&mut app, "first", "[]");
    assert!(app.model().unwrap().selection.releases.is_empty());
    releases(&mut app, "first",
        r#"[{"release_id":"r10","title":null,"released":null,"platforms":[]}]"#);
    let selection = app.pin_release("r10").unwrap().selection;
    assert_eq!(selection.releases[0].release_id, "r10");
    assert!(selection.releases[0].title.is_empty());
    assert!(selection.releases[0].platforms.is_empty());
    assert_eq!(selection.releases[0].released, None);
    assert!(app.pin_release("").unwrap().selection.pinned_release.is_empty());
    assert_eq!(app.library.game_release("first").unwrap(), None);
}

#[test]
fn pinned_release_keeps_cached_details_when_refresh_omits_it() {
    let root = tempfile::tempdir().unwrap();
    let mut app = controller(root.path());
    add_game(&mut app, "first");
    releases(&mut app, "first",
        r#"[{"release_id":"r10","title":"初回限定版","released":"2000-09-08","platforms":["win"]}]"#);
    let pinned = app.pin_release("r10").unwrap().selection;
    releases(&mut app, "first", "[]");
    let refreshed = app.model().unwrap().selection;
    assert_eq!(refreshed.pinned_release, "r10");
    assert_eq!(refreshed.releases, pinned.releases);
    releases(&mut app, "first",
        r#"[{"release_id":"r10","title":"初回限定版・更新","released":null,"platforms":[]}]"#);
    let refreshed = app.model().unwrap().selection;
    assert_eq!(refreshed.pinned_release, "r10");
    assert_eq!(refreshed.releases.len(), 1);
    assert_eq!(refreshed.releases[0].title, "初回限定版・更新");
    assert_eq!(refreshed.releases[0].released, None);
    assert!(refreshed.releases[0].platforms.is_empty());
}

#[test]
fn older_persisted_pin_is_selectable_without_inventing_release_details() {
    let root = tempfile::tempdir().unwrap();
    {
        let mut app = controller(root.path());
        add_game(&mut app, "first");
        app.library.set_game_release("first", Some(&GameRelease {
            release_id: "r10".into(),
            title: String::new(),
        })).unwrap();
    }
    let mut app = controller(root.path());
    app.selected_case_id = Some("first".into());
    releases(&mut app, "first", "[]");
    let selection = app.model().unwrap().selection;
    assert_eq!(selection.pinned_release, "r10");
    assert_eq!(selection.releases.len(), 1);
    assert_eq!(selection.releases[0].release_id, "r10");
    assert!(selection.releases[0].title.is_empty());
    assert!(selection.releases[0].platforms.is_empty());
    assert_eq!(selection.releases[0].released, None);
    assert_eq!(app.pin_release("r10").unwrap().selection.releases, selection.releases);
    assert!(app.pin_release("").unwrap().selection.releases.is_empty());
}

#[test]
fn release_cache_and_pins_are_isolated_between_installed_copies() {
    let root = tempfile::tempdir().unwrap();
    let mut app = controller(root.path());
    for id in ["retail", "download"] {
        add_game(&mut app, id);
        app.apply_metadata_payload(
            Some(id), MetadataProviderId::Vndb,
            MetadataPayload::Fetch { record: Box::new(record()), cover: None },
        ).unwrap();
    }
    releases(&mut app, "retail",
        r#"[{"release_id":"r10","title":"Retail","released":"2000-09-08","platforms":["win"]}]"#);
    releases(&mut app, "download",
        r#"[{"release_id":"r11","title":"Download","released":null,"platforms":[]}]"#);
    app.selected_case_id = Some("retail".into());
    let retail = app.pin_release("r10").unwrap().selection;
    app.selected_case_id = Some("download".into());
    assert_eq!(app.pin_release("r10").unwrap_err(), "SHINKI_EMU_VNDB_RELEASE_INVALID");
    assert!(app.model().unwrap().selection.pinned_release.is_empty());
    let download = app.pin_release("r11").unwrap().selection;
    releases(&mut app, "retail", "[]");
    assert_eq!(app.model().unwrap().selection.releases, download.releases);
    app.pin_release("").unwrap();
    app.selected_case_id = Some("retail".into());
    let selection = app.model().unwrap().selection;
    assert_eq!(selection.pinned_release, "r10");
    assert_eq!(selection.releases, retail.releases);
    drop(app);
    let mut app = controller(root.path());
    app.selected_case_id = Some("retail".into());
    let selection = app.model().unwrap().selection;
    assert_eq!(selection.pinned_release, "r10");
    assert_eq!(selection.releases[0].title, "Retail");
    assert_eq!(selection.releases[0].released, None);
    assert!(selection.releases[0].platforms.is_empty());
    app.selected_case_id = Some("download".into());
    assert!(app.model().unwrap().selection.pinned_release.is_empty());
    assert!(app.model().unwrap().selection.releases.is_empty());
}
