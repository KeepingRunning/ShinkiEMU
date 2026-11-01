use crate::{cores::Inventory, package};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs, io,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Deserialize)]
struct Lock {
    schema: String,
    vcpkg_url: String,
    vcpkg_commit: String,
    cargo_ndk_version: String,
    android_ndk_version: String,
    subtitle_windows_triplet: String,
    ffmpeg_windows_triplet: String,
    bison_url: String,
    bison_sha256: String,
    native: BTreeMap<String, NativeArchive>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct NativeArchive {
    version: String,
    url: String,
    sha256: String,
    file_name: String,
    format: String,
    strip_prefix: String,
    #[serde(default)]
    include: Vec<String>,
    required: Vec<String>,
}

#[derive(Default, Clone, Copy)]
pub(crate) struct NativeNeeds {
    pub subtitle: bool,
    pub ffmpeg: bool,
    pub dxc: bool,
    pub native_vcpkg: bool,
    pub impacto_sources: bool,
    pub renpy: bool,
    pub bison: bool,
    pub gstreamer: bool,
}

fn windows_host_bison(selected: bool, host_is_windows: bool) -> bool {
    selected && host_is_windows
}

pub(crate) fn needs(inv: &Inventory, target: &str, component: &str) -> Result<NativeNeeds> {
    let mut result = NativeNeeds::default();
    let manager = component == "manager";
    if manager {
        result.dxc = target.contains("windows");
    } else {
        inv.core(component)?;
    }
    for core in inv.cores.iter().filter(|core| {
        if manager {
            core.static_manager_targets.iter().any(|t| t == target)
        } else {
            core.id == component
        }
    }) {
        for requirement in &core.dependencies {
            match requirement.as_str() {
                "subtitle" => result.subtitle = true,
                "ffmpeg" => result.ffmpeg = true,
                "dxc" => result.dxc = true,
                "native-vcpkg" => result.native_vcpkg = true,
                "impacto-sources" => result.impacto_sources = true,
                "renpy" => result.renpy = true,
                "bison" => result.bison = true,
                "gstreamer" => result.gstreamer = target == "x86_64-unknown-linux-gnu",
                other => bail!("unknown native dependency requirement {other} for {component}"),
            }
        }
    }
    Ok(result)
}

pub(crate) fn native_cores<'a>(
    inv: &'a Inventory,
    target: &'a str,
    component: &'a str,
) -> impl Iterator<Item = &'a crate::cores::Core> {
    inv.cores.iter().filter(move |core| {
        core.dependencies
            .iter()
            .any(|dependency| dependency == "native-vcpkg")
            && if component == "manager" {
                core.static_manager_targets
                    .iter()
                    .any(|triple| triple == target)
            } else {
                core.id == component
            }
    })
}

const NATIVE_RECORD: &str = ".shinki-native.json";

#[derive(Serialize)]
struct NativeLayer<'a> {
    archive: &'a NativeArchive,
    destination: &'a str,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct NativeFile {
    path: String,
    sha256: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct NativeRecord {
    schema: String,
    layers: serde_json::Value,
    files: Vec<NativeFile>,
}

fn native_path(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("empty native dependency path");
    }
    for part in name.split('/') {
        let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
        if part.is_empty()
            || part == "."
            || part == ".."
            || part.ends_with(['.', ' '])
            || part
                .chars()
                .any(|c| c.is_control() || "\\:<>|?*\"".contains(c))
            || ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
            || (stem.len() == 4
                && (stem.starts_with("COM") || stem.starts_with("LPT"))
                && matches!(stem.as_bytes()[3], b'1'..=b'9'))
        {
            bail!("unsafe native dependency path {name}");
        }
    }
    Ok(())
}

