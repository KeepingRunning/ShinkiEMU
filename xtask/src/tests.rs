use crate::{binary, build::BuildResult, cores::Inventory, package::*};
use shinki_emu_build::{BuildAsset, CoreBuildManifest};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::LazyLock,
};
struct Fixture {
    _dir: tempfile::TempDir,
    library: PathBuf,
    dependency: PathBuf,
    target: String,
}
fn fixture() -> &'static Fixture {
    static F: LazyLock<Fixture> = LazyLock::new(|| {
        let dir = tempfile::tempdir().unwrap();
        let version = Command::new("rustc").arg("-vV").output().unwrap();
        assert!(version.status.success());
        let target = String::from_utf8(version.stdout)
            .unwrap()
            .lines()
            .find_map(|s| s.strip_prefix("host: "))
            .unwrap()
            .to_owned();
        let (prefix, suffix) = if target.contains("windows") {
            ("", "dll")
        } else if target.contains("apple") {
            ("lib", "dylib")
        } else {
            ("lib", "so")
        };
        let dep_source = dir.path().join("dependency.rs");
        fs::write(&dep_source,"#![no_std]\n#[panic_handler] fn panic(_: &core::panic::PanicInfo)->!{loop{}}\n#[unsafe(no_mangle)] pub extern \"C\" fn dep_value()->u32 {42}\n").unwrap();
        let dependency = dir
            .path()
            .join(format!("{prefix}shinki_fixture_dep.{suffix}"));
        let mut cmd = Command::new("rustc");
        cmd.arg(&dep_source)
            .args([
                "--edition=2024",
                "--crate-name",
                "shinki_fixture_dep",
                "--crate-type",
                "cdylib",
                "-C",
                "panic=abort",
                "-o",
            ])
            .arg(&dependency);
        if target.contains("windows") {
            cmd.args(["-C", "link-arg=/NOENTRY"]);
        }
        let status = cmd.status().unwrap();
        assert!(status.success());
        if target.contains("windows") {
            shinki_emu_build::generate_import_library(&dependency, dir.path(), &target).unwrap();
        }
        let source = dir.path().join("core.rs");
        fs::write(&source,"#![no_std]\n#[panic_handler] fn panic(_: &core::panic::PanicInfo)->!{loop{}}\n#[link(name=\"shinki_fixture_dep\")] unsafe extern \"C\" {fn dep_value()->u32;}\n#[unsafe(no_mangle)] pub unsafe extern \"C\" fn shinki_emu_query()->u32 {unsafe {dep_value()}}\n").unwrap();
        let library = dir
            .path()
            .join(format!("{prefix}shinki_emu_fixture.{suffix}"));
        let mut cmd = Command::new("rustc");
        cmd.arg(source)
            .args([
                "--edition=2024",
                "--crate-type",
                "cdylib",
                "-C",
                "panic=abort",
            ])
            .arg(format!("-Lnative={}", dir.path().display()))
            .arg("-o")
            .arg(&library);
        if target.contains("windows") {
            cmd.args(["-C", "link-arg=/NOENTRY"]);
        }
        let status = cmd.status().unwrap();
        assert!(status.success());
        Fixture {
            _dir: dir,
            library,
            dependency,
            target,
        }
    });
    &F
}
fn result(temp: &Path) -> BuildResult {
    let f = fixture();
    let license = temp.join("LICENSE");
    fs::write(&license, "Public synthetic test fixture, CC0-1.0\n").unwrap();
    BuildResult {
        component: "fixture".into(),
        target: f.target.clone(),
        profile: "release".into(),
        product_version: None,
        library: f.library.clone(),
        core_build: Some(CoreBuildManifest {
            schema: shinki_emu_build::SCHEMA.into(),
            plugin_id: "fixture".into(),
            target: f.target.clone(),
            profile: "release".into(),
            abi: shinki_emu_build::ABI.into(),
            artifact_kind: "rust-cdylib".into(),
            native_library: None,
            resources: vec![],
            licenses: vec![BuildAsset {
                source: license,
                dest: "licenses/fixture/LICENSE".into(),
            }],
        }),
        embedded_core_builds: vec![],
        source_commit: "fixture".into(),
        source_dirty: false,
        toolchain: "rustc".into(),
        features: vec![],
        target_dir: temp.to_owned(),
    }
}

