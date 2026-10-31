use sha2::{Digest, Sha256};
use shinki_emu_patch::{PatchErrorKind, PatchRequest, apply};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

struct Fixture {
    root: tempfile::TempDir,
    source: PathBuf,
    patch: PathBuf,
    script: PathBuf,
    output: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("原始输入");
        let patch = root.path().join("补丁输入");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&patch).unwrap();
        fs::write(source.join("assets.bin"), b"alpha|beta").unwrap();
        fs::write(source.join("untouched.dat"), b"unchanged public bytes").unwrap();
        fs::write(patch.join("overlay.dll"), b"PE_PUBLIC_OMEGA").unwrap();
        let script = root.path().join("recipe.luau");
        let output = root.path().join("独立输出");
        Self {
            root,
            source,
            patch,
            script,
            output,
        }
    }

    fn recipe(&self, body: &str) {
        fs::write(
            &self.script,
            format!(
                r#"return {{
            id = "public-fixture", version = 1, title = "公开转换夹具",
            apply = function(ctx)
                do
                {body}
                end
                return {{plugin_id="public.fixture", configuration={{script_encoding="utf8"}}}}
            end,
        }}"#
            ),
        )
        .unwrap();
    }

    fn request(&self) -> PatchRequest {
        PatchRequest {
            script: self.script.clone(),
            source: self.source.clone(),
            patch: self.patch.clone(),
            mode: shinki_emu_patch::PatchMode::NewDirectory {
                output: self.output.clone(),
            },
        }
    }

    fn unchanged(&self) {
        assert_eq!(
            fs::read(self.source.join("assets.bin")).unwrap(),
            b"alpha|beta"
        );
        assert_eq!(
            fs::read(self.source.join("untouched.dat")).unwrap(),
            b"unchanged public bytes"
        );
        assert_eq!(
            fs::read(self.patch.join("overlay.dll")).unwrap(),
            b"PE_PUBLIC_OMEGA"
        );
    }

    fn no_staging(&self) {
        assert!(!fs::read_dir(self.root.path()).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".shinki-patch-staging-")
        }));
    }
}

fn hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

