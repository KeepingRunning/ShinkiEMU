use super::*;
use shinki_emu_metadata::{IndexedFile, METADATA_INDEX_SCHEMA_VERSION};

const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

pub(crate) fn compatibility_json(entries: &str) -> Vec<u8> {
    format!(
        r#"{{"schema":"shinki.emu.compatibility.v4","generated_at_unix_ms":1,"entries":[{entries}]}}"#
    )
    .into_bytes()
}

pub(crate) fn patch_catalog_json(recipes: &[u8]) -> Vec<u8> {
    let recipes: Vec<serde_json::Value> = serde_json::from_slice(recipes).unwrap();
    let mut release_ids = std::collections::BTreeMap::new();
    for recipe in &recipes {
        for vn_id in recipe["vn_ids"].as_array().unwrap() {
            let next_id = release_ids.len() + 1;
            release_ids
                .entry(vn_id.as_str().unwrap())
                .or_insert(next_id);
        }
    }
    let entries: Vec<_> = recipes.iter().map(|r| serde_json::json!({
        "id": r["id"], "title": {"zh-CN": r["title"], "en": r["title"]},
        "summary": {"zh-CN": "公开测试配方", "en": "Public fixture recipe"},
        "version": "1.0.0", "emulators": r["emulators"],
        "recipe": {"script": r["script"]}, "license": "MIT",
        "source_url": "https://example.org/public-fixture",
        "releases": r["vn_ids"].as_array().unwrap().iter().map(|vn_id| serde_json::json!({
            "release_id": format!("r{}",release_ids[vn_id.as_str().unwrap()]), "vn_id": vn_id, "title":{"zh-CN":"公开版本","en":"Public edition"},
            "edition":"download","game_version":null,
            "fingerprints":[{"path":"match","size":13,"sha256":sha256_hex(b"public marker")}]
        })).collect::<Vec<_>>()
    })).collect();
    serde_json::to_vec(&serde_json::json!({"schema": "shinki.emu.patches.v2", "entries": entries}))
        .unwrap()
}

