use crate::{
    cores::{Core, Inventory},
    dependencies, package,
};
use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn git(directory: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(directory)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0");
    command
}

/// Committed bytes: a runner's core.autocrlf=true would rewrite LF in archives
/// (breaking LFS pointer checks) and in fork checkouts. Status checks keep the
/// checkout's own settings, or a CRLF product checkout would look modified.
fn git_committed_bytes(directory: &Path) -> Command {
    let mut command = git(directory);
    command.args([
        "-c",
        "core.autocrlf=false",
        "-c",
        "core.eol=lf",
        "-c",
        "core.filemode=false",
        "-c",
        "core.symlinks=false",
        "-c",
        "core.longpaths=true",
    ]);
    command
}

fn pin_checkout_config(directory: &Path) -> Result<()> {
    for (key, value) in [
        ("core.autocrlf", "false"),
        ("core.eol", "lf"),
        ("core.filemode", "false"),
        ("core.symlinks", "false"),
    ] {
        dependencies::run(git(directory).args(["config", "--local", "--replace-all", key, value]))?;
    }
    Ok(())
}

fn revision(directory: &Path) -> Result<String> {
    Ok(String::from_utf8(dependencies::capture(
        git(directory).args(["rev-parse", "HEAD"]),
    )?)?
    .trim()
    .to_owned())
}

fn checkout_root(directory: &Path) -> Result<()> {
    package::plain(directory)?;
    let reported = String::from_utf8(dependencies::capture(
        git(directory).args(["rev-parse", "--show-toplevel"]),
    )?)?;
    if fs::canonicalize(reported.trim())? != fs::canonicalize(directory)? {
        bail!(
            "{} is not the root of the requested fork checkout; refusing to operate on its parent repository",
            directory.display()
        );
    }
    Ok(())
}

pub struct SourceState {
    pub commit: String,
    pub dirty: bool,
}

fn modified(directory: &Path) -> Result<bool> {
    Ok(!dependencies::capture(git(directory).args([
        "status",
        "--porcelain=v1",
        "--untracked-files=all",
        "--ignore-submodules=none",
    ]))?
    .is_empty())
}

pub fn state(directory: &Path) -> Result<SourceState> {
    checkout_root(directory)?;
    Ok(SourceState {
        commit: revision(directory)?,
        dirty: modified(directory)?,
    })
}

fn require_clean(directory: &Path) -> Result<()> {
    if modified(directory)? {
        bail!(
            "fork checkout {} contains local modifications; commit or move them before explicit deps preparation (nothing was reset)",
            directory.display()
        );
    }
    Ok(())
}

fn adapter_relative(core: &Core) -> Result<PathBuf> {
    let mut components = core.manifest.components();
    if components
        .next()
        .is_none_or(|component| component.as_os_str() != "ThirdParty")
        || components.next().is_none()
    {
        bail!("external adapter manifest must be inside one ThirdParty checkout");
    }
    let relative: PathBuf = components.collect();
    if relative.as_os_str().is_empty() {
        bail!("external adapter manifest path is missing");
    }
    Ok(relative)
}
/// Verify original upstream reachability independently of organization branch names.
pub fn verify_provenance(inv: &Inventory, core: &Core) -> Result<()> {
    let Some(source) = &core.source else {
        return Ok(());
    };
    let directory = inv.root.join(".work/provenance").join(&core.id);
    if !directory.exists() {
        fs::create_dir_all(&directory)?;
        dependencies::run(
            Command::new("git")
                .args(["init", "--bare", "--quiet"])
                .arg(&directory),
        )?;
    }
    dependencies::run(git(&directory).args([
        "fetch",
        "--filter=blob:none",
        "--no-tags",
        &source.repo,
        &source.rev,
    ]))?;
    let upstream = &source.upstream_identity;
    let namespace = match upstream.ref_kind {
        shinki_emu_metadata::UpstreamRefKind::Tag => "tags",
        shinki_emu_metadata::UpstreamRefKind::Branch => "heads",
    };
    let reference = format!(
        "+refs/{namespace}/{}:refs/proof/upstream",
        upstream.ref_name
    );
    dependencies::run(git(&directory).args([
        "fetch",
        "--filter=blob:none",
        "--no-tags",
        &upstream.repo,
        &reference,
        "+refs/tags/*:refs/proof/tags/*",
    ]))?;
    verify_provenance_at(source, &directory)
}

fn verify_provenance_at(source: &crate::cores::CoreSource, directory: &Path) -> Result<()> {
    let upstream = &source.upstream_identity;
    let at = String::from_utf8(dependencies::capture(
        git(directory).args(["rev-parse", "refs/proof/upstream^{commit}"]),
    )?)?;
    if upstream.ref_kind == shinki_emu_metadata::UpstreamRefKind::Tag
        && at.trim() != upstream.commit
    {
        bail!("upstream tag does not resolve to the declared full commit");
    }
    for descendant in [&source.rev, at.trim()] {
        dependencies::run(git(directory).args([
            "merge-base",
            "--is-ancestor",
            &upstream.commit,
            descendant,
        ]))
        .context(
            "declared upstream commit must be contained in both upstream ref and adapter pin",
        )?;
    }
    if upstream.ref_kind == shinki_emu_metadata::UpstreamRefKind::Branch {
        let tags = dependencies::capture(git(directory).args([
            "for-each-ref",
            &format!("--points-at={}", upstream.commit),
            "--format=%(refname)",
            "refs/proof/tags",
        ]))?;
        if !tags.is_empty() {
            bail!("upstream commit has a verified tag; select that tag rather than a branch");
        }
    }
    Ok(())
}

fn verify_at(core: &Core, directory: &Path, clean: bool) -> Result<()> {
    let source = core
        .source
        .as_ref()
        .context("external source declaration missing")?;
    checkout_root(directory)?;
    let actual = revision(directory)?;
    if actual != source.rev {
        bail!(
            "{} source revision mismatch: expected {}, found {}; run cargo xtask deps --core {}",
            core.id,
            source.rev,
            actual,
            core.id
        );
    }
    if clean {
        require_clean(directory)?;
    }
    let manifest = directory.join(adapter_relative(core)?);
    package::plain(&manifest).with_context(|| {
        format!(
            "pinned source {}@{} lacks adapter manifest {}",
            source.repo,
            source.rev,
            manifest.display()
        )
    })?;
    if !manifest.is_file() {
        bail!("adapter manifest is not a file: {}", manifest.display());
    }
    Ok(())
}

/// Read-only preflight. Only explicit deps preparation fetches or checks out code.
pub fn verify(inv: &Inventory, core: &Core, clean: bool) -> Result<PathBuf> {
    let directory = core.source_directory(&inv.root)?;
    if core.source.is_some() {
        verify_at(core, &directory, clean)?;
    }
    Ok(directory)
}

fn checkout_pin(core: &Core, directory: &Path) -> Result<()> {
    let source = core
        .source
        .as_ref()
        .context("external source declaration missing")?;
    let object = format!("{}^{{commit}}", source.rev);
    let available = git(directory).args(["cat-file", "-e", &object]).output()?;
    if !available.status.success() {
        dependencies::run(git(directory).args([
            "fetch",
            "--filter=blob:none",
            "--no-tags",
            &source.repo,
            &source.rev,
        ]))?;
    }
    pin_checkout_config(directory)?;
    dependencies::run(git_committed_bytes(directory).args(["checkout", "--detach", &source.rev]))?;
    dependencies::run(git_committed_bytes(directory).args([
        "submodule",
        "update",
        "--init",
        "--recursive",
    ]))?;
    verify_at(core, directory, true)
}

/// Prepare one exact source revision without replacing dirty or unrelated trees.
pub fn prepare(inv: &Inventory, core: &Core, allow_dirty: bool) -> Result<()> {
    let Some(source) = &core.source else {
        return Ok(());
    };
    let directory = core.source_directory(&inv.root)?;
    if fs::symlink_metadata(&directory).is_ok() {
        checkout_root(&directory)?;
        if allow_dirty && revision(&directory)? == source.rev && modified(&directory)? {
            // Native dependencies can be prepared for unpublished adapter work.
            // Do not check out, fetch, or rewrite any part of this source tree.
            return verify_at(core, &directory, false);
        }
        require_clean(&directory)?;
        return checkout_pin(core, &directory);
    }
    package::atomic_directory(&directory, |stage| {
        dependencies::run(
            Command::new("git")
                .args([
                    "clone",
                    "--filter=blob:none",
                    "--no-checkout",
                    "--single-branch",
                    "--branch",
                    &source.branch,
                    "--config",
                    "core.autocrlf=false",
                    "--config",
                    "core.eol=lf",
                    "--config",
                    "core.filemode=false",
                    "--config",
                    "core.symlinks=false",
                    "--config",
                    "core.longpaths=true",
                    "--",
                    &source.repo,
                ])
                .arg(stage)
                .env("GIT_TERMINAL_PROMPT", "0"),
        )?;
        checkout_pin(core, stage)
    })
}