fn native_files(root: &Path) -> Result<Vec<NativeFile>> {
    package::plain(root)?;
    let mut files = Vec::new();
    let mut seen = BTreeSet::new();
    for entry in walkdir::WalkDir::new(root).follow_links(false) {
        let entry = entry?;
        package::plain(entry.path())?;
        let name = entry
            .path()
            .strip_prefix(root)?
            .to_str()
            .context("non-UTF-8 native dependency path")?
            .replace('\\', "/");
        if name.is_empty() || name == NATIVE_RECORD {
            continue;
        }
        native_path(&name)?;
        if !seen.insert(name.to_lowercase()) {
            bail!("native dependency case collision: {name}");
        }
        if entry.file_type().is_file() {
            files.push(NativeFile {
                path: name,
                sha256: package::sha256(entry.path())?,
            });
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

fn verify_native_tree(out: &Path, identity: &serde_json::Value) -> Result<()> {
    package::plain(&out.join(NATIVE_RECORD))?;
    let record: NativeRecord = serde_json::from_slice(&fs::read(out.join(NATIVE_RECORD))?)?;
    if record.schema != "shinki.emu.native_dependency.v1"
        || record.layers != *identity
        || record.files != native_files(out)?
    {
        bail!(
            "native dependency tree differs from its pinned inputs: {}; remove that owned tree before explicit re-preparation",
            out.display()
        );
    }
    Ok(())
}

fn selected_native_path(archive: &NativeArchive, raw: &str) -> Result<Option<String>> {
    let raw = raw.trim_end_matches('/');
    native_path(raw)?;
    let relative = if archive.strip_prefix.is_empty() {
        raw
    } else if raw == archive.strip_prefix {
        return Ok(None);
    } else if let Some(rest) = raw.strip_prefix(&format!("{}/", archive.strip_prefix)) {
        rest
    } else {
        return Ok(None);
    };
    if !archive.include.is_empty()
        && !archive
            .include
            .iter()
            .any(|prefix| relative == prefix || relative.starts_with(&format!("{prefix}/")))
    {
        return Ok(None);
    }
    native_path(relative)?;
    Ok(Some(relative.to_owned()))
}

fn reserve_native_path(
    seen: &mut BTreeMap<String, (String, bool)>,
    name: &str,
    directory: bool,
) -> Result<()> {
    let mut current = String::with_capacity(name.len());
    for part in name.split('/') {
        if !current.is_empty() {
            current.push('/');
        }
        current.push_str(part);
        let is_directory = current.len() != name.len() || directory;
        let key = current.to_lowercase();
        if let Some((original, previous_directory)) = seen.get(&key) {
            if original != &current || !previous_directory || !is_directory {
                bail!("duplicate or conflicting native archive path {name}");
            }
        } else {
            seen.insert(key, (current.clone(), is_directory));
        }
    }
    Ok(())
}

fn extract_native_archive(
    archive: &NativeArchive,
    input: &Path,
    root: &Path,
    destination: &str,
) -> Result<()> {
    if !destination.is_empty() {
        native_path(destination)?;
    }
    let mut seen = BTreeMap::new();
    let mut extract =
        |raw: &str, directory: bool, linked: bool, reader: &mut dyn io::Read| -> Result<()> {
            let Some(relative) = selected_native_path(archive, raw)? else {
                return Ok(());
            };
            if linked {
                bail!("linked native archive member {raw}");
            }
            let relative = if destination.is_empty() {
                relative
            } else {
                format!("{destination}/{relative}")
            };
            if relative == NATIVE_RECORD {
                bail!("native archive collides with verification record");
            }
            reserve_native_path(&mut seen, &relative, directory)?;
            let output = root.join(&relative);
            if directory {
                fs::create_dir_all(output)?;
            } else {
                fs::create_dir_all(output.parent().context("native archive parent")?)?;
                let mut file = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(output)?;
                io::copy(reader, &mut file)?;
            }
            Ok(())
        };
    match archive.format.as_str() {
        "zip" => {
            let mut zip = zip::ZipArchive::new(fs::File::open(input)?)?;
            for index in 0..zip.len() {
                let mut entry = zip.by_index(index)?;
                let name = entry.name()?.into_owned();
                let directory = entry.is_dir();
                let linked = entry.is_symlink();
                extract(&name, directory, linked, &mut entry)?;
                #[cfg(unix)]
                if !directory {
                    use std::os::unix::fs::PermissionsExt;
                    let mode = entry.unix_mode().unwrap_or(0o644) & 0o777;
                    if let Some(relative) = selected_native_path(archive, &name)? {
                        fs::set_permissions(
                            root.join(destination).join(relative),
                            fs::Permissions::from_mode(mode),
                        )?;
                    }
                }
            }
        }
        "tar-gz" => {
            let stream = flate2::read::GzDecoder::new(fs::File::open(input)?);
            let mut tar = tar::Archive::new(stream);
            for entry in tar.entries()? {
                let mut entry = entry?;
                let name = entry
                    .path()?
                    .to_str()
                    .context("non-UTF-8 tar path")?
                    .to_owned();
                let kind = entry.header().entry_type();
                extract(
                    &name,
                    kind.is_dir(),
                    !kind.is_dir() && !kind.is_file(),
                    &mut entry,
                )?;
                #[cfg(unix)]
                if kind.is_file() {
                    use std::os::unix::fs::PermissionsExt;
                    if let Some(relative) = selected_native_path(archive, &name)? {
                        fs::set_permissions(
                            root.join(destination).join(relative),
                            fs::Permissions::from_mode(entry.header().mode()? & 0o777),
                        )?;
                    }
                }
            }
        }
        other => bail!("unknown native archive format {other}"),
    }
    for required in &archive.required {
        native_path(required)?;
        let path = root.join(destination).join(required);
        package::plain(&path)
            .with_context(|| format!("native archive lacks required input {}", path.display()))?;
    }
    Ok(())
}

fn prepare_native_tree(
    root: &Path,
    name: &str,
    layers: &[NativeLayer<'_>],
    python_versions: Option<(&str, &str)>,
) -> Result<PathBuf> {
    package::portable(name)?;
    let out = root.join(".work/native-downloads").join(name);
    let identity = serde_json::to_value(layers)?;
    if fs::symlink_metadata(&out).is_ok() {
        verify_native_tree(&out, &identity)?;
        return Ok(out);
    }
    let mut archives = Vec::new();
    for layer in layers {
        package::portable(&layer.archive.file_name)?;
        if layer.archive.file_name.contains('/') {
            bail!("native download must have a plain filename");
        }
        let input = root.join(".work/downloads").join(&layer.archive.file_name);
        download(&layer.archive.url, &layer.archive.sha256, &input)?;
        archives.push(input);
    }
    package::atomic_directory(&out, |stage| {
        for (layer, input) in layers.iter().zip(&archives) {
            extract_native_archive(layer.archive, input, stage, layer.destination)?;
        }
        if name == "sdl2" && cfg!(unix) {
            run(Command::new("sh")
                .arg("configure")
                .arg("--disable-shared")
                .current_dir(stage))?;
        }
        if let Some((python, cython)) = python_versions {
            if cfg!(unix) {
                let openssl = stage.join("host-openssl");
                if cfg!(target_os = "macos") {
                    let source = stage.join("build-openssl");
                    run(Command::new("perl")
                        .arg("Configure")
                        .args(["no-shared", "no-tests", "--openssldir=/etc/ssl"])
                        .arg(format!("--prefix={}", openssl.display()))
                        .current_dir(&source))?;
                    run(Command::new("make").arg("-j3").current_dir(&source))?;
                    run(Command::new("make").arg("install_sw").current_dir(&source))?;
                }
                let mut configure = Command::new("sh");
                configure
                    .arg("configure")
                    .arg("--without-ensurepip")
                    .current_dir(stage)
                    .env("PYTHONDONTWRITEBYTECODE", "1");
                if cfg!(target_os = "macos") {
                    configure.arg(format!("--with-openssl={}", openssl.display()));
                }
                run(&mut configure)?;
                run(Command::new("make")
                    .args(["-j3", "all"])
                    .current_dir(stage)
                    .env("PYTHONDONTWRITEBYTECODE", "1"))?;
                // CPython's build tree links extension modules back to Modules/.
                // Publish ordinary files so the verified tree remains relocatable.
                let modules = fs::read_to_string(stage.join("pybuilddir.txt"))?;
                native_path(modules.trim())?;
                let stage_root = fs::canonicalize(stage)?;
                for entry in fs::read_dir(stage.join(modules.trim()))? {
                    let entry = entry?;
                    if entry.file_type()?.is_symlink() {
                        let source = fs::canonicalize(entry.path())?;
                        if !source.starts_with(&stage_root) || !source.is_file() {
                            bail!("CPython generated an external module link");
                        }
                        fs::remove_file(entry.path())?;
                        fs::copy(source, entry.path())?;
                    }
                }
                // Keep the executable distinct from CPython's Python/ source directory.
                let makefile = fs::read_to_string(stage.join("Makefile"))?;
                let suffix = makefile
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once('=')?;
                        (name.trim() == "BUILDEXE").then(|| value.trim())
                    })
                    .context("CPython BUILDEXE output")?;
                let executable = format!("python{suffix}");
                native_path(&executable)?;
                fs::rename(stage.join(executable), stage.join(build_python_relative()))?;
            }
            validate_build_python(&stage.join(build_python_relative()), python, cython)?;
        }
        let record = NativeRecord {
            schema: "shinki.emu.native_dependency.v1".into(),
            layers: identity,
            files: native_files(stage)?,
        };
        fs::write(
            stage.join(NATIVE_RECORD),
            serde_json::to_vec_pretty(&record)?,
        )?;
        Ok(())
    })?;
    Ok(out)
}

pub(crate) fn build_python_relative() -> &'static str {
    if cfg!(windows) {
        "python.exe"
    } else {
        "build-python"
    }
}

fn renpy_host_pin(name: &str) -> String {
    if !cfg!(windows) {
        format!("{name}-posix")
    } else {
        name.into()
    }
}

fn validate_build_python(executable: &Path, python: &str, cython: &str) -> Result<()> {
    run(Command::new(executable).args([
        "-B", "-I", "-c",
        "import sys, struct, Cython; assert struct.calcsize('P') == 8; assert sys.version_info[:3] == tuple(map(int, sys.argv[1].split('.'))); assert Cython.__version__ == sys.argv[2]",
        python, cython,
    ]))
}

fn download_locked_source(root: &Path, source: &serde_json::Value) -> Result<()> {
    let name = source["file_name"]
        .as_str()
        .context("source archive name")?;
    package::portable(name)?;
    let sha = source["sha256"].as_str().context("source archive SHA256")?;
    let out = root.join(".work/downloads").join(name);
    match source.get("url") {
        Some(url) => download(url.as_str().context("source archive URL")?, sha, &out),
        // Hosts such as Gitiles generate archives with varying bytes, so these
        // records pin a commit and the tar that `git archive` derives from it.
        None => git_archive(
            source["repo"].as_str().context("source repository")?,
            source["rev"].as_str().context("source revision")?,
            sha,
            &out,
        ),
    }
}

fn git_archive(repo: &str, rev: &str, sha: &str, out: &Path) -> Result<()> {
    if !repo.starts_with("https://")
        || rev.len() != 40
        || !rev
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        || sha.len() != 64
        || !sha.bytes().all(|c| c.is_ascii_hexdigit())
    {
        bail!("invalid locked Git source");
    }
    if out.exists() {
        if package::sha256(out)? == sha {
            return Ok(());
        }
        bail!("cached download hash mismatch: {}", out.display());
    }
    let parent = out.parent().context("download parent")?;
    fs::create_dir_all(parent)?;
    let checkout = tempfile::tempdir_in(parent)?;
    let git = || {
        let mut command = Command::new("git");
        command
            .arg("-C")
            .arg(checkout.path())
            .args(["-c", "core.autocrlf=false", "-c", "core.eol=lf"])
            .env("GIT_TERMINAL_PROMPT", "0");
        command
    };
    run(git().args(["init", "-q"]))?;
    run(git().args(["fetch", "-q", "--depth", "1", "--no-tags", repo, rev]))?;
    let fetched = String::from_utf8(capture(git().args(["rev-parse", "FETCH_HEAD"]))?)?;
    if fetched.trim() != rev {
        bail!("{repo} returned {} for pinned {rev}", fetched.trim());
    }
    let temp = tempfile::NamedTempFile::new_in(parent)?;
    run(git()
        .args(["archive", "--format=tar", "-o"])
        .arg(temp.path())
        .arg(rev))?;
    let actual = package::sha256(temp.path())?;
    if actual != sha {
        bail!("SHA256 mismatch for git archive {repo}@{rev}: expected {sha}, got {actual}");
    }
    temp.persist_noclobber(out).map_err(|e| e.error)?;
    Ok(())
}

fn renpy_android_root(inv: &Inventory, target: &str) -> Result<PathBuf> {
    let recipe = inv
        .core("renpy")?
        .source_directory(&inv.root)?
        .join("shinki");
    let builder = package::sha256(&recipe.join("android_runtime.py"))?;
    let patch = package::sha256(&recipe.join("android/SDL2-offscreen-default-display.patch"))?;
    let sdk = package::sha256(&recipe.join("sdk.json"))?;
    let python = package::sha256(&recipe.join("../licenses/renpy/python-sources.json"))?;
    Ok(inv
        .root
        .join(".work/native-downloads/renpy-android")
        .join(target)
        .join(format!(
            "{}-{}-{}-{}",
            &builder[..12],
            &patch[..12],
            &sdk[..12],
            &python[..12]
        )))
}

fn validate_renpy_android(inv: &Inventory, target: &str, root: &Path) -> Result<()> {
    let receipt: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("runtime.json")).with_context(|| {
            format!("target Ren'Py runtime receipt missing: {}", root.display())
        })?)?;
    let recipe = inv
        .core("renpy")?
        .source_directory(&inv.root)?
        .join("shinki");
    let sdk: serde_json::Value = serde_json::from_slice(&fs::read(recipe.join("sdk.json"))?)?;
    if receipt["target"] != target
        || receipt["renpy_build"] != sdk["android"]["renpy_build"]
        || receipt["renpy_source"] != sdk["source_revision"]
        || receipt["ndk_sha256"] != sdk["android"]["ndk_sha256"]
    {
        bail!(
            "target Ren'Py runtime provenance mismatch: {}",
            root.display()
        );
    }
    for (key, relative) in [
        ("builder_sha256", "android_runtime.py"),
        (
            "sdl_patch_sha256",
            "android/SDL2-offscreen-default-display.patch",
        ),
        ("sdk_sha256", "sdk.json"),
        (
            "python_sources_sha256",
            "../licenses/renpy/python-sources.json",
        ),
    ] {
        if receipt[key].as_str() != Some(package::sha256(&recipe.join(relative))?.as_str()) {
            bail!("target Ren'Py runtime recipe mismatch: {relative}");
        }
    }
    for (name, sha) in receipt["archives"]
        .as_object()
        .context("Ren'Py runtime archive hashes")?
    {
        package::portable(name)?;
        if sha.as_str() != Some(package::sha256(&root.join("lib").join(name))?.as_str()) {
            bail!("target Ren'Py runtime archive hash mismatch: {name}");
        }
    }
    for relative in [
        "include/python3.12/Python.h",
        "include/python3.12/pyconfig.h",
        "include/SDL2/SDL.h",
        "include/shinki-renpy-symbols.h",
        "lib/libpython3.12.a",
        "lib/librenpy.a",
        "lib/libSDL2.a",
        "lib/pkgconfig/shinki-renpy-runtime.pc",
        "LICENSE-CPython.txt",
        "LICENSE-Cubism-Headers.md",
        "Resources/lib/python3.12/encodings/__init__.py",
    ] {
        if !root.join(relative).is_file() {
            bail!(
                "target Ren'Py runtime input missing: {}",
                root.join(relative).display()
            );
        }
    }
    let sysconfig = root
        .join("Resources/lib")
        .join(target)
        .join("python3.12/_sysconfigdata__linux_.py");
    if !sysconfig.is_file() {
        bail!(
            "target Ren'Py sysconfig input missing: {}",
            sysconfig.display()
        );
    }
    Ok(())
}

fn prepare_renpy_android(inv: &Inventory, target: &str) -> Result<()> {
    let root = renpy_android_root(inv, target)?;
    if root.exists() {
        return validate_renpy_android(inv, target, &root);
    }
    let recipe = inv
        .core("renpy")?
        .source_directory(&inv.root)?
        .join("shinki");
    if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        bail!(
            "Android Ren'Py needs target archives built on Ubuntu 24.04 x64: run {} --build-root <official-renpy-build> --target {target} --output {} on that host, then transfer that complete output here",
            recipe.join("android_runtime.py").display(),
            root.display()
        );
    }
    let sdk: serde_json::Value = serde_json::from_slice(&fs::read(recipe.join("sdk.json"))?)?;
    if sdk["android"]["renpy_build"] != crate::sources::RENPY_BUILD_PIN {
        bail!("Ren'Py Android recipe and corresponding-source build pins disagree");
    }
    let build = inv.root.join(".work/renpy-build");
    let source = build.join("renpy");
    if !source.exists() {
        run(Command::new("git")
            .args([
                "clone",
                "--no-checkout",
                "https://github.com/renpy/renpy.git",
            ])
            .arg(&source))?;
        run(Command::new("git")
            .arg("-C")
            .arg(&source)
            .args(["checkout", "--detach"])
            .arg(
                sdk["source_revision"]
                    .as_str()
                    .context("Ren'Py Android source revision")?,
            ))?;
    }
    fs::create_dir_all(build.join("tars"))?;
    download(
        sdk["android"]["ndk_url"]
            .as_str()
            .context("Ren'Py Android NDK URL")?,
        sdk["android"]["ndk_sha256"]
            .as_str()
            .context("Ren'Py Android NDK SHA256")?,
        &build.join("tars/android-ndk-r29-linux.zip"),
    )?;
    download(
        sdk["android"]["cubism_url"]
            .as_str()
            .context("Ren'Py Cubism header SDK URL")?,
        sdk["android"]["cubism_sha256"]
            .as_str()
            .context("Ren'Py Cubism header SDK SHA256")?,
        &build.join("tars/CubismSdkForNative-4-r.6.2.zip"),
    )?;
    let python = executable("python3").context("official Ren'Py Android builds require Python 3 with Jinja2 and the upstream Ubuntu build prerequisites")?;
    run(Command::new(python)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .arg(recipe.join("android_runtime.py"))
        .arg("--build-root")
        .arg(build)
        .args(["--target", target])
        .arg("--output")
        .arg(&root)
        .arg("--host-python")
        .arg(
            inv.root
                .join(".work/native-downloads/python")
                .join(build_python_relative()),
        ))?;
    validate_renpy_android(inv, target, &root)
}

