use sha2::{Digest, Sha256};
use shinki_emu_metadata::PatchRelease;
use shinki_emu_patch::{PatchErrorKind, PatchMode, PatchRequest, apply_bound};
use std::{
    fs,
    path::Path,
    sync::{Arc, atomic::AtomicBool},
};

fn release(id: &str, bytes: &[u8]) -> PatchRelease {
    serde_json::from_value(serde_json::json!({"release_id":id,"vn_id":"v17",
        "title":{"zh-CN":"测试版本","en":"Test edition"},"edition":"download","game_version":null,
        "fingerprints":[{"path":"game.dat","size":bytes.len(),"sha256":hex::encode(Sha256::digest(bytes))}]})).unwrap()
}
fn request(root: &Path) -> PatchRequest {
    fs::create_dir(root.join("source")).unwrap();
    fs::write(root.join("source/game.dat"), b"original").unwrap();
    let script = root.join("recipe.luau");
    fs::write(&script,r#"return {id='bound-test',version=1,title='Bound conversion',output_mode='new_directory',apply=function(ctx)
        ctx.copy('source','game.dat','game.dat')
        local output=ctx.edit('game.dat'); output:write(0,buffer.fromstring('T')); output:close()
        return {plugin_id='rfvp',configuration={}}
    end}"#).unwrap();
    PatchRequest {
        script,
        source: root.join("source"),
        patch: root.join("source"),
        mode: PatchMode::NewDirectory {
            output: root.join("output"),
        },
    }
}
#[test]
fn concrete_release_hashes_gate_execution_and_preserve_source() {
    let root = tempfile::tempdir().unwrap();
    let request = request(root.path());
    let releases = [release("r1", b"other"), release("r2", b"original")];
    let result = apply_bound(
        request,
        &releases,
        None,
        Arc::new(AtomicBool::new(false)),
        |_| {},
    )
    .unwrap();
    assert_eq!(
        fs::read(result.output.join("game.dat")).unwrap(),
        b"Triginal"
    );
    assert_eq!(
        fs::read(root.path().join("source/game.dat")).unwrap(),
        b"original"
    );
}
#[test]
fn unmatched_and_ambiguous_releases_do_not_publish_or_execute() {
    for releases in [
        vec![release("r1", b"different")],
        vec![release("r1", b"original"), release("r2", b"original")],
    ] {
        let root = tempfile::tempdir().unwrap();
        let request = request(root.path());
        let error = apply_bound(
            request,
            &releases,
            None,
            Arc::new(AtomicBool::new(false)),
            |_| {},
        )
        .err()
        .unwrap();
        assert_eq!(error.kind(), PatchErrorKind::ReleaseMismatch);
        assert!(!root.path().join("output").exists());
        assert_eq!(
            fs::read(root.path().join("source/game.dat")).unwrap(),
            b"original"
        );
    }
}
#[test]
fn local_binding_manifest_is_required_and_bounded() {
    let root = tempfile::tempdir().unwrap();
    let request = request(root.path());
    assert!(shinki_emu_patch::read_release_bindings(&request.script).is_err());
    fs::write(
        request.script.with_extension("patch.json"),
        serde_json::to_vec(&vec![release("r1", b"original")]).unwrap(),
    )
    .unwrap();
    let bindings = shinki_emu_patch::read_release_bindings(&request.script).unwrap();
    apply_bound(
        request,
        &bindings,
        None,
        Arc::new(AtomicBool::new(false)),
        |_| {},
    )
    .unwrap();
}