/// Append the committed tree at `rev` and every gitlink at its recorded
/// revision; `git archive` alone leaves submodules empty. Working-tree edits
/// never enter the archive. Git LFS pointers are replaced by the materialized
/// working-tree payload, which must match the committed pointer.
fn append_revision(
    builder: &mut tar::Builder<fs::File>,
    repository: &Path,
    rev: &str,
    prefix: &Path,
) -> Result<()> {
    let raw = tempfile::NamedTempFile::new()?;
    dependencies::run(
        git_committed_bytes(repository)
            .args(["archive", "--format=tar", rev, "-o"])
            .arg(raw.path()),
    )?;
    for entry in tar::Archive::new(fs::File::open(raw.path())?).entries()? {
        let mut entry = entry?;
        let relative = entry.path()?.into_owned();
        let path = prefix.join(&relative);
        let mut header = entry.header().clone();
        if header.entry_type().is_file() && entry.size() <= 1024 {
            let size = entry.size() as usize;
            let mut bytes = [0u8; 1024];
            std::io::Read::read_exact(&mut entry, &mut bytes[..size])?;
            if bytes[..size].starts_with(b"version https://git-lfs.github.com/spec/v1\n") {
                let pointer = std::str::from_utf8(&bytes[..size])?;
                let oid = pointer
                    .lines()
                    .find_map(|line| line.strip_prefix("oid sha256:"))
                    .context("Git LFS source lacks SHA256")?;
                let expected: u64 = pointer
                    .lines()
                    .find_map(|line| line.strip_prefix("size "))
                    .context("Git LFS source lacks size")?
                    .parse()?;
                let source = repository.join(&relative);
                if fs::metadata(&source)
                    .with_context(|| format!("missing Git LFS source {}", source.display()))?
                    .len()
                    != expected
                    || !package::sha256(&source)?.eq_ignore_ascii_case(oid)
                {
                    bail!(
                        "Git LFS source {} is missing, unmaterialized or changed; expected SHA256 {oid}",
                        source.display()
                    );
                }
                header.set_size(expected);
                header.set_cksum();
                builder.append_data(&mut header, path, fs::File::open(source)?)?;
            } else {
                builder.append_data(&mut header, path, &bytes[..size])?;
            }
        } else {
            builder.append_data(&mut header, path, &mut entry)?;
        }
    }
    for (path, pin) in gitlinks(repository, rev)? {
        let child = repository.join(&path);
        checkout_root(&child)
            .with_context(|| format!("missing recursive source {}", child.display()))?;
        append_revision(builder, &child, &pin, &prefix.join(path))?;
    }
    Ok(())
}

fn gitlinks(repository: &Path, rev: &str) -> Result<Vec<(PathBuf, String)>> {
    let listing = dependencies::capture(git(repository).args(["ls-tree", "-rz", rev]))?;
    let mut links = Vec::new();
    for item in listing.split(|b| *b == 0).filter(|b| !b.is_empty()) {
        let item = std::str::from_utf8(item)?;
        let (record, path) = item.split_once('\t').context("git tree entry")?;
        if let Some(pin) = record.strip_prefix("160000 commit ") {
            links.push((PathBuf::from(path), pin.to_owned()));
        }
    }
    Ok(links)
}

fn copy_tree(source: &Path, dest: &Path) -> Result<()> {
    package::plain(source)?;
    fs::create_dir_all(dest)?;
    for item in walkdir::WalkDir::new(source).follow_links(false) {
        let item = item?;
        package::plain(item.path())?;
        let to = dest.join(item.path().strip_prefix(source)?);
        if item.file_type().is_dir() {
            fs::create_dir_all(to)?;
        } else {
            fs::copy(item.path(), to)?;
        }
    }
    Ok(())
}

fn copy_input(source: &Path, dest: &Path) -> Result<()> {
    package::plain(source)
        .with_context(|| format!("required corresponding-source input {}", source.display()))?;
    fs::create_dir_all(dest.parent().context("source input parent")?)?;
    fs::copy(source, dest)?;
    Ok(())
}

/// Whether every way to satisfy `expression` uses a license matching `obliging`.
/// `LicenseRef-*` terms are custom licenses the project selects explicitly
/// (Slint Royalty-free, KiriKiri2) and are not treated as source obligations.
fn obliges(expression: &str, obliging: fn(spdx::LicenseId) -> bool) -> Result<bool> {
    let parsed = spdx::Expression::parse_mode(expression, crate::rust_licenses::PARSE_MODE)
        .with_context(|| format!("invalid SPDX expression {expression}"))?;
    Ok(!parsed.evaluate(|req| req.license.id().is_none_or(|id| !obliging(id))))
}

fn copyleft(id: spdx::LicenseId) -> bool {
    id.is_copyleft()
}

/// GPL, LGPL and AGPL cover the combined, statically linked Rust binary.
fn gnu_copyleft(id: spdx::LicenseId) -> bool {
    id.is_gnu() && id.is_copyleft()
}

/// Copy the crates whose licenses require their source to accompany the
/// binary: every linked crate when a GNU copyleft license applies to any of
/// them, otherwise only copyleft crates (MPL file-level copyleft) and crates
/// without a license expression. Path crates are in the product/fork snapshot.
fn rust_sources(metadata: &Value, package: &str, root: &Path) -> Result<()> {
    let closure = crate::rust_licenses::closure(metadata, package)?;
    let mut whole = false;
    for p in &closure {
        if let Some(expression) = p["license"].as_str().filter(|s| !s.trim().is_empty()) {
            whole |= obliges(expression, gnu_copyleft)?;
        }
    }
    for p in closure {
        let Some(source) = p["source"].as_str() else {
            continue;
        };
        let required = match p["license"].as_str().filter(|s| !s.trim().is_empty()) {
            Some(expression) => whole || obliges(expression, copyleft)?,
            None => true,
        };
        if !required {
            continue;
        }
        let name = format!(
            "{}-{}+{}",
            p["name"].as_str().context("cargo package name")?,
            p["version"].as_str().context("cargo package version")?,
            crate::rust_licenses::source_tag(Some(source))
        );
        package::portable(&name)?;
        let crate_root = Path::new(
            p["manifest_path"]
                .as_str()
                .context("cargo source manifest")?,
        )
        .parent()
        .context("cargo source directory")?;
        let dest = root.join("rust-sources").join(name);
        for item in walkdir::WalkDir::new(crate_root)
            .follow_links(true)
            .into_iter()
            .filter_entry(|item| item.file_name() != ".git")
        {
            let item = item?;
            let to = dest.join(item.path().strip_prefix(crate_root)?);
            if item.file_type().is_dir() {
                fs::create_dir_all(to)?;
            } else {
                fs::copy(item.path(), to)?;
            }
        }
    }
    Ok(())
}

fn sha512(path: &Path) -> Result<String> {
    use sha2::Digest;
    use std::io::Read;
    package::plain(path)?;
    let mut file = fs::File::open(path)?;
    let mut digest = sha2::Sha512::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(hex::encode(digest.finalize()))
}

/// Read vcpkg's installed graph rather than treating every share/ directory as
/// a port. Follow explicit host-triplet dependencies as well as target ports.
fn installed_ports(installation: &Path) -> Result<Vec<(String, PathBuf)>> {
    use std::collections::{BTreeMap, BTreeSet};
    let base = installation.parent().context("vcpkg installation root")?;
    let triplet = installation
        .file_name()
        .and_then(|s| s.to_str())
        .context("vcpkg triplet")?;
    let status = base.join("vcpkg/status");
    package::plain(&status)?;
    let contents = fs::read_to_string(&status)?;
    let mut graph: BTreeMap<(&str, &str), BTreeSet<(&str, &str)>> = BTreeMap::new();
    let (mut name, mut architecture, mut depends, mut active) = (None, None, "", false);
    for line in contents.lines().chain(std::iter::once("")) {
        if line.is_empty() {
            if active {
                let name = name.context("installed vcpkg record has no Package")?;
                let architecture =
                    architecture.context("installed vcpkg record has no Architecture")?;
                let edges = graph.entry((name, architecture)).or_default();
                for dependency in depends.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                    let (package, target) = dependency
                        .split_once(':')
                        .unwrap_or((dependency, architecture));
                    let package = package.split('[').next().context("dependency package")?;
                    edges.insert((package, target));
                }
            }
            (name, architecture, depends, active) = (None, None, "", false);
        } else if let Some(value) = line.strip_prefix("Package: ") {
            name = Some(value);
        } else if let Some(value) = line.strip_prefix("Architecture: ") {
            architecture = Some(value);
        } else if let Some(value) = line.strip_prefix("Depends: ") {
            depends = value;
        } else if line == "Status: install ok installed" {
            active = true;
        }
    }
    let mut pending: Vec<_> = graph
        .keys()
        .filter(|key| key.1 == triplet)
        .copied()
        .collect();
    if pending.is_empty() {
        bail!(
            "no installed native ports for {triplet} in {}",
            status.display()
        );
    }
    let mut selected = BTreeSet::new();
    while let Some(key) = pending.pop() {
        if selected.insert(key) {
            let edges = graph.get(&key).with_context(|| {
                format!(
                    "installed native dependency {}:{} missing from {}",
                    key.0,
                    key.1,
                    status.display()
                )
            })?;
            pending.extend(edges.iter().copied());
        }
    }
    Ok(selected
        .into_iter()
        .map(|(name, arch)| (name.to_owned(), base.join(arch)))
        .collect())
}