fn prepare_native_inputs(inv: &Inventory, lock: &Lock, selected: NativeNeeds) -> Result<()> {
    let get = |name: &str| {
        lock.native
            .get(name)
            .with_context(|| format!("native dependency lock missing {name}"))
    };
    if selected.impacto_sources {
        for name in ["libatrac9", "magic_enum"] {
            prepare_native_tree(
                &inv.root,
                name,
                &[NativeLayer {
                    archive: get(name)?,
                    destination: "",
                }],
                None,
            )?;
        }
    }
    if selected.renpy {
        if !cfg!(any(windows, target_os = "linux", target_os = "macos")) {
            bail!("RenPy's pinned build interpreter requires a Windows, Linux or macOS host");
        }
        crate::sources::prepare_renpy_build(inv)?;
        for inventory in ["python-sources.json", "native-sources.json"] {
            let path = inv
                .root
                .join("ThirdParty/renpy/licenses/renpy")
                .join(inventory);
            let sources: serde_json::Value = serde_json::from_slice(&fs::read(path)?)?;
            if sources["renpy_build"] != crate::sources::RENPY_BUILD_PIN {
                bail!("RenPy source inventory build revision mismatch");
            }
            let records = if inventory == "python-sources.json" {
                ["packages", "tools"].as_slice()
            } else {
                ["sources"].as_slice()
            };
            for field in records {
                for source in sources[field].as_array().context("RenPy source records")? {
                    download_locked_source(&inv.root, source)?;
                    if let Some(wheel) = source.get("wheel") {
                        download_locked_source(&inv.root, wheel)?;
                    }
                }
            }
        }
        let sdk: serde_json::Value = serde_json::from_slice(&fs::read(
            inv.root.join("ThirdParty/renpy/shinki/sdk.json"),
        )?)?;
        let sdk_archive = get("renpy-sdk")?;
        let python = get(&renpy_host_pin("python"))?;
        let cython = get(&renpy_host_pin("cython"))?;
        let sdl = get(&renpy_host_pin("sdl2"))?;
        if sdk["url"] != sdk_archive.url
            || sdk["sha256"] != sdk_archive.sha256
            || sdk["python"] != python.version
            || sdk["cython"] != cython.version
            || sdk["sdl"] != sdl.version
        {
            bail!("RenPy sdk.json and build/dependencies.toml pins disagree");
        }
        let sdk_root = prepare_native_tree(
            &inv.root,
            "renpy-sdk",
            &[NativeLayer {
                archive: sdk_archive,
                destination: "",
            }],
            None,
        )?;
        let (runtime_key, runtime_dir) = if cfg!(windows) {
            ("windows_x86_64", "lib/py3-windows-x86_64")
        } else if cfg!(target_os = "macos") {
            ("mac_universal", "lib/py3-mac-universal")
        } else {
            ("linux_x86_64", "lib/py3-linux-x86_64")
        };
        for (name, expected) in sdk[runtime_key].as_object().context("SDK runtime hashes")? {
            package::portable(name)?;
            if Some(package::sha256(&sdk_root.join(runtime_dir).join(name))?.as_str())
                != expected.as_str()
            {
                bail!("official RenPy SDK runtime hash mismatch: {name}");
            }
        }
        let mut python_layers = vec![
            NativeLayer {
                archive: python,
                destination: "",
            },
            NativeLayer {
                archive: cython,
                destination: if cfg!(windows) {
                    "Lib/site-packages"
                } else {
                    "Lib"
                },
            },
        ];
        if cfg!(target_os = "macos") {
            python_layers.push(NativeLayer {
                archive: get("renpy-host-openssl")?,
                destination: "build-openssl",
            });
        }
        prepare_native_tree(
            &inv.root,
            "python",
            &python_layers,
            Some((&python.version, &cython.version)),
        )?;
        prepare_native_tree(
            &inv.root,
            "sdl2",
            &[NativeLayer {
                archive: sdl,
                destination: "",
            }],
            None,
        )?;
        prepare_native_tree(
            &inv.root,
            "cython-license",
            &[NativeLayer {
                archive: get("cython-license")?,
                destination: "",
            }],
            None,
        )?;
    }
    Ok(())
}
fn lock(inv: &Inventory) -> Result<Lock> {
    let l: Lock = toml::from_str(&fs::read_to_string(
        inv.root.join("build/dependencies.toml"),
    )?)?;
    if l.schema != "shinki.emu.build_dependencies.v1" {
        bail!("invalid dependency lock");
    }
    Ok(l)
}
pub fn run(cmd: &mut Command) -> Result<()> {
    let status = cmd
        .status()
        .with_context(|| format!("starting {}", cmd.get_program().to_string_lossy()))?;
    if !status.success() {
        bail!(
            "{} failed with {status}",
            cmd.get_program().to_string_lossy()
        );
    }
    Ok(())
}
pub fn capture(cmd: &mut Command) -> Result<Vec<u8>> {
    let output = cmd
        .output()
        .with_context(|| format!("starting {}", cmd.get_program().to_string_lossy()))?;
    if !output.status.success() {
        bail!(
            "{} failed: {}",
            cmd.get_program().to_string_lossy(),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(output.stdout)
}
pub fn executable(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    for dir in env::split_paths(&path) {
        for suffix in if cfg!(windows) {
            &[".exe", ".cmd", ".bat", ""][..]
        } else {
            &[""][..]
        } {
            let p = dir.join(format!("{name}{suffix}"));
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}
pub fn ndk_root(inv: &Inventory) -> Result<PathBuf> {
    let l = lock(inv)?;
    let root = env::var_os("ANDROID_NDK_HOME")
        .or_else(|| env::var_os("ANDROID_NDK_ROOT"))
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("ANDROID_HOME")
                .or_else(|| env::var_os("ANDROID_SDK_ROOT"))
                .map(|p| PathBuf::from(p).join("ndk").join(&l.android_ndk_version))
        })
        .context("set ANDROID_NDK_HOME or ANDROID_HOME")?;
    validate_ndk_revision(&root, &l.android_ndk_version)?;
    Ok(root)
}

fn validate_ndk_revision(root: &Path, required: &str) -> Result<()> {
    let props = fs::read_to_string(root.join("source.properties"))
        .context("NDK source.properties missing")?;
    if !props.lines().any(|s| {
        s.split_once('=')
            .is_some_and(|(k, v)| k.trim() == "Pkg.Revision" && v.trim() == required)
    }) {
        bail!("NDK {required} required at {}", root.display());
    }
    Ok(())
}
pub fn doctor(inv: &Inventory, target: &str, core: Option<&str>) -> Result<()> {
    let t = inv.target(target)?;
    if crate::apple::supported(target) {
        crate::apple::preflight(target)?;
    }
    if let Some(c) = core {
        inv.require_supported(c, target)?;
    }
    let selected = needs(inv, target, core.unwrap_or("manager"))?;
    let mut missing = Vec::new();
    if let Some(id) = core {
        let core = inv.core(id)?;
        match crate::sources::verify(inv, core, false) {
            Ok(directory) => {
                if let Some(source) = &core.source {
                    println!("source: {id} @ {} ({})", source.rev, directory.display());
                }
            }
            Err(error) => missing.push(format!("{error:#}")),
        }
    }
    for tool in &t.tools {
        if crate::apple::cross_host(target) && tool == "xcrun" {
            continue;
        }
        if matches!(tool.as_str(), "cmake" | "ninja")
            && !(selected.ffmpeg || selected.native_vcpkg || selected.renpy)
        {
            continue;
        }
        match executable(tool) {
            Some(path) => println!("{tool}: {}", path.display()),
            None => {
                println!("{tool}: MISSING");
                missing.push(tool.clone());
            }
        }
    }
    let targets = capture(Command::new("rustup").args(["target", "list", "--installed"]))?;
    if !String::from_utf8_lossy(&targets)
        .lines()
        .any(|s| s == target)
    {
        missing.push(format!("rustup target add {target}"));
    }
    if t.platform == "windows" {
        match cc::windows_registry::find_tool(target, "cl.exe") {
            Some(compiler) => {
                println!("MSVC: {}", compiler.path().display());
            }
            _ => {
                if env::var_os("VCToolsInstallDir").is_none() && executable("cl").is_none() {
                    missing.push("MSVC x64 compiler/build tools".into());
                }
            }
        }
        let mut paths = Vec::new();
        if selected.subtitle {
            paths.push(subtitle_install(inv, target)?.join("lib/pkgconfig/libass.pc"));
        }
        if selected.ffmpeg {
            paths.push(
                inv.root
                    .join(".work/vcpkg-ffmpeg/installed/x64-windows/lib/avcodec.lib"),
            );
        }
        if selected.dxc {
            paths.push(inv.root.join(".work/dxc/dxcompiler.dll"));
        }
        if selected.bison {
            paths.push(inv.root.join(".work/bison/win_bison.exe"));
        }
        for native in native_cores(inv, target, core.unwrap_or("manager")) {
            paths.push(native_install(inv, target, &native.id)?.join("vcpkg/status"));
        }
        for p in paths {
            if !p.is_file() {
                missing.push(format!(
                    "{} (cargo xtask deps --target {target})",
                    p.display()
                ));
            } else {
                println!("dependency: {}", p.display());
            }
        }
    }
    if windows_host_bison(selected.bison, cfg!(windows)) {
        let bison = inv.root.join(".work/bison/win_bison.exe");
        if !bison.is_file() {
            missing.push(format!(
                "{} (cargo xtask deps --target {target})",
                bison.display()
            ));
        }
    }
    if !cfg!(windows) && selected.bison && executable("bison").is_none() {
        missing.push("Bison host executable".into());
    }
    if crate::apple::supported(target) && selected.subtitle {
        let pc = subtitle_install(inv, target)?.join("lib/pkgconfig/libass.pc");
        if !pc.is_file() {
            missing.push(format!(
                "{} (cargo xtask deps --target {target})",
                pc.display()
            ));
        }
        if executable("pkg-config").is_none() {
            missing.push("pkg-config for the pinned Apple subtitle installation".into());
        }
    }
    if t.platform == "linux" {
        for package in ["alsa", "libudev", "fontconfig", "xkbcommon"] {
            if capture(Command::new("pkg-config").args(["--exists", package])).is_err() {
                missing.push(format!("Linux development package providing {package}.pc"));
            }
        }
        // Store web sign-in (wry) builds against WebKitGTK 4.1 and GTK 3.
        if core.is_none() {
            for package in ["gtk+-3.0", "webkit2gtk-4.1"] {
                if capture(Command::new("pkg-config").args(["--exists", package])).is_err() {
                    missing.push(format!("Linux development package providing {package}.pc"));
                }
            }
        }
        if selected.gstreamer {
            for package in [
                "glib-2.0 >= 2.56",
                "gstreamer-1.0 >= 1.14",
                "gstreamer-app-1.0 >= 1.14",
                "gstreamer-video-1.0 >= 1.14",
            ] {
                if capture(Command::new("pkg-config").args(["--exists", package])).is_err() {
                    missing.push(format!("Linux development package providing {package}"));
                }
            }
        }
        if selected.subtitle || selected.ffmpeg {
            for tool in ["cmake", "ninja", "nasm", "patchelf"] {
                if executable(tool).is_none() {
                    missing.push(tool.to_owned());
                }
            }
            for (needed, kind, pc) in [
                (selected.subtitle, "subtitle", "libass.pc"),
                (selected.ffmpeg, "ffmpeg", "libavcodec.pc"),
            ] {
                if needed
                    && !linux_install(inv, target, kind)
                        .join("lib/pkgconfig")
                        .join(pc)
                        .is_file()
                {
                    missing.push(format!(
                        "pinned Linux {kind} installation (cargo xtask deps --target {target})"
                    ));
                }
            }
        }
    }
    if t.platform == "android" {
        match ndk_root(inv) {
            Ok(p) => println!("NDK: {}", p.display()),
            Err(e) => missing.push(e.to_string()),
        };
        let ndk = inv.root.join(".work/tools/bin").join(if cfg!(windows) {
            "cargo-ndk.exe"
        } else {
            "cargo-ndk"
        });
        let tool = if ndk.is_file() {
            Some(ndk)
        } else {
            executable("cargo-ndk")
        };
        let ndk_version = tool.and_then(|path| capture(Command::new(path).arg("--version")).ok());
        if ndk_version
            .as_ref()
            .is_none_or(|bytes| !String::from_utf8_lossy(bytes).contains("4.1.2"))
        {
            missing.push("cargo-ndk 4.1.2 (cargo xtask deps)".into());
        }
        match Command::new("java").arg("-version").output() {
            Ok(output) => {
                let version = format!(
                    "{}{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                if !output.status.success()
                    || !version.lines().any(|line| {
                        line.contains("version \"27\"")
                            || line.contains("version \"27.")
                            || line.contains("openjdk 27.")
                    })
                {
                    missing.push("JDK 27".into());
                } else {
                    println!("Java: {}", version.lines().next().unwrap_or("JDK 27"));
                }
            }
            Err(_) => missing.push("JDK 27".into()),
        }
        if selected.subtitle
            && !subtitle_install(inv, target)?
                .join("lib/pkgconfig/libass.pc")
                .is_file()
        {
            missing.push("Android subtitle vcpkg tree (cargo xtask deps)".into());
        }
    }
    if selected.ffmpeg {
        let pc = ffmpeg_install(inv, target)?.join("lib/pkgconfig/libavcodec.pc");
        if !pc.is_file() {
            missing.push(format!(
                "{} (cargo xtask deps --target {target})",
                pc.display()
            ));
        }
    }
    if t.platform == "ios" && env::var_os("DEVELOPER_DIR").is_none() {
        println!("Xcode: using xcode-select developer directory");
    }
    if t.platform == "ohos"
        && let Err(error) = crate::ohos::tools()
    {
        missing.push(format!("{error:#}"));
    }
    if selected.renpy {
        let base = inv.root.join(".work/native-downloads");
        for (variable, path) in [
            (
                "RENPY_SDK_ROOT",
                base.join(if cfg!(windows) {
                    "renpy-sdk/lib/py3-windows-x86_64/libpython3.12.dll"
                } else if cfg!(target_os = "macos") {
                    "renpy-sdk/lib/py3-mac-universal/librenpython.dylib"
                } else {
                    "renpy-sdk/lib/py3-linux-x86_64/librenpython.so"
                }),
            ),
            (
                "RENPY_PYTHON_DEV",
                base.join(if cfg!(windows) {
                    "python/include/Python.h"
                } else {
                    "python/Include/Python.h"
                }),
            ),
            ("RENPY_SDL_INCLUDE", base.join("sdl2/include/SDL.h")),
            (
                "RENPY_CYTHON_LICENSE",
                base.join("cython-license/LICENSE.txt"),
            ),
        ] {
            if path.is_file() {
                println!("{variable}: {}", path.display());
            } else {
                missing.push(format!(
                    "{variable}: {} (cargo xtask deps --target {target} --core renpy)",
                    path.display()
                ));
            }
        }
        let pins = lock(inv)?;
        if let (Some(python), Some(cython)) = (
            pins.native.get(&renpy_host_pin("python")),
            pins.native.get(&renpy_host_pin("cython")),
        ) {
            if validate_build_python(
                &base.join("python").join(build_python_relative()),
                &python.version,
                &cython.version,
            )
            .is_err()
            {
                missing.push(
                    "task-owned CPython/Cython build interpreter (cargo xtask deps --core renpy)"
                        .into(),
                );
            }
        } else {
            missing.push("CPython/Cython dependency pins".into());
        }
        if target.ends_with("-linux-android")
            && let Err(error) =
                validate_renpy_android(inv, target, &renpy_android_root(inv, target)?)
        {
            missing.push(error.to_string());
        }
    }
    if !missing.is_empty() {
        bail!("missing prerequisites:\n  {}", missing.join("\n  "));
    }
    Ok(())
}
pub fn client() -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .user_agent("Project-Shinkirou-xtask")
        .timeout(std::time::Duration::from_secs(600))
        .build()?)
}
pub fn download(url: &str, sha: &str, out: &Path) -> Result<()> {
    if !url.starts_with("https://")
        || sha.len() != 64
        || !sha.bytes().all(|c| c.is_ascii_hexdigit())
    {
        bail!("invalid locked download");
    }
    if out.exists() {
        if package::sha256(out)? == sha {
            return Ok(());
        }
        bail!("cached download hash mismatch: {}", out.display());
    }
    fs::create_dir_all(out.parent().context("download parent")?)?;
    let mut temp = tempfile::NamedTempFile::new_in(out.parent().unwrap())?;
    let mut response = client()?
        .get(url)
        .timeout(std::time::Duration::from_secs(3600))
        .send()?
        .error_for_status()?;
    io::copy(&mut response, &mut temp)?;
    let actual = package::sha256(temp.path())?;
    if actual != sha {
        bail!("SHA256 mismatch for locked download {url}: expected {sha}, got {actual}");
    }
    temp.persist_noclobber(out).map_err(|e| e.error)?;
    Ok(())
}
fn dxc(inv: &Inventory) -> Result<()> {
    let v: serde_json::Value =
        serde_json::from_slice(&fs::read(inv.root.join("Assets/Effects/dxc.lock.json"))?)?;
    let value = |key: &str| -> Result<&str> {
        v[key]
            .as_str()
            .with_context(|| format!("DXC lock missing {key}"))
    };
    let out = inv.root.join(".work/dxc");
    let members = [
        value("library")?,
        value("validator")?,
        "LICENSE-MS.txt",
        "LICENSE-LLVM.txt",
        "LICENCE-MIT.txt",
    ];
    let archive = inv.root.join(".work/downloads").join(value("asset")?);
    download(value("url")?, value("sha256")?, &archive)?;
    if out.exists() {
        let mut zip = zip::ZipArchive::new(fs::File::open(&archive)?)?;
        for name in &members {
            let mut input = zip
                .by_name(name)
                .with_context(|| format!("locked DXC archive lacks {name}"))?;
            let dest = out.join(Path::new(name).file_name().unwrap());
            package::plain(&dest)?;
            let mut expected = Vec::new();
            io::Read::read_to_end(&mut input, &mut expected)?;
            if fs::read(dest)? != expected {
                bail!("DXC cache modified; remove .work/dxc before explicit deps");
            }
        }
        return Ok(());
    }
    package::atomic_directory(&out, |stage| {
        let mut zip = zip::ZipArchive::new(fs::File::open(&archive)?)?;
        for name in &members {
            let mut source = zip
                .by_name(name)
                .with_context(|| format!("locked DXC archive lacks {name}"))?;
            if source.is_symlink() {
                bail!("linked DXC member");
            }
            let dest = stage.join(Path::new(name).file_name().unwrap());
            io::copy(&mut source, &mut fs::File::create(dest)?)?;
        }
        Ok(())
    })
}
/// Resolve subtitle inputs from the selected consumer's locked manifest/features.
fn subtitle_source(inv: &Inventory, target: &str, component: Option<&str>) -> Result<PathBuf> {
    let mut command = Command::new("cargo");
    command.current_dir(&inv.root).args([
        "metadata",
        "--locked",
        "--format-version",
        "1",
        "--filter-platform",
        target,
    ]);
    if let Some(id) = component {
        let core = inv.core(id)?;
        command
            .arg("--manifest-path")
            .arg(inv.root.join(&core.manifest));
        if !core.features.is_empty() {
            command.arg("--features").arg(
                core.features
                    .iter()
                    .map(|feature| format!("{}/{feature}", core.package))
                    .collect::<Vec<_>>()
                    .join(","),
            );
        }
    }
    let value: serde_json::Value = serde_json::from_slice(&capture(&mut command)?)?;
    let packages = value["packages"]
        .as_array()
        .context("cargo metadata packages")?;
    let mut candidates = packages
        .iter()
        .filter(|package| package["name"] == "roukaku-subtitle");
    let package = candidates.next().with_context(|| {
        format!(
            "pinned roukaku-subtitle missing in selected consumer {} for {target}",
            component.unwrap_or("manager")
        )
    })?;
    if candidates.next().is_some() {
        bail!("selected consumer resolves multiple roukaku-subtitle sources");
    }
    let source=package["source"].as_str().context("roukaku-subtitle must resolve from the pinned RoukakuEngine git revision, not a local patch")?;
    if !source.starts_with("git+https://github.com/Project-Shinkirou/RoukakuEngine")
        || source.rsplit('#').next().is_none_or(|s| s.len() != 40)
    {
        bail!("roukaku-subtitle source is not a pinned public git revision");
    }
    Ok(Path::new(
        package["manifest_path"]
            .as_str()
            .context("subtitle manifest_path")?,
    )
    .parent()
    .context("subtitle parent")?
    .to_owned())
}
pub(crate) fn apple_subtitle_triplet(target: &str) -> Option<&'static str> {
    match target {
        "aarch64-apple-darwin" => Some("arm64-osx"),
        "x86_64-apple-darwin" => Some("x64-osx"),
        "aarch64-apple-ios" => Some("arm64-ios"),
        "aarch64-apple-ios-sim" => Some("arm64-ios-simulator"),
        "x86_64-apple-ios" => Some("x64-ios"),
        _ => None,
    }
}

