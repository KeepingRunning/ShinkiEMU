use serde_json::{Value, json};
use shinki_emu_patch::{PatchErrorKind, list_recipes};
use std::fs;

fn listing(entries: Value) -> Result<Vec<shinki_emu_metadata::PatchCatalogEntry>, PatchErrorKind> {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("patches-v2.json"),
        serde_json::to_vec(&json!({"schema":"shinki.emu.patches.v2","entries":entries})).unwrap(),
    )
    .unwrap();
    list_recipes(root.path()).map_err(|error| error.kind())
}
fn recipe() -> Value {
    json!({"id":"public-recipe","title":{"zh-CN":"公开配方","en":"Public recipe"},"version":"1.0.0",
        "emulators":["shinki.emu.musica"],"source_url":null,"license":"MIT","authorization_url":null,
        "summary":{"zh-CN":"公开配方","en":"Public recipe"},"issue_url":null,"recipe":{"script":"public.luau"},
        "releases":[{"release_id":"r17","vn_id":"v17","title":{"zh-CN":"公开版本","en":"Public edition"},
            "edition":"download","game_version":null,"fingerprints":[{"path":"game.dat","size":1,"sha256":"a".repeat(64)}]}]})
}
#[test]
fn catalog_rejects_missing_or_malformed_targets() {
    for (field, value) in [
        ("releases", json!([])),
        ("releases", json!("r17")),
        ("emulators", json!([])),
        ("emulators", json!([" "])),
        ("emulators", json!(["rfvp", "rfvp"])),
        ("emulators", json!(["reminori"])),
        ("emulators", json!(["unknown-core"])),
    ] {
        let mut value_record = recipe();
        value_record[field] = value;
        assert_eq!(
            listing(json!([value_record])).unwrap_err(),
            PatchErrorKind::InvalidRequest
        );
    }
    for field in ["releases", "emulators"] {
        let mut record = recipe();
        record.as_object_mut().unwrap().remove(field);
        assert_eq!(
            listing(json!([record])).unwrap_err(),
            PatchErrorKind::InvalidRequest
        );
    }
    let mut record = recipe();
    record["releases"][0]["vn_id"] = json!("17");
    assert_eq!(
        listing(json!([record])).unwrap_err(),
        PatchErrorKind::InvalidRequest
    );
    let mut record = recipe();
    record["releases"][0]["fingerprints"] = json!([]);
    assert_eq!(
        listing(json!([record])).unwrap_err(),
        PatchErrorKind::InvalidRequest
    );
    assert_eq!(
        listing(json!([recipe()])).unwrap()[0].releases[0].release_id,
        "r17"
    );
}