/// Registry ports are identified by the Git tree their SPDX record names, which
/// can predate the registry HEAD. Overlay ports record no tree; they ship in
/// the fork snapshot. Returns whether a recipe was extracted.
pub(crate) fn port_recipe(spdx: &Value, registry: &Path, dest: &Path) -> Result<bool> {
    let port = spdx["packages"]
        .as_array()
        .context("SPDX packages")?
        .iter()
        .find(|p| p["SPDXID"] == "SPDXRef-port")
        .context("SPDX port recipe")?;
    let Some(tree) = port["externalRefs"].as_array().and_then(|refs| {
        refs.iter()
            .filter_map(|r| r["referenceLocator"].as_str())
            .find_map(|r| r.strip_prefix("gitoid:tree:sha1:"))
    }) else {
        return Ok(false);
    };
    let archive = dependencies::capture(git_committed_bytes(registry).args([
        "archive",
        "--format=tar",
        tree,
    ]))?;
    fs::create_dir_all(dest)?;
    tar::Archive::new(archive.as_slice()).unpack(dest)?;
    Ok(true)
}

fn native_git_source(downloads: &Path, port: &str, location: &str, dest: &Path) -> Result<()> {
    let (repo, rev) = location
        .strip_prefix("git+")
        .and_then(|source| source.rsplit_once('@'))
        .context("native Git source requires a repository and commit")?;
    if !repo.starts_with("https://")
        || rev.len() != 40
        || !rev.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        bail!("native Git source requires an HTTPS repository and exact commit");
    }
    package::portable(port)?;
    let archive = dest.join(format!("{port}-{rev}.tar.gz"));
    if archive.exists() {
        return Ok(());
    }
    let cached = downloads.join("git-tmp");
    let cached_rev = dependencies::capture(
        git_committed_bytes(&cached)
            .args(["rev-parse", "--verify"])
            .arg(format!("{rev}^{{commit}}")),
    );
    let checkout;
    let source = if cached_rev
        .as_ref()
        .is_ok_and(|actual| String::from_utf8_lossy(actual).trim() == rev)
    {
        cached.as_path()
    } else {
        checkout = tempfile::tempdir()?;
        dependencies::run(git_committed_bytes(checkout.path()).args(["init", "-q"]))?;
        dependencies::run(git_committed_bytes(checkout.path()).args([
            "fetch",
            "-q",
            "--depth",
            "1",
            "--no-tags",
            repo,
            rev,
        ]))?;
        let actual = dependencies::capture(
            git_committed_bytes(checkout.path()).args(["rev-parse", "FETCH_HEAD"]),
        )?;
        if String::from_utf8_lossy(&actual).trim() != rev {
            bail!("native Git repository did not return pinned commit {rev}");
        }
        checkout.path()
    };
    fs::create_dir_all(dest)?;
    dependencies::run(
        git_committed_bytes(source)
            .args(["archive", "--format=tar.gz", "-o"])
            .arg(archive)
            .arg(rev),
    )
}

/// Collect installed target ports and their host-tool dependency closure:
/// recipes/patches and the original source archives vcpkg consumed.
fn native_sources(
    inv: &Inventory,
    component: &str,
    target: &str,
    root: &Path,
    metadata: &Value,
) -> Result<String> {
    let selected = dependencies::needs(inv, target, component)?;
    let mut installations = Vec::new();
    if selected.subtitle {
        installations.push(("subtitle", dependencies::subtitle_install(inv, target)?));
    }
    if selected.ffmpeg {
        installations.push(("ffmpeg", dependencies::ffmpeg_install(inv, target)?));
    }
    for native in dependencies::native_cores(inv, target, component) {
        installations.push((
            native.id.as_str(),
            dependencies::native_install(inv, target, &native.id)?
                .join(dependencies::native_triplet(inv, target)?),
        ));
    }
    let registry = inv.root.join(".work/vcpkg");
    let mut required = std::collections::BTreeSet::new();
    let mut commands = String::new();
    for (group, installed) in &installations {
        for (port, source_install) in installed_ports(installed)? {
            let spdx = source_install
                .join("share")
                .join(&port)
                .join("vcpkg.spdx.json");
            let record: Value =
                serde_json::from_slice(&fs::read(&spdx).with_context(|| {
                    format!("installed port {port} missing source provenance")
                })?)?;
            port_recipe(
                &record,
                &registry,
                &root.join("native/ports").join(group).join(&port),
            )
            .with_context(|| format!("native source port {port}"))?;
            for resource in record["packages"].as_array().context("SPDX packages")? {
                if !resource["SPDXID"]
                    .as_str()
                    .is_some_and(|s| s.starts_with("SPDXRef-resource-"))
                {
                    continue;
                }
                if let Some(checksum) = resource["checksums"]
                    .as_array()
                    .and_then(|a| a.iter().find(|c| c["algorithm"] == "SHA512"))
                    .and_then(|c| c["checksumValue"].as_str())
                {
                    if let Some(filename) = resource["packageFileName"].as_str() {
                        copy_named_archive(
                            &inv.root.join(".work/vcpkg-downloads"),
                            filename,
                            checksum,
                            &root.join(".work/vcpkg-downloads"),
                        )?;
                    } else {
                        required.insert(checksum.to_ascii_lowercase());
                    }
                } else {
                    native_git_source(
                        &inv.root.join(".work/vcpkg-downloads"),
                        &port,
                        resource["downloadLocation"]
                            .as_str()
                            .context("native source resource missing SHA512 or Git location")?,
                        &root.join(".work/vcpkg-downloads"),
                    )?;
                }
            }
        }
        let triplet = installed
            .file_name()
            .context("native triplet")?
            .to_string_lossy();
        let install_root = installed
            .parent()
            .context("install root")?
            .strip_prefix(&inv.root)?
            .to_string_lossy()
            .replace('\\', "/");
        let manifest = match *group {
            "subtitle" => "build/vcpkg/subtitle".to_owned(),
            "ffmpeg" => "build/vcpkg/ffmpeg".to_owned(),
            _ => inv
                .core(group)?
                .source_directory(&inv.root)?
                .strip_prefix(&inv.root)?
                .to_string_lossy()
                .replace('\\', "/"),
        };
        commands.push_str(&format!(
            "& $vcpkg install --vcpkg-root=\"$PWD/.work/vcpkg\" --triplet {triplet} --x-manifest-root=\"$PWD/{manifest}\" --x-install-root=\"$PWD/{install_root}\""
        ));
        if !matches!(*group, "subtitle" | "ffmpeg")
            && inv.root.join(&manifest).join("vcpkg/ports").is_dir()
        {
            commands.push_str(&format!(" --overlay-ports=\"$PWD/{manifest}/vcpkg/ports\""));
        }
        commands.push_str(&format!(" --overlay-ports=\"$PWD/native/ports/{group}\""));
        if *group == "subtitle" {
            commands.push_str(" --overlay-triplets=\"$PWD/native/subtitle-triplets\" --overlay-triplets=\"$PWD/build/vcpkg/apple-triplets\" --overlay-triplets=\"$PWD/build/vcpkg/mobile-triplets\"");
        }
        if *group != "subtitle" {
            commands.push_str(" --overlay-triplets=\"$PWD/build/vcpkg/apple-triplets\" --overlay-triplets=\"$PWD/build/vcpkg/mobile-triplets\"");
        }
        commands.push('\n');
        if matches!(*group, "subtitle" | "ffmpeg") {
            let prefix = installed
                .parent()
                .and_then(Path::parent)
                .context("isolated vcpkg prefix")?
                .strip_prefix(&inv.root)?
                .to_string_lossy()
                .replace('\\', "/");
            commands.push_str(&format!(
                "Copy-Item \"$PWD/.work/vcpkg/.vcpkg-root\" \"$PWD/{prefix}/.vcpkg-root\"\n"
            ));
        }
    }
    collect_archives(
        &inv.root.join(".work/vcpkg-downloads"),
        &root.join(".work/vcpkg-downloads"),
        required,
    )?;
    if selected.subtitle {
        // The pinned engine supplies custom mixed-linkage triplets.
        let engine = metadata["packages"]
            .as_array()
            .context("Cargo packages")?
            .iter()
            .find(|p| p["name"] == "roukaku-subtitle")
            .context("roukaku-subtitle source missing")?;
        let manifest = Path::new(
            engine["manifest_path"]
                .as_str()
                .context("subtitle manifest")?,
        );
        copy_tree(
            &manifest.parent().unwrap().join("triplets"),
            &root.join("native/subtitle-triplets"),
        )?;
    }
    if !installations.is_empty() {
        let pins: toml::Value = toml::from_str(&fs::read_to_string(
            inv.root.join("build/dependencies.toml"),
        )?)?;
        let url = pins["vcpkg_url"].as_str().context("vcpkg URL")?;
        let pin = pins["vcpkg_commit"].as_str().context("vcpkg commit")?;
        let tool = if target.contains("windows") {
            "& .work/vcpkg/bootstrap-vcpkg.bat -disableMetrics\n$vcpkg = \"$PWD/.work/vcpkg/vcpkg.exe\""
        } else {
            "sh .work/vcpkg/bootstrap-vcpkg.sh -disableMetrics\nvcpkg=\"$PWD/.work/vcpkg/vcpkg\""
        };
        commands = format!(
            "git clone {url} .work/vcpkg\ngit -C .work/vcpkg checkout --detach {pin}\n{tool}\n{commands}"
        );
    }
    if selected.impacto_sources {
        for name in ["libatrac9", "magic_enum"] {
            copy_tree(
                &inv.root.join(".work/native-downloads").join(name),
                &root.join(".work/native-downloads").join(name),
            )?;
        }
    }
    if selected.renpy {
        renpy_sources(inv, root)?;
    }
    Ok(commands)
}

