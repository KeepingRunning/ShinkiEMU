use crate::{
    binary,
    build::{self, BuildResult},
    cores::Inventory,
    dependencies,
};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use shinki_emu_build::BuildAsset;
use std::{
    collections::BTreeSet,
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
};

pub fn portable(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("empty package path");
    }
    for p in name.split('/') {
        let stem = p
            .split('.')
            .next()
            .unwrap_or("")
            .trim_end()
            .to_ascii_uppercase();
        if p.is_empty()
            || !p.as_bytes()[0].is_ascii_alphanumeric() && p.as_bytes()[0] != b'_'
            || !p
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_.+-@~ ".contains(&b))
            || p.ends_with(['.', ' '])
            || ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
            || (stem.len() == 4
                && (stem.starts_with("COM") || stem.starts_with("LPT"))
                && matches!(stem.as_bytes()[3], b'1'..=b'9'))
        {
            bail!("unsafe package path {name}");
        }
    }
    Ok(())
}
pub fn plain(path: &Path) -> Result<()> {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    for component in absolute.ancestors() {
        let m = fs::symlink_metadata(component)
            .with_context(|| format!("reading {}", component.display()))?;
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if m.file_attributes() & 0x400 != 0 {
                bail!("reparse input {}", component.display());
            }
        }
        if m.file_type().is_symlink() || (!m.is_file() && !m.is_dir()) {
            bail!("linked or special input {}", component.display());
        }
    }
    Ok(())
}
pub fn sha256(path: &Path) -> Result<String> {
    plain(path)?;
    let mut f = fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
    }
    Ok(hex::encode(hash.finalize()))
}
pub fn tree(root: &Path) -> Result<Vec<(PathBuf, String)>> {
    plain(root)?;
    if !root.is_dir() {
        bail!("not a directory {}", root.display());
    }
    let mut result = Vec::new();
    for e in walkdir::WalkDir::new(root).follow_links(false) {
        let e = e?;
        plain(e.path())?;
        if e.file_type().is_file() {
            let relative = e
                .path()
                .strip_prefix(root)?
                .to_str()
                .context("non UTF-8 package path")?
                .replace('\\', "/");
            portable(&relative)?;
            result.push((e.path().to_owned(), relative));
        }
    }
    result.sort_by(|a, b| a.1.cmp(&b.1));
    Ok(result)
}
pub fn publish_noreplace(source: &Path, dest: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        fs::rename(source, dest)?;
    }
    #[cfg(target_os = "linux")]
    {
        use std::{ffi::CString, os::unix::ffi::OsStrExt};
        let a = CString::new(source.as_os_str().as_bytes())?;
        let b = CString::new(dest.as_os_str().as_bytes())?;
        if unsafe {
            libc::renameat2(
                libc::AT_FDCWD,
                a.as_ptr(),
                libc::AT_FDCWD,
                b.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        } != 0
        {
            return Err(io::Error::last_os_error().into());
        }
    }
    #[cfg(target_os = "macos")]
    {
        use std::{ffi::CString, os::unix::ffi::OsStrExt};
        let a = CString::new(source.as_os_str().as_bytes())?;
        let b = CString::new(dest.as_os_str().as_bytes())?;
        if unsafe { libc::renamex_np(a.as_ptr(), b.as_ptr(), libc::RENAME_EXCL) } != 0 {
            return Err(io::Error::last_os_error().into());
        }
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        bail!("no atomic no-replace publish primitive on this host");
    }
    Ok(())
}
pub fn atomic_directory(out: &Path, f: impl FnOnce(&Path) -> Result<()>) -> Result<()> {
    if fs::symlink_metadata(out).is_ok() {
        bail!("output must be new: {}", out.display());
    }
    let parent = out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    plain(parent)?;
    let temp = tempfile::Builder::new()
        .prefix(".shinki-stage-")
        .tempdir_in(parent)?;
    let stage = temp.path().join("package");
    fs::create_dir(&stage)?;
    f(&stage)?;
    publish_noreplace(&stage, out)
}
pub fn copy_entries(entries: &[(PathBuf, String)], stage: &Path) -> Result<()> {
    let mut seen = BTreeSet::new();
    for (source, name) in entries {
        portable(name)?;
        plain(source)?;
        if !source.is_file() {
            bail!("not a regular file {}", source.display());
        }
        if !seen.insert(name.to_ascii_lowercase()) {
            bail!("case-insensitive package collision: {name}");
        }
        // Detect file/directory collisions before copying anything.
        for ancestor in Path::new(name)
            .ancestors()
            .skip(1)
            .filter(|p| !p.as_os_str().is_empty())
        {
            if seen.contains(
                &ancestor
                    .to_string_lossy()
                    .replace('\\', "/")
                    .to_ascii_lowercase(),
            ) {
                bail!("file/directory collision {name}");
            }
        }
    }
    for (source, name) in entries {
        let dest = stage.join(name);
        fs::create_dir_all(dest.parent().unwrap())?;
        fs::copy(source, &dest)?;
        if sha256(source)? != sha256(&dest)? {
            bail!("input changed during packaging: {}", source.display());
        }
    }
    Ok(())
}
pub fn extract_zip(archive: &Path, out: &Path) -> Result<()> {
    atomic_directory(out, |stage| {
        let mut z = zip::ZipArchive::new(fs::File::open(archive)?)?;
        let mut names = BTreeSet::new();
        for i in 0..z.len() {
            let mut item = z.by_index(i)?;
            if item.is_symlink() {
                bail!("linked archive input");
            }
            let name = item.name()?.trim_end_matches('/').to_owned();
            portable(&name)?;
            if !names.insert(name.to_ascii_lowercase()) {
                bail!("archive path collision");
            }
            let dest = stage.join(&name);
            if item.is_dir() {
                fs::create_dir_all(dest)?;
            } else {
                fs::create_dir_all(dest.parent().unwrap())?;
                io::copy(
                    &mut item,
                    &mut fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(dest)?,
                )?;
            }
        }
        Ok(())
    })
}
pub fn zip_entries(entries: &[(PathBuf, String)], out: &Path) -> Result<()> {
    if fs::symlink_metadata(out).is_ok() {
        bail!("archive output must be new");
    }
    let parent = out.parent().context("archive parent")?;
    fs::create_dir_all(parent)?;
    plain(parent)?;
    let mut entries = entries.to_vec();
    entries.sort_by(|a, b| a.1.cmp(&b.1));
    let mut seen = BTreeSet::new();
    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    {
        let mut zip = zip::ZipWriter::new(tmp.as_file_mut());
        for (source, name) in entries {
            portable(&name)?;
            plain(&source)?;
            if !seen.insert(name.to_ascii_lowercase()) {
                bail!("duplicate archive path");
            }
            #[cfg(unix)]
            let permissions = {
                use std::os::unix::fs::PermissionsExt;
                if fs::metadata(&source)?.permissions().mode() & 0o111 != 0 {
                    0o755
                } else {
                    0o644
                }
            };
            #[cfg(not(unix))]
            let permissions = 0o644;
            let opt = zip::write::SimpleFileOptions::default()
                .last_modified_time(zip::DateTime::default())
                .large_file(fs::metadata(&source)?.len() >= u64::from(u32::MAX))
                .unix_permissions(permissions)
                .compression_method(if name.starts_with("downloads/") {
                    zip::CompressionMethod::Stored
                } else {
                    zip::CompressionMethod::Deflated
                });
            zip.start_file(name, opt)?;
            io::copy(&mut fs::File::open(source)?, &mut zip)?;
        }
        zip.finish()?;
    }
    tmp.persist_noclobber(out).map_err(|e| e.error)?;
    Ok(())
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileRecord {
    pub path: String,
    pub sha256: String,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorePackage {
    pub schema: String,
    pub plugin_id: String,
    pub library: String,
    pub version: String,
    pub abi: String,
    pub files: Vec<FileRecord>,
}
fn asset_entries(asset: &BuildAsset) -> Result<Vec<(PathBuf, String)>> {
    portable(&asset.dest)?;
    plain(&asset.source)?;
    if asset.source.is_dir() {
        let entries = tree(&asset.source)?;
        if entries.is_empty() {
            bail!("empty resource directory");
        }
        Ok(entries
            .into_iter()
            .map(|(s, n)| (s, format!("{}/{n}", asset.dest)))
            .collect())
    } else {
        Ok(vec![(asset.source.clone(), asset.dest.clone())])
    }
}
fn reject_overlap(source: &Path, out: &Path) -> Result<()> {
    let mut destination = PathBuf::new();
    for component in std::path::absolute(out)?.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                destination.pop();
            }
            other => destination.push(other.as_os_str()),
        }
    }
    let mut ancestor = destination.as_path();
    loop {
        match fs::symlink_metadata(ancestor) {
            Ok(_) => break,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                ancestor = ancestor.parent().context("output has no existing parent")?;
            }
            Err(error) => return Err(error.into()),
        }
    }
    plain(ancestor)?;
    let destination = fs::canonicalize(ancestor)?.join(destination.strip_prefix(ancestor)?);
    let source = fs::canonicalize(source)?;
    if source.starts_with(&destination) || (source.is_dir() && destination.starts_with(&source)) {
        bail!("output overlaps package input {}", source.display());
    }
    Ok(())
}
fn write_provenance(result: &BuildResult, path: &Path) -> Result<()> {
    let info = binary::inspect(&result.library, &result.target, result.core_build.is_some())?;
    let provenance = serde_json::json!({
        "schema":"shinki.emu.build_identity.v1","component":result.component,"target":result.target,
        "profile":result.profile,"source_commit":result.source_commit,"source_dirty":result.source_dirty,"toolchain":result.toolchain,
        "features":result.features,"abi":result.core_build.as_ref().map(|m|m.abi.as_str()),
        "artifact_kind":result.core_build.as_ref().map(|m|m.artifact_kind.as_str()).unwrap_or("manager"),
        "library":result.library.file_name().and_then(|s|s.to_str()),
        "sha256":sha256(&result.library)?,"native_dependencies":info.dependencies,"runtime_search_paths":info.rpaths,
        "system_dependencies":info.dependencies.iter().filter(|name| binary::system(name, &result.target)).collect::<Vec<_>>(),
        "macho_code_signature_present":info.signed
    });
    fs::write(path, serde_json::to_vec_pretty(&provenance)?)?;
    Ok(())
}
pub fn stage_core(
    result: &BuildResult,
    version: &str,
    out: &Path,
    search: &[PathBuf],
) -> Result<PathBuf> {
    semver::Version::parse(version)?;
    let m = result
        .core_build
        .as_ref()
        .context("missing core-build manifest")?;
    m.validate()?;
    binary::inspect(&result.library, &result.target, true)?;
    if m.licenses.is_empty() {
        bail!("core package requires license files");
    }
    let library = result
        .library
        .file_name()
        .and_then(|s| s.to_str())
        .context("core filename")?;
    if !(library.starts_with("shinki_emu_") || library.starts_with("libshinki_emu_")) {
        bail!("core library needs the shinki_emu_ prefix");
    }
    let stem = Path::new(library)
        .file_stem()
        .and_then(|s| s.to_str())
        .context("core stem")?;
    portable(stem)?;
    let mut entries = vec![(result.library.clone(), library.into())];
    for asset in m.resources.iter().chain(&m.licenses) {
        entries.extend(asset_entries(asset)?);
    }
    let is_runtime = |path: &Path| {
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_ascii_lowercase();
        name.ends_with(".dll")
            || name.ends_with(".so")
            || name.ends_with(".dylib")
            || name.ends_with(".pyd")
            || name.contains(".so.")
    };
    let mut roots: Vec<PathBuf> = entries
        .iter()
        .filter(|(path, _)| is_runtime(path))
        .map(|(path, _)| path.clone())
        .collect();
    let mut runtimes = std::collections::BTreeMap::<String, PathBuf>::new();
    let mut add_runtime = |path: PathBuf| -> Result<()> {
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .context("runtime filename")?
            .to_owned();
        let key = name.to_ascii_lowercase();
        if key == library.to_ascii_lowercase() {
            if sha256(&path)? != sha256(&result.library)? {
                bail!("runtime conflicts with the core library");
            }
            return Ok(());
        }
        if key.starts_with("shinki_emu_")
            || key.starts_with("libshinki_emu_")
            || ["core.json", "build.json", "licenses", "resources"].contains(&key.as_str())
        {
            bail!("runtime dependency collides with core layout");
        }
        if let Some(existing) = runtimes.get(&key) {
            if existing != &path && sha256(existing)? != sha256(&path)? {
                bail!("conflicting runtime dependency {name}");
            }
        } else {
            runtimes.insert(key, path);
        }
        Ok(())
    };
    if m.artifact_kind == "native-shared" {
        // CMake's recorded install directory also holds modules loaded by name,
        // such as ANGLE; these do not necessarily appear in static imports.
        let directory = result
            .library
            .parent()
            .context("native install directory")?;
        for item in fs::read_dir(directory)? {
            let path = item?.path();
            if result.target == "x86_64-unknown-linux-gnu"
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        let soname = name
                            .find(".so.")
                            .and_then(|offset| {
                                name[offset + 4..]
                                    .find('.')
                                    .map(|end| &name[..offset + 4 + end])
                            })
                            .unwrap_or(name);
                        binary::system(soname, &result.target)
                    })
            {
                continue;
            }
            if path.is_file() && is_runtime(&path) && path != result.library {
                add_runtime(path.clone())?;
                roots.push(path);
            }
        }
    }
    let mut search = search.to_vec();
    search.extend(
        roots
            .iter()
            .filter_map(|path| path.parent().map(Path::to_owned)),
    );
    search.sort();
    search.dedup();
    for root in roots {
        for dependency in binary::closure(&root, &search, &result.target)? {
            add_runtime(dependency)?;
        }
    }
    entries.extend(runtimes.into_values().map(|path| {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        (path, name)
    }));
    reject_overlap(&result.library, out)?;
    for asset in m.resources.iter().chain(&m.licenses) {
        reject_overlap(&asset.source, out)?;
    }
    atomic_directory(out, |stage| {
        let dir = stage.join(stem);
        fs::create_dir(&dir)?;
        copy_entries(&entries, &dir)?;
        let staged_library = dir.join(library);
        if crate::apple::supported(&result.target) {
            crate::apple::sign_binary(&staged_library)?;
        }
        let mut packaged_result = result.clone();
        packaged_result.library = staged_library;
        write_provenance(&packaged_result, &dir.join("build.json"))?;
        let mut files = tree(&dir)?
            .into_iter()
            .map(|(s, n)| {
                Ok(FileRecord {
                    path: n,
                    sha256: sha256(&s)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        files.sort_by(|a, b| a.path.cmp(&b.path));
        let manifest = CorePackage {
            schema: "shinki.emu.core_package.v1".into(),
            plugin_id: m.plugin_id.clone(),
            library: library.into(),
            version: version.into(),
            abi: shinki_emu_build::ABI.into(),
            files,
        };
        fs::write(dir.join("core.json"), serde_json::to_vec_pretty(&manifest)?)?;
        verify_core(&dir)?;
        Ok(())
    })?;
    Ok(out.join(stem))
}
pub fn verify_core(dir: &Path) -> Result<CorePackage> {
    let m: CorePackage = serde_json::from_slice(&fs::read(dir.join("core.json"))?)?;
    if m.schema != "shinki.emu.core_package.v1" || m.abi != shinki_emu_build::ABI {
        bail!("invalid core package protocol");
    }
    portable(&m.plugin_id)?;
    semver::Version::parse(&m.version)?;
    portable(&m.library)?;
    if m.library.contains('/') || Path::new(&m.library).file_stem() != dir.file_name() {
        bail!("core library/stem mismatch");
    }
    let mut expected = BTreeSet::from(["core.json".to_owned()]);
    for f in &m.files {
        portable(&f.path)?;
        if !expected.insert(f.path.to_ascii_lowercase()) {
            bail!("duplicate core file");
        }
        if sha256(&dir.join(&f.path))? != f.sha256 {
            bail!("core file hash mismatch: {}", f.path);
        }
    }
    if !expected.contains(&m.library.to_ascii_lowercase())
        || !expected.iter().any(|s| s.starts_with("licenses/"))
    {
        bail!("core package omits library/licenses");
    }
    let actual = tree(dir)?
        .into_iter()
        .map(|(_, n)| n.to_ascii_lowercase())
        .collect::<BTreeSet<_>>();
    if actual != expected {
        bail!("core package contains unlisted files");
    }
    Ok(m)
}
fn search_dirs(inv: &Inventory, result: &BuildResult) -> Result<Vec<PathBuf>> {
    let selected = dependencies::needs(inv, &result.target, &result.component)?;
    let mut dirs = vec![
        result
            .library
            .parent()
            .context("library parent")?
            .to_owned(),
    ];
    if selected.ffmpeg
        && let Some(runtime) = dependencies::ffmpeg_runtime(inv, &result.target)?
    {
        dirs.push(runtime);
    }
    if selected.subtitle {
        dirs.push(if result.target == "x86_64-unknown-linux-gnu" {
            dependencies::linux_runtime(inv, &result.target, "subtitle")
        } else if crate::apple::supported(&result.target) {
            dependencies::subtitle_install(inv, &result.target)?.join("lib")
        } else {
            dependencies::subtitle_install(inv, &result.target)?.join("bin")
        });
    }
    if selected.dxc {
        dirs.push(inv.root.join(".work/dxc"));
    }
    if let Some(native) = &result.core_build
        && let Some(p) = &native.native_library
    {
        dirs.push(p.parent().context("native library parent")?.to_owned());
        dirs.push(p.parent().context("native library parent")?.join("lib"));
    }
    for native in dependencies::native_cores(inv, &result.target, &result.component) {
        dirs.push(if result.target == "x86_64-unknown-linux-gnu" {
            dependencies::linux_runtime(inv, &result.target, &native.id)
        } else {
            dependencies::native_install(inv, &result.target, &native.id)?
                .join(dependencies::native_triplet(inv, &result.target)?)
                .join(if result.target.contains("windows") {
                    "bin"
                } else {
                    "lib"
                })
        });
    }
    // MSVC runtime redistributables are explicit SDK redistributable inputs, never System32.
    if let Some(redist) = std::env::var_os("VCToolsRedistDir") {
        let root = PathBuf::from(redist).join("x64");
        if root.is_dir() {
            for d in fs::read_dir(root)? {
                let p = d?.path();
                if p.is_dir() {
                    dirs.push(p);
                }
            }
        }
    }
    if result.target.contains("android") {
        let ndk = dependencies::ndk_root(inv)?;
        let host = if cfg!(windows) {
            "windows-x86_64"
        } else if cfg!(target_os = "macos") {
            "darwin-x86_64"
        } else {
            "linux-x86_64"
        };
        if selected.subtitle {
            dirs.push(dependencies::subtitle_install(inv, &result.target)?.join("lib"));
        }
        let ndk_lib = ndk
            .join("toolchains/llvm/prebuilt")
            .join(host)
            .join("sysroot/usr/lib")
            .join(&result.target);
        dirs.push(ndk_lib.clone());
        // cargo-ndk builds Manager with API 26; NDK platform libraries such as
        // libcamera2ndk.so are stored beneath the target/API directory.
        dirs.push(ndk_lib.join("26"));
    }
    Ok(dirs)
}
fn licenses(
    inv: &Inventory,
    result: &BuildResult,
    scratch: &Path,
) -> Result<Vec<(PathBuf, String)>> {
    let selected = dependencies::needs(inv, &result.target, &result.component)?;
    let mut entries = vec![];
    if selected.ffmpeg {
        let share = dependencies::ffmpeg_install(inv, &result.target)?.join("share");
        entries.push((
            share.join("ffmpeg/copyright"),
            "licenses/ffmpeg/copyright".into(),
        ));
        for entry in fs::read_dir(share)? {
            let entry = entry?;
            if entry.file_name() == "ffmpeg" {
                continue;
            }
            let copyright = entry.path().join("copyright");
            if copyright.is_file() {
                entries.push((
                    copyright,
                    format!(
                        "licenses/ffmpeg/{}/copyright",
                        entry.file_name().to_string_lossy()
                    ),
                ));
            }
        }
    }
    if selected.subtitle
        && (result.target == "x86_64-unknown-linux-gnu" || crate::apple::supported(&result.target))
    {
        let share = dependencies::subtitle_install(inv, &result.target)?.join("share");
        for entry in fs::read_dir(share)? {
            let entry = entry?;
            let copyright = entry.path().join("copyright");
            if copyright.is_file() {
                entries.push((
                    copyright,
                    format!(
                        "licenses/subtitle/{}/copyright",
                        entry.file_name().to_string_lossy()
                    ),
                ));
            }
        }
    } else if selected.subtitle {
        let share = dependencies::subtitle_install(inv, &result.target)?.join("share");
        for port in [
            "libass", "freetype", "fribidi", "harfbuzz", "brotli", "bzip2", "libpng", "zlib",
        ] {
            entries.push((
                share.join(port).join("copyright"),
                format!("licenses/subtitle/{port}/copyright"),
            ));
        }
    }
    for native in dependencies::native_cores(inv, &result.target, &result.component) {
        let native_share = dependencies::native_install(inv, &result.target, &native.id)?
            .join(dependencies::native_triplet(inv, &result.target)?)
            .join("share");
        for item in fs::read_dir(native_share)? {
            let path = item?.path().join("copyright");
            if path.is_file() {
                let port = path
                    .parent()
                    .and_then(Path::file_name)
                    .context("native license port")?
                    .to_string_lossy();
                entries.push((
                    path.clone(),
                    format!("licenses/native-vcpkg/{}/{port}/copyright", native.id),
                ));
            }
        }
    }
    if result.component == "manager" {
        entries.extend([
            (
                inv.root
                    .join("Source/Manager/shinki-emu-manager-ui-slint/material-1.1.0/LICENSE.md"),
                "licenses/slint-material/LICENSE.md".into(),
            ),
            (
                inv.root
                    .join("Source/Manager/shinki-emu-manager-ui-slint/ui/icons/LICENSE"),
                "licenses/material-symbols/LICENSE".into(),
            ),
        ]);
    }
    let package = if result.component == "manager" {
        "shinki-emu-manager"
    } else {
        &inv.core(&result.component)?.package
    };
    entries.extend(crate::rust_licenses::entries(
        &crate::rust_licenses::metadata(inv, &result.target, &result.component)?,
        package,
        scratch,
    )?);
    Ok(entries)
}
fn metadata_entries(inv: &Inventory, stage: &Path) -> Result<Vec<(PathBuf, String)>> {
    let out = stage.join("snapshot");
    // Use the same metadata command as the online snapshot, with a new private output.
    crate::metadata::run(crate::metadata::MetadataCommand::Build {
        out: out.clone(),
        root: inv.root.join("Metadata"),
    })?;
    let root = if out.join("v1/index.json").is_file() {
        out.join("v1")
    } else {
        out
    };
    let index: serde_json::Value = serde_json::from_slice(&fs::read(root.join("index.json"))?)?;
    if index["schema"] != "shinki.emu.metadata.index.v1" {
        bail!("invalid metadata index");
    }
    let files = index["files"].as_array().context("metadata index files")?;
    if files.len() > 4096 {
        bail!("too many metadata files");
    }
    let mut total = 0u64;
    let mut entries = vec![(root.join("index.json"), "metadata/index.json".into())];
    let mut seen = BTreeSet::new();
    for r in files {
        let name = r["path"].as_str().context("metadata path")?;
        portable(name)?;
        if !seen.insert(name.to_ascii_lowercase()) || name == "index.json" {
            bail!("duplicate metadata entry");
        }
        let path = root.join(name);
        let size = r["size"].as_u64().context("metadata size")?;
        total = total.checked_add(size).context("metadata size overflow")?;
        if size > 8 * 1024 * 1024
            || total > 64 * 1024 * 1024
            || fs::metadata(&path)?.len() != size
            || Some(sha256(&path)?.as_str()) != r["sha256"].as_str()
        {
            bail!("metadata size/hash mismatch");
        }
        entries.push((path, format!("metadata/{name}")));
    }
    Ok(entries)
}
pub fn package(
    inv: &Inventory,
    target: &str,
    component: &str,
    version: &str,
    out: &Path,
) -> Result<PathBuf> {
    inv.require_supported(component, target)?;
    semver::Version::parse(version)?;
    let mut result = build::load_receipt(inv, target, component)?;
    reject_overlap(&result.library, out)?;
    if result.profile != "release" {
        bail!("packaging requires a release build receipt");
    }
    if component == "manager" {
        if target.contains("android") {
            return package_android(inv, &result, version, out);
        }
        atomic_directory(out, |stage| {
            let temp = tempfile::tempdir_in(stage)?;
            let name = result
                .library
                .file_name()
                .context("Manager filename")?
                .to_string_lossy()
                .into_owned();
            binary::inspect(&result.library, target, false)?;
            let mut entries = vec![
                (result.library.clone(), name),
                (
                    inv.root.join("THIRD_PARTY_NOTICES.md"),
                    "THIRD_PARTY_NOTICES.md".into(),
                ),
            ];
            for dep in binary::closure(&result.library, &search_dirs(inv, &result)?, target)? {
                entries.push((
                    dep.clone(),
                    dep.file_name().unwrap().to_string_lossy().into_owned(),
                ));
            }
            if dependencies::needs(inv, target, component)?.dxc {
                for name in [
                    "dxcompiler.dll",
                    "dxil.dll",
                    "LICENSE-MS.txt",
                    "LICENSE-LLVM.txt",
                    "LICENCE-MIT.txt",
                ] {
                    entries.push((inv.root.join(".work/dxc").join(name), name.into()));
                }
            }
            entries.extend(licenses(inv, &result, temp.path())?);
            entries.extend(metadata_entries(inv, temp.path())?);
            entries.extend(static_core_entries(inv, &result, temp.path())?);
            copy_entries(&entries, stage)?;
            drop(temp);
            fs::create_dir_all(stage.join("cores"))?;
            fs::write(
                stage.join("README.md"),
                format!(
                    "# ShinkiEMU {version}\n\nRun shinki-emu-manager. Install separately released cores from Project-Shinkirou/ShinkiEMU.\n"
                ),
            )?;
            write_provenance(&result, &stage.join("build.json"))?;
            if target == "x86_64-unknown-linux-gnu" {
                let icon = crate::icon::Icon::new()?;
                for (size, dir) in [
                    (16, "16x16"),
                    (32, "32x32"),
                    (48, "48x48"),
                    (64, "64x64"),
                    (128, "128x128"),
                    (256, "256x256"),
                ] {
                    icon.png(
                        size,
                        true,
                        &stage.join(format!("share/icons/hicolor/{dir}/apps/shinkiemu.png")),
                    )?;
                }
                fs::create_dir_all(stage.join("share/applications"))?;
                fs::write(
                    stage.join("share/applications/shinkiemu.desktop"),
                    "[Desktop Entry]\nType=Application\nName=ShinkiEMU\nComment=Visual novel emulator manager\nExec=shinki-emu-manager\nIcon=shinkiemu\nTerminal=false\nCategories=Game;Emulator;\n",
                )?;
            }
            if crate::apple::supported(target) {
                let app = crate::apple::bundle(
                    stage,
                    target,
                    "shinki-emu-manager",
                    "ShinkiEMU",
                    "org.shinkirou.shinkiemu",
                    version,
                )?;
                if target == "aarch64-apple-ios" {
                    let payload_stage = tempfile::tempdir_in(stage)?;
                    let payload = payload_stage.path().join("Payload");
                    fs::create_dir(&payload)?;
                    fs::rename(
                        &app,
                        payload.join(app.file_name().context("iOS app filename")?),
                    )?;
                    let ipa = stage.join(format!("ShinkiEMU-{version}-{target}-device-test.ipa"));
                    zip_entries(&tree(payload_stage.path())?, &ipa)?;
                }
            }
            Ok(())
        })?;
        Ok(out.to_owned())
    } else {
        let temporary = inv.root.join(".work/tmp");
        fs::create_dir_all(&temporary)?;
        let scratch = tempfile::tempdir_in(&temporary)?;
        let c = inv.core(component)?;
        let m = result
            .core_build
            .as_mut()
            .context("missing core-build.json")?;
        for path in &c.licenses {
            m.licenses.push(BuildAsset {
                source: inv.root.join(path),
                dest: path.find("licenses/").map_or_else(
                    || format!("licenses/{}/{}", c.id, path.replace('/', "_")),
                    |offset| path[offset..].to_owned(),
                ),
            });
        }
        // Manifest-owned license assets take precedence over inventory duplicates.
        let mut seen = BTreeSet::new();
        m.licenses.retain(|a| {
            seen.insert(dunce::canonicalize(&a.source).unwrap_or_else(|_| a.source.clone()))
        });
        for path in &c.resources {
            m.resources.push(BuildAsset {
                source: inv.root.join(path),
                dest: format!(
                    "Resources/{}",
                    Path::new(path).file_name().unwrap().to_string_lossy()
                ),
            });
        }
        for (source, dest) in licenses(inv, &result, scratch.path())? {
            result
                .core_build
                .as_mut()
                .unwrap()
                .licenses
                .push(BuildAsset { source, dest });
        }
        stage_core(&result, version, out, &search_dirs(inv, &result)?)
    }
}
/// Resolve license inputs from the actual Cargo checkouts used for embedding.
fn static_core_entries(
    inv: &Inventory,
    result: &BuildResult,
    scratch: &Path,
) -> Result<Vec<(PathBuf, String)>> {
    if !inv
        .cores
        .iter()
        .any(|core| core.static_manager_targets.contains(&result.target))
    {
        return Ok(Vec::new());
    }
    let meta = build::cargo_metadata(inv, &result.target)?;
    let mut entries = Vec::new();
    let mut embedded = Vec::new();
    for core in &inv.cores {
        if !core.static_manager_targets.contains(&result.target) {
            continue;
        }
        let package = meta["packages"]
            .as_array()
            .context("cargo packages")?
            .iter()
            .find(|package| package["name"] == core.package)
            .with_context(|| format!("statically linked package {} missing", core.package))?;
        let checkout = core.source_directory(&inv.root)?;
        let checkout_relative = checkout.strip_prefix(&inv.root)?;
        let manifest_relative = core.manifest.strip_prefix(checkout_relative)?;
        let mut source_root = PathBuf::from(
            package["manifest_path"]
                .as_str()
                .context("static core manifest")?,
        );
        for _ in manifest_relative.components() {
            source_root.pop();
        }
        for license in &core.licenses {
            let path = Path::new(license);
            if let Ok(relative) = path.strip_prefix(checkout_relative) {
                entries.push((
                    source_root.join(relative),
                    format!(
                        "licenses/{}/{}",
                        core.id,
                        relative.to_string_lossy().replace('\\', "/")
                    ),
                ));
            } else {
                // Some inventory assets belong to the product, not the fork.
                entries.push((inv.root.join(path), license.clone()));
            }
        }
        let native = result
            .embedded_core_builds
            .iter()
            .find(|build| build.plugin_id == core.id);
        if core.artifact_kind == "native-shared" && native.is_none() {
            bail!(
                "embedded native core {} has no recorded build manifest",
                core.id
            );
        }
        if let Some(native) = native {
            native.validate()?;
            for asset in &native.resources {
                for (source, destination) in asset_entries(asset)? {
                    entries.push((source, format!("cores/{}/{destination}", core.id)));
                }
            }
            for asset in &native.licenses {
                for (source, destination) in asset_entries(asset)? {
                    entries.push((
                        source,
                        format!("licenses/embedded/{}/{destination}", core.id),
                    ));
                }
            }
        }
        embedded.push(serde_json::json!({
            "id": core.id,
            "package": core.package,
            "source": core.source,
        }));
    }
    let manifest = scratch.join("embedded-cores.json");
    fs::write(&manifest, serde_json::to_vec_pretty(&embedded)?)?;
    entries.push((manifest, "embedded-cores.json".into()));
    Ok(entries)
}

const ANDROID_TARGETS: [(&str, &str); 2] = [
    ("aarch64-linux-android", "arm64-v8a"),
    ("x86_64-linux-android", "x86_64"),
];

/// Collect only the runtime closure of this ABI, not libraries from the host PATH.
pub(crate) fn android_inputs(inv: &Inventory, result: &BuildResult, stage: &Path) -> Result<()> {
    let abi = ANDROID_TARGETS
        .iter()
        .find(|(target, _)| *target == result.target)
        .map(|(_, abi)| *abi)
        .context("unsupported Android ABI")?;
    let temp = tempfile::tempdir()?;
    let metadata = metadata_entries(inv, temp.path())?;
    let mut entries = vec![(
        result.library.clone(),
        format!("jniLibs/{abi}/libshinki_emu_manager.so"),
    )];
    for dep in binary::closure(&result.library, &search_dirs(inv, result)?, &result.target)? {
        let name = format!(
            "jniLibs/{abi}/{}",
            dep.file_name().unwrap().to_string_lossy()
        );
        entries.push((dep, name));
    }
    let mut bundled = vec!["THIRD_PARTY_NOTICES.md".to_owned()];
    entries.push((
        inv.root.join("THIRD_PARTY_NOTICES.md"),
        "assets/THIRD_PARTY_NOTICES.md".into(),
    ));
    let ndk = dependencies::ndk_root(inv)?;
    for notice in ["NOTICE", "NOTICE.toolchain"] {
        let name = format!("licenses/android-ndk/{notice}");
        bundled.push(name.clone());
        entries.push((ndk.join(notice), format!("assets/{name}")));
    }
    for (source, name) in metadata
        .into_iter()
        .chain(licenses(inv, result, temp.path())?)
        .chain(static_core_entries(inv, result, temp.path())?)
    {
        bundled.push(name.clone());
        entries.push((source, format!("assets/{name}")));
    }
    copy_entries(&entries, stage)?;
    bundled.sort();
    fs::write(
        stage.join("assets/bundled.manifest"),
        bundled.join("\n") + "\n",
    )?;
    Ok(())
}

fn prepare_android_debug_key() -> Result<()> {
    let home = std::env::var_os("GRADLE_USER_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
                .map(|home| PathBuf::from(home).join(".gradle"))
        })
        .context("set GRADLE_USER_HOME for the Android test signing key")?;
    fs::create_dir_all(&home)?;
    let key = home.join("shinkiemu-debug.keystore");
    if key.exists() {
        return plain(&key);
    }
    let temp = tempfile::tempdir_in(&home)?;
    let generated = temp.path().join("debug.keystore");
    let java = PathBuf::from(std::env::var_os("JAVA_HOME").context("set JAVA_HOME to JDK 27")?);
    let keytool = java.join("bin").join(if cfg!(windows) {
        "keytool.exe"
    } else {
        "keytool"
    });
    dependencies::run(
        std::process::Command::new(keytool)
            .args(["-genkeypair", "-noprompt", "-storetype", "JKS", "-keystore"])
            .arg(&generated)
            .args([
                "-storepass",
                "android",
                "-keypass",
                "android",
                "-alias",
                "androiddebugkey",
                "-keyalg",
                "RSA",
                "-keysize",
                "2048",
                "-validity",
                "10000",
                "-dname",
                "CN=Android Debug,O=Android,C=US",
            ]),
    )?;
    // Publish the complete test key without overwriting a concurrent creator's key.
    match fs::hard_link(&generated, &key) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => plain(&key),
        Err(error) => Err(error.into()),
    }
}

fn package_android(
    inv: &Inventory,
    result: &BuildResult,
    version: &str,
    out: &Path,
) -> Result<PathBuf> {
    use base64::Engine as _;
    let key = std::env::var_os("SHINKI_EMU_ANDROID_KEYSTORE").filter(|s| !s.is_empty());
    let encoded = std::env::var("SHINKI_EMU_ANDROID_KEYSTORE_BASE64")
        .ok()
        .filter(|s| !s.is_empty());
    if key.is_some() && encoded.is_some() {
        bail!("provide one Android keystore input, not both path and base64");
    }
    let passwords = [
        "SHINKI_EMU_ANDROID_KEYSTORE_PASSWORD",
        "SHINKI_EMU_ANDROID_KEY_ALIAS",
        "SHINKI_EMU_ANDROID_KEY_PASSWORD",
    ];
    let present = passwords
        .iter()
        .filter(|key| std::env::var_os(key).is_some_and(|s| !s.is_empty()))
        .count();
    let signed = key.is_some() || encoded.is_some();
    if (signed && present != 3) || (!signed && present != 0) {
        bail!(
            "Android release signing requires the keystore and all three password/alias variables; partial signing configuration is an error"
        );
    }
    if !signed {
        prepare_android_debug_key()?;
    }
    let private_key = if let Some(encoded) = encoded {
        let key = tempfile::NamedTempFile::new()?;
        fs::write(
            key.path(),
            base64::engine::general_purpose::STANDARD.decode(encoded.trim())?,
        )?;
        Some(key)
    } else {
        None
    };
    let key = key
        .map(PathBuf::from)
        .or_else(|| private_key.as_ref().map(|key| key.path().to_owned()));
    if let Some(key) = &key {
        plain(key)?;
    }
    atomic_directory(out, |stage| {
        // Each ABI resolves its own Rust closure (target-specific crates differ), so
        // the asset listing is the union; every file present in both must still match.
        let mut bundled = std::collections::BTreeSet::new();
        for (target, _) in ANDROID_TARGETS {
            let built = if target == result.target {
                result.clone()
            } else {
                build::load_receipt(inv, target, "manager")?
            };
            if built.profile != "release" || built.source_commit != result.source_commit {
                bail!("both Android ABIs require release builds from the same source commit");
            }
            let inputs = tempfile::tempdir()?;
            android_inputs(inv, &built, inputs.path())?;
            // Shared metadata/license bytes must be identical for both ABIs.
            for (source, name) in tree(inputs.path())? {
                if name == "assets/bundled.manifest" {
                    bundled.extend(
                        fs::read_to_string(&source)?
                            .lines()
                            .filter(|line| !line.is_empty())
                            .map(str::to_owned),
                    );
                    continue;
                }
                let destination = stage.join(&name);
                if destination.exists() {
                    if sha256(&source)? != sha256(&destination)? {
                        bail!("Android ABI inputs disagree: {name}");
                    }
                } else {
                    copy_entries(&[(source, name)], stage)?;
                }
            }
            write_provenance(&built, &stage.join(format!("build-{target}.json")))?;
        }
        let mut manifest = bundled.into_iter().collect::<Vec<_>>().join("\n");
        manifest.push('\n');
        fs::write(stage.join("assets/bundled.manifest"), manifest)?;
        crate::icon::Icon::new()?.android(stage)?;
        let gradle = inv.root.join("Platforms/Android").join(if cfg!(windows) {
            "gradlew.bat"
        } else {
            "gradlew"
        });
        let mut cmd = std::process::Command::new(if cfg!(windows) { "cmd" } else { "sh" });
        if cfg!(windows) {
            cmd.arg("/c");
        }
        cmd.arg(gradle)
            .args([
                "--no-daemon",
                "--console=plain",
                if signed {
                    ":app:assembleRelease"
                } else {
                    ":app:assembleDebug"
                },
            ])
            .current_dir(inv.root.join("Platforms/Android"))
            .env("SHINKI_EMU_ANDROID_VERSION_NAME", version)
            .env(
                "SHINKI_EMU_ANDROID_GENERATED_ROOT",
                dunce::canonicalize(stage)?,
            )
            .env("SHINKI_EMU_ANDROID_ABI", "arm64-v8a,x86_64");
        if std::env::var_os("SHINKI_EMU_GRADLE_OFFLINE").is_some_and(|value| value == "1") {
            cmd.arg("--offline");
        }
        if let Some(key) = &key {
            cmd.env("SHINKI_EMU_ANDROID_KEYSTORE", key);
        }
        cmd.env_remove("SHINKI_EMU_ANDROID_KEYSTORE_BASE64");
        dependencies::run(&mut cmd)?;
        let outputs = inv
            .root
            .join("Platforms/Android/app/build/outputs/apk")
            .join(if signed { "release" } else { "debug" });
        let mut apks = 0;
        for (source, name) in tree(&outputs)? {
            if name.ends_with(".apk") {
                apks += 1;
                fs::copy(
                    &source,
                    stage.join(format!(
                        "ShinkiEMU-{version}-android-universal-{}-signed.apk",
                        if signed { "release" } else { "debug" }
                    )),
                )?;
            }
        }
        if apks != 1 {
            bail!("Gradle must produce exactly one signed APK");
        }
        let apk = tree(stage)?
            .into_iter()
            .find(|(_, name)| name.ends_with(".apk"))
            .context("signed APK missing")?
            .0;
        verify_android_apk(&apk)?;
        fs::write(
            stage.join("android.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "schema": "shinki.emu.android_package.v1",
                "target": result.target,
                "targets": ANDROID_TARGETS.map(|(target, _)| target),
                "abis": ["arm64-v8a", "x86_64"],
                "signing": if signed { "release" } else { "debug-test" },
                "apk": apk.file_name().unwrap().to_string_lossy(),
                "sha256": sha256(&apk)?,
                "size": fs::metadata(&apk)?.len()
            }))?,
        )?;
        write_provenance(result, &stage.join("build.json"))?;
        Ok(())
    })?;
    Ok(out.to_owned())
}