pub fn subtitle_install(inv: &Inventory, target: &str) -> Result<PathBuf> {
    let l = lock(inv)?;
    Ok(if target.contains("android") {
        inv.root
            .join(".work/vcpkg-subtitle")
            .join(target)
            .join("installed")
            .join(if target.starts_with("aarch64") {
                "arm64-android-dynbidi"
            } else {
                "x64-android-dynbidi"
            })
    } else if target == "aarch64-unknown-linux-ohos" {
        inv.root
            .join(".work/vcpkg-subtitle")
            .join(target)
            .join("installed/arm64-ohos")
    } else if target == "x86_64-unknown-linux-gnu" {
        linux_install(inv, target, "subtitle")
    } else if let Some(triplet) = apple_subtitle_triplet(target) {
        inv.root
            .join(".work/vcpkg-subtitle")
            .join(target)
            .join("installed")
            .join(triplet)
    } else {
        env::var_os("ROUKAKU_SUBTITLE_VCPKG_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| inv.root.join(".work/vcpkg-subtitle"))
            .join("installed")
            .join(l.subtitle_windows_triplet)
    })
}

pub(crate) fn native_install(inv: &Inventory, target: &str, core: &str) -> Result<PathBuf> {
    Ok(shinki_emu_build::native_vcpkg_install_root(
        &inv.root.join(".work/native-vcpkg"),
        core,
        &native_triplet(inv, target)?,
    ))
}

pub(crate) fn native_triplet(inv: &Inventory, target: &str) -> Result<String> {
    if target == "x86_64-pc-windows-msvc" {
        Ok("x64-windows".into())
    } else {
        ffmpeg_triplet(inv, target)
    }
}

/// One installation per target; manifest mode must not prune another SDK's ports.
pub(crate) fn ffmpeg_install(inv: &Inventory, target: &str) -> Result<PathBuf> {
    let triplet = ffmpeg_triplet(inv, target)?;
    Ok(ffmpeg_root(inv, target).join("installed").join(triplet))
}

fn ffmpeg_root(inv: &Inventory, target: &str) -> PathBuf {
    let root = inv.root.join(".work/vcpkg-ffmpeg");
    if target.contains("windows") {
        root
    } else {
        root.join(target)
    }
}

fn ffmpeg_triplet(inv: &Inventory, target: &str) -> Result<String> {
    Ok(match target {
        "x86_64-pc-windows-msvc" => lock(inv)?.ffmpeg_windows_triplet,
        "x86_64-unknown-linux-gnu" => "x64-linux-dynamic".into(),
        "aarch64-linux-android" => "arm64-android-ffmpeg".into(),
        "x86_64-linux-android" => "x64-android-ffmpeg".into(),
        "aarch64-unknown-linux-ohos" => "arm64-ohos".into(),
        _ => apple_subtitle_triplet(target)
            .with_context(|| format!("no FFmpeg triplet for {target}"))?
            .into(),
    })
}

/// Static Apple/mobile builds have no redistributable FFmpeg shared libraries.
pub(crate) fn ffmpeg_runtime(inv: &Inventory, target: &str) -> Result<Option<PathBuf>> {
    Ok(match target {
        "x86_64-pc-windows-msvc" => Some(ffmpeg_install(inv, target)?.join("bin")),
        "x86_64-unknown-linux-gnu" => Some(linux_runtime(inv, target, "ffmpeg")),
        _ => {
            ffmpeg_triplet(inv, target)?;
            None
        }
    })
}

pub(crate) fn ohos_sdk_native() -> Result<PathBuf> {
    let sdk = env::var_os("OHOS_SDK_NATIVE")
        .map(PathBuf::from)
        .or_else(|| {
            cfg!(windows)
                .then(|| PathBuf::from("D:/Huawei/DevEco Studio/sdk/default/openharmony/native"))
        })
        .context("OHOS_SDK_NATIVE must point to the DevEco native SDK")?;
    for required in [
        "build/cmake/ohos.toolchain.cmake",
        "sysroot/usr/include",
        "llvm/bin",
    ] {
        if !sdk.join(required).exists() {
            bail!("OHOS native SDK missing {required}: {}", sdk.display());
        }
    }
    Ok(sdk)
}

pub(crate) fn linux_install(inv: &Inventory, target: &str, kind: &str) -> PathBuf {
    inv.root
        .join(format!(".work/vcpkg-{kind}"))
        .join(target)
        .join("installed/x64-linux-dynamic")
}

pub(crate) fn linux_runtime(inv: &Inventory, target: &str, kind: &str) -> PathBuf {
    inv.root.join(".work/linux-runtime").join(target).join(kind)
}

fn prepare_linux_runtime(inv: &Inventory, target: &str, kind: &str) -> Result<()> {
    let install = linux_install(inv, target, kind);
    let runtime = linux_runtime(inv, target, kind);
    prepare_linux_runtime_tree(&install, &runtime, target)
}

fn prepare_linux_runtime_tree(install: &Path, runtime: &Path, target: &str) -> Result<()> {
    package::plain(&install.join("lib"))?;
    let root = fs::canonicalize(install.join("lib"))?;
    fs::create_dir_all(runtime)?;
    package::plain(runtime)?;
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        let name = entry.file_name();
        if !name.to_string_lossy().contains(".so") {
            continue;
        }
        // vcpkg's versioned SONAME links are build inputs, not package links.
        // Materialize only files resolving inside this pinned installation.
        let source = fs::canonicalize(entry.path())?;
        if !source.starts_with(&root) || !source.is_file() {
            bail!(
                "Linux runtime link escapes its prepared library tree: {}",
                entry.path().display()
            );
        }
        let output = runtime.join(name);
        if fs::symlink_metadata(&output).is_ok() {
            package::plain(&output)?;
        }
        fs::copy(source, &output)?;
        run(Command::new("patchelf")
            .args(["--set-rpath", "$ORIGIN"])
            .arg(&output))?;
        crate::binary::inspect(&output, target, false)?;
    }
    // Cocos' pinned Linux SDK keeps FMOD outside lib/ and omits its SONAME link.
    // This is an authorized bundled runtime, never a distribution/system ABI.
    let fmod = install.join("share/cocos2dx/linux-specific/fmod/prebuilt/64-bit/libfmod.so");
    if fmod.is_file() {
        package::plain(&fmod)?;
        if package::sha256(&fmod)?
            != "d4454884763c746c70c4ea2a9b0ca35555fef2bf174072be21924cb9a80d84a9"
        {
            bail!("Cocos FMOD runtime differs from the reviewed SDK 1.06.08 input");
        }
        let soname = capture(Command::new("patchelf").arg("--print-soname").arg(&fmod))?;
        if String::from_utf8(soname)?.trim() != "libfmod.so.6" {
            bail!("Cocos FMOD runtime must declare SONAME libfmod.so.6");
        }
        let output = runtime.join("libfmod.so.6");
        if fs::symlink_metadata(&output).is_ok() {
            package::plain(&output)?;
        }
        fs::copy(&fmod, &output)?;
        run(Command::new("patchelf")
            .args(["--set-rpath", "$ORIGIN"])
            .arg(&output))?;
        crate::binary::inspect(&output, target, false)?;
    }
    Ok(())
}
fn vcpkg_install_command(inv: &Inventory, executable: &Path) -> Result<Command> {
    let mut command = Command::new(executable);
    command.arg("install");
    command.env("VCPKG_ROOT", inv.root.join(".work/vcpkg"));
    shinki_emu_build::configure_native_environment(&inv.root.join(".work"), &mut command)?;
    Ok(command)
}