#[test]
fn manager_package_identity_cannot_relabel_old_or_unversioned_builds() {
    let temp = tempfile::tempdir().unwrap();
    let mut receipt = result(temp.path());
    receipt.component = "manager".into();
    receipt.core_build = None;
    assert!(receipt.require_product_version("0.1.0-beta.8").is_err());
    receipt.product_version = Some("0.1.0-beta.6".into());
    assert!(receipt.require_product_version("0.1.0-beta.8").is_err());
    receipt.product_version = Some("0.1.0-beta.8".into());
    receipt.require_product_version("0.1.0-beta.8").unwrap();
    let path = temp.path().join("build.json");
    write_provenance(&receipt, &path).unwrap();
    let packaged: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(packaged["product_version"], "0.1.0-beta.8");
    receipt.product_version = Some("0.1.0-beta.9".into());
    assert!(receipt.require_product_version("0.1.0-beta.8").is_err());
}
#[test]
fn paths_reject_traversal_ads_devices_and_collisions() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    fs::write(&source, "original").unwrap();
    for bad in [
        "../escape",
        "/absolute",
        "x//y",
        "x/./y",
        "x:y",
        "x\\y",
        "CON.txt",
        "LPT1",
        "name.",
        "name ",
        " leading",
        "CON .txt",
        "LPT1 .txt",
        "name\tfile",
        "name\nfile",
        "x/../../a",
    ] {
        assert!(portable(bad).is_err(), "{bad}");
    }
    let out = temp.path().join("out");
    assert!(
        atomic_directory(&out, |stage| copy_entries(
            &[
                (source.clone(), "File".into()),
                (source.clone(), "file".into())
            ],
            stage
        ))
        .is_err()
    );
    assert!(!out.exists());
    assert_eq!(fs::read_to_string(source).unwrap(), "original");
}
#[test]
fn portable_resource_punctuation_survives_packaging() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let bytes = b"<assembly>runtime resource</assembly>";
    fs::write(&source, bytes).unwrap();
    let names = [
        "lib/python3.12/site-packages/setuptools/command/launcher manifest.xml",
        "lib/python3.12/site-packages/setuptools/script (dev).tmpl",
        "Resources/data/file #1 + (copy) [final].txt",
    ];
    let stage = temp.path().join("stage");
    fs::create_dir(&stage).unwrap();
    let entries = names.map(|name| (source.clone(), name.to_owned()));
    copy_entries(&entries, &stage).unwrap();
    let archive = temp.path().join("resources.zip");
    zip_entries(&tree(&stage).unwrap(), &archive).unwrap();
    let extracted = temp.path().join("extracted");
    extract_zip(&archive, &extracted).unwrap();
    for name in names {
        assert_eq!(fs::read(extracted.join(name)).unwrap(), bytes);
    }
}
#[test]
fn atomic_publication_preserves_existing_and_racing_outputs_and_cleans_staging() {
    let t = tempfile::tempdir().unwrap();
    let out = t.path().join("out");
    fs::create_dir(&out).unwrap();
    fs::write(out.join("user"), "keep").unwrap();
    assert!(atomic_directory(&out, |_| panic!("existing output must not stage")).is_err());
    assert_eq!(fs::read_to_string(out.join("user")).unwrap(), "keep");
    let race = t.path().join("race");
    assert!(
        atomic_directory(&race, |stage| {
            fs::write(stage.join("ours"), "ours")?;
            fs::create_dir(&race)?;
            fs::write(race.join("theirs"), "theirs")?;
            Ok(())
        })
        .is_err()
    );
    assert_eq!(fs::read_to_string(race.join("theirs")).unwrap(), "theirs");
    assert!(!race.join("ours").exists());
    let failed = t.path().join("failed");
    assert!(
        atomic_directory(&failed, |stage| {
            fs::write(stage.join("partial"), "partial")?;
            anyhow::bail!("intentional failure")
        })
        .is_err()
    );
    assert!(!failed.exists());
    assert!(!fs::read_dir(t.path()).unwrap().any(|e| {
        e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".shinki-stage-")
    }));
}
#[test]
fn real_binary_dependency_arch_and_export_validation() {
    let f = fixture();
    let info = binary::inspect(&f.library, &f.target, true).unwrap();
    assert!(
        info.dependencies
            .iter()
            .any(|d| d.contains("shinki_fixture_dep"))
    );
    assert!(binary::closure(&f.library, &[], &f.target).is_err());
    let closure = binary::closure(&f.library, &[f._dir.path().to_owned()], &f.target).unwrap();
    assert!(closure.contains(&f.dependency));
    assert!(binary::inspect(&f.dependency, &f.target, true).is_err());
    let wrong = if f.target.starts_with("x86_64") {
        f.target.replacen("x86_64", "aarch64", 1)
    } else {
        f.target.replacen("aarch64", "x86_64", 1)
    };
    assert!(binary::inspect(&f.library, &wrong, true).is_err());
}
#[cfg(windows)]
#[test]
fn delay_imports_participate_in_dependency_closure() {
    let f = fixture();
    let temp = tempfile::tempdir().unwrap();
    let library = temp.path().join("shinki_emu_delayed.dll");
    let status = Command::new("rustc")
        .arg(f._dir.path().join("core.rs"))
        .args([
            "--crate-type",
            "cdylib",
            "-C",
            "panic=abort",
            "-C",
            "link-arg=/NOENTRY",
            "-C",
            "link-arg=/DELAYLOAD:shinki_fixture_dep.dll",
            "-C",
            "link-arg=delayimp.lib",
            "-C",
            "link-arg=kernel32.lib",
        ])
        .arg(format!("-Lnative={}", f._dir.path().display()))
        .arg("-o")
        .arg(&library)
        .status()
        .unwrap();
    assert!(status.success());
    let bytes = fs::read(&library).unwrap();
    let file = object::read::pe::PeFile64::parse(bytes.as_slice()).unwrap();
    assert!(
        file.data_directories()
            .delay_load_import_table(file.data(), &file.section_table())
            .unwrap()
            .is_some()
    );
    assert!(binary::closure(&library, &[], &f.target).is_err());
    assert!(
        binary::closure(&library, &[f._dir.path().to_owned()], &f.target)
            .unwrap()
            .contains(&f.dependency)
    );
}

