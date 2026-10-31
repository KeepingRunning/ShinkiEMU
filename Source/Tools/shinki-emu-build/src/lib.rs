//! Host-side build support. This crate never invokes Cargo or downloads tools.
use anyhow::{Context, Result, bail};
use object::Object;
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

pub const SCHEMA: &str = "shinki.emu.core_build.v1";
pub const ABI: &str = "shinki.emu.native.c.v3";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildAsset {
    pub source: PathBuf,
    pub dest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoreBuildManifest {
    pub schema: String,
    pub plugin_id: String,
    pub target: String,
    pub profile: String,
    pub abi: String,
    pub artifact_kind: String,
    pub native_library: Option<PathBuf>,
    pub resources: Vec<BuildAsset>,
    pub licenses: Vec<BuildAsset>,
}

impl CoreBuildManifest {
    pub fn new(plugin_id: impl Into<String>, artifact_kind: impl Into<String>) -> Result<Self> {
        Ok(Self {
            schema: SCHEMA.into(),
            plugin_id: plugin_id.into(),
            target: env::var("TARGET").context("TARGET is required in build.rs")?,
            profile: env::var("PROFILE").context("PROFILE is required in build.rs")?,
            abi: ABI.into(),
            artifact_kind: artifact_kind.into(),
            native_library: None,
            resources: Vec::new(),
            licenses: Vec::new(),
        })
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema != SCHEMA || self.abi != ABI {
            bail!("invalid core build protocol");
        }
        if self.plugin_id.is_empty()
            || self.plugin_id.len() > 128
            || !self
                .plugin_id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
        {
            bail!("invalid plugin_id");
        }
        if self.target.is_empty() || !matches!(self.profile.as_str(), "debug" | "release") {
            bail!("invalid target/profile");
        }
        match self.artifact_kind.as_str() {
            "rust-cdylib" if self.native_library.is_none() => {}
            "native-shared"
                if self
                    .native_library
                    .as_ref()
                    .is_some_and(|p| p.is_absolute() && p.is_file()) => {}
            "native-static"
                if self.native_library.as_ref().is_some_and(|path| {
                    path.is_absolute()
                        && path.is_file()
                        && path
                            .extension()
                            .is_some_and(|extension| extension == "a" || extension == "lib")
                }) => {}
            _ => bail!("artifact_kind and native_library disagree"),
        }
        for (assets, prefix) in [
            (&self.resources, "Resources/"),
            (&self.licenses, "licenses/"),
        ] {
            for asset in assets {
                if !asset.source.is_absolute() || !asset.source.exists() {
                    bail!("missing absolute build input: {}", asset.source.display());
                }
                if !(asset.dest.starts_with(prefix)
                    || (asset.dest == prefix.trim_end_matches('/') && asset.source.is_dir()))
                    || asset.dest.contains('\\')
                    || asset.dest.contains(':')
                    || asset
                        .dest
                        .split('/')
                        .any(|s| s.is_empty() || s == "." || s == "..")
                {
                    bail!("unsafe build asset destination: {}", asset.dest);
                }
            }
        }
        Ok(())
    }
}

pub fn write_core_build_manifest(manifest: &CoreBuildManifest) -> Result<PathBuf> {
    let out = PathBuf::from(env::var_os("OUT_DIR").context("OUT_DIR is required in build.rs")?);
    write_core_build_manifest_to(manifest, &out)
}

pub fn write_core_build_manifest_to(manifest: &CoreBuildManifest, out: &Path) -> Result<PathBuf> {
    manifest.validate()?;
    fs::create_dir_all(out)?;
    let output_root = dunce::canonicalize(out)?;
    for asset in manifest.resources.iter().chain(&manifest.licenses) {
        let input = dunce::canonicalize(&asset.source)?;
        if !input.starts_with(&output_root) {
            println!("cargo:rerun-if-changed={}", input.display());
        }
    }
    let path = out.join("core-build.json");
    let mut bytes = serde_json::to_vec_pretty(manifest)?;
    bytes.push(b'\n');
    fs::write(&path, bytes)?;
    Ok(path)
}

fn run(command: &mut Command) -> Result<()> {
    let status = command
        .status()
        .with_context(|| format!("starting {}", command.get_program().to_string_lossy()))?;
    if !status.success() {
        bail!(
            "{} exited with {status}",
            command.get_program().to_string_lossy()
        );
    }
    Ok(())
}

/// Keep native build/download caches and temporary files in an owned work tree.
pub fn configure_native_environment(work_root: &Path, command: &mut Command) -> Result<()> {
    let absolute = std::path::absolute(work_root)?;
    let work_root = dunce::simplified(&absolute);
    let binary_cache = work_root.join("vcpkg-binary-cache");
    let downloads = work_root.join("vcpkg-downloads");
    let temporary = work_root.join("tmp");
    for directory in [&binary_cache, &downloads, &temporary] {
        for ancestor in directory.ancestors() {
            match fs::symlink_metadata(ancestor) {
                Ok(metadata) => {
                    if metadata.file_type().is_symlink() {
                        bail!("linked native cache path: {}", ancestor.display());
                    }
                    #[cfg(windows)]
                    {
                        use std::os::windows::fs::MetadataExt;
                        if metadata.file_attributes() & 0x400 != 0 {
                            bail!("reparse native cache path: {}", ancestor.display());
                        }
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        fs::create_dir_all(directory)?;
    }
    command
        .env("VCPKG_DEFAULT_BINARY_CACHE", binary_cache)
        .env("VCPKG_DOWNLOADS", downloads)
        .env("VCPKG_MAX_CONCURRENCY", "4")
        .env("TEMP", &temporary)
        .env("TMP", temporary);
    Ok(())
}

/// Resolve one native core's installation within the Manager's shared build environment.
pub fn native_vcpkg_installation(core: &str) -> Result<PathBuf> {
    println!("cargo:rerun-if-env-changed=SHINKI_EMU_NATIVE_VCPKG_ROOT");
    let root = env::var_os("SHINKI_EMU_NATIVE_VCPKG_ROOT")
        .context("SHINKI_EMU_NATIVE_VCPKG_ROOT is required; run cargo xtask deps")?;
    let triplet = env::var("SHINKI_EMU_VCPKG_TRIPLET")
        .context("SHINKI_EMU_VCPKG_TRIPLET is required; run cargo xtask deps")?;
    let installed = PathBuf::from(root)
        .join(core)
        .join(format!("installed-{triplet}"));
    if !installed.is_dir() {
        bail!("native vcpkg installation missing: {}", installed.display());
    }
    Ok(installed)
}

/// Merge the native build's archives and publish its prepared static link closure to Cargo.
pub fn link_apple_static_core(
    core: &str,
    native: &Path,
    installations: &[PathBuf],
) -> Result<PathBuf> {
    fn archives(
        directory: &Path,
        staged: &Path,
        result: &mut std::collections::BTreeSet<PathBuf>,
    ) -> Result<()> {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                if entry.path() != staged {
                    archives(&entry.path(), staged, result)?;
                }
            } else if entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "a")
            {
                result.insert(fs::canonicalize(entry.path())?);
            }
        }
        Ok(())
    }
    let target = env::var("TARGET")?;
    let architecture = match target.split('-').next() {
        Some("aarch64") => "arm64",
        Some("x86_64") => "x86_64",
        _ => bail!("unsupported Apple static target {target}"),
    };
    if !target.contains("apple") {
        bail!("Apple static linking requires an Apple target");
    }
    let mut inputs = std::collections::BTreeSet::new();
    archives(native, &native.join("install"), &mut inputs)?;
    if inputs.is_empty() {
        bail!("native core {core} produced no static archives");
    }
    let name = format!("shinki_{}_closure", core.replace(['.', '-'], "_"));
    let output = native
        .parent()
        .context("native output parent")?
        .join(format!("lib{name}.a"));
    run(Command::new("xcrun")
        .args(["libtool", "-static", "-arch_only", architecture, "-o"])
        .arg(&output)
        .args(inputs))?;
    println!(
        "cargo:rustc-link-search=native={}",
        output.parent().unwrap().display()
    );
    println!("cargo:rustc-link-lib=static={name}");
    for installation in installations {
        let libraries = installation.join("lib");
        println!("cargo:rustc-link-search=native={}", libraries.display());
        let mut entries = fs::read_dir(&libraries)?.collect::<std::io::Result<Vec<_>>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            if path.extension().is_some_and(|extension| extension == "a") {
                let stem = path
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .context("archive name")?;
                let library = stem.strip_prefix("lib").context("Apple archive prefix")?;
                println!("cargo:rustc-link-lib=static={library}");
            }
        }
        for directory in [
            installation.join("lib/pkgconfig"),
            installation.join("share/pkgconfig"),
        ] {
            if !directory.is_dir() {
                continue;
            }
            for entry in fs::read_dir(directory)? {
                let path = entry?.path();
                if path.extension().is_some_and(|extension| extension == "pc") {
                    pkg_config::Config::new().statik(true).probe(
                        path.file_stem()
                            .and_then(|value| value.to_str())
                            .context("pkg-config name")?,
                    )?;
                }
            }
        }
    }
    println!("cargo:rustc-link-lib=c++");
    println!("cargo:rustc-link-lib=framework=Foundation");
    Ok(output)
}
fn should_auto_select_ninja(host: &str, target: &str, generator_override: bool) -> bool {
    !generator_override
        && (target.ends_with("windows-msvc")
            || (host.contains("windows") && target.contains("android")))
}

/// Configure an OUT_DIR-owned native tree, then build the requested real targets.
/// Cross builds must supply an explicit CMake toolchain; no host/target guessing.
pub fn configure_cmake(
    source: &Path,
    definitions: &[(String, String)],
    targets: &[&str],
) -> Result<PathBuf> {
    let host = env::var("HOST").context("HOST is required")?;
    let target = env::var("TARGET").context("TARGET is required")?;
    let source =
        dunce::canonicalize(source).context("canonicalizing native CMake source directory")?;
    let output = PathBuf::from(env::var_os("OUT_DIR").context("OUT_DIR is required")?);
    let out = dunce::simplified(&output).join("native");
    let profile = if env::var("PROFILE")? == "release" {
        "Release"
    } else {
        "Debug"
    };
    for name in [
        "CMAKE",
        "CMAKE_GENERATOR",
        "CMAKE_TOOLCHAIN_FILE",
        "CC",
        "CXX",
        "PATH",
        "LIB",
        "INCLUDE",
        "VCToolsInstallDir",
        "VCPKG_ROOT",
        "BISON_EXECUTABLE",
        "SHINKI_EMU_VCPKG_TRIPLET",
        "SHINKI_EMU_NATIVE_DOWNLOADS",
        "HOST",
        "TARGET",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    println!("cargo:rerun-if-changed={}", source.display());
    if targets.is_empty() {
        bail!("CMake requires a real native target");
    }
    if host != target
        && env::var_os("CMAKE_TOOLCHAIN_FILE").is_none()
        && !definitions.iter().any(|(k, _)| k == "CMAKE_TOOLCHAIN_FILE")
    {
        bail!("cross CMake build {host} -> {target} requires CMAKE_TOOLCHAIN_FILE");
    }
    let cmake = env::var_os("CMAKE").unwrap_or_else(|| "cmake".into());
    let work_root = if let Some(downloads) = env::var_os("SHINKI_EMU_NATIVE_DOWNLOADS") {
        PathBuf::from(downloads)
            .parent()
            .context("native downloads work root")?
            .to_owned()
    } else {
        PathBuf::from(env::var_os("OUT_DIR").context("OUT_DIR is required")?).join("host-cache")
    };
    let mut configure = Command::new(&cmake);
    configure_native_environment(&work_root, &mut configure)?;
    configure.env("VCPKG_DISABLE_METRICS", "1");
    configure
        .arg("-S")
        .arg(source)
        .arg("-B")
        .arg(&out)
        .arg(format!("-DCMAKE_BUILD_TYPE={profile}"))
        .arg(format!(
            "-DCMAKE_INSTALL_PREFIX={}",
            out.join("install").display()
        ))
        .arg("-DFETCHCONTENT_FULLY_DISCONNECTED=ON")
        .arg("-DFETCHCONTENT_UPDATES_DISCONNECTED=ON")
        .arg("-DVCPKG_MANIFEST_INSTALL=OFF");
    if let Some(generator) = env::var_os("CMAKE_GENERATOR") {
        configure.arg("-G").arg(generator);
    } else if host.contains("windows")
        && should_auto_select_ninja(&host, &target, false)
        && env::var_os("VCToolsInstallDir").is_some()
        && env::var_os("LIB").is_some()
        && env::var_os("INCLUDE").is_some()
        && let Some(ninja) = env::var_os("PATH").and_then(|paths| {
            env::split_paths(&paths)
                .map(|directory| directory.join("ninja.exe"))
                .find(|path| path.is_file())
        })
    {
        configure
            .args(["-G", "Ninja"])
            .arg(format!("-DCMAKE_MAKE_PROGRAM={}", ninja.display()));
    }
    if let Some(toolchain) = env::var_os("CMAKE_TOOLCHAIN_FILE") {
        configure.arg(format!(
            "-DCMAKE_TOOLCHAIN_FILE={}",
            dunce::simplified(Path::new(&toolchain)).display()
        ));
    }
    if let Ok(triplet) = env::var("SHINKI_EMU_VCPKG_TRIPLET") {
        configure.arg(format!("-DVCPKG_TARGET_TRIPLET={triplet}"));
    }
    for (key, value) in definitions {
        configure.arg(format!(
            "-D{key}={}",
            dunce::simplified(Path::new(value)).display()
        ));
    }
    run(&mut configure)?;
    let mut build = Command::new(cmake);
    configure_native_environment(&work_root, &mut build)?;
    build.env("VCPKG_DISABLE_METRICS", "1");
    build
        .arg("--build")
        .arg(&out)
        .arg("--config")
        .arg(profile)
        .arg("--target")
        .args(targets);
    if let Some(jobs) = env::var_os("NUM_JOBS") {
        build.arg("--parallel").arg(jobs);
    }
    run(&mut build)?;
    Ok(out)
}

/// Generate an MSVC import library from the official runtime DLL's exports.
pub fn generate_import_library(dll: &Path, out_dir: &Path, target: &str) -> Result<PathBuf> {
    if !target.ends_with("windows-msvc") {
        bail!("MSVC import libraries require a windows-msvc target");
    }
    let bytes = fs::read(dll).with_context(|| format!("reading {}", dll.display()))?;
    let file = object::File::parse(bytes.as_slice())?;
    let machine = match (target.split('-').next(), file.architecture()) {
        (Some("x86_64"), object::Architecture::X86_64) => "X64",
        (Some("aarch64"), object::Architecture::Aarch64) => "ARM64",
        (Some("i686"), object::Architecture::I386) => "X86",
        _ => bail!("DLL architecture does not match {target}"),
    };
    if file.format() != object::BinaryFormat::Pe {
        bail!("import source must be a PE DLL");
    }
    let name = dll
        .file_name()
        .and_then(|v| v.to_str())
        .context("DLL has no UTF-8 filename")?;
    if !name
        .bytes()
        .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
    {
        bail!("unsafe DLL filename");
    }
    let mut exports = Vec::new();
    for export in file.exports()? {
        if let object::read::NameOrOrdinal::Name(name) = export?.name() {
            exports.push(std::str::from_utf8(name)?.to_owned());
        }
    }
    exports.sort();
    exports.dedup();
    if exports.is_empty() {
        bail!("DLL has no named exports");
    }
    if exports.iter().any(|s| {
        s.is_empty()
            || s.bytes()
                .any(|c| c.is_ascii_whitespace() || b"\";=".contains(&c))
    }) {
        bail!("unsafe DLL export name");
    }
    fs::create_dir_all(out_dir)?;
    let stem = dll
        .file_stem()
        .and_then(|name| name.to_str())
        .context("DLL has no UTF-8 stem")?;
    let definition = out_dir.join(format!("{stem}.def"));
    let library = out_dir.join(format!("{stem}.lib"));
    fs::write(
        &definition,
        format!("LIBRARY {name}\nEXPORTS\n{}\n", exports.join("\n")),
    )?;
    let tool = cc::windows_registry::find_tool(target, "lib.exe")
        .context("MSVC lib.exe not found; install Visual C++ build tools for the target")?;
    run(tool
        .to_command()
        .arg("/NOLOGO")
        .arg(format!("/MACHINE:{machine}"))
        .arg(format!("/DEF:{}", definition.display()))
        .arg(format!("/OUT:{}", library.display())))?;
    if !library.is_file() {
        bail!("lib.exe did not produce {}", library.display());
    }
    println!("cargo:rerun-if-changed={}", dll.display());
    Ok(library)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ninja_auto_selection_respects_host_target_and_override() {
        assert!(should_auto_select_ninja(
            "x86_64-pc-windows-msvc",
            "x86_64-pc-windows-msvc",
            false
        ));
        assert!(should_auto_select_ninja(
            "x86_64-pc-windows-msvc",
            "aarch64-linux-android",
            false
        ));
        assert!(!should_auto_select_ninja(
            "x86_64-unknown-linux-gnu",
            "aarch64-linux-android",
            false
        ));
        assert!(!should_auto_select_ninja(
            "x86_64-pc-windows-msvc",
            "x86_64-pc-windows-msvc",
            true
        ));
    }

    #[test]
    fn malformed_assets_do_not_publish_manifest() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("license");
        fs::write(&source, b"license").unwrap();
        let mut manifest = CoreBuildManifest {
            schema: SCHEMA.into(),
            abi: ABI.into(),
            plugin_id: "test".into(),
            target: "x86_64-pc-windows-msvc".into(),
            profile: "release".into(),
            artifact_kind: "rust-cdylib".into(),
            native_library: None,
            resources: vec![],
            licenses: vec![BuildAsset {
                source,
                dest: "licenses/../escape".into(),
            }],
        };
        assert!(write_core_build_manifest_to(&manifest, &temp.path().join("out")).is_err());
        assert!(!temp.path().join("out/core-build.json").exists());
        manifest.licenses[0].dest = "licenses/test/LICENSE".into();
        let path = write_core_build_manifest_to(&manifest, &temp.path().join("out")).unwrap();
        let decoded: CoreBuildManifest = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        decoded.validate().unwrap();
    }
    #[cfg(windows)]
    #[test]
    fn dotted_dll_import_library_links_and_executes_a_real_consumer() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("dependency.rs");
        fs::write(&source, "#![no_std]\n#[panic_handler] fn panic(_: &core::panic::PanicInfo)->! {loop{}}\n#[unsafe(no_mangle)] pub extern \"C\" fn dependency_answer()->u32 {42}\n").unwrap();
        let dll = temp.path().join("libpython3.12.dll");
        let status = Command::new("rustc")
            .arg(&source)
            .arg("--edition=2024")
            .args([
                "--crate-type",
                "cdylib",
                "-C",
                "panic=abort",
                "-C",
                "link-arg=/NOENTRY",
                "-o",
            ])
            .arg(&dll)
            .status()
            .unwrap();
        assert!(status.success());
        let host = Command::new("rustc").arg("-vV").output().unwrap();
        let version = String::from_utf8(host.stdout).unwrap();
        let target = version
            .lines()
            .find_map(|line| line.strip_prefix("host: "))
            .unwrap();
        let imports = temp.path().join("imports");
        generate_import_library(&dll, &imports, target).unwrap();
        let consumer = temp.path().join("consumer.rs");
        fs::write(&consumer, "#[link(name=\"libpython3.12\")] unsafe extern \"C\" {fn dependency_answer()->u32;}\nfn main(){if unsafe {dependency_answer()} != 42 {std::process::exit(2);}}\n").unwrap();
        let executable = temp.path().join("consumer.exe");
        let status = Command::new("rustc")
            .arg(&consumer)
            .arg("--edition=2024")
            .arg(format!("-Lnative={}", imports.display()))
            .arg("-o")
            .arg(&executable)
            .status()
            .unwrap();
        assert!(status.success());
        assert!(Command::new(&executable).status().unwrap().success());
    }
}