fn registry_config(directory: &Path, key: &str) -> Result<Option<String>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(["config", "--local", "--get", key])
        .output()?;
    match output.status.code() {
        Some(0) => Ok(Some(String::from_utf8(output.stdout)?.trim().to_owned())),
        Some(1) => Ok(None),
        _ => bail!(
            "cannot read vcpkg repository configuration: {}",
            String::from_utf8_lossy(&output.stderr)
        ),
    }
}

fn materialize_registry_history(inv: &Inventory, directory: &Path, pins: &Lock) -> Result<()> {
    let keys = [
        "remote.origin.promisor",
        "remote.origin.partialclonefilter",
        "extensions.partialClone",
    ];
    let mut partial = false;
    for key in keys {
        partial |= registry_config(directory, key)?.is_some();
    }
    let shallow = capture(
        Command::new("git")
            .arg("-C")
            .arg(directory)
            .args(["rev-parse", "--is-shallow-repository"]),
    )?;
    let shallow = String::from_utf8_lossy(&shallow).trim() == "true";
    if !(partial || shallow) {
        return Ok(());
    }
    let mut fetch = Command::new("git");
    fetch
        .arg("-C")
        .arg(directory)
        .args(["fetch", "--refetch", "--no-filter", "--tags"]);
    if shallow {
        fetch.arg("--unshallow");
    }
    fetch
        .arg(&pins.vcpkg_url)
        .arg("refs/heads/*:refs/remotes/origin/*");
    shinki_emu_build::configure_native_environment(&inv.root.join(".work"), &mut fetch)?;
    run(&mut fetch)?;
    for key in keys {
        if registry_config(directory, key)?.is_some() {
            run(Command::new("git").arg("-C").arg(directory).args([
                "config",
                "--local",
                "--unset-all",
                key,
            ]))?;
        }
    }
    Ok(())
}