pub(crate) fn files(compatibility: Vec<u8>, presets: &str) -> Vec<(String, Vec<u8>)> {
    let recipes = br#"[{"id":"musica-eden-cn","title":"eden","script":"musica-eden-cn.luau","vn_ids":["v17"],"emulators":["shinki.emu.musica"]}]"#;
    vec![
        (COMPATIBILITY_FILE.into(), compatibility),
        (
            PRESETS_FILE.into(),
            format!(r#"{{"schema":"shinki.emu.presets.v1","entries":[{presets}]}}"#).into_bytes(),
        ),
        (
            CORES_FILE.into(),
            br#"{"schema":"shinki.emu.cores.v1","entries":[]}"#.to_vec(),
        ),
        (PATCHES_FILE.into(), patch_catalog_json(recipes)),
        ("patches/catalog.json".into(), recipes.to_vec()),
        ("patches/musica-eden-cn.luau".into(), b"return {}".to_vec()),
    ]
}

pub(crate) fn index(generated_at_unix_ms: i64, files: &[(String, Vec<u8>)]) -> MetadataIndex {
    MetadataIndex {
        schema: METADATA_INDEX_SCHEMA_VERSION.into(),
        generated_at_unix_ms,
        source_commit: COMMIT.into(),
        files: files
            .iter()
            .map(|(path, bytes)| IndexedFile {
                path: path.clone(),
                sha256: sha256_hex(bytes),
                size: bytes.len() as u64,
            })
            .collect(),
    }
}

/// Write a complete metadata tree as `build_index.py` would publish it.
pub(crate) fn write_tree(root: &Path, generated_at_unix_ms: i64, files: &[(String, Vec<u8>)]) {
    for (path, bytes) in files {
        let destination = indexed_path(root, path);
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        fs::write(destination, bytes).unwrap();
    }
    fs::write(
        root.join(METADATA_INDEX_FILE),
        serde_json::to_vec(&index(generated_at_unix_ms, files)).unwrap(),
    )
    .unwrap();
}

#[test]
fn indexed_v2_exposes_external_snapshots_while_legacy_catalog_stays_readable() {
    let data = tempfile::tempdir().unwrap();
    let bundled = tempfile::tempdir().unwrap();
    let mut tree = files(compatibility_json(""), "");
    let identity = shinki_emu_metadata::CoreIdentity {
        upstream: shinki_emu_metadata::UpstreamIdentity {
            repo: "https://github.com/example/engine".into(),
            ref_kind: shinki_emu_metadata::UpstreamRefKind::Branch,
            ref_name: "release/next".into(),
            commit: "a".repeat(40),
        },
        adapter: shinki_emu_metadata::AdapterIdentity {
            repo: "https://github.com/Project-Shinkirou/engine".into(),
            branch: "shinki".into(),
            commit: "b".repeat(40),
        },
        release_sequence: 1,
    };
    let version = identity.version();
    let tag = format!("core-engine-{version}");
    let entry = shinki_emu_metadata::CoreCatalogEntry {
        plugin_id: "engine".into(),
        display_name: "Engine".into(),
        family_name: "Engine".into(),
        version,
        abi: "shinki.emu.native.c.v3".into(),
        platform: "windows-x86_64".into(),
        url: format!(
            "{}{tag}/engine.zip",
            shinki_emu_metadata::CORE_RELEASE_URL_PREFIX
        ),
        sha256: "c".repeat(64),
        size: 1,
        license: "MIT".into(),
        release_tag: tag,
        identity: Some(identity),
    };
    let modern = shinki_emu_metadata::CoreCatalog {
        schema: shinki_emu_metadata::CORE_CATALOG_SCHEMA_VERSION.into(),
        entries: vec![entry.clone()],
    };
    tree.push((CORES_V2_FILE.into(), serde_json::to_vec(&modern).unwrap()));
    let report = serde_json::json!({
        "schema": shinki_emu_metadata::COMPATIBILITY_SCHEMA_VERSION,
        "generated_at_unix_ms":20,
        "entries":[{"vn_id":"v1","release_id":"r1","emulator":"rfvp","status":"boot_only",
            "emulator_version":entry.version,"platform":"windows","architecture":"x86_64",
            "variant":"desktop","updated_at_unix_ms":20}]
    });
    tree.push((
        COMPATIBILITY_V5_FILE.into(),
        serde_json::to_vec(&report).unwrap(),
    ));
    write_tree(bundled.path(), 20, &tree);
    let selected = select_catalog(data.path(), Some(bundled.path())).unwrap();
    assert_eq!(
        selected
            .cores
            .latest("engine", "windows-x86_64", "shinki.emu.native.c.v3"),
        Some(&entry)
    );
    assert_eq!(
        selected.compatibility.entries[0]
            .emulator_version
            .as_deref(),
        Some(entry.version.as_str())
    );
    let legacy = tree.iter().find(|(path, _)| path == CORES_FILE).unwrap();
    assert_eq!(
        parse_core_catalog(&legacy.1).unwrap().schema,
        shinki_emu_metadata::LEGACY_CORE_CATALOG_SCHEMA_VERSION
    );
    tree.retain(|(path, _)| path != CORES_V2_FILE && path != COMPATIBILITY_V5_FILE);
    write_tree(bundled.path(), 21, &tree);
    assert!(
        select_catalog(data.path(), Some(bundled.path()))
            .unwrap()
            .cores
            .entries
            .is_empty()
    );
}

#[test]
fn newer_bundled_snapshot_wins_and_equal_age_prefers_synchronized_cache() {
    let data = tempfile::tempdir().unwrap();
    let bundled = tempfile::tempdir().unwrap();
    let tree = files(compatibility_json(""), "");
    assert!(select_catalog(data.path(), Some(bundled.path())).is_none());
    write_tree(bundled.path(), 20, &tree);
    write_tree(&data.path().join(METADATA_DIRECTORY), 10, &tree);
    let selected = select_catalog(data.path(), Some(bundled.path())).unwrap();
    assert_eq!(selected.source, MetadataSource::Bundled);
    assert_eq!(selected.patches.entries[0].id, "musica-eden-cn");
    write_tree(&data.path().join(METADATA_DIRECTORY), 20, &tree);
    assert_eq!(
        select_catalog(data.path(), Some(bundled.path()))
            .unwrap()
            .source,
        MetadataSource::Synchronized
    );
}

#[test]
fn damaged_synchronized_cache_falls_back_to_bundled_snapshot() {
    let data = tempfile::tempdir().unwrap();
    let bundled = tempfile::tempdir().unwrap();
    let tree = files(compatibility_json(""), "");
    write_tree(bundled.path(), 1, &tree);
    let cache = data.path().join(METADATA_DIRECTORY);
    write_tree(&cache, 50, &tree);
    fs::write(cache.join(PRESETS_FILE), b"{}").unwrap();
    assert_eq!(
        select_catalog(data.path(), Some(bundled.path()))
            .unwrap()
            .source,
        MetadataSource::Bundled
    );
}

#[test]
fn store_replaces_cache_and_reuses_unchanged_files() {
    let data = tempfile::tempdir().unwrap();
    let first = files(compatibility_json(""), "");
    let stored = store_synchronized(data.path(), None, &index(1, &first), first.clone()).unwrap();
    assert_eq!(stored.root, data.path().join(METADATA_DIRECTORY));
    let entry = r#"{"vn_id":"v1","release_id":"r1","emulator":"shinki.emu.musica","status":"perfect","platform":"windows","architecture":"x86_64","variant":"desktop","updated_at_unix_ms":1}"#;
    let second = files(compatibility_json(entry), "");
    let changed = vec![second[0].clone()];
    let updated =
        store_synchronized(data.path(), Some(&stored), &index(2, &second), changed).unwrap();
    assert_eq!(updated.compatibility.entries.len(), 1);
    assert_eq!(updated.patches.entries.len(), 1);
    assert!(!data.path().join(STAGING_DIRECTORY).exists());
    assert!(!data.path().join(PREVIOUS_DIRECTORY).exists());
}

#[test]
fn tampered_or_unparsable_update_keeps_the_previous_cache() {
    let data = tempfile::tempdir().unwrap();
    let first = files(compatibility_json(""), "");
    let stored = store_synchronized(data.path(), None, &index(1, &first), first.clone()).unwrap();
    let entry = r#"{"vn_id":"v1","release_id":"r1","emulator":"shinki.emu.musica","status":"perfect","platform":"windows","architecture":"x86_64","variant":"desktop","updated_at_unix_ms":1}"#;
    let second = files(compatibility_json(entry), "");
    let mut tampered = second.clone();
    tampered[0].1.push(b' ');
    let error =
        store_synchronized(data.path(), Some(&stored), &index(2, &second), tampered).unwrap_err();
    assert!(
        error.starts_with("SHINKI_EMU_METADATA_HASH_MISMATCH"),
        "{error}"
    );

    let mut unparsable = second;
    unparsable[1].1 = b"{\"schema\":\"shinki.emu.presets.v0\",\"entries\":[]}".to_vec();
    let error = store_synchronized(
        data.path(),
        Some(&stored),
        &index(2, &unparsable),
        unparsable.clone(),
    )
    .unwrap_err();
    assert!(error.contains("SCHEMA_MISMATCH"), "{error}");

    let kept = select_catalog(data.path(), None).unwrap();
    assert_eq!(kept.index.generated_at_unix_ms, 1);
    assert!(kept.compatibility.entries.is_empty());
    assert!(!data.path().join(STAGING_DIRECTORY).exists());
}

/// A valid catalog written under `root/sample` for controller tests.
pub(crate) fn sample_catalog(root: &Path) -> MetadataCatalog {
    let directory = root.join("sample-metadata");
    write_tree(&directory, 1, &files(compatibility_json(""), ""));
    MetadataCatalog::load(&directory, MetadataSource::Bundled).unwrap()
}

#[test]
fn a_non_recipe_cannot_enter_the_execution_catalog() {
    let root = tempfile::tempdir().unwrap();
    let mut tree = files(compatibility_json(""), "");
    let (_, bytes) = tree.iter_mut().find(|(p, _)| p == PATCHES_FILE).unwrap();
    let mut patches: serde_json::Value = serde_json::from_slice(bytes).unwrap();
    patches["entries"][0]["kind"] = "external".into();
    patches["entries"][0]
        .as_object_mut()
        .unwrap()
        .remove("recipe");
    *bytes = serde_json::to_vec(&patches).unwrap();
    // All hashes match: this rejects an invalid executable projection, not file corruption.
    write_tree(root.path(), 1, &tree);
    assert!(MetadataCatalog::load(root.path(), MetadataSource::Bundled).is_err());
}
