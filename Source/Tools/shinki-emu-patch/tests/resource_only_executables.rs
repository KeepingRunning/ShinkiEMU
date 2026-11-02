use shinki_emu_patch::{
    PatchErrorKind, PatchMode, PatchRequest, apply, has_backup, restore_latest,
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
};

const PROGRAM: &[u8] = b"MZ\0unchanged original program bytes";
fn request(root: &Path, in_place: bool, body: &str) -> (PatchRequest, PathBuf) {
    let game = root.join("game");
    fs::create_dir_all(game.join("Binaries")).unwrap();
    fs::write(game.join("Binaries/Game.EXE"), PROGRAM).unwrap();
    fs::write(game.join("resource.dat"), b"resource").unwrap();
    let script = root.join("resource-patch.luau");
    fs::write(&script,format!("return {{id='resource-only',version=1,title='Resource only',apply=function(ctx) {body};return {{plugin_id='rfvp',configuration={{}}}} end}}")).unwrap();
    let mode = if in_place {
        PatchMode::InPlace
    } else {
        PatchMode::NewDirectory {
            output: root.join("output"),
        }
    };
    (
        PatchRequest {
            script,
            source: game.clone(),
            patch: game.clone(),
            mode,
        },
        game,
    )
}
#[test]
fn in_place_omission_preserves_original_program_and_restore_leaves_it_untouched() {
    let root = tempfile::tempdir().unwrap();
    let (request, game) = request(
        root.path(),
        true,
        "ctx.copy('source','resource.dat','resource.dat');local output=ctx.edit('resource.dat');output:write(0,buffer.fromstring('R'));output:close()",
    );
    apply(request, Arc::new(AtomicBool::new(false)), |_| {}).unwrap();
    assert_eq!(fs::read(game.join("Binaries/Game.EXE")).unwrap(), PROGRAM);
    assert_eq!(fs::read(game.join("resource.dat")).unwrap(), b"Resource");
    assert!(has_backup(&game).unwrap());
    restore_latest(&game, Arc::new(AtomicBool::new(false)), |_| {}).unwrap();
    assert_eq!(fs::read(game.join("resource.dat")).unwrap(), b"resource");
    assert_eq!(fs::read(game.join("Binaries/Game.EXE")).unwrap(), PROGRAM);
}
#[test]
fn both_modes_allow_byte_identical_original_executable_copy() {
    for in_place in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let (request, game) = request(
            root.path(),
            in_place,
            "ctx.mkdir('Binaries');ctx.copy('source','Binaries/Game.EXE','Binaries/Game.EXE');ctx.copy('source','resource.dat','resource.dat')",
        );
        let result = apply(request, Arc::new(AtomicBool::new(false)), |_| {}).unwrap();
        assert_eq!(
            fs::read(result.output.join("Binaries/Game.EXE")).unwrap(),
            PROGRAM
        );
        assert_eq!(fs::read(game.join("Binaries/Game.EXE")).unwrap(), PROGRAM);
    }
}
#[test]
fn both_modes_reject_executable_replacement_and_creation_before_publication() {
    for in_place in [false, true] {
        for body in [
            "ctx.mkdir('Binaries');ctx.copy('source','Binaries/Game.EXE','Binaries/Game.EXE');local output=ctx.edit('Binaries/Game.EXE');output:write(0,buffer.fromstring('altered'));output:close()",
            "local output=ctx.create('new.exe');output:write(0,buffer.fromstring('new executable'));output:close()",
        ] {
            let root = tempfile::tempdir().unwrap();
            let (request, game) = request(root.path(), in_place, body);
            let error = apply(request, Arc::new(AtomicBool::new(false)), |_| {})
                .err()
                .unwrap();
            assert_eq!(error.kind(), PatchErrorKind::InvalidRequest);
            assert!(error.rolled_back());
            assert_eq!(fs::read(game.join("Binaries/Game.EXE")).unwrap(), PROGRAM);
            assert_eq!(fs::read(game.join("resource.dat")).unwrap(), b"resource");
            assert!(!root.path().join("output").exists());
            assert!(!has_backup(&game).unwrap());
        }
    }
}
