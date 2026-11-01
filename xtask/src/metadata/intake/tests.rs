use super::*;

const SCRIPT: &str = r#"return {
    id = "public-test", version = 1, title = "Public test",
    apply = function(ctx)
        ctx.copy("source", "game.dat", "result.dat")
        return { plugin_id = "ReMinori", configuration = {} }
    end,
}"#;

fn submission() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("Patch id", "public-test".into()),
        ("Title (zh-CN)", "公开测试".into()),
        ("Title (en)", "Public test".into()),
        ("Version", "1.0.0".into()),
        ("Cores", "ReMinori".into()),
        ("License", "MIT".into()),
        ("Source URL", "https://example.org/project".into()),
        ("Summary (zh-CN)", "原生文件转换".into()),
        ("Summary (en)", "Native file conversion".into()),
        ("Release bindings (JSON)", json!([{
            "release_id":"r2", "vn_id":"v1", "title":{"zh-CN":"盒装版","en":"Boxed"},
            "edition":"boxed", "game_version":null,
            "fingerprints":[{"path":"game.dat","size":1,"sha256":digest(b"x")}]
        }]).to_string()),
        ("Downloads (JSON)", "[]".into()),
        ("Luau script", format!("```luau\n{SCRIPT}\n```")),
    ]).into_iter().map(|(key, value)| (key.to_owned(), value)).collect()
}

fn event(directory: &Path, fields: &BTreeMap<String, String>) -> PathBuf {
    let path = directory.join("issue.json");
    let body: String = fields.iter().map(|(key, value)| format!("### {key}\r\n\r\n{value}\r\n\r\n")).collect();
    fs::write(&path, pretty(&json!({"issue":{
        "number":2, "body":body, "updated_at":"2026-10-24T10:00:00+08:00"
    }})).unwrap()).unwrap();
    path
}

fn reject(fields: &BTreeMap<String, String>, message: &str) {
    let temp = tempfile::tempdir().unwrap();
    let event = event(temp.path(), fields);
    let out = temp.path().join("review");
    let error = intake(IntakeKind::Patch, &event, &out).unwrap_err();
    let error = format!("{error:#}");
    assert!(error.contains(message), "expected {message:?}, got {error}");
    assert!(!out.exists(), "rejected submission published a partial tree");
}

#[test]
fn valid_luau_submission_generates_entry_script_and_approval_tree() {
    let temp = tempfile::tempdir().unwrap();
    let mut fields = submission();
    let download = json!({"url":"https://example.org/data","sha256":digest(b"x"),"size":1});
    fields.insert("Downloads (JSON)".into(), json!([download]).to_string());
    let event = event(temp.path(), &fields);
    let out = temp.path().join("review");
    intake(IntakeKind::Patch, &event, &out).unwrap();
    let entry: PatchCatalogEntry = decode(&out.join("Metadata/patches/entries/public-test.json")).unwrap();
    entry.validate().unwrap();
    assert_eq!(entry.releases[0].release_id, "r2");
    assert_eq!(entry.releases[0].vn_id, "v1");
    assert_eq!(entry.license, "MIT");
    assert_eq!(entry.source_url.as_deref(), Some("https://example.org/project"));
    assert_eq!(entry.recipe.script, "public-test.luau");
    assert_eq!(serde_json::to_value(&entry.downloads[0]).unwrap(), download);
    assert_eq!(read(&out.join("Metadata/patches/public-test.luau")).unwrap(), SCRIPT.as_bytes());
    let approval: Value = decode(&out.join("approval.json")).unwrap();
    let event: Value = decode(&event).unwrap();
    assert_eq!(approval["body_sha256"], digest(event["issue"]["body"].as_str().unwrap().as_bytes()));
    assert_eq!(approval["paths"], json!(["patches/entries/public-test.json", "patches/public-test.luau"]));
    assert_eq!(approval["path"], "patches/entries/public-test.json");
}

#[test]
fn missing_required_fields_publish_nothing() {
    for key in ["License", "Source URL", "Luau script", "Release bindings (JSON)", "Downloads (JSON)"] {
        let mut fields = submission();
        fields.remove(key);
        reject(&fields, &format!("missing form field: {key}"));
        fields.insert(key.into(), String::new());
        reject(&fields, &format!("missing form field: {key}"));
    }
}

#[test]
fn release_id_and_fingerprints_are_validated_before_publication() {
    for (field, replacement, expected) in [
        ("release_id", Value::String("v2".into()), "invalid VNDB rID"),
        ("fingerprints", json!([]), "invalid release binding"),
    ] {
        let mut fields = submission();
        let mut releases: Value = serde_json::from_str(&fields["Release bindings (JSON)"]).unwrap();
        releases[0][field] = replacement;
        fields.insert("Release bindings (JSON)".into(), releases.to_string());
        reject(&fields, expected);
    }
}

#[test]
fn downloads_require_https_sha256_and_size() {
    for (declaration, expected) in [
        (json!([{"url":"http://example.org/data","sha256":digest(b"x"),"size":1}]), "URL must be HTTPS"),
        (json!([{"url":"https://example.org/data","sha256":"broken","size":1}]), "sha256 must contain"),
        (json!([{"url":"https://example.org/data","size":1}]), "invalid form field: Downloads"),
        (json!([{"url":"https://example.org/data","sha256":digest(b"x")}]), "invalid form field: Downloads"),
        (json!([{"url":"https://example.org/data","sha256":digest(b"x"),"size":-1}]), "invalid form field: Downloads"),
    ] {
        let mut fields = submission();
        fields.insert("Downloads (JSON)".into(), declaration.to_string());
        reject(&fields, expected);
    }
}

#[test]
fn malformed_luau_is_rejected_without_publishing_files() {
    let mut fields = submission();
    fields.insert("Luau script".into(), "return { apply = function(".into());
    reject(&fields, "Luau syntax check failed");
}

#[test]
fn syntax_check_never_executes_submission() {
    let mut fields = submission();
    fields.insert("Luau script".into(), "error('intake must compile without executing this script')".into());
    let temp = tempfile::tempdir().unwrap();
    let event = event(temp.path(), &fields);
    intake(IntakeKind::Patch, &event, &temp.path().join("accepted")).unwrap();
}

#[test]
fn duplicate_form_sections_and_json_keys_are_rejected() {
    assert!(form("### License\nMIT\n### License\nApache-2.0").is_err());
    let mut fields = submission();
    fields.insert("Downloads (JSON)".into(), "[{\"url\":\"https://example.org/one\",\"url\":\"https://example.org/two\",\"sha256\":\"a\",\"size\":1}]".into());
    reject(&fields, "invalid JSON in form field: Downloads");
}

#[test]
fn form_preserves_headings_inside_script_fences() {
    let body = "### Luau script\n```luau\nlocal s = [[\n### License\n]]\nreturn {}\n```\n### License\nMIT";
    let fields = form(body).unwrap();
    assert_eq!(fields["License"], "MIT");
    assert!(fields["Luau script"].contains("### License"));
}

#[test]
fn unsafe_patch_identifier_never_becomes_an_output_path() {
    let mut fields = submission();
    fields.insert("Patch id".into(), "../escape".into());
    reject(&fields, "invalid Patch id");
}
