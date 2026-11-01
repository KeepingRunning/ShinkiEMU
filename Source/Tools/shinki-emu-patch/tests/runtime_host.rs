use sha2::{Digest, Sha256};
use shinki_emu_patch::{PatchErrorKind, PatchMode, PatchRequest, apply, apply_bound};
use std::{
    fs,
    io::Write,
    path::Path,
    sync::{Arc, atomic::AtomicBool},
};

fn request(root: &Path, script: &str, mode: PatchMode) -> PatchRequest {
    fs::create_dir_all(root.join("game")).unwrap();
    fs::write(root.join("game/game.dat"), b"original").unwrap();
    let path = root.join("patch.luau");
    fs::write(&path, script).unwrap();
    PatchRequest {
        script: path,
        source: root.join("game"),
        patch: root.join("game"),
        mode,
    }
}
fn sha(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

#[test]
fn declared_catalog_and_local_downloads_use_verified_cache_handles() {
    for channel in ["local", "catalog", "denied"] {
        let root = tempfile::tempdir().unwrap();
        let digest = sha(b"x");
        let script = format!(
            r#"return {{id='download-flow',version=1,title='Download flow',apply=function(ctx)
            local partial=ctx.create('partial.bin');partial:write(0,buffer.fromstring('partial'));partial:close()
            local file=ctx.download('https://example.invalid/pinned.bin','{digest}',1)
            assert(file:size()==1 and file:sha256()=='{digest}')
            local data=file:read(0,1);buffer.writeu8(data,0,buffer.readu8(data,0)+1)
            local output=ctx.create('asset.bin');output:write(0,data);output:close()
            return {{plugin_id='rfvp',configuration={{}}}}
        end}}"#
        );
        let output = root.path().join("output");
        let request = request(
            root.path(),
            &script,
            PatchMode::NewDirectory {
                output: output.clone(),
            },
        );
        let cache = request.source.join(".shinki/patch");
        fs::create_dir_all(&cache).unwrap();
        fs::write(cache.join(format!("{digest}.bin")), b"x").unwrap();
        let release:shinki_emu_metadata::PatchRelease=serde_json::from_value(serde_json::json!({
            "release_id":"r1","vn_id":"v1","title":{"zh-CN":"测试","en":"Test"},"edition":"download","game_version":null,
            "fingerprints":[{"path":"game.dat","size":8,"sha256":sha(b"original")}]})).unwrap();
        let declared = vec![shinki_emu_metadata::PatchDownload {
            url: "https://example.invalid/pinned.bin".into(),
            sha256: digest.clone(),
            size: 1,
        }];
        let empty = [];
        let declarations = match channel {
            "local" => None,
            "catalog" => Some(declared.as_slice()),
            _ => Some(empty.as_slice()),
        };
        let result = apply_bound(
            request,
            &[release],
            declarations,
            Arc::new(AtomicBool::new(false)),
            |_| {},
        );
        if channel == "denied" {
            let error = result.err().unwrap();
            assert_eq!(error.kind(), PatchErrorKind::InvalidRequest);
            assert!(error.to_string().contains("undeclared download"));
            assert!(error.rolled_back());
            assert!(!output.exists());
        } else {
            assert_eq!(
                fs::read(result.unwrap().output.join("asset.bin")).unwrap(),
                b"y"
            );
        }
        assert_eq!(fs::read(cache.join(format!("{digest}.bin"))).unwrap(), b"x");
        assert_eq!(
            fs::read(root.path().join("game/game.dat")).unwrap(),
            b"original"
        );
    }
}

fn zip(path: &Path, entries: &[(&str, &[u8])], link: bool) {
    let mut archive = zip::ZipWriter::new(fs::File::create(path).unwrap());
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, bytes) in entries {
        archive.start_file(*name, options).unwrap();
        archive.write_all(bytes).unwrap();
    }
    if link {
        archive
            .add_symlink("link", "../../outside", options)
            .unwrap();
    }
    archive.finish().unwrap();
}
#[test]
fn local_zip_reads_only_its_modules_and_resources() {
    let root = tempfile::tempdir().unwrap();
    let script = r#"return {id='zip-native',version=1,title='ZIP native',apply=function(ctx)
        local increment=ctx.load('common/increment.luau')
        local data=ctx.open('package','assets/value.bin'):read(0,1);increment(data)
        local output=ctx.create('translated.bin');output:write(0,data);output:close()
        return {plugin_id='rfvp',configuration={}}
    end}"#;
    let output = root.path().join("output");
    let mut request = request(root.path(), script, PatchMode::NewDirectory { output });
    let package = root.path().join("patch.zip");
    zip(
        &package,
        &[
            ("patch.luau", script.as_bytes()),
            (
                "common/increment.luau",
                b"return function(data) buffer.writeu8(data,0,buffer.readu8(data,0)+1) end",
            ),
            ("assets/value.bin", &[41]),
        ],
        false,
    );
    let before = fs::read(&package).unwrap();
    request.script = package.clone();
    let result = apply(request, Arc::new(AtomicBool::new(false)), |_| {}).unwrap();
    assert_eq!(
        fs::read(result.output.join("translated.bin")).unwrap(),
        [42]
    );
    assert_eq!(fs::read(package).unwrap(), before);
}
#[test]
fn zip_paths_and_links_cannot_escape_private_extraction() {
    let script=b"return {id='safe',version=1,title='Safe',apply=function(ctx) return {plugin_id='rfvp',configuration={}} end}";
    for (bad, link) in [
        ("../outside", false),
        ("/absolute", false),
        ("C:/outside", false),
        ("allowed", true),
    ] {
        let root = tempfile::tempdir().unwrap();
        let output = root.path().join("output");
        let mut request = request(
            root.path(),
            "",
            PatchMode::NewDirectory {
                output: output.clone(),
            },
        );
        let package = root.path().join("patch.zip");
        zip(
            &package,
            &[("patch.luau", script), (bad, b"untrusted")],
            link,
        );
        request.script = package;
        assert!(apply(request, Arc::new(AtomicBool::new(false)), |_| {}).is_err());
        assert!(!output.exists());
        assert_eq!(
            fs::read(root.path().join("game/game.dat")).unwrap(),
            b"original"
        );
    }
}
#[test]
fn script_fail_and_native_error_roll_back_game_copy_and_keep_message() {
    for fail in [
        "ctx.fail('INTEGRITY','verbatim failure marker')",
        "error('verbatim failure marker')",
    ] {
        let root = tempfile::tempdir().unwrap();
        let script = format!(
            "return {{id='failure',version=1,title='Failure',apply=function(ctx) ctx.copy('source','game.dat','game.dat');local file=ctx.edit('game.dat');file:write(0,buffer.fromstring('changed'));file:close();{fail};return {{plugin_id='rfvp',configuration={{}}}} end}}"
        );
        let request = request(root.path(), &script, PatchMode::InPlace);
        let error = apply(request, Arc::new(AtomicBool::new(false)), |_| {})
            .err()
            .unwrap();
        assert!(error.to_string().contains("verbatim failure marker"));
        assert!(error.rolled_back());
        assert_eq!(
            fs::read(root.path().join("game/game.dat")).unwrap(),
            b"original"
        );
        assert!(!shinki_emu_patch::has_backup(&root.path().join("game")).unwrap());
    }
}