fn copy_named_archive(downloads: &Path, filename: &str, checksum: &str, dest: &Path) -> Result<()> {
    package::portable(filename)?;
    let source = downloads.join(filename);
    if !sha512(&source)?.eq_ignore_ascii_case(checksum) {
        bail!(
            "native source archive SHA512 mismatch: {}",
            source.display()
        );
    }
    copy_input(&source, &dest.join(filename))
}

/// Copy the downloaded archives vcpkg recorded by SHA512 in the installed SPDX.
pub(crate) fn collect_archives(
    downloads: &Path,
    dest: &Path,
    mut required: std::collections::BTreeSet<String>,
) -> Result<()> {
    if required.is_empty() {
        return Ok(());
    }
    for entry in fs::read_dir(downloads)? {
        let path = entry?.path();
        if path.is_file() && required.remove(&sha512(&path)?) {
            copy_input(&path, &dest.join(path.file_name().unwrap()))?;
        }
    }
    if !required.is_empty() {
        bail!(
            "required corresponding-source archives missing or SHA512 mismatch in {}: {}",
            downloads.display(),
            required.into_iter().collect::<Vec<_>>().join(", ")
        );
    }
    Ok(())
}

pub(crate) const RENPY_BUILD_PIN: &str = "7bfab40c1174f622f644b24669afd5fb167fbb79";

pub(crate) fn prepare_renpy_build(inv: &Inventory) -> Result<()> {
    let repo = inv.root.join(".work/renpy-build");
    // A fresh `--no-checkout` clone has no index yet, so `git status` reports
    // every tracked file as deleted; only an existing checkout must be clean
    // before it is moved to the pin.
    let checkout = |directory: &Path, existing: bool| -> Result<()> {
        checkout_root(directory)?;
        pin_checkout_config(directory)?;
        if existing {
            require_clean(directory)?;
        }
        if !git(directory)
            .args(["cat-file", "-e", RENPY_BUILD_PIN])
            .status()?
            .success()
        {
            dependencies::run(git(directory).args([
                "fetch",
                "--no-tags",
                "https://github.com/renpy/renpy-build.git",
                RENPY_BUILD_PIN,
            ]))?;
        }
        dependencies::run(git_committed_bytes(directory).args([
            "checkout",
            "--detach",
            RENPY_BUILD_PIN,
        ]))?;
        dependencies::run(git_committed_bytes(directory).args([
            "submodule",
            "update",
            "--init",
            "--recursive",
        ]))?;
        require_clean(directory)
    };
    if repo.exists() {
        return checkout(&repo, true);
    }
    package::atomic_directory(&repo, |stage| {
        dependencies::run(
            Command::new("git")
                .args([
                    "clone",
                    "--no-checkout",
                    "--config",
                    "core.autocrlf=false",
                    "--config",
                    "core.eol=lf",
                    "--config",
                    "core.filemode=false",
                    "--config",
                    "core.symlinks=false",
                    "https://github.com/renpy/renpy-build.git",
                ])
                .arg(stage),
        )?;
        checkout(stage, false)
    })
}

/// Ren'Py's LGPL relink material: the pinned renpy-build recipes, the source
/// archives inventoried by the fork (downloaded and hash-checked by `deps`),
/// and the prepared SDK/Python/SDL trees the adapter build consumes.
fn renpy_sources(inv: &Inventory, root: &Path) -> Result<()> {
    let repo = inv.root.join(".work/renpy-build");
    checkout_root(&repo).context(
        "Ren'Py corresponding source requires .work/renpy-build; run cargo xtask deps --core renpy",
    )?;
    fs::create_dir_all(root.join("native"))?;
    let mut builder = tar::Builder::new(fs::File::create(root.join("native/renpy-build.tar"))?);
    append_revision(&mut builder, &repo, RENPY_BUILD_PIN, Path::new(""))?;
    builder.finish()?;
    let licenses = inv
        .core("renpy")?
        .source_directory(&inv.root)?
        .join("licenses/renpy");
    for (inventory, dest, fields) in [
        (
            "python-sources.json",
            "native/python-sources",
            ["packages", "tools"].as_slice(),
        ),
        (
            "native-sources.json",
            "native/renpy-sources",
            ["sources"].as_slice(),
        ),
    ] {
        let path = licenses.join(inventory);
        let records: Value = serde_json::from_slice(&fs::read(&path)?)?;
        let dest = root.join(dest);
        for field in fields {
            for record in records[field]
                .as_array()
                .with_context(|| format!("{inventory} {field}"))?
            {
                for archive in std::iter::once(record).chain(record.get("wheel")) {
                    let name = archive["file_name"]
                        .as_str()
                        .context("source archive filename")?;
                    package::portable(name)?;
                    copy_input(
                        &inv.root.join(".work/downloads").join(name),
                        &dest.join(name),
                    )?;
                }
            }
        }
        copy_input(&path, &dest.join("sources.json"))?;
    }
    for name in ["renpy-sdk", "python", "cython-license", "sdl2"] {
        copy_tree(
            &inv.root.join(".work/native-downloads").join(name),
            &root.join(".work/native-downloads").join(name),
        )?;
    }
    Ok(())
}