pub fn apply_environment(
    inv: &Inventory,
    target: &str,
    component: &str,
    cmd: &mut Command,
) -> Result<()> {
    shinki_emu_build::configure_native_environment(&inv.root.join(".work"), cmd)?;
    let selected = needs(inv, target, component)?;
    cmd.env_remove("SHINKI_EMU_FFMPEG_INSTALLED");
    let subtitle = subtitle_install(inv, target)?;
    if target.contains("windows") {
        cmd.env_remove("ROUKAKU_SUBTITLE_VCPKG_ROOT")
            .env_remove("VCPKG_ROOT")
            .env_remove("BISON_EXECUTABLE");
        let mut paths = Vec::new();
        if selected.subtitle {
            let root = subtitle
                .parent()
                .and_then(Path::parent)
                .context("subtitle root")?;
            cmd.env("ROUKAKU_SUBTITLE_VCPKG_ROOT", root);
            paths.push(subtitle.join("bin"));
        }
        if selected.ffmpeg {
            cmd.env("VCPKG_ROOT", inv.root.join(".work/vcpkg-ffmpeg"));
        }
        if selected.dxc {
            paths.push(inv.root.join(".work/dxc"));
        }
        if selected.bison {
            paths.push(inv.root.join(".work/bison"));
            cmd.env(
                "BISON_EXECUTABLE",
                inv.root.join(".work/bison/win_bison.exe"),
            );
        }
        cmd.env(
            "PATH",
            env::join_paths(
                paths
                    .into_iter()
                    .chain(env::split_paths(&env::var_os("PATH").unwrap_or_default())),
            )?,
        );
    }
    if selected.renpy {
        let base = inv.root.join(".work/native-downloads");
        cmd.env("RENPY_SDK_ROOT", base.join("renpy-sdk"))
            .env("RENPY_PYTHON_DEV", base.join("python"))
            .env("RENPY_SDL_INCLUDE", base.join("sdl2/include"))
            .env(
                "Python3_EXECUTABLE",
                base.join("python").join(build_python_relative()),
            )
            .env(
                "RENPY_CYTHON_LICENSE",
                base.join("cython-license/LICENSE.txt"),
            )
            .env("PYTHONDONTWRITEBYTECODE", "1");
        if target.ends_with("-linux-android") {
            cmd.env("RENPY_STATIC_ROOT", renpy_android_root(inv, target)?);
        }
    }
    if cfg!(windows) && !target.contains("windows") && selected.bison {
        let bison = inv.root.join(".work/bison");
        let path = env::var_os("PATH").unwrap_or_default();
        cmd.env("BISON_EXECUTABLE", bison.join("win_bison.exe"))
            .env(
                "PATH",
                env::join_paths(std::iter::once(bison).chain(env::split_paths(&path)))?,
            );
    }
    if crate::apple::supported(target) && selected.subtitle {
        // Apple triplets statically link the complete libass closure, including
        // pkg-config's private libraries and system frameworks.
        for var in [
            "PKG_CONFIG_PATH",
            "PKG_CONFIG_LIBDIR",
            "PKG_CONFIG_SYSROOT_DIR",
        ] {
            cmd.env_remove(var)
                .env_remove(format!("{var}_{target}"))
                .env_remove(format!("{var}_{}", target.replace('-', "_")))
                .env_remove(format!("TARGET_{var}"));
        }
        cmd.env("PKG_CONFIG_ALLOW_CROSS", "1")
            .env("PKG_CONFIG_ALL_STATIC", "1")
            .env("PKG_CONFIG_LIBDIR", subtitle.join("lib/pkgconfig"));
    }
    if !cfg!(windows) && selected.bison {
        cmd.env(
            "BISON_EXECUTABLE",
            executable("bison").context("Bison host executable is missing")?,
        );
    }
    if target == "x86_64-unknown-linux-gnu" && (selected.subtitle || selected.ffmpeg) {
        let mut paths = Vec::new();
        if selected.subtitle {
            paths.push(subtitle.join("lib/pkgconfig"));
        }
        if selected.ffmpeg {
            paths.push(linux_install(inv, target, "ffmpeg").join("lib/pkgconfig"));
        }
        cmd.env("PKG_CONFIG_PATH", env::join_paths(paths)?);
        let existing = env::var("CARGO_ENCODED_RUSTFLAGS").unwrap_or_else(|_| {
            env::var("RUSTFLAGS")
                .unwrap_or_default()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join("\x1f")
        });
        let flags = if existing.is_empty() {
            "-C\x1flink-arg=-Wl,-rpath,$ORIGIN".to_owned()
        } else {
            format!("{existing}\x1f-C\x1flink-arg=-Wl,-rpath,$ORIGIN")
        };
        cmd.env("CARGO_ENCODED_RUSTFLAGS", flags);
    }
    if target.contains("android") {
        let ndk = ndk_root(inv)?;
        for var in [
            "PKG_CONFIG",
            "PKG_CONFIG_PATH",
            "PKG_CONFIG_LIBDIR",
            "PKG_CONFIG_SYSROOT_DIR",
        ] {
            cmd.env_remove(format!("{var}_{target}"))
                .env_remove(format!("{var}_{}", target.replace('-', "_")))
                .env_remove(format!("TARGET_{var}"));
        }
        cmd.env_remove("PKG_CONFIG_PATH")
            .env_remove("PKG_CONFIG_SYSROOT_DIR");
        cmd.env(
            "ROUKAKU_SUBTITLE_VCPKG_ROOT",
            subtitle
                .parent()
                .and_then(Path::parent)
                .context("Android subtitle root")?,
        );
        cmd.env("ANDROID_NDK_HOME", &ndk)
            .env("ANDROID_NDK_ROOT", ndk)
            .env("PKG_CONFIG_ALLOW_CROSS", "1")
            .env("PKG_CONFIG_LIBDIR", subtitle.join("lib/pkgconfig"));
        cmd.env("PKG_CONFIG", cross_pkg_config(inv, target)?);
        if let Some(jar) = env::var_os("ANDROID_JAR") {
            cmd.env("ANDROID_JAR", jar);
        } else if let Some(sdk) =
            env::var_os("ANDROID_HOME").or_else(|| env::var_os("ANDROID_SDK_ROOT"))
        {
            let platforms = PathBuf::from(sdk).join("platforms");
            let mut jars = Vec::new();
            if platforms.is_dir() {
                for item in fs::read_dir(platforms)? {
                    let item = item?;
                    let name = item.file_name();
                    if let Some(version) = name
                        .to_str()
                        .and_then(|s| s.strip_prefix("android-"))
                        .and_then(|s| {
                            let (major, minor) = s.split_once('.').unwrap_or((s, "0"));
                            Some((major.parse::<u32>().ok()?, minor.parse::<u32>().ok()?))
                        })
                    {
                        let jar = item.path().join("android.jar");
                        if jar.is_file() {
                            jars.push((version, jar));
                        }
                    }
                }
            }
            jars.sort_by_key(|(version, _)| *version);
            if let Some((_, jar)) = jars.last() {
                cmd.env("ANDROID_JAR", jar);
            }
        }
        let tools = inv.root.join(".work/tools/bin");
        cmd.env(
            "PATH",
            env::join_paths(
                std::iter::once(tools)
                    .chain(env::split_paths(&env::var_os("PATH").unwrap_or_default())),
            )?,
        );
    }
    if selected.native_vcpkg || selected.renpy {
        cmd.env_remove("SHINKI_EMU_VCPKG_INSTALLED")
            .env(
                "SHINKI_EMU_NATIVE_VCPKG_ROOT",
                inv.root.join(".work/native-vcpkg"),
            )
            .env("SHINKI_EMU_VCPKG_TRIPLET", native_triplet(inv, target)?)
            .env(
                "SHINKI_EMU_NATIVE_DOWNLOADS",
                inv.root.join(".work/native-downloads"),
            );
        if selected.native_vcpkg {
            cmd.env("VCPKG_ROOT", inv.root.join(".work/vcpkg"));
            if component != "manager" {
                cmd.env(
                    "SHINKI_EMU_VCPKG_INSTALLED",
                    native_install(inv, target, component)?,
                );
            }
        }
    }
    if (crate::apple::supported(target) || target.contains("android") || target.ends_with("ohos"))
        && (selected.ffmpeg || selected.subtitle || selected.native_vcpkg || selected.renpy)
    {
        let mut paths = Vec::new();
        if selected.ffmpeg {
            cmd.env("SHINKI_EMU_FFMPEG_INSTALLED", ffmpeg_install(inv, target)?);
            paths.push(ffmpeg_install(inv, target)?.join("lib/pkgconfig"));
        }
        if selected.subtitle {
            paths.push(subtitle.join("lib/pkgconfig"));
        }
        for native in native_cores(inv, target, component) {
            let installation =
                native_install(inv, target, &native.id)?.join(native_triplet(inv, target)?);
            paths.push(installation.join("lib/pkgconfig"));
            paths.push(installation.join("share/pkgconfig"));
        }
        if selected.renpy && target.ends_with("-linux-android") {
            paths.push(renpy_android_root(inv, target)?.join("lib/pkgconfig"));
        }
        // Both private closures are static. Never discover host/Homebrew .pc files.
        for var in [
            "PKG_CONFIG_PATH",
            "PKG_CONFIG_LIBDIR",
            "PKG_CONFIG_SYSROOT_DIR",
        ] {
            cmd.env_remove(var)
                .env_remove(format!("{var}_{target}"))
                .env_remove(format!("{var}_{}", target.replace('-', "_")))
                .env_remove(format!("TARGET_{var}"));
        }
        cmd.env_remove("FFMPEG_DIR")
            .env("PKG_CONFIG_ALLOW_CROSS", "1")
            .env("PKG_CONFIG_ALL_STATIC", "1")
            .env("PKG_CONFIG_LIBDIR", env::join_paths(paths)?);
    }
    if selected.ffmpeg && crate::apple::supported(target) && cfg!(target_os = "macos") {
        let sdk = if target.ends_with("darwin") {
            "macosx"
        } else if target.ends_with("-sim") || target.starts_with("x86_64") {
            "iphonesimulator"
        } else {
            "iphoneos"
        };
        let sysroot = String::from_utf8(capture(Command::new("xcrun").args([
            "--sdk",
            sdk,
            "--show-sdk-path",
        ]))?)?;
        cmd.env(
            format!("BINDGEN_EXTRA_CLANG_ARGS_{}", target.replace('-', "_")),
            format!("-isysroot \"{}\"", sysroot.trim()),
        );
    }
    if selected.ffmpeg && target.contains("android") {
        let host = if cfg!(windows) {
            "windows-x86_64"
        } else if cfg!(target_os = "macos") {
            "darwin-x86_64"
        } else {
            "linux-x86_64"
        };
        let sysroot = ndk_root(inv)?
            .join("toolchains/llvm/prebuilt")
            .join(host)
            .join("sysroot");
        let clang_target = if target.starts_with("aarch64") {
            "aarch64-linux-android28"
        } else {
            "x86_64-linux-android28"
        };
        let clang_args = format!(
            "--target={clang_target} --sysroot=\"{}\"",
            sysroot.display()
        );
        // cargo-ndk 4.1.2 replaces the underscored key with sysroot-only flags.
        // Bindgen prefers the hyphenated target key, preserving the Android API.
        cmd.env(format!("BINDGEN_EXTRA_CLANG_ARGS_{target}"), &clang_args)
            .env(
                format!("BINDGEN_EXTRA_CLANG_ARGS_{}", target.replace('-', "_")),
                &clang_args,
            );
    }
    if target.ends_with("ohos") {
        let sdk = ohos_sdk_native()?;
        let suffix = if cfg!(windows) { ".exe" } else { "" };
        let tools = sdk.join("llvm/bin");
        let target_key = target.replace('-', "_");
        let sysroot = sdk.join("sysroot");
        let clang_args = format!(
            "--target=aarch64-linux-ohos --sysroot=\"{}\"",
            sysroot.display()
        );
        // The cc crate splits *FLAGS on whitespace without honoring quotes, and
        // the DevEco path contains a space: pass the flags as a response file.
        let response = inv.root.join(".work/ohos/clang-target.rsp");
        fs::create_dir_all(response.parent().context("response file parent")?)?;
        fs::write(
            &response,
            format!(
                "--target=aarch64-linux-ohos\n\"--sysroot={}\"\n",
                sysroot.to_string_lossy().replace('\\', "/")
            ),
        )?;
        let cc_flags = format!("@{}", response.to_string_lossy().replace('\\', "/"));
        if cc_flags.contains(char::is_whitespace) {
            bail!("the repository path must not contain whitespace for OHOS builds");
        }
        // Rust's default linker name (aarch64-unknown-linux-ohos-clang) is a
        // shell wrapper in the SDK: link with the clang driver and give every
        // final link (Manager and dependency cdylibs alike) target and sysroot.
        // With --target, Cargo applies these flags to target crates only.
        let existing = env::var("CARGO_ENCODED_RUSTFLAGS").unwrap_or_else(|_| {
            env::var("RUSTFLAGS")
                .unwrap_or_default()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join("\x1f")
        });
        let link = format!("-C\x1flink-arg={cc_flags}");
        cmd.env(
            "CARGO_ENCODED_RUSTFLAGS",
            if existing.is_empty() {
                link
            } else {
                format!("{existing}\x1f{link}")
            },
        );
        cmd.env("OHOS_SDK_NATIVE", &sdk)
            .env(
                format!("CARGO_TARGET_{}_LINKER", target_key.to_ascii_uppercase()),
                tools.join(format!("clang{suffix}")),
            )
            .env(
                format!("CC_{target_key}"),
                tools.join(format!("clang{suffix}")),
            )
            .env(
                format!("CXX_{target_key}"),
                tools.join(format!("clang++{suffix}")),
            )
            .env(
                format!("AR_{target_key}"),
                tools.join(format!("llvm-ar{suffix}")),
            )
            .env(format!("CFLAGS_{target_key}"), &cc_flags)
            .env(format!("CXXFLAGS_{target_key}"), &cc_flags)
            // OHOS ships libc++ (no libstdc++); apps bundle libc++_shared.so.
            // Read by cc and luau0-src, which otherwise default to stdc++.
            .env(format!("CXXSTDLIB_{target_key}"), "c++_shared")
            .env(
                format!("BINDGEN_EXTRA_CLANG_ARGS_{target_key}"),
                &clang_args,
            )
            // The cmake crate drives the SDK's ohos.toolchain.cmake; use the
            // SDK's CMake and Ninja, not the host default generator (NMake on
            // Windows).
            .env(format!("CMAKE_GENERATOR_{target_key}"), "Ninja")
            .env(
                "PATH",
                env::join_paths(
                    std::iter::once(sdk.join("build-tools/cmake/bin"))
                        .chain(env::split_paths(&env::var_os("PATH").unwrap_or_default())),
                )?,
            );
        if selected.ffmpeg || selected.subtitle {
            cmd.env("PKG_CONFIG", cross_pkg_config(inv, target)?);
        }
    }
    Ok(())
}