#[test]
fn independent_output_receipt_and_module_cache_preserve_every_input() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.root.path().join("common")).unwrap();
    fs::write(
        fixture.root.path().join("common/fixture.luau"),
        "module_hits=(module_hits or 0)+1; return {serial=module_hits}",
    )
    .unwrap();
    fixture.recipe(&format!(
        r#"
        local first = ctx.load("common/fixture.luau")
        local again = ctx.load("common/fixture.luau")
        assert(first == again and first.serial == 1)
        local source = ctx.open("source", "assets.bin")
        local patch = ctx.open("patch", "overlay.dll")
        assert(source:sha256() == "{}")
        assert(patch:sha256() == "{}")
        ctx.copy("source", "assets.bin", "assets.bin")
        ctx.copy("source", "untouched.dat", "untouched.dat")
        local output = ctx.edit("assets.bin")
        output:write(6, patch:read(10, 5))
        output:truncate(11)
        output:close()
        ctx.mkdir("目录")
        ctx.create("目录/零字节"):close()
        retained_output = ctx.create("auto-closed.bin")
        retained_output:write(0, buffer.fromstring("owned"))
        ctx.progress("fixture complete", 4, 4)
    "#,
        hash(b"alpha|beta"),
        hash(b"PE_PUBLIC_OMEGA")
    ));
    let result = apply(fixture.request(), Arc::new(AtomicBool::new(false)), |_| {}).unwrap();
    assert_eq!(result.profile.plugin_id, "public.fixture");
    assert_eq!(result.profile.configuration["script_encoding"], "utf8");
    assert_eq!(
        fs::canonicalize(&result.output).unwrap(),
        fs::canonicalize(&fixture.output).unwrap()
    );
    assert_eq!(
        fs::read(result.output.join("assets.bin")).unwrap(),
        b"alpha|OMEGA"
    );
    assert_eq!(
        fs::read(result.output.join("untouched.dat")).unwrap(),
        b"unchanged public bytes"
    );
    assert_eq!(
        fs::read(result.output.join("auto-closed.bin")).unwrap(),
        b"owned"
    );
    assert_eq!(
        fs::metadata(result.output.join("目录/零字节"))
            .unwrap()
            .len(),
        0
    );
    let receipt: serde_json::Value =
        serde_json::from_slice(&fs::read(result.output.join("shinki-patch-result.json")).unwrap())
            .unwrap();
    assert_eq!(receipt["schema"], "shinki.emu.file_patch.v1");
    assert_eq!(receipt["recipe"]["id"], "public-fixture");
    let scripts = receipt["scripts"].as_array().unwrap();
    assert_eq!(scripts.len(), 2);
    for script in scripts {
        let path = script["path"].as_str().unwrap();
        assert!(Path::new(path).is_relative());
        assert_eq!(
            script["sha256"],
            hash(&fs::read(fixture.root.path().join(path)).unwrap())
        );
    }
    let inputs = receipt["inputs"].as_array().unwrap();
    assert_eq!(inputs.len(), 3);
    for input in inputs {
        let path = input["path"].as_str().unwrap();
        assert!(Path::new(path).is_relative());
        let root = match input["root"].as_str().unwrap() {
            "source" => &fixture.source,
            "patch" => &fixture.patch,
            _ => panic!("invalid root"),
        };
        assert_eq!(input["sha256"], hash(&fs::read(root.join(path)).unwrap()));
    }
    let outputs = receipt["outputs"].as_array().unwrap();
    assert_eq!(outputs.len(), 4);
    for output in outputs {
        let path = output["path"].as_str().unwrap();
        assert_ne!(path, "shinki-patch-result.json");
        let bytes = fs::read(result.output.join(path)).unwrap();
        assert_eq!(output["sha256"], hash(&bytes));
        assert_eq!(output["size"], bytes.len() as u64);
    }
    fixture.unchanged();
    fixture.no_staging();
    fs::remove_dir_all(&result.output).unwrap(); // all VM/output handles were released
}

#[test]
fn lua_fingerprint_mismatch_preserves_changed_input_and_leaves_no_output() {
    let fixture = Fixture::new();
    fixture.recipe(&format!(
        r#"
        if ctx.open("source", "assets.bin"):sha256() ~= "{}" then
            ctx.fail("UNSUPPORTED_INPUT", "public fixture fingerprint mismatch")
        end
        ctx.copy("source", "assets.bin", "assets.bin")
    "#,
        hash(b"alpha|beta")
    ));
    fs::write(fixture.source.join("assets.bin"), b"Alpha|beta").unwrap();
    let error = apply(fixture.request(), Arc::new(AtomicBool::new(false)), |_| {})
        .err()
        .unwrap();
    assert_eq!(error.kind(), PatchErrorKind::UnsupportedInput);
    assert!(error.to_string().contains("recipe.luau"));
    assert_eq!(
        fs::read(fixture.source.join("assets.bin")).unwrap(),
        b"Alpha|beta"
    );
    assert!(!fixture.output.exists());
    fixture.no_staging();
}

#[test]
fn existing_output_is_rejected_before_recipe_execution() {
    let fixture = Fixture::new();
    fixture.recipe("error('recipe must not execute')");
    fs::create_dir(&fixture.output).unwrap();
    fs::write(fixture.output.join("user.bin"), b"user data").unwrap();
    let error = apply(fixture.request(), Arc::new(AtomicBool::new(false)), |_| {})
        .err()
        .unwrap();
    assert_eq!(error.kind(), PatchErrorKind::OutputExists);
    assert_eq!(
        fs::read(fixture.output.join("user.bin")).unwrap(),
        b"user data"
    );
    fixture.unchanged();
    fixture.no_staging();
}

