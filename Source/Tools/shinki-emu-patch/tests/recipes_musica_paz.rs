#[path = "support/musica.rs"]
mod native;
use shinki_emu_patch::{PatchErrorKind, PatchRequest, apply};
use std::{
    fs,
    path::Path,
    sync::{Arc, atomic::AtomicBool},
};

fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

// Data-only PE32 fixture. This is never executable code: its only section is
// read-only initialized data containing original pointer pairs or GBK names.
fn image(base: u32) -> Vec<u8> {
    let mut bytes = vec![0; 1024];
    bytes[..2].copy_from_slice(b"MZ");
    put32(&mut bytes, 0x3c, 0x80);
    bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
    bytes[0x84..0x86].copy_from_slice(&0x14c_u16.to_le_bytes());
    bytes[0x86..0x88].copy_from_slice(&1_u16.to_le_bytes());
    bytes[0x94..0x96].copy_from_slice(&0xe0_u16.to_le_bytes());
    bytes[0x96..0x98].copy_from_slice(&0x102_u16.to_le_bytes());
    bytes[0x98..0x9a].copy_from_slice(&0x10b_u16.to_le_bytes());
    put32(&mut bytes, 0xb4, base);
    put32(&mut bytes, 0xb8, 4096);
    put32(&mut bytes, 0xbc, 512);
    put32(&mut bytes, 0xd0, 8192);
    put32(&mut bytes, 0xd4, 512);
    put32(&mut bytes, 0xf4, 16);
    bytes[0x178..0x17d].copy_from_slice(b".data");
    put32(&mut bytes, 0x180, 512);
    put32(&mut bytes, 0x184, 4096);
    put32(&mut bytes, 0x188, 512);
    put32(&mut bytes, 0x18c, 512);
    put32(&mut bytes, 0x19c, 0x4000_0040);
    bytes
}

fn fixtures() -> (Vec<u8>, Vec<u8>) {
    let mut original = image(0x400000);
    put32(&mut original, 512, 0x401180);
    put32(&mut original, 516, 0x401100);
    put32(&mut original, 520, 0x401190);
    put32(&mut original, 524, 0x401120);
    original[768..776].copy_from_slice(b"old-one\0");
    original[800..808].copy_from_slice(b"old-two\0");
    original[896..904].copy_from_slice(b"speaker\0");
    // Display operands keep their raw GBK identity, not the reference pointer.
    original[912..917].copy_from_slice(b"raw\x81\x40");
    let mut patch = image(0x10000000);
    patch[768..772].copy_from_slice(b"\xb9\xab\xbf\xaa"); // 公开
    patch[800..804].copy_from_slice(b"\xc3\xfb\xd7\xd6"); // 名字
    (original, patch)
}

fn request(root: &Path, original: &[u8], patch: &[u8]) -> PatchRequest {
    let source = root.join("source");
    let patch_root = root.join("patch");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&patch_root).unwrap();
    fs::write(source.join("original.exe"), original).unwrap();
    fs::write(patch_root.join("names.dll"), patch).unwrap();
    let sc = b".message 1  speaker Original\n.message 2  raw\x81\x40 Original\n.message 3  \xb9\xab\xbf\xaa Original\n.message 4  \xc3\xfb\xd7\xd6 Original\n.end\n";
    let mut profile = String::from(
        "schema='shinki.emu.musica.keys.v2'\npaz_version=0\nindex_size_xor=0\nrc4_skip_crc=false\n",
    );
    for role in ["bg", "bgm", "scr", "st", "sys", "se", "voice", "mov"] {
        let (name, payload) = if role == "scr" {
            ("test.sc", sc.as_slice())
        } else {
            ("asset.bin", b"asset".as_slice())
        };
        fs::write(
            source.join(format!("{role}.paz")),
            native::archive(role, name, payload),
        )
        .unwrap();
        profile.push_str(&native::key_role(role));
    }
    fs::write(source.join("key.toml"), profile).unwrap();
    let common = root.join("common");
    fs::create_dir(&common).unwrap();
    fs::write(
        common.join("musica.luau"),
        include_str!("../../../../Metadata/patches/common/musica.luau"),
    )
    .unwrap();
    fs::write(
        common.join("pe.luau"),
        include_str!("../../../../Metadata/patches/common/pe.luau"),
    )
    .unwrap();
    fs::write(
        common.join("binary.luau"),
        include_str!("../../../../Metadata/patches/common/binary.luau"),
    )
    .unwrap();
    let script = root.join("names.luau");
    fs::write(&script, r#"
return { id="public-musica-names", version=1, title="Public PE names", apply=function(ctx)
    local executable = ctx.open("source", "original.exe")
    local patch = ctx.open("patch", "names.dll")
    local names = ctx.load("common/musica.luau").names(ctx, executable, patch,
        {table_rva=0x1000,targets={0x1100,0x1120}})
    for _,role in {'bg','bgm','scr','st','sys','se','voice','mov'} do ctx.copy('source',role..'.paz',role..'.paz') end
    ctx.copy('source','key.toml','key.toml')
    ctx.musica_embed('key.toml',names,{},'scripts')
    return {plugin_id='shinki.emu.musica',required_capabilities=256,configuration={profile_file='key.toml',script_encoding='gbk',script_directory='scripts'}}
end }
"#).unwrap();
    PatchRequest {
        script,
        source,
        patch: patch_root,
        mode: shinki_emu_patch::PatchMode::NewDirectory {
            output: root.join("output"),
        },
    }
}