#[test]
fn unsafe_zip_paths_cannot_escape_or_publish_partial_output() {
    use std::io::Write;
    let temp = tempfile::tempdir().unwrap();
    let archive = temp.path().join("unsafe.zip");
    let mut zip = zip::ZipWriter::new(fs::File::create(&archive).unwrap());
    zip.start_file("../outside", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(b"untrusted archive").unwrap();
    zip.finish().unwrap();
    let out = temp.path().join("out");
    assert!(extract_zip(&archive, &out).is_err());
    assert!(!out.exists());
    assert!(!temp.path().join("outside").exists());
}
#[test]
fn staged_disk_set_hashes_and_deterministic_archive_match() {
    let t = tempfile::tempdir().unwrap();
    let r = result(t.path());
    let f = fixture();
    let dir = stage_core(
        &r,
        "1.0.0",
        None,
        &t.path().join("out"),
        &[f._dir.path().to_owned()],
    )
    .unwrap();
    let m = verify_core(&dir).unwrap();
    let actual = tree(&dir).unwrap();
    assert_eq!(actual.len(), m.files.len() + 1);
    for record in &m.files {
        assert_eq!(sha256(&dir.join(&record.path)).unwrap(), record.sha256);
    }
    let one = t.path().join("one.zip");
    let two = t.path().join("two.zip");
    zip_entries(&actual, &one).unwrap();
    zip_entries(&actual, &two).unwrap();
    assert_eq!(fs::read(&one).unwrap(), fs::read(&two).unwrap());
    let original = fs::read(&one).unwrap();
    assert!(zip_entries(&actual, &one).is_err());
    assert_eq!(fs::read(&one).unwrap(), original);
    fs::write(dir.join(&m.library), "corrupt").unwrap();
    assert!(verify_core(&dir).is_err());
}
#[cfg(unix)]
#[test]
fn desktop_archive_preserves_executables_without_executable_notices() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let executable = temp.path().join("manager");
    let notice = temp.path().join("NOTICE");
    fs::write(&executable, b"public synthetic executable").unwrap();
    fs::write(&notice, b"public synthetic notice").unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o751)).unwrap();
    fs::set_permissions(&notice, fs::Permissions::from_mode(0o640)).unwrap();
    let archive = temp.path().join("package.zip");
    zip_entries(
        &[(executable, "manager".into()), (notice, "NOTICE".into())],
        &archive,
    )
    .unwrap();
    let mut zip = zip::ZipArchive::new(fs::File::open(archive).unwrap()).unwrap();
    assert_eq!(
        zip.by_name("manager").unwrap().unix_mode().unwrap() & 0o777,
        0o755
    );
    assert_eq!(
        zip.by_name("NOTICE").unwrap().unix_mode().unwrap() & 0o777,
        0o644
    );
}
#[test]
fn unlisted_duplicate_and_missing_licenses_fail_without_output() {
    let t = tempfile::tempdir().unwrap();
    let f = fixture();
    let mut r = result(t.path());
    r.core_build.as_mut().unwrap().licenses.clear();
    assert!(
        stage_core(
            &r,
            "1.0.0",
            None,
            &t.path().join("no-license"),
            &[f._dir.path().to_owned()]
        )
        .is_err()
    );
    assert!(!t.path().join("no-license").exists());
    let r = result(t.path());
    let dir = stage_core(
        &r,
        "1.0.0",
        None,
        &t.path().join("out"),
        &[f._dir.path().to_owned()],
    )
    .unwrap();
    let mut m = verify_core(&dir).unwrap();
    fs::write(dir.join("unlisted"), "hidden").unwrap();
    assert!(verify_core(&dir).is_err());
    fs::remove_file(dir.join("unlisted")).unwrap();
    let first = &m.files[0];
    m.files.push(FileRecord {
        path: first.path.to_ascii_uppercase(),
        sha256: first.sha256.clone(),
    });
    fs::write(dir.join("core.json"), serde_json::to_vec(&m).unwrap()).unwrap();
    assert!(verify_core(&dir).is_err());
}
#[cfg(windows)]
#[test]
fn reparse_input_is_rejected() {
    use std::os::windows::fs::symlink_dir;
    let t = tempfile::tempdir().unwrap();
    let real = t.path().join("real");
    fs::create_dir(&real).unwrap();
    fs::write(real.join("file"), "data").unwrap();
    let link = t.path().join("link");
    if symlink_dir(&real, &link).is_err() {
        let status = Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(&link)
            .arg(&real)
            .status()
            .unwrap();
        assert!(
            status.success(),
            "junction creation is required to exercise reparse rejection"
        );
    }
    assert!(tree(&link).is_err());
    assert!(plain(&link.join("file")).is_err());
    assert!(
        zip_entries(
            &[(real.join("file"), "file".into())],
            &link.join("package.zip")
        )
        .is_err()
    );
    assert!(!real.join("package.zip").exists());
}
#[cfg(unix)]
#[test]
fn linked_parent_and_files_are_rejected() {
    use std::os::unix::fs::symlink;
    let t = tempfile::tempdir().unwrap();
    let real = t.path().join("real");
    fs::create_dir(&real).unwrap();
    fs::write(real.join("file"), "data").unwrap();
    let link = t.path().join("link");
    symlink(&real, &link).unwrap();
    assert!(tree(&link).is_err());
    assert!(plain(&link.join("file")).is_err());
    assert!(
        zip_entries(
            &[(real.join("file"), "file".into())],
            &link.join("package.zip")
        )
        .is_err()
    );
    assert!(!real.join("package.zip").exists());
}
#[test]
fn matrix_covers_each_core_and_target_without_accepting_unknown_keys() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let inv = Inventory::load(root).unwrap();
    let rows = inv.rows().unwrap();
    for core in &inv.cores {
        for target in &inv.targets {
            assert!(
                rows.iter()
                    .any(|r| r.component == core.id && r.target == target.triple)
            );
        }
    }
    assert!(inv.blocked("unknown", &inv.targets[0].triple).is_err());
    assert!(inv.blocked("manager", "unknown").is_err());
}

#[test]
fn macho_identity_is_not_a_runtime_dependency() {
    let temp = tempfile::tempdir().unwrap();
    let name = b"@rpath/libexample.dylib\0";
    let size = (24 + name.len() + 7) & !7;
    for (command, expected_dependencies) in [
        (object::macho::LC_ID_DYLIB, 0),
        (object::macho::LC_LOAD_DYLIB, 1),
    ] {
        let mut bytes = Vec::new();
        for word in [
            object::macho::MH_MAGIC_64,
            object::macho::CPU_TYPE_ARM64.0,
            0,
            object::macho::MH_DYLIB.0,
            1,
            size as u32,
            0,
            0,
            command.0,
            size as u32,
            24,
            0,
            0,
            0,
        ] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes.extend_from_slice(name);
        bytes.resize(32 + size, 0);
        let path = temp.path().join("libexample.dylib");
        fs::write(&path, bytes).unwrap();
        let info = binary::inspect(&path, "aarch64-apple-darwin", false).unwrap();
        assert_eq!(info.dependencies.len(), expected_dependencies);
    }
}