#[test]
fn script_and_module_path_escape_roll_back_the_entire_output() {
    for body in [
        r#"ctx.create("written"):close(); ctx.create("../escape")"#,
        r#"ctx.create("written"):close(); ctx.create("C:/escape")"#,
        r#"ctx.create("written"):close(); ctx.load("../outside.luau")"#,
        r#"ctx.create("written"):close(); ctx.open("patch", "overlay.dll:secret")"#,
    ] {
        let fixture = Fixture::new();
        fixture.recipe(body);
        let error = apply(fixture.request(), Arc::new(AtomicBool::new(false)), |_| {})
            .err()
            .unwrap();
        assert_eq!(error.kind(), PatchErrorKind::Path);
        assert!(!fixture.output.exists());
        assert!(!fixture.root.path().join("escape").exists());
        fixture.unchanged();
        fixture.no_staging();
    }
}

#[test]
fn pure_lua_cancellation_joins_worker_and_a_fresh_task_succeeds() {
    let fixture = Fixture::new();
    fixture.recipe(
        r#"
        ctx.mkdir("nested")
        local output = ctx.create("nested/partial")
        output:write(0, buffer.fromstring("partial"))
        ctx.progress("infinite loop", 0, nil)
        while true do end
    "#,
    );
    let request = fixture.request();
    let cancelled = Arc::new(AtomicBool::new(false));
    let flag = cancelled.clone();
    let (started, observed) = std::sync::mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        apply(request, flag, |progress| {
            if progress.phase == "infinite loop" {
                started.send(()).unwrap();
            }
        })
    });
    observed
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    cancelled.store(true, Ordering::Release);
    let error = worker.join().unwrap().err().unwrap();
    assert_eq!(error.kind(), PatchErrorKind::Cancelled);
    assert!(!fixture.output.exists());
    fixture.no_staging();
    fixture.recipe("ctx.copy('source', 'assets.bin', 'assets.bin')");
    let output = apply(fixture.request(), Arc::new(AtomicBool::new(false)), |_| {}).unwrap();
    assert_eq!(
        fs::read(output.output.join("assets.bin")).unwrap(),
        b"alpha|beta"
    );
    fixture.unchanged();
}

#[test]
fn invalid_profile_or_memory_exhaustion_never_publishes_partial_files() {
    for (body, kind) in [
        (
            r#"ctx.create("partial"):close(); return {plugin_id="",configuration={}}"#,
            PatchErrorKind::Script,
        ),
        (
            r#"ctx.create("partial"):close(); buffer.create(257 * 1024 * 1024)"#,
            PatchErrorKind::Memory,
        ),
    ] {
        let fixture = Fixture::new();
        fixture.recipe(body);
        let error = apply(fixture.request(), Arc::new(AtomicBool::new(false)), |_| {})
            .err()
            .unwrap();
        assert_eq!(error.kind(), kind);
        assert!(!fixture.output.exists());
        fixture.unchanged();
        fixture.no_staging();
    }
}

#[test]
fn tiny_fractions_are_not_accepted_as_zero_offsets_lengths_or_counts() {
    for body in [
        r#"ctx.open("source","assets.bin"):read(1e-200,1)"#,
        r#"ctx.open("source","assets.bin"):read(0,1e-200)"#,
        r#"ctx.create("partial"):truncate(-1e-200)"#,
        r#"ctx.progress("invalid count",1e-200,nil)"#,
        r#"ctx.codec.inflate(buffer.fromstring(""),1e-200)"#,
        r#"ctx.crypto.rc4(buffer.fromstring(""),buffer.fromstring("key"),1e-200)"#,
        r#"ctx.image.png(1e-200,1,buffer.fromstring(""))"#,
        r#"ctx.open("source","assets.bin"):read("0",1)"#,
    ] {
        let fixture = Fixture::new();
        fixture.recipe(body);
        let error = apply(fixture.request(), Arc::new(AtomicBool::new(false)), |_| {})
            .err()
            .unwrap();
        assert_eq!(error.kind(), PatchErrorKind::Bounds);
        assert!(!fixture.output.exists());
        fixture.unchanged();
        fixture.no_staging();
    }
}

