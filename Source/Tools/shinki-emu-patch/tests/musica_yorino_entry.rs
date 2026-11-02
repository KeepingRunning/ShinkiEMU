#[path = "support/musica.rs"]
mod native;
use sha2::{Digest, Sha256};
use shinki_emu_patch::{PatchErrorKind, PatchRequest, apply};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
};

const HELPER: &str = include_str!("../../../../Metadata/patches/common/yorino_entry.luau");
const ACR: &str = include_str!("../../../../Metadata/patches/common/musica_acr.luau");
const COPY: &str = include_str!("../../../../Metadata/patches/common/copy.luau");
const ROLES: [(&str, &str); 8] = [
    ("bg.paz", "bg"),
    ("bgm.paz", "bgm"),
    ("cnscr.paz", "scr"),
    ("st.paz", "st"),
    ("cnsys.paz", "sys"),
    ("se.paz", "se"),
    ("voice.paz", "voice"),
    ("mov.paz", "mov"),
];

fn put32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn source_hash(bytes: &[u8]) -> u32 {
    bytes
        .iter()
        .copied()
        .filter(|b| *b != 0)
        .fold(0_u32, |h, b| h.wrapping_mul(131).wrapping_add(u32::from(b)))
        & 0x7fff_ffff
}

fn acr(count: usize) -> Vec<u8> {
    let mut bytes = vec![0; 16 + count * 20];
    put32(&mut bytes, 0, count as u32);
    for i in 0..count {
        // Distinct source identities collide in rows 1/2; first record wins.
        let original: &[u8] = match i {
            1 => &[1, 132],
            2 => &[2, 1],
            _ => b"\xd6\xd0",
        };
        let target: &[u8] = match i {
            1 => b"first-name",
            2 => b"second-name",
            _ => b"\xd6\xd0\xce\xc4",
        };
        let from = bytes.len();
        bytes.extend_from_slice(original);
        let to = bytes.len();
        bytes.extend_from_slice(target);
        for (field, value) in [
            source_hash(original),
            from as u32,
            original.len() as u32,
            to as u32,
            target.len() as u32,
        ]
        .into_iter()
        .enumerate()
        {
            put32(&mut bytes, 16 + i * 20 + field * 4, value);
        }
    }
    let stored = bytes.len() as u32 - 16;
    put32(&mut bytes, 8, stored);
    put32(&mut bytes, 12, stored);
    bytes
}

