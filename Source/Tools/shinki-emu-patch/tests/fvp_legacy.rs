use flate2::{Compression, write::ZlibEncoder};
use shinki_emu_patch::{PatchRequest, apply};
use std::{fs, io::Write, sync::{Arc, atomic::AtomicBool}};

fn original_hcb() -> Vec<u8> {
    let mut hcb = vec![0; 32];
    hcb[..4].copy_from_slice(&32u32.to_le_bytes());
    hcb[4..9].copy_from_slice(&[2, 20, 0, 0, 0]);
    hcb[9..14].copy_from_slice(&[14, 3, b'a', b'b', 0]);
    hcb[14..19].copy_from_slice(&[14, 3, b'c', b'd', 0]);
    hcb[19] = 4;
    hcb[20..24].copy_from_slice(&[1, 0, 0, 4]);
    hcb[24..28].copy_from_slice(&[1, 4, 0, 4]);
    hcb.extend_from_slice(&4u32.to_le_bytes());
    hcb.extend_from_slice(&[0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0]);
    hcb
}
fn dictionary(records: &[(u32, &[u8])]) -> Vec<u8> {
    let mut body = (records.len() as u32).to_le_bytes().to_vec();
    for (offset, bytes) in records {
        body.extend_from_slice(&offset.to_le_bytes());
        body.extend_from_slice(&(bytes.len() as u16).to_le_bytes());
        body.extend_from_slice(bytes);
    }
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&body).unwrap();
    let compressed = encoder.finish().unwrap();
    let mut wrapped = Vec::new();
    for word in [0, 123, body.len() as u32, compressed.len() as u32] {
        wrapped.extend_from_slice(&word.to_le_bytes());
    }
    wrapped.extend(compressed);
    wrapped
}
fn fixture(root: &std::path::Path, dat: &[u8]) -> PatchRequest {
    let source = root.join("source");
    let patch = root.join("patch");
    let scripts = root.join("scripts");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&patch).unwrap();
    fs::create_dir_all(scripts.join("common")).unwrap();
    fs::write(source.join("public.hcb"), original_hcb()).unwrap();
    fs::write(patch.join("patch.dat"), dat).unwrap();
    for (name, content) in [
        ("fvp.luau", include_str!("../../../../Metadata/patches/common/fvp.luau")),
        ("hcb.luau", include_str!("../../../../Metadata/patches/common/hcb.luau")),
    ] { fs::write(scripts.join("common").join(name), content).unwrap(); }
    let script = scripts.join("fixture.luau");
    fs::write(&script, r#"return {id="public-fvp",version=2,title="Public FVP",apply=function(ctx)
        local fvp=ctx.load("common/fvp.luau")
        local hcb=ctx.load("common/hcb.luau")
        local source=ctx.open("source","public.hcb")
        local records=fvp.dictionary(ctx,ctx.open("patch","patch.dat"))
        local native=hcb.native(ctx,source,records,"cp932","gbk")
        local patched=fvp.warning(ctx,native,{site=4,original_call=20,helper=24,wait_syscall=137,yield_syscall=133})
        local output=ctx.create("public.hcb") output:write(0,patched) output:close()
        return {plugin_id="rfvp",configuration={script_encoding="gbk"}}
    end}"#).unwrap();
    PatchRequest { script, source, patch, mode: shinki_emu_patch::PatchMode::NewDirectory {output: root.join("output")} }
}
fn word(bytes: &[u8], at: usize) -> usize {
    u32::from_le_bytes(bytes[at..at+4].try_into().unwrap()) as usize
}
// Interpret only standard Jmp/Nop/PushString, preserving actual payload identities.
fn literal(bytes: &[u8], mut pc: usize) -> (String, usize, usize) {
    for _ in 0..512 {
        match bytes[pc] {
            0 => pc += 1,
            6 => pc = word(bytes, pc+1),
            14 => {
                let n=bytes[pc+1] as usize;
                assert_eq!(bytes[pc+1+n],0);
                let (text,_,bad)=encoding_rs::GBK.decode(&bytes[pc+2..pc+1+n]);
                assert!(!bad);
                return (text.into_owned(),pc+2,pc+2+n);
            }
            op => panic!("unexpected native opcode {op}"),
        }
    }
    panic!("native branch loop")
}
#[test]
fn native_literals_grow_without_relocating_calls_and_return_to_original_continuation() {
    let root=tempfile::tempdir().unwrap();
    let request=fixture(root.path(),&dictionary(&[(16,b"second\0"),(11,b"\xd6\xd0\xce\xc4\0")]));
    let result=apply(request,Arc::new(AtomicBool::new(false)), |_|{}).unwrap();
    let bytes=fs::read(result.output.join("public.hcb")).unwrap();
    let (text,id,next)=literal(&bytes,9);
    assert_eq!(text,"中文");
    assert!(id>=34 && id<word(&bytes,0));
    assert_eq!(bytes[next],6);
    assert_eq!(word(&bytes,next+1),14);
    assert_eq!(literal(&bytes,word(&bytes,next+1)).0,"second");
    let warning=word(&bytes,5);
    assert_eq!(bytes[warning],2);
    assert_eq!(word(&bytes,warning+1),20);
    assert_eq!(word(&bytes,warning+22),24);
    assert_eq!(word(&bytes,warning+36),9);
    assert_eq!(word(&bytes,word(&bytes,0)),4);
    assert_eq!(bytes[20..28],original_hcb()[20..28]);
    assert_eq!(fs::read(root.path().join("source/public.hcb")).unwrap(),original_hcb());
    assert!(!result.output.join("translation.bin").exists());
}
#[test]
fn native_gbk_preserves_ruby_names_and_long_lines_with_one_byte_lengths() {
    let root=tempfile::tempdir().unwrap();
    let line=format!("姓名[注音|正文]{}", "长行".repeat(40));
    let (encoded,_,bad)=encoding_rs::GBK.encode(&line); assert!(!bad);
    let mut value=encoded.into_owned();value.push(0);
    let request=fixture(root.path(),&dictionary(&[(11,&value)]));
    let result=apply(request,Arc::new(AtomicBool::new(false)), |_|{}).unwrap();
    assert_eq!(literal(&fs::read(result.output.join("public.hcb")).unwrap(),9).0,line);
}
#[test]
fn rejects_invalid_dictionary_boundaries_and_unrepresentable_native_lengths() {
    let mut overlong=vec![b'a';255];overlong.push(0);
    for rows in [vec![(11,&b"a\0"[..]),(11,&b"b\0"[..])],vec![(11,&b""[..])],
        vec![(11,&b"a\0b\0"[..])],vec![(11,&b"\x81\0"[..])],vec![(11,&b"abc"[..])],
        vec![(12,&b"x\0"[..])],vec![(11,overlong.as_slice())]] {
        let root=tempfile::tempdir().unwrap();let request=fixture(root.path(),&dictionary(&rows));
        assert!(apply(request,Arc::new(AtomicBool::new(false)), |_|{}).is_err());
        assert!(!root.path().join("output").exists());
    }
}
#[test]
fn rejects_interior_code_edges_and_short_expansion_sites_before_publication() {
    for interior in [true,false] {
        let root=tempfile::tempdir().unwrap();
        let request=fixture(root.path(),&dictionary(&[(11,b"long native string\0")]));
        let mut bytes=original_hcb();
        if interior {bytes[5..9].copy_from_slice(&11u32.to_le_bytes());}
        else {bytes[10]=1;bytes[11]=0;bytes[12]=0;bytes[13]=0;}
        fs::write(request.source.join("public.hcb"),bytes).unwrap();
        assert!(apply(request,Arc::new(AtomicBool::new(false)), |_|{}).is_err());
        assert!(!root.path().join("output").exists());
    }
}
#[test]
fn rejects_dictionary_crc_corruption_and_truncation_before_publication() {
    for truncate in [true,false] {
        let root=tempfile::tempdir().unwrap();let mut dat=dictionary(&[(11,b"a\0")]);
        if truncate {dat.pop();}else{*dat.last_mut().unwrap()^=1;}
        let request=fixture(root.path(),&dat);
        assert!(apply(request,Arc::new(AtomicBool::new(false)), |_|{}).is_err());
        assert!(!root.path().join("output").exists());
    }
}

