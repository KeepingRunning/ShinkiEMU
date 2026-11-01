use anyhow::{Context, Result, anyhow, ensure};
use clap::{Subcommand, ValueEnum};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use shinki_emu_metadata::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Debug, Subcommand)]
pub enum MetadataCommand {
    Schema {
        #[arg(long, default_value = "Metadata/schema")]
        out: PathBuf,
    },
    Validate {
        #[arg(long, default_value = "Metadata")]
        root: PathBuf,
    },
    Build {
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value = "Metadata")]
        root: PathBuf,
    },
    /// Resolve an installable published core without downloading or publishing it.
    ResolveCore {
        #[arg(long)]
        plugin_id: String,
        #[arg(long)]
        platform: String,
        #[arg(long)]
        abi: String,
        #[arg(long, default_value = "Metadata")]
        root: PathBuf,
    },
    Intake {
        #[arg(long, value_enum)]
        kind: IntakeKind,
        #[arg(long)]
        event: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
}
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum IntakeKind {
    Compatibility,
    Patch,
}
pub fn run(cmd: MetadataCommand) -> Result<()> {
    match cmd {
        MetadataCommand::Schema { out } => {
            fs::create_dir_all(&out)?;
            for (name, schema) in metadata_json_schemas() {
                fs::write(out.join(name), pretty(&schema)?)?;
            }
            Ok(())
        }
        MetadataCommand::Validate { root } => {
            collect(&root, 0, true)?;
            Ok(())
        }
        MetadataCommand::ResolveCore {
            plugin_id,
            platform,
            abi,
            root,
        } => {
            let files = collect(&root, 0, true)?;
            let catalog = parse_core_catalog(&files["cores-v2.json"])?;
            let entry = catalog
                .latest(&plugin_id, &platform, &abi)
                .context("no published core matches plugin/platform/ABI")?;
            println!("{}", serde_json::to_string(entry)?);
            Ok(())
        }
        MetadataCommand::Build { out, root } => {
            let (commit, timestamp) = revision(&root)?;
            let mut files = collect(&root, timestamp, true)?;
            let index = MetadataIndex {
                schema: METADATA_INDEX_SCHEMA_VERSION.into(),
                generated_at_unix_ms: timestamp,
                source_commit: commit,
                files: files
                    .iter()
                    .map(|(p, b)| IndexedFile {
                        path: p.clone(),
                        sha256: digest(b),
                        size: b.len() as u64,
                    })
                    .collect(),
            };
            index.validate()?;
            files.insert("index.json".into(), pretty(&index)?);
            write_new_tree(&out, &files)
        }
        MetadataCommand::Intake { kind, event, out } => intake(kind, &event, &out),
    }
}
fn pretty(value: &impl Serialize) -> Result<Vec<u8>> {
    let mut b = serde_json::to_vec_pretty(value)?;
    b.push(b'\n');
    Ok(b)
}
fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
fn read(path: &Path) -> Result<Vec<u8>> {
    let m = fs::symlink_metadata(path).with_context(|| format!("reading {}", path.display()))?;
    ensure!(
        m.is_file() && !m.file_type().is_symlink() && !reparse(&m),
        "not a regular file: {}",
        path.display()
    );
    ensure!(
        m.len() <= MAX_METADATA_FILE_BYTES,
        "file exceeds 8 MiB: {}",
        path.display()
    );
    let b = fs::read(path)?;
    ensure!(
        b.len() as u64 <= MAX_METADATA_FILE_BYTES,
        "file grew past limit"
    );
    Ok(b)
}
#[cfg(windows)]
fn reparse(m: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    m.file_attributes() & 0x400 != 0
}
#[cfg(not(windows))]
fn reparse(_: &fs::Metadata) -> bool {
    false
}
fn decode<T: DeserializeOwned>(path: &Path) -> Result<T> {
    Ok(serde_json::from_value(parse_unique_json(&read(path)?)?)?)
}
fn walk(root: &Path, relative: &str, out: &mut Vec<String>) -> Result<()> {
    let dir = root.join(relative);
    if !dir.exists() {
        return Ok(());
    }
    let m = fs::symlink_metadata(&dir)?;
    ensure!(
        m.is_dir() && !m.file_type().is_symlink() && !reparse(&m),
        "unsafe directory: {}",
        dir.display()
    );
    for e in fs::read_dir(dir)? {
        let e = e?;
        let name = e
            .file_name()
            .into_string()
            .map_err(|_| anyhow!("non-Unicode path"))?;
        if name == ".gitkeep" {
            continue;
        }
        let p = if relative.is_empty() {
            name
        } else {
            format!("{relative}/{name}")
        };
        ensure!(is_safe_metadata_relative_path(&p), "unsafe path: {p}");
        let m = fs::symlink_metadata(e.path())?;
        ensure!(
            !m.file_type().is_symlink() && !reparse(&m),
            "links forbidden: {p}"
        );
        if m.is_dir() {
            walk(root, &p, out)?;
        } else {
            ensure!(m.is_file(), "special file: {p}");
            out.push(p);
        }
        ensure!(out.len() <= 4096, "too many metadata files");
    }
    out.sort();
    Ok(())
}
fn entries<T: DeserializeOwned>(root: &Path, dir: &str) -> Result<Vec<(String, T)>> {
    let mut paths = Vec::new();
    walk(root, dir, &mut paths)?;
    paths
        .into_iter()
        .map(|p| {
            ensure!(p.ends_with(".json"), "unexpected entry: {p}");
            let v = decode(&root.join(&p))?;
            Ok((p, v))
        })
        .collect()
}
/// Reject counter reuse/rollback before producing a new external release.
pub(crate) fn require_release_sequence(
    root: &Path,
    component: &str,
    identity: &CoreIdentity,
) -> Result<()> {
    let version = identity.version();
    for (_, entry) in entries::<CoreCatalogEntry>(root, "engines/cores")? {
        entry.validate()?;
        if entry.plugin_id != component {
            continue;
        }
        if let Some(previous) = &entry.identity {
            ensure!(
                entry.version == version || identity.release_sequence > previous.release_sequence,
                "external release_sequence must increase beyond every previously published identity for {component}"
            );
        }
    }
    Ok(())
}
fn compat_path(e: &CompatibilityEntry) -> String {
    format!(
        "compatibility/entries/{}/{}/{}/{}-{}-{}.json",
        e.vn_id, e.release_id, e.emulator, e.platform, e.architecture, e.variant
    )
}
fn collect(root: &Path, timestamp: i64, schemas: bool) -> Result<BTreeMap<String, Vec<u8>>> {
    let metadata_root = fs::symlink_metadata(root)?;
    ensure!(
        metadata_root.is_dir()
            && !metadata_root.file_type().is_symlink()
            && !reparse(&metadata_root),
        "metadata root must be a real directory"
    );
    if schemas {
        let mut paths = Vec::new();
        walk(root, "schema", &mut paths)?;
        let expected = metadata_json_schemas();
        ensure!(
            paths.len() == expected.len(),
            "schema set differs; run cargo xtask metadata schema"
        );
        for (name, schema) in expected {
            ensure!(
                decode::<Value>(&root.join("schema").join(name))? == schema,
                "stale schema; run cargo xtask metadata schema"
            );
        }
    }
    let mut compatibility = CompatibilityDatabase {
        schema: COMPATIBILITY_SCHEMA_VERSION.into(),
        generated_at_unix_ms: timestamp,
        entries: Vec::new(),
    };
    for (path, e) in entries::<CompatibilityEntry>(root, "compatibility/entries")? {
        e.validate()?;
        ensure!(
            path == compat_path(&e),
            "compatibility path/content mismatch: {path}"
        );
        compatibility.entries.push(e);
    }
    compatibility.validate()?;
    let legacy_compatibility = CompatibilityDatabase {
        schema: LEGACY_COMPATIBILITY_SCHEMA_VERSION.into(),
        generated_at_unix_ms: timestamp,
        entries: compatibility
            .entries
            .iter()
            .filter(|entry| {
                entry
                    .emulator_version
                    .as_deref()
                    .is_none_or(|version| version.chars().count() <= 64)
            })
            .cloned()
            .collect(),
    };
    legacy_compatibility.validate()?;
    let mut presets = PresetDatabase {
        schema: PRESET_DATABASE_SCHEMA_VERSION.into(),
        entries: Vec::new(),
    };
    for (path, p) in entries::<Preset>(root, "engines/presets")? {
        p.validate()?;
        ensure!(
            path == format!("engines/presets/{}/{}.json", p.release_id, p.emulator),
            "preset path/content mismatch: {path}"
        );
        presets.entries.push(p);
    }
    presets.validate()?;
    let mut release_works = BTreeMap::new();
    for (release, work) in compatibility
        .entries
        .iter()
        .map(|e| (&e.release_id, &e.vn_id))
        .chain(presets.entries.iter().map(|e| (&e.release_id, &e.vn_id)))
    {
        if let Some(previous) = release_works.insert(release, work) {
            ensure!(
                previous == work,
                "release assigned to different works: {release}"
            );
        }
    }
    let mut cores = CoreCatalog {
        schema: CORE_CATALOG_SCHEMA_VERSION.into(),
        entries: Vec::new(),
    };
    for (path, c) in entries::<CoreCatalogEntry>(root, "engines/cores")? {
        c.validate()?;
        ensure!(
            path == format!(
                "engines/cores/{}/{}/{}.json",
                c.plugin_id, c.platform, c.version
            ),
            "core path/content mismatch: {path}"
        );
        cores.entries.push(c);
    }
    cores.validate()?;
    let legacy_cores = CoreCatalog {
        schema: LEGACY_CORE_CATALOG_SCHEMA_VERSION.into(),
        entries: cores
            .entries
            .iter()
            .filter(|entry| entry.identity.is_none())
            .cloned()
            .collect(),
    };
    legacy_cores.validate()?;
    let mut patches = PatchCatalog {
        schema: PATCH_CATALOG_SCHEMA_VERSION.into(),
        entries: Vec::new(),
    };
    for (path, p) in entries::<PatchCatalogEntry>(root, "patches/entries")? {
        p.validate()?;
        ensure!(
            path == format!("patches/entries/{}.json", p.id),
            "patch path/content mismatch: {path}"
        );
        patches.entries.push(p);
    }
    patches.validate()?;
    for preset in &presets.entries {
        if let Some(id) = &preset.patch {
            ensure!(
                patches.entries.iter().any(|p| p.id == *id
                    && p.emulators.contains(&preset.emulator)
                    && p.releases
                        .iter()
                        .any(|release| release.vn_id == preset.vn_id
                            && release.release_id == preset.release_id)),
                "preset references missing or inapplicable recipe: {id}"
            );
        }
    }
    let mut files = BTreeMap::from([
        ("compatibility.json".into(), pretty(&legacy_compatibility)?),
        ("compatibility-v5.json".into(), pretty(&compatibility)?),
        ("presets.json".into(), pretty(&presets)?),
        ("cores.json".into(), pretty(&legacy_cores)?),
        ("cores-v2.json".into(), pretty(&cores)?),
        ("patches-v2.json".into(), pretty(&patches)?),
        (
            "patches.json".into(),
            read(&root.join("legacy/patches.json"))?,
        ),
        (
            "patches/catalog.json".into(),
            read(&root.join("legacy/patches/catalog.json"))?,
        ),
    ]);
    let mut script_paths = Vec::new();
    walk(root, "patches", &mut script_paths)?;
    for path in script_paths {
        if path.ends_with(".luau") {
            files.insert(path.clone(), read(&root.join(path))?);
        }
    }
    let mut legacy_scripts = Vec::new();
    walk(root, "legacy/patches", &mut legacy_scripts)?;
    for path in legacy_scripts {
        if let Some(relative) = path.strip_prefix("legacy/patches/")
            && path.ends_with(".luau")
        {
            files.insert(
                format!("patches/legacy/{relative}"),
                read(&root.join(&path))?,
            );
        }
    }
    for p in &patches.entries {
        ensure!(
            files.contains_key(&format!("patches/{}", p.recipe.script)),
            "missing recipe script: {}",
            p.recipe.script
        );
        let manifest = format!(
            "patches/{}.patch.json",
            p.recipe
                .script
                .strip_suffix(".luau")
                .expect("validated recipe suffix")
        );
        ensure!(
            files.insert(manifest, pretty(&p.releases)?).is_none(),
            "duplicate recipe manifest"
        );
    }
    let mut case_paths = BTreeSet::new();
    for path in files.keys() {
        ensure!(
            case_paths.insert(path.to_ascii_lowercase()),
            "case-colliding path: {path}"
        );
    }
    ensure!(
        files.len() <= 4096
            && files.values().map(|b| b.len() as u64).sum::<u64>() <= MAX_METADATA_TOTAL_BYTES,
        "snapshot exceeds bounds"
    );
    ensure!(
        files
            .values()
            .all(|b| b.len() as u64 <= MAX_METADATA_FILE_BYTES),
        "generated file exceeds 8 MiB"
    );
    Ok(files)
}
fn revision(root: &Path) -> Result<(String, i64)> {
    let o = Command::new("git")
        .args(["log", "-1", "--format=%H%n%ct"])
        .current_dir(root)
        .output()?;
    ensure!(o.status.success(), "cannot read source revision");
    let text = String::from_utf8(o.stdout)?;
    let mut lines = text.lines();
    let commit = lines.next().context("missing commit")?.to_owned();
    let time: i64 = lines.next().context("missing commit time")?.parse()?;
    ensure!(time >= 0, "negative source timestamp");
    Ok((
        commit,
        time.checked_mul(1000).context("timestamp overflow")?,
    ))
}
/// Creates the root exclusively. Failure removes only files created by this call.
fn write_new_tree(out: &Path, files: &BTreeMap<String, Vec<u8>>) -> Result<()> {
    if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    fs::create_dir(out)
        .with_context(|| format!("output must be a new directory: {}", out.display()))?;
    let mut written = Vec::new();
    let mut dirs = BTreeSet::new();
    let result = (|| -> Result<()> {
        for (name, bytes) in files {
            ensure!(is_safe_metadata_relative_path(name), "unsafe output path");
            let path = out.join(name);
            let mut parent = path.parent().unwrap();
            let mut parents = Vec::new();
            while parent != out {
                parents.push(parent.to_path_buf());
                parent = parent.parent().context("invalid output")?;
            }
            for dir in parents.into_iter().rev() {
                if !dirs.contains(&dir) {
                    fs::create_dir(&dir)?;
                    dirs.insert(dir);
                }
            }
            let mut f = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?;
            written.push(path);
            f.write_all(bytes)?;
            f.sync_all()?;
        }
        Ok(())
    })();
    if result.is_err() {
        for path in written.into_iter().rev() {
            let _ = fs::remove_file(path);
        }
        for dir in dirs.into_iter().rev() {
            let _ = fs::remove_dir(dir);
        }
        let _ = fs::remove_dir(out);
    }
    result
}
mod intake;
use intake::intake;