#[cfg(test)]
mod cargo_regressions {
    use super::*;

    #[test]
    fn unchanged_generated_assets_leave_the_real_cargo_library_fresh() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join("src")).unwrap();
        fs::write(temp.path().join("Cargo.toml"), format!(
            "[package]\nname=\"generated-asset-fixture\"\nversion=\"0.1.0\"\nedition=\"2024\"\nbuild=\"build.rs\"\n[lib]\ncrate-type=[\"cdylib\"]\n[workspace]\n[build-dependencies]\nshinki-emu-build={{path={:?}}}\n",
            env!("CARGO_MANIFEST_DIR")
        )).unwrap();
        fs::write(
            temp.path().join("LICENSE"),
            "Public synthetic fixture: CC0-1.0\n",
        )
        .unwrap();
        fs::write(
            temp.path().join("src/lib.rs"),
            "include!(concat!(env!(\"OUT_DIR\"), \"/generated.rs\"));\n",
        )
        .unwrap();
        fs::write(temp.path().join("build.rs"), r##"
use shinki_emu_build::{BuildAsset, CoreBuildManifest, write_core_build_manifest};
fn main() {
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let generated = out.join("generated.rs");
    std::fs::write(&generated, "#[unsafe(no_mangle)] pub extern \"C\" fn generated_answer()->u32 {42}\n").unwrap();
    let mut manifest = CoreBuildManifest::new("fixture", "rust-cdylib").unwrap();
    manifest.resources.push(BuildAsset {source: generated, dest: "Resources/generated.rs".into()});
    manifest.licenses.push(BuildAsset {
        source: std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("LICENSE"),
        dest: "licenses/fixture/LICENSE".into(),
    });
    write_core_build_manifest(&manifest).unwrap();
}
"##).unwrap();
        let cargo = |args: &[&str]| {
            let output = Command::new(env!("CARGO"))
                .current_dir(temp.path())
                .args(args)
                .env("CARGO_TARGET_DIR", temp.path().join("target"))
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            output.stdout
        };
        cargo(&["generate-lockfile", "--offline"]);
        let first = cargo(&["build", "--locked", "--offline", "--message-format=json"]);
        let second = cargo(&["build", "--locked", "--offline", "--message-format=json"]);
        let library = |bytes: &[u8]| {
            String::from_utf8_lossy(bytes)
                .lines()
                .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
                .find(|message| {
                    message["reason"] == "compiler-artifact"
                        && message["target"]["name"] == "generated_asset_fixture"
                })
                .expect("Cargo must report the actual generated library")
        };
        assert_eq!(library(&first)["fresh"], false);
        assert_eq!(library(&second)["fresh"], true);
    }
}
