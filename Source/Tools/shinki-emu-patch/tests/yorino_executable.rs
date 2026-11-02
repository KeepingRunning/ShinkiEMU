use shinki_emu_patch::{PatchMode, PatchRequest, apply};
use std::{
    fs,
    sync::{Arc, atomic::AtomicBool},
};

/// Copies private input first; neither original nor output executable is run.
#[test]
#[ignore = "requires SHINKI_YORINO_SOURCE with copied local game data"]
fn real_yorino_executable_is_unchanged_and_constants_are_native_resources() {
    let input = std::path::PathBuf::from(
        std::env::var_os("SHINKI_YORINO_SOURCE").expect("copied Yorino source"),
    );
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let patch = root.path().join("patch");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&patch).unwrap();
    let original = fs::read(input.join("yorino.exe")).unwrap();
    fs::write(source.join("yorino.exe"), &original).unwrap();
    let records = fs::read(input.join("name.acr")).unwrap();
    fs::write(patch.join("name.acr"), &records).unwrap();
    fs::write(
        root.path().join("musica_acr.luau"),
        include_str!("../../../../Metadata/patches/common/musica_acr.luau"),
    )
    .unwrap();
    let script = root.path().join("entry.luau");
    fs::write(&script,r#"return {id='yorino-resource-copy',version=1,title='Yorino native name resources',apply=function(ctx)
        local executable=ctx.open('source','yorino.exe')
        if executable:size()~=2278400 or executable:sha256()~='0664a773545cccfa04f6f0e840fbf6302913a721bc2fc956cf206111b9c93553' then ctx.fail('UNSUPPORTED_INPUT','Unexpected Yorino executable') end
        ctx.copy('source','yorino.exe','yorino.exe')
        local acr=ctx.load('musica_acr.luau')
        acr.name_resource(ctx,acr.read(ctx,ctx.open('patch','name.acr')),'scripts/shinki-names.sc')
        return {plugin_id='shinki.emu.musica',required_capabilities=768,configuration={script_directory='scripts',script_encoding='gbk'}}
    end}"#).unwrap();
    let result = apply(
        PatchRequest {
            script,
            source: source.clone(),
            patch,
            mode: PatchMode::NewDirectory {
                output: root.path().join("output"),
            },
        },
        Arc::new(AtomicBool::new(false)),
        |_| {},
    )
    .unwrap();
    assert_eq!(
        fs::read(result.output.join("yorino.exe")).unwrap(),
        original
    );
    assert_eq!(fs::read(source.join("yorino.exe")).unwrap(), original);
    assert_eq!(fs::read(input.join("yorino.exe")).unwrap(), original);
    fn u32_at(bytes: &[u8], at: usize) -> usize {
        u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize
    }
    let literal = |row: usize| {
        let at = 16 + row * 20;
        let start = u32_at(&records, at + 12);
        let size = u32_at(&records, at + 16);
        let text = encoding_rs::GBK
            .decode_without_bom_handling_and_without_replacement(&records[start..start + size])
            .unwrap();
        let mut literal = String::new();
        for character in text.chars() {
            use std::fmt::Write;
            write!(&mut literal, "\\u{{{:x}}}", u32::from(character)).unwrap();
        }
        literal
    };
    let resource = fs::read_to_string(result.output.join("scripts/shinki-names.sc")).unwrap();
    assert_eq!(
        resource,
        format!(
            ".pragma shinki_name_unknown {}\n.pragma shinki_history_name_unknown {}\n",
            literal(27),
            literal(43)
        )
    );
    let parsed = shinki_emu_musica::parse_sc_with_encoding(
        resource.as_bytes(),
        &shinki_emu_musica::ScOpcodeCatalog::observed_musica(),
        shinki_emu_musica::ScriptEncoding::Gbk,
    )
    .unwrap();
    assert_eq!(parsed.lines.len(), 2);
    assert_eq!(
        result.profile.required_capabilities,
        shinki_emu_abi::CAP_NATIVE_EMBEDDED_TEXT | shinki_emu_abi::CAP_NATIVE_NAME_RESOURCES
    );
}
