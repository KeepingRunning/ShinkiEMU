use std::{env, process::Command};

use sha2::{Digest, Sha256};

fn write_runtime_icon() {
    use resvg::{tiny_skia, usvg};
    let tree = usvg::Tree::from_data(
        include_bytes!("../../../Assets/Branding/shinkiemu-icon.svg"),
        &usvg::Options::default(),
    )
    .expect("SHINKI_EMU_MANAGER_ICON_INVALID");
    let mut pixmap =
        tiny_skia::Pixmap::new(64, 64).expect("SHINKI_EMU_MANAGER_ICON_DIMENSIONS_INVALID");
    pixmap.fill(tiny_skia::Color::from_rgba8(11, 18, 38, 255));
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(2.0, 2.0),
        &mut pixmap.as_mut(),
    );
    let output = env::var_os("OUT_DIR").expect("SHINKI_EMU_MANAGER_OUT_DIR_MISSING");
    std::fs::write(
        std::path::PathBuf::from(output).join("window-icon.rgba"),
        pixmap.data(),
    )
    .expect("SHINKI_EMU_MANAGER_WINDOW_ICON_WRITE_FAILED");
}

#[cfg(windows)]
fn embed_windows_icon() {
    use ico::{IconDir, IconDirEntry, IconImage, ResourceType};
    use resvg::{tiny_skia, usvg};
    use std::{fs, io::Cursor};

    let tree = usvg::Tree::from_data(
        include_bytes!("../../../Assets/Branding/shinkiemu-icon.svg"),
        &usvg::Options::default(),
    )
    .expect("SHINKI_EMU_MANAGER_ICON_INVALID");
    let output = env::var_os("OUT_DIR").expect("SHINKI_EMU_MANAGER_OUT_DIR_MISSING");
    let mut icons = IconDir::new(ResourceType::Icon);
    for size in [16, 24, 32, 48, 64, 128, 256] {
        let mut pixmap =
            tiny_skia::Pixmap::new(size, size).expect("SHINKI_EMU_MANAGER_ICON_DIMENSIONS_INVALID");
        pixmap.fill(tiny_skia::Color::from_rgba8(11, 18, 38, 255));
        resvg::render(
            &tree,
            tiny_skia::Transform::from_scale(
                size as f32 / tree.size().width(),
                size as f32 / tree.size().height(),
            ),
            &mut pixmap.as_mut(),
        );
        let image = IconImage::read_png(Cursor::new(
            pixmap
                .encode_png()
                .expect("SHINKI_EMU_MANAGER_ICON_ENCODING_FAILED"),
        ))
        .expect("SHINKI_EMU_MANAGER_ICON_PNG_INVALID");
        icons
            .add_entry(IconDirEntry::encode(&image).expect("SHINKI_EMU_MANAGER_ICON_ENTRY_FAILED"));
    }
    let icon_path = std::path::PathBuf::from(output).join("shinkiemu.ico");
    icons
        .write(fs::File::create(&icon_path).expect("SHINKI_EMU_MANAGER_ICON_CREATE_FAILED"))
        .expect("SHINKI_EMU_MANAGER_ICON_WRITE_FAILED");
    winresource::WindowsResource::new()
        .set_icon(icon_path.to_str().expect("icon path"))
        .compile()
        .expect("SHINKI_EMU_MANAGER_ICON_RESOURCE_FAILED");
}