/// Write `<out>` as a zip holding `source.tar` and `BUILD-FROM-SOURCE.md`.
pub(crate) fn bundle(inv: &Inventory, component: &str, target: &str, out: &Path) -> Result<()> {
    let temporary = inv.root.join(".work/tmp");
    fs::create_dir_all(&temporary)?;
    let temp = tempfile::tempdir_in(&temporary)?;
    let product_commit = revision(&inv.root)?;
    let metadata = crate::rust_licenses::metadata(inv, target, component)?;
    let archive = temp.path().join("source.tar");
    let mut builder = tar::Builder::new(fs::File::create(&archive)?);
    append_revision(&mut builder, &inv.root, &product_commit, Path::new(""))?;
    let mut references = serde_json::json!({
        "product": {"repo":"https://github.com/Project-Shinkirou/ShinkiEMU","rev":product_commit},
        "core": null, "target": target
    });
    let (manifest, package, features) = if component == "manager" {
        let mut embedded = Vec::new();
        for core in &inv.cores {
            if core
                .static_manager_targets
                .iter()
                .any(|triple| triple == target)
                && let Some(source) = &core.source
            {
                let repo = core.source_directory(&inv.root)?;
                append_revision(
                    &mut builder,
                    &repo,
                    &source.rev,
                    repo.strip_prefix(&inv.root)?,
                )?;
                embedded.push(serde_json::json!({"id": core.id, "source": source}));
            }
        }
        references["embedded_cores"] = serde_json::Value::Array(embedded);
        (
            inv.root.join("Cargo.toml"),
            "shinki-emu-manager".to_owned(),
            Vec::new(),
        )
    } else {
        let core = inv.core(component)?;
        if let Some(source) = &core.source {
            let repo = core.source_directory(&inv.root)?;
            checkout_root(&repo)?;
            append_revision(
                &mut builder,
                &repo,
                &source.rev,
                repo.strip_prefix(&inv.root)?,
            )?;
            references["core"] = serde_json::to_value(source)?;
            references["identity"] = serde_json::to_value(source.identity())?;
            references["version"] = source.identity().version().into();
        }
        (
            inv.root.join(&core.manifest),
            core.package.clone(),
            core.features.clone(),
        )
    };
    // Files outside the Git snapshots, appended under the same root.
    let root = temp.path().join("extra");
    fs::create_dir(&root)?;
    rust_sources(&metadata, &package, &root)?;
    let native_commands = native_sources(inv, component, target, &root, &metadata)?;
    fs::write(
        root.join("source-references.json"),
        serde_json::to_vec_pretty(&references)?,
    )?;
    let manifest_relative = manifest
        .strip_prefix(&inv.root)?
        .to_string_lossy()
        .replace('\\', "/");
    let feature_args = if features.is_empty() {
        String::new()
    } else {
        format!(" --features {}", features.join(","))
    };
    let mut instructions = format!(
        "# Rebuild {component} for {target}\n\nProduct commit: {product_commit}. Exact core revision: see source-references.json.\n\
         Unpack source.tar into a new directory and run these commands there in a developer PowerShell (Windows) or the equivalent shell on the target OS.\n\
         Prerequisites: the Rust toolchain in rust-toolchain.toml, the {target} standard library, the system C/C++ compiler/SDK, CMake, Ninja and Git. Build tools and OS SDKs are system prerequisites.\n\
         Cargo.lock pins every Rust dependency to an exact registry checksum or Git commit; Cargo downloads them. \
         rust-sources/ holds the exact sources of each linked crate whose license requires source distribution (every linked crate when a GPL-family license applies). \
         To build with a modified copy, add a `[patch]` entry pointing at its directory.\n\n```powershell\n\
         $env:CARGO_TARGET_DIR = \"$PWD/target\"\n"
    );
    let selected = dependencies::needs(inv, target, component)?;
    if target.contains("android") {
        instructions.push_str(
            "# Rebuild on a Linux host with the Android NDK revision pinned in build/dependencies.toml.\n\
             : \"${ANDROID_NDK_HOME:?Set ANDROID_NDK_HOME to the pinned Android NDK}\"\n\
             export ANDROID_NDK_HOME\n",
        );
    }
    if !native_commands.is_empty() {
        instructions.push_str(
            "$env:VCPKG_DOWNLOADS = \"$PWD/.work/vcpkg-downloads\"\n\
             $env:VCPKG_BINARY_SOURCES = 'clear'\n",
        );
        instructions.push_str(&native_commands);
        if selected.subtitle {
            let install = dependencies::subtitle_install(inv, target)?;
            let prefix = install
                .parent()
                .and_then(Path::parent)
                .context("subtitle prefix")?
                .strip_prefix(&inv.root)?
                .to_string_lossy()
                .replace('\\', "/");
            instructions.push_str(&format!(
                "$env:ROUKAKU_SUBTITLE_VCPKG_ROOT = \"$PWD/{prefix}\"\n"
            ));
            if target.contains("linux") {
                instructions.push_str(&format!("$env:PKG_CONFIG_PATH = \"$PWD/{prefix}/installed/x64-linux-dynamic/lib/pkgconfig\"\n"));
            } else if dependencies::apple_subtitle_triplet(target).is_some() {
                let relative = install
                    .strip_prefix(&inv.root)?
                    .to_string_lossy()
                    .replace('\\', "/");
                instructions.push_str(&format!(
                    "$env:PKG_CONFIG_ALLOW_CROSS = '1'\n$env:PKG_CONFIG_ALL_STATIC = '1'\n$env:PKG_CONFIG_LIBDIR = \"$PWD/{relative}/lib/pkgconfig\"\n"
                ));
            }
        }
        if selected.ffmpeg {
            instructions.push_str("$env:VCPKG_ROOT = \"$PWD/.work/vcpkg-ffmpeg\"\n");
        }
    }
    if selected.native_vcpkg {
        let triplet = dependencies::native_triplet(inv, target)?;
        instructions.push_str(&format!(
            "$env:VCPKG_ROOT = \"$PWD/.work/vcpkg\"\n$env:SHINKI_EMU_NATIVE_VCPKG_ROOT = \"$PWD/.work/native-vcpkg\"\n$env:SHINKI_EMU_VCPKG_TRIPLET = \"{triplet}\"\n"
        ));
        if component != "manager" {
            let relative = dependencies::native_install(inv, target, component)?
                .strip_prefix(&inv.root)?
                .to_string_lossy()
                .replace('\\', "/");
            instructions.push_str(&format!(
                "$env:SHINKI_EMU_VCPKG_INSTALLED = \"$PWD/{relative}\"\n"
            ));
        }
    }
    if selected.impacto_sources || selected.renpy {
        instructions
            .push_str("$env:SHINKI_EMU_NATIVE_DOWNLOADS = \"$PWD/.work/native-downloads\"\n");
    }
    if selected.renpy {
        instructions.push_str(&format!(
            "$env:RENPY_SDK_ROOT = \"$PWD/.work/native-downloads/renpy-sdk\"\n\
             $env:RENPY_PYTHON_DEV = \"$PWD/.work/native-downloads/python\"\n\
             $env:RENPY_SDL_INCLUDE = \"$PWD/.work/native-downloads/sdl2/include\"\n\
             $env:Python3_EXECUTABLE = \"$PWD/.work/native-downloads/python/{}\"\n\
             $env:RENPY_CYTHON_LICENSE = \"$PWD/.work/native-downloads/cython-license/LICENSE.txt\"\n",
             dependencies::build_python_relative()
        ));
    }
    if selected.bison {
        if target.contains("windows") {
            instructions.push_str("$env:BISON_EXECUTABLE = (Get-Command win_bison.exe).Source\n");
        } else {
            instructions.push_str("$env:BISON_EXECUTABLE = \"$(command -v bison)\"\n");
        }
    }
    let build_command = if target.contains("android") {
        format!(
            "cargo ndk -t {} -P 26 build",
            if target.starts_with("aarch64") {
                "arm64-v8a"
            } else {
                "x86_64"
            }
        )
    } else {
        format!("cargo build --target {target}")
    };
    let artifact = if component != "manager" || target.contains("android") {
        "--lib"
    } else {
        "--bin shinki-emu-manager"
    };
    if target.ends_with("ohos") && component == "manager" {
        instructions.push_str(&format!(
            "cargo xtask build --target {target} --manager --release\n```\n"
        ));
    } else {
        instructions.push_str(&format!(
            "{build_command} --locked --release --manifest-path {manifest_relative} -p {package} {artifact}{feature_args}\n```\n"
        ));
    }
    if !native_commands.is_empty() {
        instructions.push_str(
            "\nnative/ports/ holds the exact vcpkg recipes and patches of the installed registry ports; overlay ports are part of the fork snapshot. \
             .work/vcpkg-downloads/ holds the original source archives vcpkg consumed. \
             Static libraries can be modified, rebuilt with these recipes, and relinked by rebuilding the consumer above. \
             Compatible replacement and reverse engineering for debugging modifications to LGPL components are permitted.\n",
        );
    }
    if selected.renpy && target.ends_with("-linux-android") {
        instructions.push_str(
            "\n## Ren'Py Android runtime rebuild and relink\n\n\
             Manager statically links the target CPython 3.12.8, SDL2, Ren'Py and codec archives. \
             The complete fork, its Android SDL patch and private-symbol builder are under ThirdParty/renpy; \
             native/renpy-build.tar contains the exact official recipes and tracked source archives. \
             native/renpy-sources and native/python-sources contain the additional native inputs and \
             the exact Python source/wheel inventory used by the target stage, not host-native wheels.\n\n\
             On Ubuntu 24.04 install the build prerequisites listed in the pinned recipe README. \
             Clone renpy/renpy-build at 7bfab40c1174f622f644b24669afd5fb167fbb79 and renpy/renpy at \
             39895c1e017f0b36ffea2447d97eccd69d76ee1c into its renpy/ subdirectory, then copy \
             the supplied source archives into that build root. Place the NDK r29 Linux archive and \
             Cubism 4-r.6.2 header SDK in tars/ using the URLs and checksums in ThirdParty/renpy/shinki/sdk.json. \
             Cubism Core is not part of Manager; its proprietary implementation is game-supplied.\n\n\
             Run ThirdParty/renpy/shinki/android_runtime.py with --build-root <recipe-root>, \
             --target aarch64-linux-android or x86_64-linux-android, and a new --output directory. \
             It stages target headers/archives, standard library, private symbols and exact Python artifacts. \
             Modify the LGPL sources or supplied patches before rebuilding; preserve modified sources. \
             Point RENPY_STATIC_ROOT at the rebuilt stage, then rebuild the full Manager from these sources \
             to relink every static dependency. The complete Manager and Rust dependency source is supplied \
             so no proprietary Manager object is required for relinking. Android packaging/signing tools \
             remain external build tools; use your own signing key for an installable modified APK.\n",
        );
    } else if selected.renpy {
        instructions.push_str(
            "\n## Ren'Py combined-runtime rebuild and relink\n\n\
             The shipped SDK is 8.5.3.26051504, SHA256 ff57648f9c04f27e381c48af6d8e3ee3cdec296bed4d3831f47f09b0a71b505e. \
             Its SDK metadata names Ren'Py source 39895c1e017f0b36ffea2447d97eccd69d76ee1c and renpy-build tag renpy-8.5.3.26051504, resolved to 7bfab40c1174f622f644b24669afd5fb167fbb79. \
             The SDK bytecode contains urllib3 2.2.2 and certifi 2026.04.22; the build recipe instead pins urllib3 2.7.0 and leaves certifi unpinned. \
             This mismatch establishes that the tag alone does not reproduce the Python payload; the actual copied versions and source hashes are in native/python-sources/sources.json. \
             It does not establish a different renpy-build commit. Keep those versions when rebuilding the copied Python payload.\n\n\
             native/renpy-build.tar contains the complete pinned recipes, source/ffmpeg-4.3.1.tar.gz, source/fribidi-1.0.7.tar.bz2, SDL sources and patches. \
             native/renpy-sources supplies exact additional static-library source snapshots and the official SDK Ren'Py source; native/python-sources includes readable chardet and certifi sdists. \
             These sources rebuild the LGPL parts inside librenpython.dll; libEGL.dll/libGLESv2.dll and the Windows compiler/OS SDK remain separate permissive/system inputs, with notices.\n\n\
             Rebuilding the combined DLL uses upstream's Ubuntu 24.04 cross-build environment (see renpy-build/README.rst). \
             Use the llvm-mingw-20241217-ucrt-ubuntu-20.04-x86_64 toolchain, host Python 3.12 with Cython 3.2.4/Jinja2/requests, autoconf/automake/libtool, CMake, Ninja, NASM and the system development libraries listed in prepare.sh. \
             Supply these build tools instead of running the online prepare.sh. Extract the archives and run the supplied archive-backed driver:\n\n\
             ```sh\nmkdir -p native/renpy-build\ntar -xf native/renpy-build.tar -C native/renpy-build\n\
             mkdir -p native/renpy-build/tmp/tars\n\
             cp \"$LLVM_MINGW_ARCHIVE\" native/renpy-build/tmp/tars/llvm-mingw-20241217-ucrt-ubuntu-20.04-x86_64.tar.xz\n\
             python3 native/renpy-offline.py --platform windows --arch x86_64 --stop renpython\n```\n\n\
             The link command in tasks/renpython.py combines librenpy.a, FFmpeg, FriBidi and the other static libraries into librenpython.dll. \
             Modify the LGPL sources/patches, rebuild those libraries, and run that link task again, retaining compatible exports. \
             Replace .work/native-downloads/renpy-sdk/lib/py3-windows-x86_64/librenpython.dll with the rebuilt DLL and rebuild the adapter above. \
             The adapter's shinki/CMakeLists.txt rebuilds Resources/overrides/display.pyd from the modified fork source and installs the bridge; replacing only display.pyd does not relink FFmpeg/FriBidi.\n"
        );
        fs::write(root.join("native/renpy-offline.py"), renpy_offline_driver())?;
    }
    if !target.contains("windows") {
        instructions = instructions
            .replace("```powershell", "```sh")
            .replace("$env:", "export ")
            .replace(" = ", "=")
            .replace("& $vcpkg ", "\"$vcpkg\" ")
            .replace("Copy-Item ", "cp ");
    }
    let readme = temp.path().join("BUILD-FROM-SOURCE.md");
    fs::write(&readme, &instructions)?;
    fs::write(root.join("BUILD-FROM-SOURCE.md"), instructions)?;
    builder.append_dir_all(".", &root)?;
    drop(builder.into_inner()?);
    package::zip_entries(
        &[
            (archive, "source.tar".into()),
            (readme, "BUILD-FROM-SOURCE.md".into()),
        ],
        out,
    )
}

