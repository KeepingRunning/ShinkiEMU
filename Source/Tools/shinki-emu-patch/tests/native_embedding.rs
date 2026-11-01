#[path = "support/musica.rs"]
mod native;
use shinki_emu_patch::{PatchMode, PatchRequest, apply};
use std::{
    fs,
    path::Path,
    sync::{Arc, atomic::AtomicBool},
};

fn stage(root: &Path, script: &[u8]) {
    fs::create_dir(root.join("source")).unwrap();
    fs::create_dir(root.join("patch")).unwrap();
    let mut profile = String::from(
        "schema='shinki.emu.musica.keys.v2'\npaz_version=0\nindex_size_xor=0\nrc4_skip_crc=false\n",
    );
    for role in ["bg", "bgm", "scr", "st", "sys", "se", "voice", "mov"] {
        let (name, payload) = if role == "scr" {
            ("test.sc", script)
        } else {
            ("asset.bin", b"public asset".as_slice())
        };
        fs::write(
            root.join(format!("source/{role}.paz")),
            native::archive(role, name, payload),
        )
        .unwrap();
        profile.push_str(&native::key_role(role));
    }
    fs::write(root.join("source/key.toml"), profile).unwrap();
    fs::write(root.join("convert.luau"), r#"
return {id='public-native-embedding',version=1,title='Native embedding',output_mode='new_directory',apply=function(ctx)
    for _,role in {'bg','bgm','scr','st','sys','se','voice','mov'} do ctx.copy('source',role..'.paz',role..'.paz') end
    ctx.copy('source','key.toml','key.toml')
    local function row(source,text) local h=0 for i=1,#source do h=(h*131+string.byte(source,i))%2147483648 end
        return {source_hash=h,source_sha256=ctx.hash.sha256(buffer.fromstring(source)),text=text} end
    local count=ctx.musica_embed('key.toml',{version=2,source_encoding='gbk',hash_collision_policy='first_record',
        names={row('Speaker','姓名'),row('First','选项: 一')},dialogue={row('Hello','译文\\a\\v')}},{},'scripts')
    assert(count==3)
    return {plugin_id='shinki.emu.musica',required_capabilities=256,configuration={profile_file='key.toml',script_directory='scripts',script_encoding='gbk'}}
end}
"#).unwrap();
}

fn request(root: &Path, mode: PatchMode) -> PatchRequest {
    PatchRequest {
        script: root.join("convert.luau"),
        source: root.join("source"),
        patch: root.join("patch"),
        mode,
    }
}

#[test]
fn native_reader_embeds_literal_text_and_choices_without_runtime_tables() {
    let root = tempfile::tempdir().unwrap();
    let original =
        b".message 1  Speaker Hello\r\n.select First:done Second:done\r\n.label done\r\n.end\r\n";
    stage(root.path(), original);
    let result = apply(
        request(
            root.path(),
            PatchMode::NewDirectory {
                output: root.path().join("output"),
            },
        ),
        Arc::new(AtomicBool::new(false)),
        |_| {},
    )
    .unwrap();
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
    assert_eq!(text, "译文\\a\\v");
    assert_eq!(speaker.as_deref(), Some("姓名"));
    vm.resolve_wait(&token_id).unwrap();
    assert!(matches!(
        vm.step(2).unwrap(),
        Some(shinki_emu_musica::MusicaVmEvent::Choice)
    ));
    assert_eq!(
        vm.choice_display().unwrap().unwrap().0,
        ["选项: 一", "Second"]
    );
    vm.focus_choice(0).unwrap();
    vm.commit_choice().unwrap();
    assert_eq!(
        fs::read(root.path().join("source/scr.paz")).unwrap(),
        native::archive("scr", "test.sc", original)
    );
    assert_eq!(
        result.profile.required_capabilities,
        shinki_emu_abi::CAP_NATIVE_EMBEDDED_TEXT
    );
    assert!(
        !result
            .profile
            .configuration
            .contains_key("translation_table")
    );
}

#[test]
fn embedded_recipe_can_patch_and_restore_a_game_copy_in_place() {
    let root = tempfile::tempdir().unwrap();
    stage(
        root.path(),
        b".message 1  Speaker Hello\r\n.select First:done Second:done\r\n.label done\r\n.end\r\n",
    );
    let before = fs::read(root.path().join("source/scr.paz")).unwrap();
    let result = apply(
        request(root.path(), PatchMode::InPlace),
        Arc::new(AtomicBool::new(false)),
        |_| {},
    )
    .unwrap();
    assert!(result.output.join("scripts/test.sc").is_file());
    assert!(shinki_emu_patch::has_backup(&result.output).unwrap());
    shinki_emu_patch::restore_latest(&result.output, Arc::new(AtomicBool::new(false)), |_| {})
        .unwrap();
    assert_eq!(
        fs::read(root.path().join("source/scr.paz")).unwrap(),
        before
    );
    assert!(!result.output.join("scripts/test.sc").exists());
}

#[test]
#[ignore = "requires SHINKI_YORINO_SCRIPTS (private copied SC) and SHINKI_YORINO_SOURCE"]
fn real_yorino_names_embed_without_changing_chinese_bodies() {
    let scripts = std::path::PathBuf::from(
        std::env::var_os("SHINKI_YORINO_SCRIPTS").expect("copied SC directory"),
    );
    let source = std::path::PathBuf::from(
        std::env::var_os("SHINKI_YORINO_SOURCE").expect("local source directory"),
    );
    let root = tempfile::tempdir().unwrap();
    stage(root.path(), b".end\r\n");
    let mut paths: Vec<_> = fs::read_dir(scripts)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    paths.sort();
    let scripts: Vec<_> = paths
        .iter()
        .map(|path| {
            (
                path.file_name().unwrap().to_str().unwrap().to_owned(),
                fs::read(path).unwrap(),
            )
        })
        .collect();
    let entries: Vec<_> = scripts
        .iter()
        .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
        .collect();
    fs::write(
        root.path().join("source/scr.paz"),
        native::archive_entries("scr", &entries),
    )
    .unwrap();
    let names = fs::read(source.join("name.acr")).unwrap();
    fs::write(root.path().join("patch/name.acr"), &names).unwrap();
    fn u32_at(bytes: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
    }
    let mut targets = std::collections::BTreeMap::new();
    for index in 0..u32_at(&names, 0) as usize {
        let at = 16 + index * 20;
        let offset = u32_at(&names, at + 12) as usize;
        let length = u32_at(&names, at + 16) as usize;
        let target = encoding_rs::GBK
            .decode_without_bom_handling_and_without_replacement(&names[offset..offset + length])
            .unwrap();
        targets.entry(u32_at(&names, at)).or_insert_with(|| {
            target
                .strip_prefix(['@', '#'])
                .unwrap_or(&target)
                .to_owned()
        });
    }
    fs::write(
        root.path().join("musica_acr.luau"),
        include_str!("../../../../Metadata/patches/common/musica_acr.luau"),
    )
    .unwrap();
    fs::write(root.path().join("convert.luau"),r#"return {id='real-yorino-native-sc',version=1,title='Copied Yorino SC',apply=function(ctx)
        for _,role in {'bg','bgm','scr','st','sys','se','voice','mov'} do ctx.copy('source',role..'.paz',role..'.paz') end
        ctx.copy('source','key.toml','key.toml')
        local acr=ctx.load('musica_acr.luau')
        local names=acr.read(ctx,ctx.open('patch','name.acr'))
        local translations=acr.translation_table(ctx,names,{},{source='gbk',names='gbk',dialogue='gbk'})
        translations.dialogue=ctx.json.decode('[]')
        local count=ctx.musica_embed('key.toml',translations,{},'scripts')
        local audit=ctx.create('audit.json'); audit:write(0,buffer.fromstring(ctx.json.encode({display_operands=count}))); audit:close()
        return {plugin_id='shinki.emu.musica',required_capabilities=256,configuration={profile_file='key.toml',script_directory='scripts',script_encoding='gbk'}}
    end}"#).unwrap();
    let result = apply(
        request(
            root.path(),
            PatchMode::NewDirectory {
                output: root.path().join("output"),
            },
        ),
        Arc::new(AtomicBool::new(false)),
        |_| {},
    )
    .unwrap();
    let catalog = shinki_emu_musica::ScOpcodeCatalog::observed_musica();
    let mut expected_hits = 0;
    let mut matched_names = std::collections::BTreeSet::new();
    for (name, original) in &scripts {
        let bytes = fs::read(result.output.join("scripts").join(name)).unwrap();
        let parse = |bytes: &[u8]| {
            shinki_emu_musica::parse_sc_with_encoding(
                bytes,
                &catalog,
                shinki_emu_musica::ScriptEncoding::detect(
                    bytes,
                    shinki_emu_musica::ScriptEncoding::Gbk,
                ),
            )
            .unwrap()
        };
        let old = parse(original);
        let new = parse(&bytes);
        assert_eq!(old.lines.len(), new.lines.len());
        for (old, new) in old.lines.iter().zip(&new.lines) {
            let (
                shinki_emu_musica::ScLineKind::Command {
                    command: old_command,
                },
                shinki_emu_musica::ScLineKind::Command {
                    command: new_command,
                },
            ) = (&old.kind, &new.kind)
            else {
                assert_eq!(old.raw, new.raw);
                continue;
            };
            if old_command.opcode != "message" {
                assert_eq!(old.raw, new.raw);
                continue;
            }
            let mut before = old_command
                .raw_operands
                .splitn(4, |byte| matches!(byte, b' ' | b'\t'));
            let mut after = new_command
                .raw_operands
                .splitn(4, |byte| matches!(byte, b' ' | b'\t'));
            for _ in 0..2 {
                assert_eq!(before.next(), after.next());
            }
            let speaker = before.next();
            let embedded = after.next();
            let body = before.next();
            assert_eq!(body, after.next(), "Chinese message body changed in {name}");
            if let Some(speaker) = speaker.filter(|speaker| !speaker.is_empty() && body.is_some()) {
                let hash = shinki_emu_musica::musica_display_cstr_hash(speaker);
                if let Some(target) = targets.get(&hash) {
                    use std::fmt::Write;
                    let mut expected = String::new();
                    for character in target.chars() {
                        write!(&mut expected, "\\u{{{:x}}}", u32::from(character)).unwrap();
                    }
                    assert_eq!(embedded, Some(expected.as_bytes()));
                    expected_hits += 1;
                    matched_names.insert(hash);
                } else {
                    assert_eq!(embedded, Some(speaker));
                }
            } else {
                assert_eq!(embedded, speaker);
            }
        }
    }
    let audit: serde_json::Value =
        serde_json::from_slice(&fs::read(result.output.join("audit.json")).unwrap()).unwrap();
    assert_eq!(audit["display_operands"], expected_hits);
    assert_eq!(matched_names.len(), 27);
    println!(
        "Actual copied Yorino SC: scripts={}, native speaker endpoints={}, distinct names={}, Chinese bodies unchanged",
        scripts.len(),
        expected_hits,
        matched_names.len()
    );
}
