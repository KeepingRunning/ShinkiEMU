use shinki_emu_patch::{apply,PatchRequest,PatchMode};
use std::{fs,sync::{Arc,atomic::AtomicBool}};
fn pack(names:&[&str])->Vec<u8> {
    let mut table=Vec::new();let mut offsets=Vec::new();
    for name in names {
        offsets.push(table.len() as u32);
        let (encoded,_,bad)=encoding_rs::SHIFT_JIS.encode(name);assert!(!bad);
        table.extend_from_slice(&encoded);table.push(0);
    }
    let data=8+12*names.len()+table.len();
    let mut out=(names.len() as u32).to_le_bytes().to_vec();out.extend_from_slice(&(table.len() as u32).to_le_bytes());
    for (index,offset) in offsets.iter().enumerate() {
        out.extend_from_slice(&offset.to_le_bytes());out.extend_from_slice(&((data+index*4) as u32).to_le_bytes());out.extend_from_slice(&4u32.to_le_bytes());
    }
    out.extend(table);
    for index in 0..names.len() {out.extend_from_slice(&(index as u32+100).to_le_bytes());}
    out
}
fn run(root:&std::path::Path,bytes:&[u8])->Result<shinki_emu_patch::PatchResult,shinki_emu_patch::PatchError> {
    for p in ["source","patch","scripts/common"] {fs::create_dir_all(root.join(p)).unwrap();}
    fs::write(root.join("source/graph.bin"),bytes).unwrap();
    fs::write(root.join("scripts/common/fvp_bin.luau"),include_str!("../../../../Metadata/patches/common/fvp_bin.luau")).unwrap();
    let script=root.join("scripts/pack.luau");
    fs::write(&script,r#"return {id="public-bin",version=1,title="Public BIN",apply=function(ctx)
        ctx.load("common/fvp_bin.luau").native(ctx,"source","graph.bin","cp932","gbk")
        return {plugin_id="rfvp",configuration={script_encoding="gbk"}}
    end}"#).unwrap();
    apply(PatchRequest{script,source:root.join("source"),patch:root.join("patch"),mode:PatchMode::NewDirectory{output:root.join("output")}},Arc::new(AtomicBool::new(false)), |_|{})
}
#[test]
fn converts_standard_names_without_moving_or_mutating_payloads() {
    let root=tempfile::tempdir().unwrap();let source=pack(&["日本","ASCII"]);
    let result=run(root.path(),&source).unwrap();let output=fs::read(result.output.join("graph.bin")).unwrap();
    let names_at=32;let n=u32::from_le_bytes(output[4..8].try_into().unwrap()) as usize;
    assert_eq!(&output[..names_at],&source[..names_at]);
    let (name,_,bad)=encoding_rs::GBK.decode(&output[names_at..names_at+4]);assert!(!bad);assert_eq!(name,"日本");
    assert_eq!(&output[names_at+n..],&source[names_at+n..]);
    assert_eq!(fs::read(root.path().join("source/graph.bin")).unwrap(),source);
}
#[test]
fn rejects_collisions_width_changes_bad_terminators_and_payload_bounds() {
    let mut unterminated=pack(&["日本"]);let names_at=20;unterminated[names_at+4]=b'x';
    let mut interior=pack(&["日本"]);interior[8..12].copy_from_slice(&1u32.to_le_bytes());
    let mut payload=pack(&["日本"]);payload[12..16].copy_from_slice(&0u32.to_le_bytes());
    for invalid in [pack(&["A","a"]),pack(&["ｱ"]),unterminated,interior,payload] {
        let root=tempfile::tempdir().unwrap();assert!(run(root.path(),&invalid).is_err());assert!(!root.path().join("output").exists());
    }
}