fn renpy_offline_driver() -> &'static str {
    r#"#!/usr/bin/env python3
"""Run pinned upstream build tasks with archive-backed source preparation."""
import json, os, pathlib, runpy, shlex, shutil, subprocess, sys, tarfile

native = pathlib.Path(__file__).resolve().parent
root = native / "renpy-build"
sources = json.loads((native / "renpy-sources/sources.json").read_text())["sources"]
for item in sources:
    dest = root / ("renpy" if item["name"] == "renpy" else "unpacked/" + item["name"])
    if dest.exists():
        continue
    dest.mkdir(parents=True, exist_ok=True)
    with tarfile.open(native / "renpy-sources" / item["file_name"]) as archive:
        for member in archive.getmembers():
            parts = pathlib.PurePosixPath(member.name).parts[item["strip_components"]:]
            if not parts:
                continue
            member.name = "/".join(parts)
            archive.extract(member, dest, filter="data")
for item in sources:
    name = item["name"]
    if name in ("renpy", "cpython-mingw", "pyobjus"):
        continue
    dest = root / "tmp/source" / name
    if not dest.exists():
        shutil.copytree(root / "unpacked" / name, dest)
        patch = {"assimp": "assimp.diff", "libyuv": "libyuv-noshared.diff"}.get(name)
        if patch:
            subprocess.run(["patch", "-p1", "-i", str(root / "patches" / patch)], cwd=dest, check=True)
# Upstream's Windows Python task clones this local prepared source by name.
python_source = root / "unpacked/cpython-mingw"
if not (python_source / ".git").exists():
    subprocess.run(["git", "init", "-b", "mingw-v3.12.7"], cwd=python_source, check=True)
    subprocess.run(["git", "add", "."], cwd=python_source, check=True)
    subprocess.run(["git", "-c", "user.name=Source bundle", "-c", "user.email=source@example.invalid",
                    "-c", "commit.gpgsign=false", "commit", "-m", "Archive-backed Python source"], cwd=python_source, check=True)
python_archives = native / "python-sources"
# The shipped Python payload includes this already-generated permissive module,
# but ShinkiEMU does not redistribute or rebuild the proprietary Steam SDK.
steam = root / "tmp/py3/steam"
steam.mkdir(parents=True, exist_ok=True)
shutil.copy2(root / "steamapi/steamapi.py", steam / "steamapi.py")
os.environ["PYTHONHASHSEED"] = "0"
os.environ.update(PIP_NO_INDEX="1", PIP_FIND_LINKS=str(python_archives),
                  PIP_NO_BUILD_ISOLATION="1", PIP_DISABLE_PIP_VERSION_CHECK="1")
os.chdir(root)
sys.path.insert(0, str(root))
import tasks
import renpybuild.task
# Downloads are replaced with the exact extracted source inputs above; the
# cross compiler archive is supplied as a preinstalled system build tool.
renpybuild.task.tasks[:] = [t for t in renpybuild.task.tasks if t.task != "download"]
import tasks.python3
def install_python_libraries(c):
    c.run("{{ install }}/bin/hostpython3 -s -m ensurepip")
    inventory = json.loads((python_archives / "sources.json").read_text())
    def install(records):
        c.run("{{ install }}/bin/hostpython3 -s -m pip install --no-index --no-build-isolation --no-compile --no-deps " +
              " ".join(shlex.quote(str(python_archives / p["file_name"])) for p in records))
    install(inventory["tools"])
    install([p.get("wheel", p) for p in inventory["packages"]])
def unpack_pyobjus(c):
    c.clean()
    c.copytree(str(root / "unpacked/pyobjus"), "pyobjus")
    c.chdir("pyobjus")
    c.patch("pyobjus/ffi-h.diff")
for task in renpybuild.task.tasks:
    if task.name == "python3" and task.task == "pip":
        task.function = install_python_libraries
    if task.name == "pyobjus" and task.task in ("host_unpack", "unpack"):
        task.function = unpack_pyobjus