#[cfg(test)]
mod tests {
    use super::*;
    fn root(path: &Path) {
        fs::create_dir_all(path.join("schema")).unwrap();
        for (n, s) in metadata_json_schemas() {
            fs::write(path.join("schema").join(n), pretty(&s).unwrap()).unwrap();
        }
        fs::create_dir_all(path.join("legacy/patches")).unwrap();
        fs::write(
            path.join("legacy/patches.json"),
            br#"{"schema":"shinki.emu.patches.v1","entries":[]}"#,
        )
        .unwrap();
        fs::write(path.join("legacy/patches/catalog.json"), b"[]").unwrap();
    }
    fn event(path: &Path, platform: &str) {
        let body = format!(
            "### VNDB vID\r\n\r\nv1\r\n### VNDB rID\r\nr2\r\n### Core\r\nReMinori\r\n### Status\r\nflawed\r\n### Platform\r\n{platform}\r\n### Architecture\r\nx86_64\r\n### Variant\r\ndesktop\r\n### Core version\r\n0.1.0\r\n### Manager version\r\n0.1.0\r\n### Notes / reproduction\r\n_No response_\r\n"
        );
        fs::write(
            path,
            pretty(
                &json!({"issue":{"number":1,"body":body,"updated_at":"2026-10-24T10:00:00+08:00"}}),
            )
            .unwrap(),
        )
        .unwrap();
    }
    #[test]
    fn intake_roundtrip_keeps_platforms_separate_and_parses_timezone_crlf() {
        let t = tempfile::tempdir().unwrap();
        let r = t.path().join("root");
        root(&r);
        let ev = t.path().join("event.json");
        for platform in ["windows", "linux"] {
            event(&ev, platform);
            let out = t.path().join(platform);
            intake(IntakeKind::Compatibility, &ev, &out).unwrap();
            let p = format!("compatibility/entries/v1/r2/ReMinori/{platform}-x86_64-desktop.json");
            fs::create_dir_all(r.join(&p).parent().unwrap()).unwrap();
            fs::copy(out.join("Metadata").join(&p), r.join(&p)).unwrap();
        }
        let files = collect(&r, 0, true).unwrap();
        let db = parse_compatibility_database(&files["compatibility.json"]).unwrap();
        assert_eq!(db.entries.len(), 2);
        assert_eq!(db.entries[0].notes, None);
        assert_eq!(
            db.entries[0].updated_at_unix_ms,
            chrono::DateTime::parse_from_rfc3339("2026-10-24T02:00:00Z")
                .unwrap()
                .timestamp_millis()
        );
    }
    #[test]
    fn dual_catalog_keeps_published_downloads_and_adds_typed_release() {
        let repository = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let published = collect(&repository.join("Metadata"), 0, false).unwrap();
        let existing = parse_core_catalog(&published["cores.json"]).unwrap();
        let old = existing
            .entries
            .iter()
            .find(|entry| entry.plugin_id == "rfvp")
            .expect("published RFVP")
            .clone();
        let inventory = crate::cores::Inventory::load(repository).unwrap();
        let identity = inventory
            .core("rfvp")
            .unwrap()
            .source
            .as_ref()
            .unwrap()
            .identity();
        let mut new = old.clone();
        new.version = identity.version();
        new.release_tag = format!("core-rfvp-{}", new.version);
        new.url = format!("{CORE_RELEASE_URL_PREFIX}{}/core.zip", new.release_tag);
        new.identity = Some(identity);
        let temp = tempfile::tempdir().unwrap();
        root(temp.path());
        for entry in [&old, &new] {
            let path = temp.path().join(format!(
                "engines/cores/{}/{}/{}.json",
                entry.plugin_id, entry.platform, entry.version
            ));
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, pretty(entry).unwrap()).unwrap();
        }
        let report: CompatibilityEntry = serde_json::from_value(json!({
            "vn_id":"v1","release_id":"r1","emulator":"rfvp","status":"boot_only",
            "emulator_version":new.version,"platform":"windows","architecture":"x86_64",
            "variant":"desktop","updated_at_unix_ms":0
        }))
        .unwrap();
        let report_path = temp.path().join(compat_path(&report));
        fs::create_dir_all(report_path.parent().unwrap()).unwrap();
        fs::write(report_path, pretty(&report).unwrap()).unwrap();
        let files = collect(temp.path(), 0, true).unwrap();
        let legacy = parse_core_catalog(&files["cores.json"]).unwrap();
        assert_eq!(legacy.schema, LEGACY_CORE_CATALOG_SCHEMA_VERSION);
        assert_eq!(legacy.entries, vec![old.clone()]);
        let modern = parse_core_catalog(&files["cores-v2.json"]).unwrap();
        assert_eq!(modern.entries.len(), 2);
        assert_eq!(modern.latest("rfvp", &old.platform, &old.abi), Some(&new));
        assert!(modern.entries.contains(&old));
        let modern_reports = parse_compatibility_database(&files["compatibility-v5.json"]).unwrap();
        assert_eq!(modern_reports.entries, vec![report]);
        let legacy_reports = parse_compatibility_database(&files["compatibility.json"]).unwrap();
        assert_eq!(legacy_reports.schema, LEGACY_COMPATIBILITY_SCHEMA_VERSION);
        assert!(legacy_reports.entries.is_empty());
        let mut adapter_update = new.identity.clone().unwrap();
        adapter_update.adapter.commit = "f".repeat(40);
        assert!(require_release_sequence(temp.path(), "rfvp", &adapter_update).is_err());
        adapter_update.release_sequence += 1;
        require_release_sequence(temp.path(), "rfvp", &adapter_update).unwrap();
    }
    #[test]
    fn deterministic_snapshot_and_existing_output_protection() {
        let t = tempfile::tempdir().unwrap();
        let r = t.path().join("root");
        root(&r);
        let a = collect(&r, 123, true).unwrap();
        let b = collect(&r, 123, true).unwrap();
        assert_eq!(a, b);
        let out = t.path().join("out");
        write_new_tree(&out, &a).unwrap();
        assert!(
            write_new_tree(
                &out,
                &BTreeMap::from([("cores.json".into(), b"bad".to_vec())])
            )
            .is_err()
        );
        assert_eq!(fs::read(out.join("cores.json")).unwrap(), a["cores.json"]);
        // Exercise the public build path with a real source commit, including index hashes.
        for args in [
            vec!["init"],
            vec!["add", "."],
            vec![
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.org",
                "commit",
                "-m",
                "public fixture",
            ],
        ] {
            assert!(
                Command::new("git")
                    .args(args)
                    .current_dir(&r)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        let first = t.path().join("snapshot-a");
        let second = t.path().join("snapshot-b");
        run(MetadataCommand::Build {
            out: first.clone(),
            root: r.clone(),
        })
        .unwrap();
        run(MetadataCommand::Build {
            out: second.clone(),
            root: r.clone(),
        })
        .unwrap();
        let mut paths = Vec::new();
        walk(&first, "", &mut paths).unwrap();
        for path in paths {
            assert_eq!(
                fs::read(first.join(&path)).unwrap(),
                fs::read(second.join(path)).unwrap()
            );
        }
        let index = parse_metadata_index(&fs::read(first.join("index.json")).unwrap()).unwrap();
        let mut actual = Vec::new();
        walk(&first, "", &mut actual).unwrap();
        actual.retain(|p| p != "index.json");
        assert_eq!(
            actual,
            index
                .files
                .iter()
                .map(|f| f.path.clone())
                .collect::<Vec<_>>()
        );
        for f in index.files {
            let bytes = fs::read(first.join(f.path)).unwrap();
            assert_eq!(digest(&bytes), f.sha256);
            assert_eq!(bytes.len() as u64, f.size);
        }
    }
    #[test]
    fn illegal_core_id_in_intake_publishes_nothing() {
        let t = tempfile::tempdir().unwrap();
        let ev = t.path().join("event.json");
        event(&ev, "windows");
        let mut v: Value = decode(&ev).unwrap();
        v["issue"]["body"] = v["issue"]["body"]
            .as_str()
            .unwrap()
            .replace("ReMinori", "../shell")
            .into();
        fs::write(&ev, pretty(&v).unwrap()).unwrap();
        let out = t.path().join("out");
        assert!(intake(IntakeKind::Compatibility, &ev, &out).is_err());
        assert!(!out.exists());
    }
}