fn main() {
    write_embedded_core_identities();
    println!("cargo:rerun-if-changed=../../../Assets/Branding/shinkiemu-icon.svg");
    write_runtime_icon();
    #[cfg(windows)]
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_windows_icon();
    }
    if env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("ohos") {
        build_ohos_glue();
    }
    for asset in [
        "../../../Assets/Effects/dxc.lock.json",
        "../../../Assets/Effects/Builtin/scale.hlsl",
        "../../../Assets/Effects/Builtin/sharpen.hlsl",
        "../../../Assets/Effects/Anime4K/restore_cnn_s.hlsl",
        "../../../Assets/Effects/Anime4K/upscale_cnn_x2_s.hlsl",
    ] {
        println!("cargo:rerun-if-changed={asset}");
    }
    let rustc = env::var_os("RUSTC").expect("SHINKI_EMU_MANAGER_BUILD_RUSTC_MISSING");
    let output = Command::new(rustc)
        .arg("-Vv")
        .output()
        .expect("SHINKI_EMU_MANAGER_BUILD_RUSTC_EXECUTION_FAILED");
    assert!(
        output.status.success(),
        "SHINKI_EMU_MANAGER_BUILD_RUSTC_IDENTITY_FAILED"
    );
    let identity = String::from_utf8(output.stdout)
        .expect("SHINKI_EMU_MANAGER_BUILD_RUSTC_IDENTITY_NOT_UTF8")
        .lines()
        .filter(|line| {
            line.starts_with("release:")
                || line.starts_with("commit-hash:")
                || line.starts_with("host:")
                || line.starts_with("LLVM version:")
        })
        .collect::<Vec<_>>()
        .join(";");
    assert!(
        !identity.is_empty(),
        "SHINKI_EMU_MANAGER_BUILD_RUSTC_IDENTITY_EMPTY"
    );
    println!(
        "cargo:rustc-env=SHINKI_EMU_MANAGER_RUSTC_FINGERPRINT=sha256.{}",
        hex_sha256(identity.as_bytes())
    );
    println!(
        "cargo:rustc-env=SHINKI_EMU_TARGET={}",
        env::var("TARGET").expect("SHINKI_EMU_MANAGER_BUILD_TARGET_MISSING")
    );
    for name in [
        "SHINKI_EMU_FAMILY_SIGNER_ID",
        "SHINKI_EMU_FAMILY_PUBLIC_KEY_HEX",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
        println!(
            "cargo:rustc-env={name}={}",
            env::var(name).unwrap_or_default()
        );
    }
    println!("cargo:rerun-if-env-changed=RUSTC");
}

fn write_embedded_core_identities() {
    const INVENTORY: &str = "../../../build/cores.toml";
    println!("cargo:rerun-if-changed={INVENTORY}");
    let inventory: toml::Value =
        toml::from_str(&std::fs::read_to_string(INVENTORY).expect("core inventory"))
            .expect("valid core inventory");
    let target = env::var("TARGET").expect("build target");
    let mut identities = serde_json::Map::new();
    for core in inventory["cores"].as_array().expect("cores") {
        let embedded = core
            .get("static_manager_targets")
            .and_then(toml::Value::as_array)
            .is_some_and(|targets| targets.iter().any(|value| value.as_str() == Some(&target)));
        if !embedded {
            continue;
        }
        if let Some(source) = core.get("source") {
            identities.insert(core["id"].as_str().expect("core id").to_owned(), serde_json::json!({
                "upstream": source["upstream_identity"],
                "adapter": {"repo":source["repo"],"branch":source["branch"],"commit":source["rev"]},
                "release_sequence": source["release_sequence"],
            }));
        }
    }
    let output = std::path::PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"))
        .join("embedded-core-identities.json");
    std::fs::write(
        output,
        serde_json::to_vec(&identities).expect("identity JSON"),
    )
    .expect("embedded identity output");
}

fn hex_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// NAPI/XComponent and OHAudio glue from `Platforms/OHOS/native` and the
/// system libraries it uses. xtask supplies the clang target/sysroot.
fn build_ohos_glue() {
    let sources = [
        "../../../Platforms/OHOS/native/bridge.c",
        "../../../Platforms/OHOS/native/audio.c",
    ];
    for source in sources {
        println!("cargo:rerun-if-changed={source}");
    }
    cc::Build::new()
        .files(sources)
        .flag("-Wall")
        .flag("-Werror")
        .compile("shinki_ohos_glue");
    for library in [
        "ace_napi.z",
        "ace_ndk.z",
        "native_window",
        "native_buffer",
        "native_vsync",
        "ohaudio",
        "hilog_ndk.z",
    ] {
        println!("cargo:rustc-link-lib=dylib={library}");
    }
}