/// pkg-config for Android/OHOS cross builds. On Windows this is vcpkg's native
/// pkgconf, not an unrelated Perl/MSYS wrapper from PATH (which can drop or
/// rewrite cross include flags).
fn cross_pkg_config(inv: &Inventory, target: &str) -> Result<PathBuf> {
    if let Some(path) = env::var_os("PKG_CONFIG") {
        return Ok(PathBuf::from(path));
    }
    if cfg!(windows) {
        let tools = inv.root.join(".work/vcpkg-downloads/tools/msys2");
        return walkdir::WalkDir::new(&tools)
            .into_iter()
            .filter_map(|e| e.ok())
            .find(|e| e.file_type().is_file() && e.path().ends_with("mingw64/bin/pkgconf.exe"))
            .map(|e| e.path().to_owned())
            .with_context(|| {
                format!("vcpkg's native pkgconf is missing; run cargo xtask deps --target {target}")
            });
    }
    executable("pkgconf")
        .or_else(|| executable("pkg-config"))
        .with_context(|| format!("pkg-config is required for {target} native dependencies"))
}
fn prepare_ffmpeg(inv: &Inventory, target: &str, binary: &Path) -> Result<()> {
    let root = ffmpeg_root(inv, target);
    fs::create_dir_all(&root)?;
    let mut command = vcpkg_install_command(inv, binary)?;
    command
        .arg("--x-wait-for-lock")
        .arg(format!(
            "--x-manifest-root={}",
            inv.root.join("build/vcpkg/ffmpeg").display()
        ))
        .arg(format!(
            "--overlay-triplets={}",
            inv.root.join("build/vcpkg/apple-triplets").display()
        ))
        .arg(format!(
            "--overlay-triplets={}",
            inv.root.join("build/vcpkg/mobile-triplets").display()
        ))
        .args(["--triplet", &ffmpeg_triplet(inv, target)?])
        .arg(format!(
            "--x-install-root={}",
            root.join("installed").display()
        ))
        .args([
            "--clean-buildtrees-after-build",
            "--clean-packages-after-build",
        ])
        .env("VCPKG_DISABLE_METRICS", "1")
        .env_remove("MACOSX_DEPLOYMENT_TARGET")
        .env_remove("IPHONEOS_DEPLOYMENT_TARGET");
    if target.contains("android") {
        command.env("ANDROID_NDK_HOME", ndk_root(inv)?);
    }
    if target.ends_with("ohos") {
        command.env("OHOS_SDK_NATIVE", ohos_sdk_native()?);
    }
    run(&mut command)?;
    fs::copy(
        binary.parent().context("vcpkg root")?.join(".vcpkg-root"),
        root.join(".vcpkg-root"),
    )?;
    if target == "x86_64-unknown-linux-gnu" {
        prepare_linux_runtime(inv, target, "ffmpeg")?;
    }
    Ok(())
}

pub fn prepare(
    inv: &Inventory,
    target: &str,
    core: Option<&str>,
    allow_dirty_sources: bool,
) -> Result<()> {
    let target_spec = inv.target(target)?;
    if let Some(core) = core {
        if inv
            .core(core)?
            .static_manager_targets
            .iter()
            .any(|triple| triple == target)
        {
            inv.require_supported("manager", target)?;
        } else {
            inv.require_supported(core, target)?;
        }
    }
    let selected = needs(inv, target, core.unwrap_or("manager"))?;
    if crate::apple::supported(target) {
        crate::apple::preflight(target)?;
        if (selected.subtitle || selected.ffmpeg || selected.native_vcpkg || selected.renpy)
            && !cfg!(target_os = "macos")
        {
            bail!("Apple native dependency preparation requires a native macOS host with Xcode");
        }
    }
    if !matches!(
        target_spec.platform.as_str(),
        "windows" | "android" | "linux" | "macos" | "ios" | "ohos"
    ) {
        bail!("no dependency preparation path for blocked target {target}");
    }
    if let Some(id) = core {
        crate::sources::prepare(inv, inv.core(id)?, allow_dirty_sources)?;
    }
    if core.is_none() {
        for embedded in inv.cores.iter().filter(|entry| {
            entry
                .static_manager_targets
                .iter()
                .any(|triple| triple == target)
                && entry.source.is_some()
        }) {
            crate::sources::prepare(inv, embedded, allow_dirty_sources)?;
        }
    }
    if crate::apple::supported(target)
        && !(selected.subtitle || selected.ffmpeg || selected.native_vcpkg || selected.renpy)
    {
        return doctor(inv, target, core);
    }
    if target_spec.platform == "linux"
        && !(selected.subtitle || selected.ffmpeg || selected.native_vcpkg || selected.renpy)
    {
        return doctor(inv, target, core);
    }
    let pins = lock(inv)?;
    prepare_native_inputs(inv, &pins, selected)?;
    if selected.renpy && target_spec.platform == "android" {
        prepare_renpy_android(inv, target)?;
    }
    if selected.dxc {
        dxc(inv)?;
    }
    if windows_host_bison(selected.bison, cfg!(windows)) {
        let archive = inv.root.join(".work/downloads/win_flex_bison-2.5.25.zip");
        download(&pins.bison_url, &pins.bison_sha256, &archive)?;
        let out = inv.root.join(".work/bison");
        if !out.exists() {
            package::extract_zip(&archive, &out)?;
        }
    }
    if target_spec.platform == "android" {
        let tool = inv.root.join(".work/tools/bin").join(if cfg!(windows) {
            "cargo-ndk.exe"
        } else {
            "cargo-ndk"
        });
        let current = if tool.exists() {
            Some(capture(Command::new(&tool).arg("--version"))?)
        } else {
            None
        };
        if current.as_ref().is_none_or(|version| {
            !String::from_utf8_lossy(version).contains(&pins.cargo_ndk_version)
        }) {
            run(Command::new("cargo")
                .current_dir(&inv.root)
                .args([
                    "install",
                    "--locked",
                    "cargo-ndk",
                    "--version",
                    &format!("={}", pins.cargo_ndk_version),
                    "--root",
                ])
                .arg(inv.root.join(".work/tools")))?;
        }
    }
    if !(selected.subtitle || selected.ffmpeg || selected.native_vcpkg) {
        return Ok(());
    }
    let vcpkg = inv.root.join(".work/vcpkg");
    if !vcpkg.exists() {
        let mut clone = Command::new("git");
        clone.args(["clone", &pins.vcpkg_url]).arg(&vcpkg);
        shinki_emu_build::configure_native_environment(&inv.root.join(".work"), &mut clone)?;
        run(&mut clone)?;
        run(Command::new("git").arg("-C").arg(&vcpkg).args([
            "checkout",
            "--detach",
            &pins.vcpkg_commit,
        ]))?;
    }
    let actual = capture(
        Command::new("git")
            .arg("-C")
            .arg(&vcpkg)
            .args(["rev-parse", "HEAD"]),
    )?;
    if String::from_utf8_lossy(&actual).trim() != pins.vcpkg_commit {
        bail!("vcpkg cache revision differs from dependency lock");
    }
    materialize_registry_history(inv, &vcpkg, &pins)?;
    let binary = vcpkg.join(if cfg!(windows) { "vcpkg.exe" } else { "vcpkg" });
    if !binary.is_file() {
        let mut bootstrap = if cfg!(windows) {
            let mut command = Command::new("cmd");
            command.arg("/c").arg(vcpkg.join("bootstrap-vcpkg.bat"));
            command
        } else {
            let mut command = Command::new("sh");
            command.arg(vcpkg.join("bootstrap-vcpkg.sh"));
            command
        };
        bootstrap.arg("-disableMetrics");
        shinki_emu_build::configure_native_environment(&inv.root.join(".work"), &mut bootstrap)?;
        run(&mut bootstrap)?;
    }
    if selected.subtitle {
        let subtitle = subtitle_source(inv, target, core)?;
        let triplet = if target_spec.platform == "android" {
            if target_spec.architecture == "aarch64" {
                "arm64-android-dynbidi"
            } else {
                "x64-android-dynbidi"
            }
        } else if target_spec.platform == "linux" {
            "x64-linux-dynamic"
        } else if target_spec.platform == "ohos" {
            "arm64-ohos"
        } else if let Some(triplet) = apple_subtitle_triplet(target) {
            triplet
        } else {
            &pins.subtitle_windows_triplet
        };
        // vcpkg manifest mode removes packages for other triplets from an install root.
        // Keep each target independent, including device and simulator SDKs.
        let install = if target_spec.platform != "windows" {
            inv.root
                .join(".work/vcpkg-subtitle")
                .join(target)
                .join("installed")
        } else {
            inv.root.join(".work/vcpkg-subtitle/installed")
        };
        fs::create_dir_all(install.parent().unwrap())?;
        let mut command = vcpkg_install_command(inv, &binary)?;
        command
            .arg("--x-wait-for-lock")
            .arg(format!(
                "--x-manifest-root={}",
                inv.root.join("build/vcpkg/subtitle").display()
            ))
            .arg(format!(
                "--overlay-triplets={}",
                subtitle.join("triplets").display()
            ))
            .arg(format!(
                "--overlay-triplets={}",
                inv.root.join("build/vcpkg/apple-triplets").display()
            ))
            .arg(format!(
                "--overlay-triplets={}",
                inv.root.join("build/vcpkg/mobile-triplets").display()
            ))
            .args(["--triplet", triplet])
            .arg(format!("--x-install-root={}", install.display()))
            .args([
                "--clean-buildtrees-after-build",
                "--clean-packages-after-build",
            ])
            .env("VCPKG_DISABLE_METRICS", "1");
        if target_spec.platform == "android" {
            command.env("ANDROID_NDK_HOME", ndk_root(inv)?);
        }
        if target_spec.platform == "ohos" {
            command.env("OHOS_SDK_NATIVE", ohos_sdk_native()?);
        }
        // Apple deployment targets come from build/vcpkg/apple-triplets. Never
        // export them here: they would also retarget the macOS host triplet.
        for var in ["MACOSX_DEPLOYMENT_TARGET", "IPHONEOS_DEPLOYMENT_TARGET"] {
            command.env_remove(var);
        }
        run(&mut command)?;
        fs::copy(
            vcpkg.join(".vcpkg-root"),
            install.parent().unwrap().join(".vcpkg-root"),
        )?;
        if target_spec.platform == "linux" {
            prepare_linux_runtime(inv, target, "subtitle")?;
        }
    }
    if selected.ffmpeg {
        prepare_ffmpeg(inv, target, &binary)?;
    }
    for native in native_cores(inv, target, core.unwrap_or("manager")) {
        let source = inv
            .root
            .join(native.manifest.components().take(2).collect::<PathBuf>());
        if !source.join("vcpkg.json").is_file() {
            bail!(
                "native core manifest missing vcpkg.json: {}",
                source.display()
            );
        }
        let installed = native_install(inv, target, &native.id)?;
        let mut command = vcpkg_install_command(inv, &binary)?;
        command
            .arg("--x-wait-for-lock")
            .arg(format!("--x-manifest-root={}", source.display()))
            .arg(format!("--x-install-root={}", installed.display()))
            .args([
                "--triplet",
                &native_triplet(inv, target)?,
                "--clean-buildtrees-after-build",
                "--clean-packages-after-build",
            ])
            .env("VCPKG_DISABLE_METRICS", "1");
        command
            .arg(format!(
                "--overlay-triplets={}",
                inv.root.join("build/vcpkg/apple-triplets").display()
            ))
            .arg(format!(
                "--overlay-triplets={}",
                inv.root.join("build/vcpkg/mobile-triplets").display()
            ));
        if target.contains("android") {
            command.env("ANDROID_NDK_HOME", ndk_root(inv)?);
        }
        if target.ends_with("ohos") {
            command.env("OHOS_SDK_NATIVE", ohos_sdk_native()?);
        }
        let overlay = source.join("vcpkg/ports");
        if overlay.is_dir() {
            command.arg(format!("--overlay-ports={}", overlay.display()));
        }
        run(&mut command)?;
        if target_spec.platform == "linux" {
            prepare_linux_runtime_tree(
                &installed.join("x64-linux-dynamic"),
                &linux_runtime(inv, target, &native.id),
                target,
            )?;
        }
    }
    Ok(())
}

