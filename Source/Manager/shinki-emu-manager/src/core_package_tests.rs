use super::*;
use zip::write::SimpleFileOptions;

const CORE_STEM: &str = "shinki_emu_fixture";

fn library_name(stem: &str) -> String {
    format!("{stem}.{}", std::env::consts::DLL_EXTENSION)
}

fn options() -> SimpleFileOptions {
    SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated)
}

fn manifest(plugin_id: &str, files: &[(String, &[u8])]) -> Vec<u8> {
    let mut files: Vec<CoreFile> = files
        .iter()
        .map(|(path, bytes)| CoreFile {
            path: path.clone(),
            sha256: sha256_hex(bytes),
        })
        .collect();
    files.sort_by(|left, right| left.path.cmp(&right.path));
    serde_json::to_vec(&CoreManifest {
        schema: CORE_PACKAGE_SCHEMA.into(),
        plugin_id: plugin_id.into(),
        library: library_name(CORE_STEM),
        version: "0.2.0".into(),
        identity: None,
        abi: INDEPENDENT_FAMILY_ABI_FINGERPRINT.into(),
        files,
    })
    .unwrap()
}

fn write_zip(path: &Path, entries: &[(String, &[u8])]) {
    let mut writer = zip::ZipWriter::new(fs::File::create(path).unwrap());
    for (name, bytes) in entries {
        writer.start_file(name.as_str(), options()).unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap();
}

fn valid_files() -> Vec<(String, &'static [u8])> {
    vec![
        (library_name(CORE_STEM), b"new library".as_slice()),
        (library_name("avcodec-62"), b"dependency"),
        ("licenses/LICENSE.txt".into(), b"license"),
    ]
}

fn valid_package(path: &Path, plugin_id: &str) {
    let files = valid_files();
    let manifest = manifest(plugin_id, &files);
    let mut entries = files;
    entries.push((CORE_MANIFEST.into(), &manifest));
    write_zip(path, &entries);
}

fn pending_is_empty(cores: &Path) -> bool {
    fs::read_dir(cores.join(PENDING)).map_or(true, |mut entries| entries.next().is_none())
}

#[test]
fn package_with_parent_directory_entry_is_rejected() {
    let root = tempfile::tempdir().unwrap();
    let cores = root.path().join("cores");
    let archive = root.path().join("core.zip");
    let mut files = valid_files();
    files.push(("../evil.dll".into(), b"escape"));
    let manifest = manifest("shinki.emu.fixture", &valid_files());
    files.push((CORE_MANIFEST.into(), &manifest));
    write_zip(&archive, &files);
    let error =
        stage_package(&archive, Some(("shinki.emu.fixture", "0.2.0", None)), &cores.join(PENDING)).unwrap_err();
    assert_eq!(error, "SHINKI_EMU_CORE_PACKAGE_PATH");
    assert!(!root.path().join("evil.dll").exists());
    assert!(pending_is_empty(&cores));
}

#[test]
fn package_with_absolute_symlink_or_duplicate_entry_is_rejected() {
    let root = tempfile::tempdir().unwrap();
    let pending = root.path().join("cores").join(PENDING);
    let archive = root.path().join("core.zip");
    write_zip(&archive, &[("/abs.dll".into(), b"x")]);
    assert_eq!(
        stage_package(&archive, None, &pending).unwrap_err(),
        "SHINKI_EMU_CORE_PACKAGE_PATH"
    );
    let mut writer = zip::ZipWriter::new(fs::File::create(&archive).unwrap());
    writer
        .add_symlink("link.dll", "../../outside", options())
        .unwrap();
    writer.finish().unwrap();
    assert_eq!(
        stage_package(&archive, None, &pending).unwrap_err(),
        "SHINKI_EMU_CORE_PACKAGE_PATH"
    );
    let mut files = valid_files();
    files.push((library_name("avcodec-62").to_ascii_uppercase(), b"other"));
    write_zip(&archive, &files);
    assert_eq!(
        stage_package(&archive, None, &pending).unwrap_err(),
        "SHINKI_EMU_CORE_PACKAGE_DUPLICATE"
    );
}

#[test]
fn package_with_hash_mismatch_or_foreign_plugin_is_rejected() {
    let root = tempfile::tempdir().unwrap();
    let cores = root.path().join("cores");
    let archive = root.path().join("core.zip");
    let manifest = manifest("shinki.emu.fixture", &valid_files());
    let mut files = valid_files();
    files[0].1 = b"tampered library";
    files.push((CORE_MANIFEST.into(), &manifest));
    write_zip(&archive, &files);
    let error =
        stage_package(&archive, Some(("shinki.emu.fixture", "0.2.0", None)), &cores.join(PENDING)).unwrap_err();
    assert!(
        error.starts_with("SHINKI_EMU_CORE_PACKAGE_HASH_MISMATCH"),
        "{error}"
    );
    assert!(pending_is_empty(&cores));

    valid_package(&archive, "shinki.emu.fixture");
    assert_eq!(
        stage_package(&archive, Some(("shinki.emu.other", "0.2.0", None)), &cores.join(PENDING)).unwrap_err(),
        "SHINKI_EMU_CORE_PACKAGE_PLUGIN_MISMATCH"
    );
    assert!(pending_is_empty(&cores));
}

#[test]
fn valid_package_is_staged_and_apply_pending_replaces_the_core_directory() {
    let root = tempfile::tempdir().unwrap();
    let cores = root.path().join("cores");
    let installed = cores.join(CORE_STEM);
    let library = library_name(CORE_STEM);
    fs::create_dir_all(&installed).unwrap();
    fs::write(installed.join(&library), b"old library").unwrap();
    let stale_dependency = library_name("stale-dependency");
    fs::write(installed.join(&stale_dependency), b"old").unwrap();
    let archive = root.path().join("core.zip");
    valid_package(&archive, "shinki.emu.fixture");

    let (stem, staged) =
        stage_package(&archive, Some(("shinki.emu.fixture", "0.2.0", None)), &cores.join(PENDING)).unwrap();
    assert_eq!(stem, CORE_STEM);
    assert_eq!(staged.version, "0.2.0");
    assert_eq!(
        pending_entries(&cores).unwrap(),
        BTreeMap::from([(stem.clone(), PendingKind::Install)])
    );
    assert_eq!(
        fs::read(cores.join(PENDING).join(&stem).join(&library)).unwrap(),
        b"new library"
    );

    assert!(apply_pending(&cores).is_empty());
    assert_eq!(fs::read(installed.join(&library)).unwrap(), b"new library");
    assert_eq!(
        fs::read(installed.join("licenses/LICENSE.txt")).unwrap(),
        b"license"
    );
    assert!(!installed.join(&stale_dependency).exists());
    assert_eq!(
        read_manifest(&installed).unwrap().plugin_id,
        "shinki.emu.fixture"
    );
    assert!(pending_is_empty(&cores));
}

#[test]
fn removal_marker_deletes_the_core_directory() {
    let root = tempfile::tempdir().unwrap();
    let cores = root.path().join("cores");
    fs::create_dir_all(cores.join("shinki_emu_y")).unwrap();
    fs::write(
        cores.join("shinki_emu_y").join(library_name("shinki_emu_y")),
        b"y",
    )
    .unwrap();
    mark_removal(&cores, "shinki_emu_y").unwrap();
    assert_eq!(
        pending_entries(&cores).unwrap(),
        BTreeMap::from([("shinki_emu_y".to_owned(), PendingKind::Remove)])
    );
    assert!(apply_pending(&cores).is_empty());
    assert!(!cores.join("shinki_emu_y").exists());
    assert!(pending_is_empty(&cores));
}

#[test]
fn legacy_top_level_library_is_migrated_into_its_directory() {
    let root = tempfile::tempdir().unwrap();
    let cores = root.path().join("cores");
    fs::create_dir_all(&cores).unwrap();
    let library = library_name("shinki_emu_x");
    let dependency = library_name("avcodec-62");
    fs::write(cores.join(&library), b"legacy").unwrap();
    fs::write(cores.join(&dependency), b"dependency").unwrap();
    assert!(apply_pending(&cores).is_empty());
    assert!(!cores.join(&library).exists());
    assert_eq!(
        fs::read(cores.join("shinki_emu_x").join(&library)).unwrap(),
        b"legacy"
    );
    let manifest = read_manifest(&cores.join("shinki_emu_x")).unwrap();
    assert_eq!(manifest.library, library);
    assert_eq!(manifest.files[0].sha256, sha256_hex(b"legacy"));
    assert!(cores.join(&dependency).exists());
}

#[test]
fn single_library_install_generates_a_manifest() {
    let root = tempfile::tempdir().unwrap();
    let cores = root.path().join("cores");
    let name = library_name(CORE_STEM);
    let library = root.path().join(&name);
    fs::write(&library, b"bare").unwrap();
    let stem = stage_library(&library, &cores.join(PENDING)).unwrap();
    let staged = cores.join(PENDING).join(&stem);
    assert_eq!(fs::read(staged.join(&name)).unwrap(), b"bare");
    let manifest = read_manifest(&staged).unwrap();
    assert_eq!(manifest.files.len(), 1);
    assert_eq!(manifest.files[0].sha256, sha256_hex(b"bare"));
    let dependency = root.path().join(library_name("avcodec-62"));
    fs::write(&dependency, b"dependency").unwrap();
    assert_eq!(
        stage_library(&dependency, &cores.join(PENDING)).unwrap_err(),
        "SHINKI_EMU_CORE_INSTALL_NAME"
    );
}

#[cfg(windows)]
#[test]
fn failed_install_restores_the_previous_directory() {
    use std::os::windows::fs::OpenOptionsExt;
    let root = tempfile::tempdir().unwrap();
    let cores = root.path().join("cores");
    let installed = cores.join(CORE_STEM);
    let library = library_name(CORE_STEM);
    fs::create_dir_all(&installed).unwrap();
    fs::write(installed.join(&library), b"installed").unwrap();
    let staged = cores.join(PENDING).join(CORE_STEM);
    fs::create_dir_all(&staged).unwrap();
    fs::write(staged.join(&library), b"replacement").unwrap();
    let locked = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(staged.join(&library))
        .unwrap();
    assert_eq!(apply_pending(&cores).len(), 1);
    assert_eq!(fs::read(installed.join(&library)).unwrap(), b"installed");
    drop(locked);
    assert_eq!(fs::read(staged.join(&library)).unwrap(), b"replacement");
    assert!(apply_pending(&cores).is_empty());
    assert_eq!(fs::read(installed.join(&library)).unwrap(), b"replacement");
    assert!(pending_is_empty(&cores));
}

#[test]
fn installs_branch_identity_and_rejects_catalog_receipt_disagreement() {
    let identity = shinki_emu_metadata::CoreIdentity {
        upstream: shinki_emu_metadata::UpstreamIdentity {
            repo: "https://github.com/example/engine".into(),
            ref_kind: shinki_emu_metadata::UpstreamRefKind::Branch,
            ref_name: "release/next".into(), commit: "a".repeat(40),
        },
        adapter: shinki_emu_metadata::AdapterIdentity {
            repo: "https://github.com/Project-Shinkirou/engine".into(),
            branch: "shinki".into(), commit: "b".repeat(40),
        },
        release_sequence: 4,
    };
    let files = valid_files();
    let mut receipt: CoreManifest = serde_json::from_slice(&manifest("engine", &files)).unwrap();
    receipt.version = identity.version();
    receipt.identity = Some(identity.clone());
    let root = tempfile::tempdir().unwrap();
    let archive = root.path().join("branch.zip");
    let encoded = serde_json::to_vec(&receipt).unwrap();
    let mut entries = files;
    entries.push((CORE_MANIFEST.into(), &encoded));
    write_zip(&archive, &entries);
    let pending = root.path().join("pending");
    let (stem, accepted) = stage_package(&archive, Some(("engine", &receipt.version, Some(&identity))), &pending).unwrap();
    assert_eq!(accepted.identity, Some(identity.clone()));
    assert_eq!(read_manifest(&pending.join(&stem)).unwrap().version, receipt.version);
    let mut changed = identity.clone();
    changed.adapter.commit = "c".repeat(40);
    changed.release_sequence += 1;
    assert!(stage_package(&archive, Some(("engine", &changed.version(), Some(&changed))), &pending).is_err());
    assert_eq!(read_manifest(&pending.join(stem)).unwrap().identity, Some(identity));
}

#[test]
fn published_beta_package_without_identity_remains_installable() {
    let root = tempfile::tempdir().unwrap();
    let archive = root.path().join("legacy.zip");
    let files = valid_files();
    let mut receipt: CoreManifest = serde_json::from_slice(&manifest("engine", &files)).unwrap();
    receipt.version = "0.1.0-beta.3".into();
    let encoded = serde_json::to_vec(&receipt).unwrap();
    let mut entries = files;
    entries.push((CORE_MANIFEST.into(), &encoded));
    write_zip(&archive, &entries);
    let (_, staged) = stage_package(&archive, Some(("engine", "0.1.0-beta.3", None)), &root.path().join(PENDING)).unwrap();
    assert_eq!(staged.version, "0.1.0-beta.3");
    assert!(staged.identity.is_none());
}