#[test]
#[ignore = "requires authorized private Sakura inputs; writes only a new isolated output"]
fn private_sakura_recipe_native_receipt() {
    let source=std::path::PathBuf::from(std::env::var_os("SHINKI_SAKURA_SOURCE").expect("SHINKI_SAKURA_SOURCE"));
    let patch=std::path::PathBuf::from(std::env::var_os("SHINKI_SAKURA_PATCH").expect("SHINKI_SAKURA_PATCH"));
    let output=std::path::PathBuf::from(std::env::var_os("SHINKI_SAKURA_OUTPUT").expect("SHINKI_SAKURA_OUTPUT"));
    let script=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../Metadata/patches/fvp-sakura-cn.luau").canonicalize().unwrap();
    let result=apply(PatchRequest {script,source,patch,mode:shinki_emu_patch::PatchMode::NewDirectory {output}},
        Arc::new(AtomicBool::new(false)), |_|{}).unwrap();
    assert_eq!(result.profile.configuration.get("script_encoding").map(String::as_str),Some("gbk"));
    assert_eq!(result.profile.configuration.get("font_path").map(String::as_str),Some("font/HanaMinA.ttf"));
    assert!(!result.profile.configuration.contains_key("translation_table"));
    assert!(!result.output.join("shinki-translations.bin").exists());
    assert!(!result.output.join("save").exists());
    let receipt:serde_json::Value=serde_json::from_slice(&fs::read(result.output.join("shinki-patch-result.json")).unwrap()).unwrap();
    assert_eq!(receipt["recipe"]["version"],2);
    let bytes=fs::read(result.output.join("Sakura.hcb")).unwrap();
    use sha2::{Digest,Sha256};
    let hash=hex::encode(Sha256::digest(&bytes));
    let marker:serde_json::Value=serde_json::from_slice(&fs::read(result.output.join("shinki-native-script.json")).unwrap()).unwrap();
    assert_eq!(marker["sha256"],hash);
    assert!(receipt["outputs"].as_array().unwrap().iter().any(|row|row["path"]=="Sakura.hcb" && row["sha256"]==hash));
    println!("native recipe receipt: HCB sha256={hash}, bytes={}, recipe=2, no table, no copied saves",bytes.len());
}