#[test]
fn apple_subtitle_installs_isolate_architectures_and_platforms() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let inventory = Inventory::load(root).unwrap();
    let mut installs = std::collections::BTreeSet::new();
    for target in inventory
        .targets
        .iter()
        .filter(|t| crate::apple::supported(&t.triple))
    {
        let install = crate::dependencies::subtitle_install(&inventory, &target.triple).unwrap();
        assert!(installs.insert(install));
        let needs = crate::dependencies::needs(&inventory, &target.triple, "rfvp").unwrap();
        assert!(needs.subtitle);
        assert!(!needs.gstreamer);
        let manager = crate::dependencies::needs(&inventory, &target.triple, "manager").unwrap();
        assert_eq!(manager.subtitle, target.platform == "ios");
    }
}

#[test]
fn corresponding_sources_copy_downloads_matching_installed_sha512() {
    use sha2::Digest;
    let temp = tempfile::tempdir().unwrap();
    let downloads = temp.path().join("downloads");
    fs::create_dir(&downloads).unwrap();
    let upstream = downloads.join("source.tar.gz");
    fs::write(&upstream, b"upstream source archive").unwrap();
    let required = [hex::encode(sha2::Sha512::digest(
        fs::read(&upstream).unwrap(),
    ))]
    .into_iter()
    .collect();
    let output = temp.path().join("output");
    crate::sources::collect_archives(&downloads, &output, required).unwrap();
    assert_eq!(
        fs::read(output.join("source.tar.gz")).unwrap(),
        fs::read(&upstream).unwrap()
    );
    let required = [hex::encode(sha2::Sha512::digest(
        fs::read(&upstream).unwrap(),
    ))]
    .into_iter()
    .collect::<std::collections::BTreeSet<_>>();
    fs::write(&upstream, b"same filename, changed contents").unwrap();
    assert!(
        crate::sources::collect_archives(
            &downloads,
            &temp.path().join("changed"),
            required.clone()
        )
        .is_err()
    );
    fs::remove_file(&upstream).unwrap();
    assert!(
        crate::sources::collect_archives(&downloads, &temp.path().join("missing"), required)
            .is_err()
    );
}

#[test]
fn native_packages_include_dynamically_loaded_modules_and_validate_resource_binaries() {
    let temp = tempfile::tempdir().unwrap();
    let fixture = fixture();
    let native = temp.path().join("native");
    let resources = temp.path().join("resources");
    fs::create_dir(&native).unwrap();
    fs::create_dir(&resources).unwrap();
    let library = native.join(fixture.library.file_name().unwrap());
    fs::copy(&fixture.library, &library).unwrap();
    fs::copy(
        &fixture.dependency,
        native.join(fixture.dependency.file_name().unwrap()),
    )
    .unwrap();
    let optional = fixture
        .dependency
        .file_name()
        .unwrap()
        .to_string_lossy()
        .replace("_dep", "_optional");
    fs::copy(&fixture.dependency, native.join(&optional)).unwrap();
    let extension = if cfg!(windows) {
        "extension.pyd"
    } else {
        "extension.so"
    };
    fs::copy(&fixture.dependency, resources.join(extension)).unwrap();
    let mut result = result(temp.path());
    result.library = library.clone();
    let manifest = result.core_build.as_mut().unwrap();
    manifest.artifact_kind = "native-shared".into();
    manifest.native_library = Some(library);
    manifest.resources.push(BuildAsset {
        source: resources.clone(),
        dest: "Resources".into(),
    });
    let output = temp.path().join("complete");
    let core = stage_core(
        &result,
        "1.0.0",
        None,
        &output,
        std::slice::from_ref(&native),
    )
    .unwrap();
    let manifest = verify_core(&core).unwrap();
    assert!(manifest.files.iter().any(|file| file.path == optional));
    assert!(
        manifest
            .files
            .iter()
            .any(|file| file.path == format!("Resources/{extension}"))
    );
    fs::write(resources.join(extension), b"invalid native extension").unwrap();
    let rejected = temp.path().join("rejected");
    assert!(stage_core(&result, "1.0.0", None, &rejected, &[native]).is_err());
    assert!(!rejected.exists());
}

#[test]
fn output_nested_in_resource_inputs_is_rejected_without_modifying_sources() {
    let temp = tempfile::tempdir().unwrap();
    let resources = temp.path().join("resources");
    fs::create_dir(&resources).unwrap();
    let source = resources.join("public.txt");
    fs::write(&source, b"original public resource").unwrap();
    let mut result = result(temp.path());
    result
        .core_build
        .as_mut()
        .unwrap()
        .resources
        .push(BuildAsset {
            source: resources.clone(),
            dest: "Resources".into(),
        });
    let output = resources.join("nested").join("output");
    let fixture = fixture();
    assert!(
        stage_core(
            &result,
            "1.0.0",
            None,
            &output,
            &[fixture._dir.path().to_owned()]
        )
        .is_err()
    );
    assert!(!resources.join("nested").exists());
    assert_eq!(fs::read(&source).unwrap(), b"original public resource");
}

