use shinki_emu_patch::{PatchMode, PatchRequest, apply};
use std::{
    fs,
    sync::{Arc, atomic::AtomicBool},
};

/// Local private inputs are copied first. This never starts either executable.
#[test]
#[ignore = "requires SHINKI_YORINO_SOURCE with locally supplied game data"]
fn real_yorino_executable_constants_and_generated_names_patch_in_independent_copy() {
    let input = std::path::PathBuf::from(
        std::env::var_os("SHINKI_YORINO_SOURCE").expect("local Yorino source"),
    );
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let patch = root.path().join("patch");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&patch).unwrap();
    let before = fs::read(input.join("yorino.exe")).unwrap();
    fs::write(source.join("yorino.exe"), &before).unwrap();
    fs::copy(input.join("name.acr"), patch.join("name.acr")).unwrap();
    fs::write(
        root.path().join("musica_acr.luau"),
        include_str!("../../../../Metadata/patches/common/musica_acr.luau"),
    )
    .unwrap();
    let script = root.path().join("entry.luau");
    fs::write(&script, r#"
return {id='yorino-executable-copy',version=1,title='Yorino copied executable',output_mode='new_directory',apply=function(ctx)
    local acr=ctx.load('musica_acr.luau')
    local names=acr.read(ctx,ctx.open('patch','name.acr'))
    acr.embed_executable(ctx,names,{
        path='yorino.exe',size=2278400,sha256='0664a773545cccfa04f6f0e840fbf6302913a721bc2fc956cf206111b9c93553',
        constants={{row=27,offset=0x112098},{row=43,offset=0x121f18}},
        brackets={{offset=0x121f24,text='】'},{offset=0x121f28,text='【'}},
        generated_names={{28,1},{29,2},{30,3},{31,6},{32,7},{33,9},{34,10},{35,10},{36,12},{37,13},{38,15},{39,16},{40,17},{41,21},{42,25}},
    })
    return {plugin_id='shinki.emu.musica',configuration={}}
end}
"#).unwrap();
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
    let after = fs::read(result.output.join("yorino.exe")).unwrap();
    assert_eq!(after.len(), before.len());
    let spans = [(0x112098, 6), (0x121f18, 10), (0x121f24, 2), (0x121f28, 2)];
    for (at, (old, new)) in before.iter().zip(&after).enumerate() {
        if !spans
            .iter()
            .any(|(start, size)| (*start..start + size).contains(&at))
        {
            assert_eq!(old, new, "unexpected PE change at {at:x}");
        }
    }
    for (at, size) in [(0x112098, 6), (0x121f18, 10)] {
        let original = encoding_rs::SHIFT_JIS
            .decode_without_bom_handling_and_without_replacement(&before[at..at + size])
            .unwrap();
        let translated = encoding_rs::GBK
            .decode_without_bom_handling_and_without_replacement(&after[at..at + size])
            .unwrap();
        assert_eq!(original, translated);
        assert_eq!(after[at + size], 0);
        assert_ne!(before[at..at + size], after[at..at + size]);
    }
    assert_eq!(&after[0x121f24..0x121f26], &[0xa1, 0xbf]);
    assert_eq!(&after[0x121f28..0x121f2a], &[0xa1, 0xbe]);
    assert_eq!(fs::read(source.join("yorino.exe")).unwrap(), before);
    assert_eq!(fs::read(input.join("yorino.exe")).unwrap(), before);
}