fn verify_android_apk(apk: &Path) -> Result<()> {
    let sdk = std::env::var_os("ANDROID_HOME")
        .or_else(|| std::env::var_os("ANDROID_SDK_ROOT"))
        .map(PathBuf::from)
        .context("set ANDROID_HOME")?;
    let signer = sdk.join("build-tools/37.0.0").join(if cfg!(windows) {
        "apksigner.bat"
    } else {
        "apksigner"
    });
    let mut command = if cfg!(windows) {
        let mut command = std::process::Command::new("cmd");
        command.arg("/c");
        command
    } else {
        std::process::Command::new("sh")
    };
    dependencies::run(
        command
            .arg(signer)
            .args(["verify", "--verbose", "--print-certs"])
            .arg(apk),
    )?;
    let extracted = tempfile::tempdir()?;
    let mut archive = zip::ZipArchive::new(fs::File::open(apk)?)?;
    let mut metadata = false;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let name = entry.name()?.into_owned();
        metadata |= name == "assets/metadata/index.json";
        if name.starts_with("lib/") && name.ends_with(".so") {
            portable(&name)?;
            let destination = extracted.path().join(&name);
            fs::create_dir_all(destination.parent().unwrap())?;
            let mut output = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(destination)?;
            io::copy(&mut entry, &mut output)?;
        }
    }
    for (target, abi) in ANDROID_TARGETS {
        let dir = extracted.path().join("lib").join(abi);
        let manager = dir.join("libshinki_emu_manager.so");
        binary::inspect(&manager, target, false)?;
        for (library, name) in tree(&dir)? {
            if name.ends_with(".so") {
                let info = binary::inspect(&library, target, false)?;
                println!("APK {abi}/{name} NEEDED: {:?}", info.dependencies);
                let closure = binary::closure(&library, std::slice::from_ref(&dir), target)?;
                println!(
                    "APK {abi}/{name}: {} bundled dependencies; remaining NEEDED entries are Android system libraries",
                    closure.len()
                );
            }
        }
    }
    if !metadata {
        bail!("APK metadata snapshot missing");
    }
    Ok(())
}