runpy.run_module("renpybuild", run_name="__main__")
"#
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cores::CoreSource;

    fn commit(directory: &Path, text: &str) -> String {
        fs::write(directory.join("payload.txt"), text).unwrap();
        dependencies::run(git(directory).args(["add", "Cargo.toml", "payload.txt"])).unwrap();
        dependencies::run(git(directory).args([
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "public fixture",
        ]))
        .unwrap();
        revision(directory).unwrap()
    }

    #[test]
    fn provenance_requires_real_ref_resolution_and_shared_adapter_ancestry() {
        let (_temp, _inv, core, repository, first, second) = fixture();
        let mut source = core.source.unwrap();
        source.rev = second.clone();
        source.upstream_identity.commit = first.clone();
        source.upstream_identity.ref_kind = shinki_emu_metadata::UpstreamRefKind::Branch;
        dependencies::run(git(&repository).args(["update-ref", "refs/proof/upstream", &second]))
            .unwrap();
        verify_provenance_at(&source, &repository).unwrap();
        source.upstream_identity.ref_kind = shinki_emu_metadata::UpstreamRefKind::Tag;
        assert!(verify_provenance_at(&source, &repository).is_err());
        dependencies::run(git(&repository).args(["update-ref", "refs/proof/upstream", &first]))
            .unwrap();
        verify_provenance_at(&source, &repository).unwrap();
        source.upstream_identity.commit = second;
        assert!(verify_provenance_at(&source, &repository).is_err());
    }

    fn fixture() -> (tempfile::TempDir, Inventory, Core, PathBuf, String, String) {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("upstream");
        fs::create_dir(&source).unwrap();
        dependencies::run(
            Command::new("git")
                .args(["init", "-b", "shinki"])
                .arg(&source),
        )
        .unwrap();
        fs::write(source.join("Cargo.toml"), "[package]\nname = \"public-source-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n").unwrap();
        let first = commit(&source, "first public source\n");
        let second = commit(&source, "second public source\n");
        let repository = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let mut inv = Inventory::load(repository).unwrap();
        inv.root = temp.path().join("product");
        fs::create_dir(&inv.root).unwrap();
        let mut core = inv.core("rfvp").unwrap().clone();
        core.manifest = "ThirdParty/fixture/Cargo.toml".into();
        core.source = Some(CoreSource {
            repo: source.to_string_lossy().into_owned(),
            branch: "shinki".into(),
            rev: first.clone(),
            upstream: "https://github.com/xmoezzz/rfvp".into(),
            upstream_identity: inv
                .core("rfvp")
                .unwrap()
                .source
                .as_ref()
                .unwrap()
                .upstream_identity
                .clone(),
            release_sequence: 1,
        });
        (temp, inv, core, source, first, second)
    }

    fn unpack(archive: &Path, out: &Path) {
        tar::Archive::new(fs::File::open(archive).unwrap())
            .unpack(out)
            .unwrap();
    }

    #[test]
    fn source_checkout_uses_pinned_commit_and_preserves_dirty_work_before_update() {
        let (_temp, inv, mut core, _upstream, first, second) = fixture();
        prepare(&inv, &core, false).unwrap();
        let directory = verify(&inv, &core, true).unwrap();
        assert_eq!(revision(&directory).unwrap(), first);
        assert_eq!(
            fs::read_to_string(directory.join("payload.txt")).unwrap(),
            "first public source\n"
        );
        fs::write(directory.join("payload.txt"), "user's unpublished change\n").unwrap();
        assert!(prepare(&inv, &core, false).is_err());
        prepare(&inv, &core, true).unwrap();
        assert_eq!(
            fs::read_to_string(directory.join("payload.txt")).unwrap(),
            "user's unpublished change\n"
        );
        core.source.as_mut().unwrap().rev = second.clone();
        assert!(prepare(&inv, &core, false).is_err());
        assert!(prepare(&inv, &core, true).is_err());
        assert_eq!(revision(&directory).unwrap(), first);
        assert_eq!(
            fs::read_to_string(directory.join("payload.txt")).unwrap(),
            "user's unpublished change\n"
        );
        fs::write(directory.join("payload.txt"), "first public source\n").unwrap();
        prepare(&inv, &core, false).unwrap();
        assert_eq!(revision(&directory).unwrap(), second);
        assert_eq!(
            fs::read_to_string(directory.join("payload.txt")).unwrap(),
            "second public source\n"
        );
    }

    #[test]
    fn crlf_product_checkout_is_clean_and_still_archives_committed_bytes() {
        // Windows runners check the product out with core.autocrlf=true.
        let (temp, _inv, _core, upstream, _first, second) = fixture();
        let checkout = temp.path().join("crlf-checkout");
        dependencies::run(
            Command::new("git")
                .args(["clone", "-q", "--config", "core.autocrlf=true"])
                .arg(&upstream)
                .arg(&checkout),
        )
        .unwrap();
        assert_eq!(
            fs::read(checkout.join("payload.txt")).unwrap(),
            b"second public source\r\n"
        );
        assert!(!state(&checkout).unwrap().dirty);
        let archive = temp.path().join("crlf.tar");
        let mut builder = tar::Builder::new(fs::File::create(&archive).unwrap());
        append_revision(&mut builder, &checkout, &second, Path::new("")).unwrap();
        builder.finish().unwrap();
        let extracted = temp.path().join("crlf-extracted");
        unpack(&archive, &extracted);
        assert_eq!(
            fs::read(extracted.join("payload.txt")).unwrap(),
            b"second public source\n"
        );
    }

    #[test]
    fn existing_plain_folder_cannot_redirect_checkout_into_the_product_repository() {
        let (_temp, inv, core, _upstream, _first, _second) = fixture();
        dependencies::run(Command::new("git").args(["init"]).arg(&inv.root)).unwrap();
        fs::write(inv.root.join("Cargo.toml"), "[workspace]\n").unwrap();
        let product_head = commit(&inv.root, "product source\n");
        let directory = core.source_directory(&inv.root).unwrap();
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("user.txt"), "keep this folder").unwrap();
        assert!(prepare(&inv, &core, false).is_err());
        assert!(prepare(&inv, &core, true).is_err());
        assert_eq!(revision(&inv.root).unwrap(), product_head);
        assert_eq!(
            fs::read_to_string(directory.join("user.txt")).unwrap(),
            "keep this folder"
        );
    }

    #[test]
    fn source_archive_expands_gitlinks_at_their_pins_without_working_tree_edits() {
        let (temp, inv, _core, upstream, _first, _second) = fixture();
        dependencies::run(Command::new("git").args(["init"]).arg(&inv.root)).unwrap();
        fs::write(inv.root.join("Cargo.toml"), "[workspace]\n").unwrap();
        dependencies::run(
            git(&inv.root)
                .args(["-c", "protocol.file.allow=always", "submodule", "add"])
                .arg(&upstream)
                .arg("nested"),
        )
        .unwrap();
        dependencies::run(git(&inv.root).args(["add", ".gitmodules", "nested"])).unwrap();
        let pin = commit(&inv.root, "outer\n");
        fs::write(inv.root.join("nested/payload.txt"), "unpublished change").unwrap();
        let archive = temp.path().join("source.tar");
        let mut builder = tar::Builder::new(fs::File::create(&archive).unwrap());
        append_revision(&mut builder, &inv.root, &pin, Path::new("")).unwrap();
        builder.finish().unwrap();
        let extracted = temp.path().join("extracted");
        unpack(&archive, &extracted);
        assert_eq!(
            fs::read_to_string(extracted.join("nested/payload.txt")).unwrap(),
            "second public source\n"
        );
    }

    #[test]
    fn named_native_source_caches_keep_nested_paths_and_reject_modified_archives() {
        let (temp, _inv, _core, upstream, first, _second) = fixture();
        let downloads = temp.path().join("downloads");
        let filename = "native-cache/library/source.tar.gz";
        let source = downloads.join(filename);
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        dependencies::run(
            git_committed_bytes(&upstream)
                .args(["archive", "--format=tar.gz", "-o"])
                .arg(&source)
                .arg(&first),
        )
        .unwrap();
        let checksum = sha512(&source).unwrap();
        let output = temp.path().join("copied-source");
        copy_named_archive(&downloads, filename, &checksum, &output).unwrap();
        let extracted = temp.path().join("nested-source");
        tar::Archive::new(flate2::read::GzDecoder::new(
            fs::File::open(output.join(filename)).unwrap(),
        ))
        .unpack(&extracted)
        .unwrap();
        assert_eq!(
            fs::read_to_string(extracted.join("payload.txt")).unwrap(),
            "first public source\n"
        );
        fs::write(source, b"modified source archive").unwrap();
        let rejected = temp.path().join("rejected-source");
        assert!(copy_named_archive(&downloads, filename, &checksum, &rejected).is_err());
        assert!(!rejected.exists());
    }

    #[test]
    fn native_git_sources_archive_the_pinned_commit_without_touching_working_edits() {
        let (temp, _inv, _core, upstream, first, _second) = fixture();
        let downloads = temp.path().join("downloads");
        fs::create_dir(&downloads).unwrap();
        let cached = downloads.join("git-tmp");
        fs::rename(upstream, &cached).unwrap();
        fs::write(cached.join("payload.txt"), "preserved working edit\n").unwrap();
        let output = temp.path().join("sources");
        native_git_source(
            &downloads,
            "native-dependency",
            &format!("git+https://example.invalid/native-source@{first}"),
            &output,
        )
        .unwrap();
        let archive =
            fs::File::open(output.join(format!("native-dependency-{first}.tar.gz"))).unwrap();
        let extracted = temp.path().join("extracted-git-source");
        tar::Archive::new(flate2::read::GzDecoder::new(archive))
            .unpack(&extracted)
            .unwrap();
        assert_eq!(
            fs::read_to_string(extracted.join("payload.txt")).unwrap(),
            "first public source\n"
        );
        assert_eq!(
            fs::read_to_string(cached.join("payload.txt")).unwrap(),
            "preserved working edit\n"
        );
    }

    #[test]
    fn source_archive_materializes_only_checksum_matched_lfs_payloads() {
        use sha2::Digest;
        let (temp, _inv, _core, upstream, _first, _second) = fixture();
        let payload = b"materialized public fixture bytes";
        let oid = hex::encode(sha2::Sha256::digest(payload));
        let pointer = format!(
            "version https://git-lfs.github.com/spec/v1\noid sha256:{oid}\nsize {}\n",
            payload.len()
        );
        let pin = commit(&upstream, &pointer);
        fs::write(upstream.join("payload.txt"), payload).unwrap();
        let archive = temp.path().join("materialized.tar");
        let mut builder = tar::Builder::new(fs::File::create(&archive).unwrap());
        append_revision(&mut builder, &upstream, &pin, Path::new("")).unwrap();
        builder.finish().unwrap();
        let extracted = temp.path().join("materialized");
        unpack(&archive, &extracted);
        assert_eq!(fs::read(extracted.join("payload.txt")).unwrap(), payload);
        fs::write(upstream.join("payload.txt"), pointer).unwrap();
        let mut builder =
            tar::Builder::new(fs::File::create(temp.path().join("pointer.tar")).unwrap());
        assert!(append_revision(&mut builder, &upstream, &pin, Path::new("")).is_err());
        fs::write(upstream.join("payload.txt"), vec![b'x'; payload.len()]).unwrap();
        let mut builder =
            tar::Builder::new(fs::File::create(temp.path().join("modified.tar")).unwrap());
        assert!(append_revision(&mut builder, &upstream, &pin, Path::new("")).is_err());
    }

    #[test]
    fn source_obligation_follows_the_selectable_license() {
        for (expression, source, whole) in [
            ("MIT OR Apache-2.0", false, false),
            ("MPL-2.0", true, false),
            ("MIT AND MPL-2.0", true, false),
            ("Apache-2.0 OR GPL-2.0-only", false, false),
            (
                "GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0",
                false,
                false,
            ),
            ("AGPL-3.0-only", true, true),
            ("MIT AND LGPL-2.1-or-later", true, true),
        ] {
            assert_eq!(
                obliges(expression, copyleft).unwrap(),
                source,
                "{expression}"
            );
            assert_eq!(
                obliges(expression, gnu_copyleft).unwrap(),
                whole,
                "{expression}"
            );
        }
        assert!(obliges("not an SPDX expression", copyleft).is_err());
    }

    #[test]
    fn source_bundle_ships_snapshot_and_only_obligated_crate_sources() {
        let (temp, _, mut embedded, upstream, _, _) = fixture();
        fs::write(
            upstream.join("runtime.cpp"),
            "int native_runtime() { return 7; }\n",
        )
        .unwrap();
        dependencies::run(git(&upstream).args(["add", "runtime.cpp"])).unwrap();
        let embedded_pin = commit(&upstream, "embedded pinned source\n");
        commit(&upstream, "later upstream source\n");
        embedded.source.as_mut().unwrap().rev = embedded_pin.clone();
        embedded.dependencies.clear();
        embedded.static_manager_targets = vec!["x86_64-pc-windows-msvc".into()];
        let mut dependencies_table = toml::Table::new();
        for (name, license) in [("weak-copyleft", "MPL-2.0"), ("permissive", "MIT")] {
            let repo = temp.path().join(name);
            fs::create_dir_all(repo.join("src")).unwrap();
            fs::write(
                repo.join("Cargo.toml"),
                format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\nlicense = \"{license}\"\n"),
            )
            .unwrap();
            fs::write(repo.join("src/lib.rs"), format!("// {name}\n")).unwrap();
            dependencies::run(git(&repo).args(["init"])).unwrap();
            dependencies::run(git(&repo).args(["add", "."])).unwrap();
            dependencies::run(git(&repo).args([
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-m",
                "dependency fixture",
            ]))
            .unwrap();
            dependencies_table.insert(
                name.into(),
                toml::Value::try_from(serde_json::json!({
                    "git": reqwest::Url::from_directory_path(&repo).unwrap().as_str(),
                    "rev": revision(&repo).unwrap(),
                }))
                .unwrap(),
            );
        }
        let original = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let mut inv = Inventory::load(original).unwrap();
        inv.root = temp.path().join("product");
        fs::create_dir_all(inv.root.join("src")).unwrap();
        let mut manifest = toml::Table::new();
        manifest.insert(
            "package".into(),
            toml::Value::try_from(serde_json::json!({
                "name":"shinki-emu-manager", "version":"0.1.0", "edition":"2024", "license":"MIT",
            }))
            .unwrap(),
        );
        manifest.insert("dependencies".into(), dependencies_table.into());
        fs::write(
            inv.root.join("Cargo.toml"),
            toml::to_string(&manifest).unwrap(),
        )
        .unwrap();
        fs::write(
            inv.root.join("src/lib.rs"),
            "pub fn answer() -> u32 { 42 }\n",
        )
        .unwrap();
        fs::write(inv.root.join(".gitignore"), "/.work/\n").unwrap();
        dependencies::run(
            Command::new("cargo")
                .current_dir(&inv.root)
                .arg("generate-lockfile"),
        )
        .unwrap();
        dependencies::run(Command::new("git").args(["init"]).arg(&inv.root)).unwrap();
        dependencies::run(git(&inv.root).args(["add", "Cargo.lock", ".gitignore", "src"])).unwrap();
        commit(&inv.root, "product source\n");
        inv.cores.retain(|core| core.id != embedded.id);
        inv.cores.push(embedded.clone());
        prepare(&inv, &embedded, false).unwrap();
        let output = temp.path().join("source.zip");
        bundle(&inv, "manager", "x86_64-pc-windows-msvc", &output).unwrap();
        let mut zip = zip::ZipArchive::new(fs::File::open(output).unwrap()).unwrap();
        let extracted = temp.path().join("fresh");
        tar::Archive::new(zip.by_name("source.tar").unwrap())
            .unpack(&extracted)
            .unwrap();
        assert_eq!(
            fs::read_to_string(extracted.join("payload.txt")).unwrap(),
            "product source\n"
        );
        assert!(extracted.join("BUILD-FROM-SOURCE.md").is_file());
        assert_eq!(
            fs::read(extracted.join("ThirdParty/fixture/runtime.cpp")).unwrap(),
            fs::read(upstream.join("runtime.cpp")).unwrap()
        );
        assert_eq!(
            fs::read_to_string(extracted.join("ThirdParty/fixture/payload.txt")).unwrap(),
            "embedded pinned source\n"
        );
        let references: Value =
            serde_json::from_slice(&fs::read(extracted.join("source-references.json")).unwrap())
                .unwrap();
        assert_eq!(
            references["embedded_cores"][0]["source"]["rev"],
            embedded_pin
        );
        let crates: Vec<_> = fs::read_dir(extracted.join("rust-sources"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(crates.len(), 1, "{crates:?}");
        assert!(crates[0].starts_with("weak-copyleft-0.1.0+git-"));
        assert_eq!(
            fs::read_to_string(
                extracted
                    .join("rust-sources")
                    .join(&crates[0])
                    .join("src/lib.rs")
            )
            .unwrap(),
            "// weak-copyleft\n"
        );
    }

    #[test]
    fn native_graph_follows_host_tools_and_ignores_removed_or_unrelated_records() {
        let temp = tempfile::tempdir().unwrap();
        let install = temp.path().join("installed/target");
        fs::create_dir_all(install.join("share/doc")).unwrap();
        fs::create_dir_all(temp.path().join("installed/vcpkg")).unwrap();
        assert!(installed_ports(&install).is_err());
        let status = "Package: library\nArchitecture: target\nDepends: generator:host\nStatus: install ok installed\n\n\
                      Package: generator\nArchitecture: host\nDepends: tool:host\nStatus: install ok installed\n\n\
                      Package: tool\nArchitecture: host\nStatus: install ok installed\n\n\
                      Package: unrelated\nArchitecture: host\nStatus: install ok installed\n\n\
                      Package: removed\nArchitecture: target\nStatus: purge ok not-installed\n";
        fs::write(temp.path().join("installed/vcpkg/status"), status).unwrap();
        let ports = installed_ports(&install).unwrap();
        assert_eq!(ports.len(), 3);
        assert!(
            ports
                .iter()
                .any(|(name, root)| name == "library" && root == &install)
        );
        assert!(
            ports
                .iter()
                .any(|(name, root)| name == "tool" && root.ends_with("host"))
        );
        fs::write(
            temp.path().join("installed/vcpkg/status"),
            status.replace("Package: tool", "Package: missing"),
        )
        .unwrap();
        assert!(installed_ports(&install).is_err());
    }

    #[test]
    fn installed_registry_tree_wins_over_head_and_overlays_are_skipped() {
        let temp = tempfile::tempdir().unwrap();
        let registry = temp.path().join("registry");
        let head = registry.join("ports/example");
        fs::create_dir_all(&head).unwrap();
        dependencies::run(git(&registry).args(["init"])).unwrap();
        let commit = || {
            dependencies::run(git(&registry).args(["add", "."])).unwrap();
            dependencies::run(git(&registry).args([
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-m",
                "recipe fixture",
            ]))
            .unwrap()
        };
        fs::write(head.join("portfile.cmake"), "original recipe\n").unwrap();
        fs::write(head.join("fix.patch"), "original patch\n").unwrap();
        commit();
        let tree = String::from_utf8(
            dependencies::capture(git(&registry).args(["rev-parse", "HEAD:ports/example"]))
                .unwrap(),
        )
        .unwrap();
        fs::write(head.join("portfile.cmake"), "new HEAD recipe\n").unwrap();
        commit();
        let provenance = serde_json::json!({"packages":[{"SPDXID":"SPDXRef-port", "externalRefs":[{
            "referenceLocator":format!("gitoid:tree:sha1:{}", tree.trim())
        }]}]});
        let output = temp.path().join("bundled");
        assert!(port_recipe(&provenance, &registry, &output).unwrap());
        assert_eq!(
            fs::read_to_string(output.join("portfile.cmake")).unwrap(),
            "original recipe\n"
        );
        assert_eq!(
            fs::read_to_string(output.join("fix.patch")).unwrap(),
            "original patch\n"
        );
        let overlay = serde_json::json!({"packages":[{"SPDXID":"SPDXRef-port", "downloadLocation":"NOASSERTION"}]});
        let skipped = temp.path().join("overlay");
        assert!(!port_recipe(&overlay, &registry, &skipped).unwrap());
        assert!(!skipped.exists());
    }
}