fn cnbg(count: usize) -> (Vec<u8>, Vec<u32>) {
    let mut bytes = vec![0; 24];
    bytes[..8].copy_from_slice(b"FUYINPAK");
    put32(&mut bytes, 8, 24);
    let mut records = Vec::new();
    let mut crcs = Vec::new();
    for index in 0..count {
        let name = format!("public_{index}.bmp\0");
        let name_at = bytes.len() as u32;
        bytes.extend_from_slice(name.as_bytes());
        let data_at = bytes.len() as u32;
        let mut bmp = vec![0; 72]; // 54-byte header + 16 BGRA bytes + proven two-zero tail.
        bmp[..2].copy_from_slice(b"BM");
        put32(&mut bmp, 2, 72);
        put32(&mut bmp, 10, 54);
        put32(&mut bmp, 14, 40);
        put32(&mut bmp, 18, 2);
        put32(&mut bmp, 22, 2);
        bmp[26..28].copy_from_slice(&1_u16.to_le_bytes());
        bmp[28..30].copy_from_slice(&32_u16.to_le_bytes());
        put32(&mut bmp, 34, 18);
        // Bottom-up: blue/white bottom row, red/green top row. Zero alpha is data.
        bmp[54..70].copy_from_slice(&[
            255, 0, 0, 127, 255, 255, 255, 255, 0, 0, 255, 0, 0, 255, 0, 32,
        ]);
        crcs.push(crc32fast::hash(&bmp));
        bytes.extend_from_slice(&bmp);
        records.push([name_at, name.len() as u32, data_at, 72, 72]);
    }
    let index_at = bytes.len() as u32;
    for record in records {
        for value in record {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    put32(&mut bytes, 16, index_at);
    put32(&mut bytes, 20, count as u32 * 20);
    (bytes, crcs)
}

struct Fixture {
    root: PathBuf,
    edition: String,
    snapshots: Vec<(PathBuf, Vec<u8>)>,
}

impl Fixture {
    fn new(root: &Path, names: usize, images: usize, bad_crc: bool, missing_key: bool) -> Self {
        let source = root.join("source");
        let patch = root.join("patch");
        let scripts = root.join("scripts");
        fs::create_dir_all(scripts.join("common")).unwrap();
        fs::create_dir(&source).unwrap();
        fs::create_dir(&patch).unwrap();
        for (name, text) in [
            ("yorino_entry.luau", HELPER),
            ("musica_acr.luau", ACR),
            ("copy.luau", COPY),
        ] {
            fs::write(scripts.join("common").join(name), text).unwrap();
        }
        let mut inputs = Vec::new();
        for (path, role) in ROLES {
            let payload: &[u8] = if role == "scr" {
                b".message 1  \xd6\xd0 \xd6\xd0\r\n.message 2  \x02\x01 literal\r\n.end\r\n"
            } else {
                b"public native resource"
            };
            let bytes = native::archive(
                role,
                if role == "scr" {
                    "test.sc"
                } else {
                    "fixture.bin"
                },
                payload,
            );
            fs::write(source.join(path), &bytes).unwrap();
            inputs.push(format!(
                "{{path={path:?},role={role:?},size={},sha256=\"{}\"}}",
                bytes.len(),
                hex::encode(Sha256::digest(&bytes))
            ));
        }
        for path in ["bg.pazA", "st.pazA"] {
            let bytes = b"public opaque split resource";
            fs::write(source.join(path), bytes).unwrap();
            inputs.push(format!(
                "{{path={path:?},size={},sha256=\"{}\"}}",
                bytes.len(),
                hex::encode(Sha256::digest(bytes))
            ));
        }
        let (bitmaps, mut crcs) = cnbg(images);
        for (path, bytes) in [("name.acr", acr(names)), ("cnbg.paz", bitmaps)] {
            fs::write(patch.join(path), &bytes).unwrap();
            inputs.push(format!(
                "{{path={path:?},patch=true,size={},sha256=\"{}\"}}",
                bytes.len(),
                hex::encode(Sha256::digest(&bytes))
            ));
        }
        if bad_crc {
            *crcs.last_mut().unwrap() ^= 1;
        }
        let mut profile = String::from(
            "schema='shinki.emu.musica.keys.v2'\npaz_version=0\nindex_size_xor=0\nrc4_skip_crc=false\n",
        );
        profile.push_str("[texture_overrides]\n'musica:/bg/public_0.png'='existing.png'\n'musica:/sys/unrelated.png'='existing.png'\n");
        for (_, role) in ROLES {
            if !(missing_key && role == "mov") {
                profile.push_str(&native::key_role(role));
            }
        }
        fs::write(source.join("key.toml"), profile).unwrap();
        image::RgbaImage::from_pixel(1, 1, image::Rgba([9, 8, 7, 6]))
            .save(source.join("existing.png"))
            .unwrap();
        fs::write(
            source.join("font.ttf"),
            b"public font copy fixture; not a native font validation",
        )
        .unwrap();
        fs::write(
            source.join("original.exe"),
            b"public forbidden original entry",
        )
        .unwrap();
        fs::write(source.join("legacy.dll"), b"public forbidden DLL").unwrap();
        fs::create_dir(source.join("savedata")).unwrap();
        fs::write(
            source.join("savedata/slot.sav"),
            b"public save must remain private and untouched",
        )
        .unwrap();
        let edition = format!(
            "{{names={names},width=2,height=2,inputs={{{}}},background_crc32={{{}}},coverage={{status='synthetic'}}}}",
            inputs.join(","),
            crcs.iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(",")
        );
        let mut snapshots = Vec::new();
        for dir in [source.clone(), patch.clone(), source.join("savedata")] {
            for entry in fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_file() {
                    snapshots.push((path.clone(), fs::read(path).unwrap()));
                }
            }
        }
        Self {
            root: root.to_owned(),
            edition,
            snapshots,
        }
    }

    fn run(&self) -> Result<shinki_emu_patch::PatchResult, shinki_emu_patch::PatchError> {
        let script = self.root.join("scripts/entry.luau");
        fs::write(&script, format!("return {{id='public-yorino',version=1,title='Public Yorino conversion fixture',apply=function(ctx) return ctx.load('common/yorino_entry.luau').apply(ctx,{}) end}}", self.edition)).unwrap();
        let result = apply(
            PatchRequest {
                script,
                source: self.root.join("source"),
                patch: self.root.join("patch"),
                mode: shinki_emu_patch::PatchMode::NewDirectory {
                    output: self.root.join("output"),
                },
            },
            Arc::new(AtomicBool::new(false)),
            |_| {},
        );
        for (path, bytes) in &self.snapshots {
            assert_eq!(&fs::read(path).unwrap(), bytes);
        }
        result
    }
}

#[test]
fn ordered_acr_rows_embed_into_native_sc_and_backgrounds_preserve_alpha() {
    let root = tempfile::tempdir().unwrap();
    let fixture = Fixture::new(root.path(), 44, 40, false, false);
    let result = fixture.run().unwrap();
    let config = &result.profile.configuration;
    assert_eq!(config["scr_archive"], "cnscr.paz");
    assert_eq!(config["sys_archive"], "cnsys.paz");
    assert!(!config.contains_key("translation_mode"));
    assert!(!config.contains_key("translation_table"));
    assert_eq!(config["script_directory"], "scripts");
    assert_eq!(
        result.profile.required_capabilities,
        shinki_emu_abi::CAP_NATIVE_EMBEDDED_TEXT | shinki_emu_abi::CAP_NATIVE_NAME_RESOURCES
    );
    assert_eq!(config["script_encoding"], "gbk");
    assert_eq!(config["font_path"], "font.ttf");
    assert!(!result.output.join("yorino-translations.json").exists());
    let bytes = fs::read(result.output.join("scripts/test.sc")).unwrap();
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
    let Some(shinki_emu_musica::MusicaVmEvent::Message {
        text,
        speaker,
        wait: shinki_emu_musica::MusicaWaitState::Input { token_id },
        ..
    }) = vm.step(1).unwrap()
    else {
        panic!("message expected")
    };
    assert_eq!(text, "中");
    assert_eq!(speaker.as_deref(), Some("中文"));
    vm.resolve_wait(&token_id).unwrap();
    let Some(shinki_emu_musica::MusicaVmEvent::Message {
        text,
        speaker,
        wait,
        ..
    }) = vm.step(2).unwrap()
    else {
        panic!("message expected")
    };
    assert_eq!(text, "literal");
    assert_eq!(speaker.as_deref(), Some("first-name"));
    assert!(matches!(
        wait,
        shinki_emu_musica::MusicaWaitState::Input { .. }
    ));
    let coverage: serde_json::Value =
        serde_json::from_slice(&fs::read(result.output.join("yorino-coverage.json")).unwrap())
            .unwrap();
    assert_eq!(coverage["embedded_display_operands"], 2);
    assert_eq!(coverage["names"], 44);
    assert_eq!(coverage["record_audit"]["different_identity_collisions"], 1);
    let profile: toml::Value =
        toml::from_str(&fs::read_to_string(result.output.join("key.toml")).unwrap()).unwrap();
    assert_eq!(
        profile["texture_overrides"]["musica:/sys/unrelated.png"].as_str(),
        Some("existing.png")
    );
    assert_eq!(
        profile["texture_overrides"]["musica:/bg/public_0.png"].as_str(),
        Some("yorino-backgrounds/public_0.png")
    );
    for i in 0..40 {
        let image = image::open(
            result
                .output
                .join(format!("yorino-backgrounds/public_{i}.png")),
        )
        .unwrap()
        .to_rgba8();
        assert_eq!(
            image.as_raw(),
            &[
                255, 0, 0, 0, 0, 255, 0, 32, 0, 0, 255, 127, 255, 255, 255, 255
            ]
        );
    }
    for (path, _) in ROLES {
        assert_eq!(
            fs::read(result.output.join(path)).unwrap(),
            fs::read(root.path().join("source").join(path)).unwrap()
        );
    }
    for forbidden in [
        "original.exe",
        "legacy.dll",
        "savedata",
        "name.acr",
        "AsyncScript.acr",
        "cnbg.paz",
        "scr.paz",
        "sys.paz",
    ] {
        assert!(!result.output.join(forbidden).exists());
    }
    assert!(result.output.join("existing.png").is_file());
    assert!(result.output.join("bg.pazA").is_file());
    assert!(result.output.join("st.pazA").is_file());
}

#[test]
fn final_background_crc_failure_rolls_back_all_copied_archives_and_generated_images() {
    let root = tempfile::tempdir().unwrap();
    let fixture = Fixture::new(root.path(), 44, 3, true, false);
    assert_eq!(
        fixture.run().err().unwrap().kind(),
        PatchErrorKind::Integrity
    );
    assert!(!root.path().join("output").exists());
    assert!(
        !fs::read_dir(root.path()).unwrap().any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("staging"))
    );
}

#[test]
fn missing_eighth_key_role_never_publishes_partial_output() {
    let root = tempfile::tempdir().unwrap();
    let fixture = Fixture::new(root.path(), 44, 2, false, true);
    assert_eq!(
        fixture.run().err().unwrap().kind(),
        PatchErrorKind::Integrity
    );
    assert!(!root.path().join("output").exists());
}

#[test]
fn changed_pinned_source_fingerprint_is_rejected_without_touching_any_input() {
    let root = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(root.path(), 44, 2, false, false);
    let fingerprint = hex::encode(Sha256::digest(
        fs::read(root.path().join("source/bg.paz")).unwrap(),
    ));
    fixture.edition = fixture.edition.replacen(&fingerprint, &"0".repeat(64), 1);
    assert_eq!(
        fixture.run().err().unwrap().kind(),
        PatchErrorKind::UnsupportedInput
    );
    assert!(!root.path().join("output").exists());
}