#[test]
fn in_place_apply_and_restore_round_trip_changed_added_and_removed_files() {
    let fixture = Fixture::new();
    fixture.recipe(
        r#"
        local changed = ctx.create("assets.bin")
        changed:write(0, buffer.fromstring("patched bytes")); changed:close()
        ctx.mkdir("new")
        local added = ctx.create("new/file.bin")
        added:write(0, buffer.fromstring("added bytes")); added:close()
    "#,
    );
    let mut request = fixture.request();
    request.mode = shinki_emu_patch::PatchMode::InPlace;
    let result = apply(request, Arc::new(AtomicBool::new(false)), |_| {}).unwrap();
    assert_eq!(result.output, fs::canonicalize(&fixture.source).unwrap());
    assert_eq!(
        fs::read(fixture.source.join("assets.bin")).unwrap(),
        b"patched bytes"
    );
    assert_eq!(
        fs::read(fixture.source.join("new/file.bin")).unwrap(),
        b"added bytes"
    );
    assert!(!fixture.source.join("untouched.dat").exists());
    let state = fixture.source.join(".shinki");
    let backups = fs::read_dir(state.join("backup"))
        .unwrap()
        .collect::<Vec<_>>();
    assert_eq!(backups.len(), 1);
    let backup = backups[0].as_ref().unwrap().path();
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(backup.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["recipe_id"], "public-fixture");
    assert_eq!(manifest["entries"].as_array().unwrap().len(), 4);
    assert_eq!(fs::read(backup.join("assets.bin")).unwrap(), b"alpha|beta");
    assert_eq!(
        fs::read(backup.join("untouched.dat")).unwrap(),
        b"unchanged public bytes"
    );
    assert!(state.join("patch-result.json").is_file());
    assert!(shinki_emu_patch::has_backup(&fixture.source).unwrap());
    shinki_emu_patch::restore_latest(&fixture.source, Arc::new(AtomicBool::new(false)), |_| {})
        .unwrap();
    assert_eq!(
        fs::read(fixture.source.join("assets.bin")).unwrap(),
        b"alpha|beta"
    );
    assert_eq!(
        fs::read(fixture.source.join("untouched.dat")).unwrap(),
        b"unchanged public bytes"
    );
    assert!(!fixture.source.join("new/file.bin").exists());
    assert!(!state.join("patch-result.json").exists());
    assert!(!shinki_emu_patch::has_backup(&fixture.source).unwrap());
    assert!(fs::read_dir(state.join("staging")).map_or(true, |entries| entries.count() == 0));
}