#[test]
fn external_package_receipt_matches_exact_upstream_and_adapter_pins() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let inventory = Inventory::load(root).unwrap();
    let source = inventory.core("rfvp").unwrap().source.as_ref().unwrap();
    let identity = source.identity();
    let temp = tempfile::tempdir().unwrap();
    let fixture = fixture();
    let mut receipt = result(temp.path());
    receipt.source_commit = source.rev.clone();
    let output = temp
        .path()
        .join(".work/release")
        .join(format!("core-rfvp-{}", identity.version()))
        .join("x86_64-pc-windows-msvc/package");
    let staged = stage_core(
        &receipt,
        &identity.version(),
        Some(&identity),
        &output,
        &[fixture._dir.path().to_owned()],
    )
    .unwrap();
    let packaged = verify_core(&staged).unwrap();
    assert_eq!(packaged.identity.as_ref(), Some(&identity));
    assert_eq!(packaged.version, identity.version());
    assert_eq!(
        packaged.identity.as_ref().unwrap().adapter.commit,
        receipt.source_commit
    );
    let archive = output.parent().unwrap().join(format!(
        "shinki-emu-core-rfvp-{}-windows-x86_64.zip",
        identity.version()
    ));
    zip_entries(&tree(&staged).unwrap(), &archive).unwrap();
    let archive_hash = sha256(&archive).unwrap();
    assert!(zip_entries(&tree(&staged).unwrap(), &archive).is_err());
    assert_eq!(sha256(&archive).unwrap(), archive_hash);
    let extracted = temp
        .path()
        .join("extracted")
        .join(staged.file_name().unwrap());
    extract_zip(&archive, &extracted).unwrap();
    let extracted_manifest = verify_core(&extracted).unwrap();
    assert_eq!(extracted_manifest.identity.as_ref(), Some(&identity));
    assert_eq!(
        sha256(&extracted.join(&extracted_manifest.library)).unwrap(),
        sha256(&receipt.library).unwrap()
    );
    let mut changed = identity.clone();
    changed.adapter.commit = "f".repeat(40);
    assert!(
        stage_core(
            &receipt,
            &changed.version(),
            Some(&changed),
            &temp.path().join("wrong-pin"),
            &[]
        )
        .is_err()
    );
    assert!(
        stage_core(
            &receipt,
            "0.1.0-beta.5",
            Some(&identity),
            &temp.path().join("false-version"),
            &[]
        )
        .is_err()
    );
}