pub fn prepare_all(inv: &Inventory, target: &str, allow_dirty_sources: bool) -> Result<()> {
    inv.target(target)?;
    let mut prepared = false;
    for core in &inv.cores {
        if let Some(reason) = inv.blocked(&core.id, target)? {
            eprintln!("{}: blocked: {reason}", core.id);
            continue;
        }
        prepare(inv, target, Some(&core.id), allow_dirty_sources)?;
        prepared = true;
    }
    if !prepared {
        bail!("no standalone core supports {target}; select Manager prerequisites instead");
    }
    Ok(())
}

#[cfg(test)]
mod native_tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn windows_host_prepares_bison_for_android_core() {
        let inv = Inventory::load(Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()).unwrap();
        let selected = needs(&inv, "aarch64-linux-android", "kirikiroid2-web").unwrap();
        assert!(selected.bison);
    }

    #[test]
    fn native_manifest_installations_keep_each_target_closure() {
        let inv = Inventory::load(Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()).unwrap();
        let windows = native_install(&inv, "x86_64-pc-windows-msvc", "kirikiroid2-web").unwrap();
        let arm = native_install(&inv, "aarch64-linux-android", "kirikiroid2-web").unwrap();
        let x64 = native_install(&inv, "x86_64-linux-android", "kirikiroid2-web").unwrap();
        assert!(windows.ends_with("kirikiroid2-web/installed-x64-windows"));
        assert!(arm.ends_with("kirikiroid2-web/installed-arm64-android-ffmpeg"));
        assert!(x64.ends_with("kirikiroid2-web/installed-x64-android-ffmpeg"));
        assert_ne!(windows, arm);
        assert_ne!(arm, x64);
    }

    #[test]
    fn ndk_requires_the_exact_stable_revision() {
        let temp = tempfile::tempdir().unwrap();
        let properties = temp.path().join("source.properties");
        for revision in ["30.0.16248370-beta2", "30.0.15729638", "29.0.14206865"] {
            fs::write(&properties, format!("Pkg.Revision = {revision}\n")).unwrap();
            assert!(validate_ndk_revision(temp.path(), "30.0.16248370").is_err());
        }
        fs::write(
            &properties,
            "Pkg.Desc = Android NDK\nPkg.Revision = 30.0.16248370\n",
        )
        .unwrap();
        validate_ndk_revision(temp.path(), "30.0.16248370").unwrap();
    }

    fn cached_zip(root: &Path, entries: &[(&str, &[u8])]) -> (PathBuf, NativeArchive) {
        let archive = root.join(".work/downloads/fixture.zip");
        fs::create_dir_all(archive.parent().unwrap()).unwrap();
        let mut zip = zip::ZipWriter::new(fs::File::create(&archive).unwrap());
        for (name, bytes) in entries {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
        let pin = NativeArchive {
            version: "public-fixture".into(),
            url: "https://not-used.invalid/fixture.zip".into(),
            sha256: package::sha256(&archive).unwrap(),
            file_name: "fixture.zip".into(),
            format: "zip".into(),
            strip_prefix: "fixture".into(),
            include: Vec::new(),
            required: vec!["source.txt".into()],
        };
        (archive, pin)
    }
    #[cfg(unix)]
    #[test]
    fn native_archives_preserve_executable_inputs() {
        let temp = tempfile::tempdir().unwrap();
        let bytes = b"#!/bin/sh\nexit 7\n";
        for format in ["zip", "tar-gz"] {
            let input = temp.path().join(format);
            if format == "zip" {
                let mut zip = zip::ZipWriter::new(fs::File::create(&input).unwrap());
                zip.start_file(
                    "fixture/tool",
                    zip::write::SimpleFileOptions::default().unix_permissions(0o755),
                )
                .unwrap();
                zip.write_all(bytes).unwrap();
                zip.finish().unwrap();
            } else {
                let gzip = flate2::write::GzEncoder::new(
                    fs::File::create(&input).unwrap(),
                    flate2::Compression::default(),
                );
                let mut tar = tar::Builder::new(gzip);
                let mut header = tar::Header::new_gnu();
                header.set_size(bytes.len() as u64);
                header.set_mode(0o755);
                header.set_cksum();
                tar.append_data(&mut header, "fixture/tool", &bytes[..])
                    .unwrap();
                tar.into_inner().unwrap().finish().unwrap();
            }
            let pin = NativeArchive {
                version: "fixture".into(),
                url: "https://not-used.invalid/fixture".into(),
                sha256: package::sha256(&input).unwrap(),
                file_name: format.into(),
                format: format.into(),
                strip_prefix: "fixture".into(),
                include: Vec::new(),
                required: vec!["tool".into()],
            };
            let out = temp.path().join(format!("out-{format}"));
            extract_native_archive(&pin, &input, &out, "bin").unwrap();
            assert_eq!(
                Command::new(out.join("bin/tool")).status().unwrap().code(),
                Some(7)
            );
        }
    }

    #[test]
    fn verified_native_tree_reuses_files_without_an_archive_or_network() {
        let temp = tempfile::tempdir().unwrap();
        let (archive, pin) = cached_zip(
            temp.path(),
            &[
                ("fixture/source.txt", b"public source"),
                ("fixture/.gitignore", b"generated/\n"),
            ],
        );
        let layers = [NativeLayer {
            archive: &pin,
            destination: "",
        }];
        let tree = prepare_native_tree(temp.path(), "fixture", &layers, None).unwrap();
        fs::remove_file(&archive).unwrap();
        assert_eq!(
            prepare_native_tree(temp.path(), "fixture", &layers, None).unwrap(),
            tree
        );
        assert_eq!(fs::read(tree.join("source.txt")).unwrap(), b"public source");
        assert_eq!(fs::read(tree.join(".gitignore")).unwrap(), b"generated/\n");
        assert!(!archive.exists());
        fs::write(tree.join("source.txt"), b"user modification").unwrap();
        assert!(prepare_native_tree(temp.path(), "fixture", &layers, None).is_err());
        assert_eq!(
            fs::read(tree.join("source.txt")).unwrap(),
            b"user modification"
        );
        assert!(!archive.exists());
    }

    #[test]
    fn bad_native_archive_hash_never_publishes_a_partial_tree() {
        let temp = tempfile::tempdir().unwrap();
        let (archive, pin) = cached_zip(temp.path(), &[("fixture/source.txt", b"public source")]);
        fs::write(&archive, b"changed download").unwrap();
        let layers = [NativeLayer {
            archive: &pin,
            destination: "",
        }];
        assert!(prepare_native_tree(temp.path(), "fixture", &layers, None).is_err());
        assert!(!temp.path().join(".work/native-downloads/fixture").exists());
        assert_eq!(fs::read(&archive).unwrap(), b"changed download");
    }

    #[test]
    fn unsafe_native_archive_cleans_its_staging_and_preserves_inputs() {
        let temp = tempfile::tempdir().unwrap();
        let (archive, pin) = cached_zip(
            temp.path(),
            &[
                ("fixture/source.txt", b"public source"),
                ("fixture/../escaped.txt", b"unsafe path"),
            ],
        );
        let before = package::sha256(&archive).unwrap();
        let layers = [NativeLayer {
            archive: &pin,
            destination: "",
        }];
        assert!(prepare_native_tree(temp.path(), "fixture", &layers, None).is_err());
        assert!(!temp.path().join(".work/native-downloads/fixture").exists());
        assert!(
            !temp
                .path()
                .join(".work/native-downloads/escaped.txt")
                .exists()
        );
        assert!(
            fs::read_dir(temp.path().join(".work/native-downloads"))
                .unwrap()
                .next()
                .is_none()
        );
        assert_eq!(package::sha256(&archive).unwrap(), before);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_runtime_links_cannot_copy_files_outside_the_pinned_installation() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let mut inv =
            Inventory::load(Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()).unwrap();
        inv.root = temp.path().to_owned();
        let target = "x86_64-unknown-linux-gnu";
        let lib = linux_install(&inv, target, "subtitle").join("lib");
        fs::create_dir_all(&lib).unwrap();
        let outside = temp.path().join("private.so");
        fs::write(&outside, b"not a redistributable input").unwrap();
        symlink(&outside, lib.join("libass.so.9")).unwrap();
        assert!(prepare_linux_runtime(&inv, target, "subtitle").is_err());
        assert_eq!(fs::read(&outside).unwrap(), b"not a redistributable input");
        assert!(
            fs::read_dir(linux_runtime(&inv, target, "subtitle"))
                .unwrap()
                .next()
                .is_none()
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_runtime_copy_rejects_dangling_output_links() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let mut inv =
            Inventory::load(Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()).unwrap();
        inv.root = temp.path().to_owned();
        let target = "x86_64-unknown-linux-gnu";
        let lib = linux_install(&inv, target, "subtitle").join("lib");
        let runtime = linux_runtime(&inv, target, "subtitle");
        fs::create_dir_all(&lib).unwrap();
        fs::create_dir_all(&runtime).unwrap();
        fs::write(lib.join("libass.so.9"), b"synthetic source").unwrap();
        let outside = temp.path().join("must-not-be-created");
        symlink(&outside, runtime.join("libass.so.9")).unwrap();
        assert!(prepare_linux_runtime(&inv, target, "subtitle").is_err());
        assert!(!outside.exists());
    }
}