#[test]
fn in_place_cancel_or_recipe_failure_before_commit_preserves_game() {
    let fixture = Fixture::new();
    fixture.recipe(r#"ctx.create("assets.bin"):close(); ctx.fail("UNSUPPORTED_INPUT", "abort")"#);
    let mut request = fixture.request();
    request.mode = shinki_emu_patch::PatchMode::InPlace;
    assert_eq!(
        apply(request, Arc::new(AtomicBool::new(false)), |_| {})
            .err()
            .unwrap()
            .kind(),
        PatchErrorKind::UnsupportedInput
    );
    fixture.unchanged();
    assert!(!shinki_emu_patch::has_backup(&fixture.source).unwrap());
    fixture.recipe(r#"ctx.create("assets.bin"):close()"#);
    let cancelled = Arc::new(AtomicBool::new(false));
    let signal = cancelled.clone();
    let mut request = fixture.request();
    request.mode = shinki_emu_patch::PatchMode::InPlace;
    assert_eq!(
        apply(request, cancelled, move |progress| {
            if progress.phase == "verifying output" {
                signal.store(true, Ordering::Release);
            }
        })
        .err()
        .unwrap()
        .kind(),
        PatchErrorKind::Cancelled
    );
    fixture.unchanged();
    assert!(!shinki_emu_patch::has_backup(&fixture.source).unwrap());
}

#[test]
fn restore_refuses_modified_patched_file_and_retains_backup() {
    let fixture = Fixture::new();
    fixture.recipe(r#"ctx.create("assets.bin"):close()"#);
    let mut request = fixture.request();
    request.mode = shinki_emu_patch::PatchMode::InPlace;
    apply(request, Arc::new(AtomicBool::new(false)), |_| {}).unwrap();
    fs::write(fixture.source.join("assets.bin"), b"edited by user").unwrap();
    let error =
        shinki_emu_patch::restore_latest(&fixture.source, Arc::new(AtomicBool::new(false)), |_| {})
            .unwrap_err();
    assert_eq!(error.kind(), PatchErrorKind::Changed);
    assert_eq!(
        fs::read(fixture.source.join("assets.bin")).unwrap(),
        b"edited by user"
    );
    assert!(shinki_emu_patch::has_backup(&fixture.source).unwrap());
}

#[test]
fn recipe_cannot_write_game_state_namespace() {
    for body in [
        r#"ctx.create(".shinki/patch/hijack"):close()"#,
        r#"ctx.mkdir(".shinki/backup")"#,
        r#"ctx.create(".SHINKI"):close()"#,
    ] {
        let fixture = Fixture::new();
        fixture.recipe(body);
        let mut request = fixture.request();
        request.mode = shinki_emu_patch::PatchMode::InPlace;
        assert_eq!(
            apply(request, Arc::new(AtomicBool::new(false)), |_| {})
                .err()
                .unwrap()
                .kind(),
            PatchErrorKind::Path
        );
        fixture.unchanged();
    }
}

#[test]
fn native_capability_requirements_survive_real_conversion_and_receipt_loading() {
    for required in [0, shinki_emu_abi::CAP_NATIVE_SCRIPT_IDENTITY
        | shinki_emu_abi::CAP_CONFIGURED_FONT_PRIORITY] {
        let fixture = Fixture::new();
        fixture.recipe(&format!(
            r#"ctx.copy("source","assets.bin","assets.bin"); return {{
                plugin_id="public.fixture", configuration={{script_encoding="utf8"}},
                required_capabilities={required}
            }}"#
        ));
        let result = apply(fixture.request(), Arc::new(AtomicBool::new(false)), |_| {}).unwrap();
        assert_eq!(fs::read(result.output.join("assets.bin")).unwrap(), b"alpha|beta");
        let loaded = shinki_emu_patch::read_output_profile(
            &result.output,
            &AtomicBool::new(false),
        ).unwrap().unwrap();
        assert_eq!(loaded.required_capabilities, required);
        let receipt_path = result.output.join("shinki-patch-result.json");
        let mut receipt: serde_json::Value =
            serde_json::from_slice(&fs::read(&receipt_path).unwrap()).unwrap();
        if required == 0 {
            #[derive(serde::Deserialize)]
            #[serde(deny_unknown_fields)]
            struct LegacyProfile {
                plugin_id: String,
                configuration: std::collections::BTreeMap<String, String>,
            }
            let legacy: LegacyProfile = serde_json::from_value(receipt["profile"].clone()).unwrap();
            assert_eq!(legacy.plugin_id, loaded.plugin_id);
            assert_eq!(legacy.configuration, loaded.configuration);
        }
        receipt["profile"]["required_capabilities"] = serde_json::json!(required | 2);
        fs::write(receipt_path, serde_json::to_vec(&receipt).unwrap()).unwrap();
        assert!(shinki_emu_patch::read_output_profile(
            &result.output,
            &AtomicBool::new(false),
        ).is_err());
        assert_eq!(fs::read(result.output.join("assets.bin")).unwrap(), b"alpha|beta");
        fixture.unchanged();
    }
}

#[test]
fn unsupported_or_invalid_required_capabilities_never_publish_partial_output() {
    for required in ["2", "194", "-1", "192.5", "1e-200", "\"192\""] {
        let fixture = Fixture::new();
        fixture.recipe(&format!(
            r#"ctx.copy("source","assets.bin","partial"); return {{
                plugin_id="public.fixture", configuration={{}},
                required_capabilities={required}
            }}"#
        ));
        let error = apply(fixture.request(), Arc::new(AtomicBool::new(false)), |_| {})
            .err().unwrap();
        assert_eq!(error.kind(), PatchErrorKind::Script);
        assert!(!fixture.output.exists());
        fixture.unchanged();
        fixture.no_staging();
    }
}