// Public synthetic loader metadata, generated directly from the ELF64 format.
fn sectionless_elf(needed: &str, rpath: &str) -> Vec<u8> {
    let strings = format!("\0{needed}\0{rpath}\0").into_bytes();
    let size = 256 + strings.len();
    let mut bytes = vec![0u8; size];
    bytes[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
    bytes[16..18].copy_from_slice(&3u16.to_le_bytes());
    bytes[18..20].copy_from_slice(&62u16.to_le_bytes());
    bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
    bytes[32..40].copy_from_slice(&64u64.to_le_bytes());
    bytes[52..54].copy_from_slice(&64u16.to_le_bytes());
    bytes[54..56].copy_from_slice(&56u16.to_le_bytes());
    bytes[56..58].copy_from_slice(&2u16.to_le_bytes());
    bytes[64..68].copy_from_slice(&1u32.to_le_bytes());
    bytes[68..72].copy_from_slice(&4u32.to_le_bytes());
    bytes[96..104].copy_from_slice(&(size as u64).to_le_bytes());
    bytes[104..112].copy_from_slice(&(size as u64).to_le_bytes());
    bytes[112..120].copy_from_slice(&4096u64.to_le_bytes());
    bytes[120..124].copy_from_slice(&2u32.to_le_bytes());
    bytes[124..128].copy_from_slice(&4u32.to_le_bytes());
    bytes[128..136].copy_from_slice(&176u64.to_le_bytes());
    bytes[136..144].copy_from_slice(&176u64.to_le_bytes());
    bytes[144..152].copy_from_slice(&176u64.to_le_bytes());
    bytes[152..160].copy_from_slice(&80u64.to_le_bytes());
    bytes[160..168].copy_from_slice(&80u64.to_le_bytes());
    bytes[168..176].copy_from_slice(&8u64.to_le_bytes());
    for (index, (tag, value)) in [
        (5u64, 256u64),
        (10, strings.len() as u64),
        (1, 1),
        (29, (needed.len() + 2) as u64),
        (0, 0),
    ]
    .into_iter()
    .enumerate()
    {
        let offset = 176 + index * 16;
        bytes[offset..offset + 8].copy_from_slice(&tag.to_le_bytes());
        bytes[offset + 8..offset + 16].copy_from_slice(&value.to_le_bytes());
    }
    bytes[256..].copy_from_slice(&strings);
    bytes
}

#[test]
fn elf_dynamic_segments_resolve_needed_and_runpath_without_section_headers() {
    let temp = tempfile::tempdir().unwrap();
    let library = temp.path().join("libsynthetic.so");
    fs::write(&library, sectionless_elf("libmissing.so", "$ORIGIN")).unwrap();
    let info = binary::inspect(&library, "x86_64-unknown-linux-gnu", false).unwrap();
    assert!(info.dependencies.contains("libmissing.so"));
    assert!(info.rpaths.iter().any(|path| path == "$ORIGIN"));
    assert!(binary::closure(&library, &[], "x86_64-unknown-linux-gnu").is_err());
    fs::write(
        &library,
        sectionless_elf("libmissing.so", "/developer/private"),
    )
    .unwrap();
    assert!(binary::inspect(&library, "x86_64-unknown-linux-gnu", false).is_err());
}

fn synthetic_macho(dependency: &[u8], rpath: &str) -> Vec<u8> {
    let library_size = (24 + dependency.len() + 7) & !7;
    let rpath_size = (12 + rpath.len() + 1 + 7) & !7;
    let mut bytes = vec![0u8; 32 + library_size + rpath_size];
    bytes[0..4].copy_from_slice(&0xfeedfacfu32.to_le_bytes());
    bytes[4..8].copy_from_slice(&0x01000007u32.to_le_bytes());
    bytes[8..12].copy_from_slice(&3u32.to_le_bytes());
    bytes[12..16].copy_from_slice(&6u32.to_le_bytes());
    bytes[16..20].copy_from_slice(&2u32.to_le_bytes());
    bytes[20..24].copy_from_slice(&((library_size + rpath_size) as u32).to_le_bytes());
    bytes[32..36].copy_from_slice(&0xcu32.to_le_bytes());
    bytes[36..40].copy_from_slice(&(library_size as u32).to_le_bytes());
    bytes[40..44].copy_from_slice(&24u32.to_le_bytes());
    bytes[56..56 + dependency.len()].copy_from_slice(dependency);
    let offset = 32 + library_size;
    bytes[offset..offset + 4].copy_from_slice(&0x8000001cu32.to_le_bytes());
    bytes[offset + 4..offset + 8].copy_from_slice(&(rpath_size as u32).to_le_bytes());
    bytes[offset + 8..offset + 12].copy_from_slice(&12u32.to_le_bytes());
    bytes[offset + 12..offset + 12 + rpath.len()].copy_from_slice(rpath.as_bytes());
    bytes
}

#[test]
fn macho_load_commands_require_relocatable_rpaths_and_a_complete_closure() {
    let temp = tempfile::tempdir().unwrap();
    let library = temp.path().join("libsynthetic.dylib");
    fs::write(
        &library,
        synthetic_macho(b"@rpath/libmissing.dylib\0", "@loader_path"),
    )
    .unwrap();
    let info = binary::inspect(&library, "x86_64-apple-darwin", false).unwrap();
    assert!(info.dependencies.contains("@rpath/libmissing.dylib"));
    assert!(info.rpaths.iter().any(|path| path == "@loader_path"));
    assert!(binary::closure(&library, &[], "x86_64-apple-darwin").is_err());
    fs::write(
        &library,
        synthetic_macho(b"@rpath/libmissing.dylib\0", "/developer/private"),
    )
    .unwrap();
    assert!(binary::inspect(&library, "x86_64-apple-darwin", false).is_err());
}

#[test]
fn macho_runtime_bundles_keep_architecture_dependency_and_query_checks() {
    let temp = tempfile::tempdir().unwrap();
    let module = temp.path().join("display.so");
    let mut bytes = synthetic_macho(b"@rpath/libpython3.12.dylib\0", "@loader_path");
    bytes[12..16].copy_from_slice(&object::macho::MH_BUNDLE.0.to_le_bytes());
    fs::write(&module, &bytes).unwrap();
    let info = binary::inspect(&module, "x86_64-apple-darwin", false).unwrap();
    assert!(info.dependencies.contains("@rpath/libpython3.12.dylib"));
    assert!(binary::inspect(&module, "aarch64-apple-darwin", false).is_err());
    assert!(binary::inspect(&module, "x86_64-apple-darwin", true).is_err());
    assert!(binary::closure(&module, &[], "x86_64-apple-darwin").is_err());
    let dependency = temp.path().join("libpython3.12.dylib");
    fs::write(
        &dependency,
        synthetic_macho(b"/usr/lib/libSystem.B.dylib\0", "@loader_path"),
    )
    .unwrap();
    assert_eq!(
        binary::closure(&module, &[temp.path().into()], "x86_64-apple-darwin").unwrap(),
        vec![dependency]
    );
    bytes[12..16].copy_from_slice(&object::macho::MH_OBJECT.0.to_le_bytes());
    fs::write(&module, bytes).unwrap();
    assert!(binary::inspect(&module, "x86_64-apple-darwin", false).is_err());
}

#[test]
fn universal_macho_inspection_selects_and_bounds_checks_each_target_slice() {
    let temp = tempfile::tempdir().unwrap();
    let x86 = synthetic_macho(b"/usr/lib/libSystem.B.dylib\0", "@loader_path");
    let mut arm = x86.clone();
    arm[4..8].copy_from_slice(&object::macho::CPU_TYPE_ARM64.0.to_le_bytes());
    let mut universal = Vec::new();
    universal.extend_from_slice(&object::macho::FAT_MAGIC.to_be_bytes());
    universal.extend_from_slice(&2u32.to_be_bytes());
    for (cpu, subtype, offset, size) in [
        (object::macho::CPU_TYPE_X86_64.0, 3, 48, x86.len() as u32),
        (
            object::macho::CPU_TYPE_ARM64.0,
            0,
            48 + x86.len() as u32,
            arm.len() as u32,
        ),
    ] {
        for word in [cpu, subtype, offset, size, 0] {
            universal.extend_from_slice(&word.to_be_bytes());
        }
    }
    universal.extend_from_slice(&x86);
    universal.extend_from_slice(&arm);
    let library = temp.path().join("universal.dylib");
    fs::write(&library, &universal).unwrap();
    for target in ["x86_64-apple-darwin", "aarch64-apple-darwin"] {
        let info = binary::inspect(&library, target, false).unwrap();
        assert!(info.dependencies.contains("/usr/lib/libSystem.B.dylib"));
    }
    assert!(binary::target_slice(&universal, object::Architecture::I386).is_err());
    universal[40..44].copy_from_slice(&u32::MAX.to_be_bytes());
    fs::write(&library, universal).unwrap();
    assert!(binary::inspect(&library, "aarch64-apple-darwin", false).is_err());
}

#[test]
fn rust_license_closure_collects_files_and_spdx_and_rejects_missing_data() {
    use serde_json::json;
    let temp = tempfile::tempdir().unwrap();
    let mut packages = Vec::new();
    for (id, name, version, license, license_file) in [
        (
            "manager",
            "shinki-emu-test-core",
            "1.0.0",
            Some("MIT"),
            None,
        ),
        (
            "direct",
            "steamroom",
            "0.3.0",
            Some("MIT OR Apache-2.0"),
            Some("legal/custom.txt"),
        ),
        ("transitive", "steamroom-client", "0.3.0", None, None),
        (
            "build",
            "generator",
            "1.0.0",
            Some("(Apache-2.0 WITH LLVM-exception) AND (MIT OR BSD-3-Clause)"),
            None,
        ),
        ("old", "steamroom", "0.2.0", Some("MIT/Apache-2.0"), None),
        ("dev", "test-only", "1.0.0", None, None),
        ("other", "other-platform", "1.0.0", None, None),
    ] {
        let root = temp.path().join(id);
        fs::create_dir_all(root.join("legal")).unwrap();
        packages.push(json!({
            "id": id, "name": name, "version": version, "license": license,
            "license_file": license_file, "manifest_path": root.join("Cargo.toml")
        }));
    }
    fs::write(
        temp.path().join("direct/legal/custom.txt"),
        "custom license",
    )
    .unwrap();
    for filename in [
        "LICENSE-MIT",
        "COPYING.txt",
        "COPYRIGHT",
        "NOTICE",
        "license-apache",
    ] {
        fs::write(temp.path().join("direct").join(filename), filename).unwrap();
    }
    fs::write(temp.path().join("transitive/LICENSE"), "transitive license").unwrap();
    fs::create_dir(temp.path().join("transitive/LICENSES")).unwrap();
    fs::write(
        temp.path()
            .join("transitive/LICENSES/LicenseRef-Custom.txt"),
        "original custom license",
    )
    .unwrap();
    fs::write(temp.path().join("direct/README.md"), "not a license").unwrap();
    // Hidden residue such as Nix inputs under .direnv is not a notice for the crate.
    fs::create_dir_all(temp.path().join("transitive/.direnv/inputs")).unwrap();
    fs::write(
        temp.path().join("transitive/.direnv/inputs/LICENSE"),
        "unrelated input",
    )
    .unwrap();
    let fonts = temp.path().join("transitive/fonts");
    fs::create_dir(&fonts).unwrap();
    for name in [
        "UFL.txt",
        "OFL.txt",
        "emoji-mit-license.txt",
        "ExampleFont.txt",
    ] {
        fs::write(fonts.join(name), format!("provided font license: {name}\n")).unwrap();
    }
    fs::write(fonts.join("ExampleFont.ttf"), b"public font fixture").unwrap();
    fs::create_dir(temp.path().join("build/src")).unwrap();
    fs::write(
        temp.path().join("build/src/license.rs"),
        "pub struct License;\n",
    )
    .unwrap();
    let git_checkout = temp.path().join("git-workspace");
    fs::create_dir_all(git_checkout.join(".git")).unwrap();
    fs::create_dir_all(git_checkout.join("crates/binding")).unwrap();
    fs::write(
        git_checkout.join("LICENSE"),
        "MIT license text with the upstream workspace copyright\n",
    )
    .unwrap();
    packages.push(json!({
        "id": "binding", "name": "binding", "version": "1.0.0", "license": "MIT",
        "license_file": null, "source": "git+https://example.org/workspace#revision",
        "manifest_path": git_checkout.join("crates/binding/Cargo.toml")
    }));
    // A path crate inside a fork checkout (not the product workspace) inherits the
    // checkout-root license, as siglus_rs crates/* do.
    fs::create_dir_all(git_checkout.join("crates/fork-path")).unwrap();
    packages.push(json!({
        "id": "fork-path", "name": "fork-path", "version": "0.1.0", "license": null,
        "license_file": null, "source": null,
        "manifest_path": git_checkout.join("crates/fork-path/Cargo.toml")
    }));
    // A git source with the same name-version as another source (ruffle swf-0.2.2 vs
    // crates.io swf-0.2.2) keeps a separate notice group.
    let duplicate = temp.path().join("git-duplicate");
    fs::create_dir_all(&duplicate).unwrap();
    fs::write(duplicate.join("LICENSE-APACHE"), "git apache").unwrap();
    packages.push(json!({
        "id": "duplicate", "name": "steamroom", "version": "0.2.0", "license": "MIT OR Apache-2.0",
        "license_file": null,
        "source": "git+https://example.org/steamroom?tag=v0.2.0#a1ac4ce8647352ad03044031cad0454a636427b8",
        "manifest_path": duplicate.join("Cargo.toml")
    }));
    let dep = |pkg: &str, kind: Option<&str>| {
        json!({
            "pkg": pkg, "dep_kinds": [{"kind": kind, "target": null}]
        })
    };
    let mut metadata = json!({
        "packages": packages,
        "resolve": {"nodes": [
            {"id": "manager", "deps": [dep("direct", None), dep("duplicate", None), dep("build", Some("build")), dep("binding", None), dep("fork-path", None), dep("dev", Some("dev"))]},
            {"id": "direct", "deps": [dep("transitive", None), dep("old", None)]},
            {"id": "transitive", "deps": [dep("direct", None)]},
            {"id": "build", "deps": []},
            {"id": "old", "deps": []},
            {"id": "dev", "deps": []},
            {"id": "other", "deps": []},
            {"id": "binding", "deps": []},
            {"id": "fork-path", "deps": []},
            {"id": "duplicate", "deps": []}
        ]}
    });
    let scratch = temp.path().join("scratch");
    let entries =
        crate::rust_licenses::entries(&metadata, "shinki-emu-test-core", &scratch).unwrap();
    let output = temp.path().join("package");
    fs::create_dir(&output).unwrap();
    copy_entries(&entries, &output).unwrap();
    assert_eq!(
        fs::read_to_string(output.join("licenses/rust/steamroom-0.3.0/custom.txt")).unwrap(),
        "custom license"
    );
    for filename in [
        "LICENSE-MIT",
        "COPYING.txt",
        "COPYRIGHT",
        "NOTICE",
        "license-apache",
    ] {
        assert_eq!(
            fs::read_to_string(output.join("licenses/rust/steamroom-0.3.0").join(filename))
                .unwrap(),
            filename
        );
    }
    for file in ["package.json", "LICENSE-MIT", "LICENSE-Apache-2.0"] {
        assert!(
            output
                .join("licenses/rust/steamroom-0.2.0+path")
                .join(file)
                .is_file()
        );
    }
    assert_eq!(
        fs::read_to_string(
            output.join("licenses/rust/steamroom-0.2.0+git-a1ac4ce86473/LICENSE-APACHE")
        )
        .unwrap(),
        "git apache"
    );
    assert!(!output.join("licenses/rust/steamroom-0.2.0").exists());
    assert!(
        output
            .join("licenses/rust/steamroom-client-0.3.0/LICENSE")
            .is_file()
    );
    assert_eq!(
        fs::read_to_string(
            output.join("licenses/rust/steamroom-client-0.3.0/LICENSES/LicenseRef-Custom.txt")
        )
        .unwrap(),
        "original custom license"
    );
    for name in [
        "UFL.txt",
        "OFL.txt",
        "emoji-mit-license.txt",
        "ExampleFont.txt",
    ] {
        assert_eq!(
            sha256(
                &output
                    .join("licenses/rust/steamroom-client-0.3.0/fonts")
                    .join(name)
            )
            .unwrap(),
            sha256(&fonts.join(name)).unwrap()
        );
    }
    assert!(
        !output
            .join("licenses/rust/steamroom-client-0.3.0/fonts/ExampleFont.ttf")
            .exists()
    );
    assert!(
        !output
            .join("licenses/rust/generator-1.0.0/src/license.rs")
            .exists()
    );
    let record: serde_json::Value = serde_json::from_slice(
        &fs::read(output.join("licenses/rust/generator-1.0.0/package.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(record["text_origin"], "canonical-spdx");
    assert!(record["canonical_text_source"]["sha256"].is_string());
    for id in ["Apache-2.0", "LLVM-exception", "MIT", "BSD-3-Clause"] {
        let path = output.join(format!("licenses/rust/generator-1.0.0/LICENSE-{id}"));
        assert!(fs::metadata(path).unwrap().len() > 100);
    }
    assert!(
        !output
            .join("licenses/rust/steamroom-0.3.0/LICENSE-Apache-2.0")
            .exists()
    );
    assert_eq!(
        fs::read_dir(output.join("licenses/rust")).unwrap().count(),
        8
    );
    assert_eq!(
        sha256(&output.join("licenses/rust/fork-path-0.1.0/LICENSE")).unwrap(),
        sha256(&git_checkout.join("LICENSE")).unwrap()
    );
    assert!(
        !output
            .join("licenses/rust/steamroom-client-0.3.0/.direnv")
            .exists()
    );
    assert_eq!(
        sha256(&output.join("licenses/rust/binding-1.0.0/LICENSE")).unwrap(),
        sha256(&git_checkout.join("LICENSE")).unwrap()
    );
    assert!(
        !output
            .join("licenses/rust/binding-1.0.0/LICENSE-MIT")
            .exists()
    );
    assert!(
        !output
            .join("licenses/rust/steamroom-0.3.0/README.md")
            .exists()
    );
    metadata["packages"][3]["license"] = json!(null);
    let error = crate::rust_licenses::entries(&metadata, "shinki-emu-test-core", &scratch)
        .unwrap_err()
        .to_string();
    assert!(error.contains("generator-1.0.0"), "{error}");
    for unknown in [
        "Not-A-License",
        "MIT OR LicenseRef-Unknown",
        "Apache-2.0 WITH AdditionRef-Unknown",
    ] {
        metadata["packages"][3]["license"] = json!(unknown);
        assert!(
            crate::rust_licenses::entries(&metadata, "shinki-emu-test-core", &scratch).is_err(),
            "{unknown}"
        );
    }
    metadata["packages"][3]["license"] = json!("Apache-2.0");
    metadata["packages"][1]["license_file"] = json!("missing.txt");
    assert!(crate::rust_licenses::entries(&metadata, "shinki-emu-test-core", &scratch).is_err());
}

#[test]
fn embedded_font_notices_resolve_custom_terms_without_inventing_license_text() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("crate");
    fs::create_dir_all(root.join("fonts")).unwrap();
    fs::write(
        root.join("fonts/UFL.txt"),
        "original Ubuntu font terms and attribution",
    )
    .unwrap();
    fs::write(
        root.join("fonts/OFL.txt"),
        "original open font terms and attribution",
    )
    .unwrap();
    fs::write(
        root.join("fonts/Hack-Regular.txt"),
        "original named-font copyright",
    )
    .unwrap();
    fs::write(
        root.join("fonts/Hack-Regular.ttf"),
        "font data is not a notice",
    )
    .unwrap();
    let metadata = serde_json::json!({
        "packages": [{
            "id":"font-core", "name":"font-core", "version":"1.0.0",
            "manifest_path":root.join("Cargo.toml"),
            "license":"(MIT OR Apache-2.0) AND OFL-1.1 AND LicenseRef-UFL-1.0"
        }],
        "resolve":{"nodes":[{"id":"font-core", "deps":[]}]}
    });
    let entries =
        crate::rust_licenses::entries(&metadata, "font-core", &temp.path().join("scratch"))
            .unwrap();
    let output = temp.path().join("package");
    fs::create_dir(&output).unwrap();
    copy_entries(&entries, &output).unwrap();
    let notices = output.join("licenses/rust/font-core-1.0.0");
    assert_eq!(
        fs::read(notices.join("fonts/UFL.txt")).unwrap(),
        fs::read(root.join("fonts/UFL.txt")).unwrap()
    );
    assert!(notices.join("fonts/Hack-Regular.txt").is_file());
    assert!(notices.join("LICENSE-MIT").is_file());
    assert!(notices.join("LICENSE-Apache-2.0").is_file());
    assert!(!notices.join("LICENSE-OFL-1.1").exists());
    assert!(!notices.join("fonts/Hack-Regular.ttf").exists());
    fs::remove_file(root.join("fonts/UFL.txt")).unwrap();
    assert!(
        crate::rust_licenses::entries(&metadata, "font-core", &temp.path().join("missing"))
            .is_err()
    );
}

#[test]
fn apple_icon_names_remain_portable_without_allowing_traversal() {
    for name in [
        "ShinkiEMU.app/AppIcon83.5x83.5@2x.png",
        "ShinkiEMU.app/Info~ipad.plist",
        "Assets.xcassets/AppIcon.appiconset/Contents.json",
    ] {
        portable(name).unwrap();
    }
    for name in [
        "../AppIcon.png",
        "ShinkiEMU.app/../outside",
        "/AppIcon.png",
        "C:/AppIcon.png",
        "ShinkiEMU.app\\outside",
        "AppIcon\0.png",
        "AppIcon\n.png",
    ] {
        assert!(portable(name).is_err(), "{name:?}");
    }
}