fn speakers(output: &Path) -> Vec<String> {
    let bytes = fs::read(output.join("scripts/test.sc")).unwrap();
    let script = shinki_emu_musica::parse_sc_with_encoding(
        &bytes,
        &shinki_emu_musica::ScOpcodeCatalog::observed_musica(),
        shinki_emu_musica::ScriptEncoding::Gbk,
    )
    .unwrap();
    let mut vm = shinki_emu_musica::MusicaVm::new(
        "musica:/scr/test.sc".into(),
        roukaku_core::Hash256::from_sha256(&bytes),
        script,
        0,
    )
    .unwrap();
    let mut speakers = Vec::new();
    for _ in 0..4 {
        let Some(shinki_emu_musica::MusicaVmEvent::Message {
            speaker,
            text,
            wait: shinki_emu_musica::MusicaWaitState::Input { token_id },
            ..
        }) = vm.step(vm.state().fixed_tick + 1).unwrap()
        else {
            panic!("native message expected")
        };
        assert_eq!(text, "Original");
        speakers.push(speaker.unwrap());
        vm.resolve_wait(&token_id).unwrap();
    }
    speakers
}

#[test]
fn display_pointer_and_already_localized_gbk_speakers_embed_without_runtime_tables() {
    let root = tempfile::tempdir().unwrap();
    let (original, patch) = fixtures();
    let request = request(root.path(), &original, &patch);
    let source_path = request.source.join("original.exe");
    let patch_path = request.patch.join("names.dll");
    let original_scripts = fs::read(request.source.join("scr.paz")).unwrap();
    let result = apply(request, Arc::new(AtomicBool::new(false)), |_| {}).unwrap();
    assert_eq!(speakers(&result.output), ["公开", "名字", "公开", "名字"]);
    assert!(!result.output.join("names.json").exists());
    assert!(
        !result
            .profile
            .configuration
            .contains_key("translation_table")
    );
    assert_eq!(fs::read(source_path).unwrap(), original);
    assert_eq!(fs::read(patch_path).unwrap(), patch);
    assert_eq!(
        fs::read(root.path().join("source/scr.paz")).unwrap(),
        original_scripts
    );
}

#[test]
fn invalid_target_gbk_fails_without_publishing() {
    let root = tempfile::tempdir().unwrap();
    let (original, mut patch) = fixtures();
    patch[768..773].copy_from_slice(b"\x81\0\0\0\0");
    let request = request(root.path(), &original, &patch);
    let shinki_emu_patch::PatchMode::NewDirectory { output } = &request.mode else {
        unreachable!()
    };
    let output = output.clone();
    let error = apply(request, Arc::new(AtomicBool::new(false)), |_| {})
        .err()
        .expect("invalid GBK must fail");
    assert_eq!(error.kind(), PatchErrorKind::Codec);
    assert!(!output.exists());
}

#[test]
fn duplicate_display_identity_preserves_first_native_translation() {
    let root = tempfile::tempdir().unwrap();
    let (mut original, patch) = fixtures();
    put32(&mut original, 520, 0x401180);
    let request = request(root.path(), &original, &patch);
    let result = apply(request, Arc::new(AtomicBool::new(false)), |_| {}).unwrap();
    assert_eq!(speakers(&result.output)[0], "公开");
}
