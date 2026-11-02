use crate::{cores::Inventory, dependencies, sources};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use shinki_emu_build::CoreBuildManifest;
use std::{
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildResult {
    pub component: String,
    pub target: String,
    pub profile: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub product_version: Option<String>,
    pub library: PathBuf,
    pub core_build: Option<CoreBuildManifest>,
    pub embedded_core_builds: Vec<CoreBuildManifest>,
    pub source_commit: String,
    pub source_dirty: bool,
    pub toolchain: String,
    pub features: Vec<String>,
    pub target_dir: PathBuf,
}

impl BuildResult {
    pub fn require_product_version(&self, expected: &str) -> Result<()> {
        let actual = self.product_version.as_deref().context(
            "Manager build receipt lacks compiled product_version; rebuild with SHINKI_EMU_PRODUCT_VERSION",
        )?;
        if actual != expected {
            bail!(
                "Manager was compiled as {actual}, not requested package version {expected}; rebuild with the requested product version"
            );
        }
        Ok(())
    }
}

fn manager_product_version<'a>(
    package_version: &'a str,
    explicit: Option<&'a str>,
    ci: Option<&'a str>,
) -> Result<&'a str> {
    if let (Some(explicit), Some(ci)) = (explicit, ci)
        && explicit != ci
    {
        bail!("SHINKI_EMU_PRODUCT_VERSION and SHINKI_EMU_CI_VERSION disagree");
    }
    let version = explicit.or(ci).unwrap_or(package_version);
    semver::Version::parse(version).context("Manager product version must be SemVer")?;
    Ok(version)
}

#[derive(Deserialize)]
struct ManagerBuildVersion {
    version: String,
}

fn manager_build_version(path: &Path, expected: &str) -> Result<String> {
    let compiled: ManagerBuildVersion = serde_json::from_slice(
        &fs::read(path)
            .with_context(|| format!("missing Manager compile identity {}", path.display()))?,
    )?;
    if compiled.version != expected {
        bail!(
            "Manager build-script version {} differs from requested compile version {expected}",
            compiled.version
        );
    }
    Ok(compiled.version)
}
fn receipt_path(inv: &Inventory, target: &str, component: &str) -> PathBuf {
    inv.root
        .join(".work/build")
        .join(target)
        .join(format!("{component}.json"))
}

fn target_directory(inv: &Inventory, target: &str, component: &str) -> Result<PathBuf> {
    let standard = dunce::simplified(&inv.root)
        .join(".work/target")
        .join(target)
        .join(component);
    let explicit = std::env::var_os("SHINKI_EMU_TARGET_ROOT").map(PathBuf::from);
    // Leave headroom for shaderc/CMake's nested source and object filenames.
    if explicit.is_none() && (!cfg!(windows) || standard.as_os_str().len() <= 60) {
        return Ok(standard);
    }
    let repository = dunce::canonicalize(&inv.root)?;
    let owner = serde_json::json!({
        "repository": repository, "target": target, "component": component
    });
    let identity = hex::encode(Sha256::digest(serde_json::to_vec(&owner)?));
    let short_root = if let Some(root) = explicit {
        dunce::simplified(&std::path::absolute(root)?).to_owned()
    } else {
        repository.join(".work/t")
    };
    let directory = short_root.join(&identity[..12]);
    fs::create_dir_all(&directory).with_context(|| {
        format!("cannot create isolated short target {}; set SHINKI_EMU_TARGET_ROOT to a writable short directory", directory.display())
    })?;
    crate::package::plain(&directory)?;
    let marker = directory.join("owner.json");
    if marker.exists() {
        let actual: serde_json::Value = serde_json::from_slice(&fs::read(&marker)?)?;
        if actual != owner {
            bail!(
                "short target directory belongs to another build: {}",
                directory.display()
            );
        }
    } else {
        let file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(marker)?;
        serde_json::to_writer_pretty(file, &owner)?;
    }
    Ok(directory)
}
pub fn load_receipt(inv: &Inventory, target: &str, component: &str) -> Result<BuildResult> {
    inv.require_buildable(component, target)?;
    let path = receipt_path(inv, target, component);
    let r: BuildResult = serde_json::from_slice(&fs::read(&path).with_context(|| {
        format!(
            "missing {}; run cargo xtask build --target {target} --{} --release",
            path.display(),
            if component == "manager" {
                "manager".into()
            } else {
                format!("core {component}")
            }
        )
    })?)?;
    if r.target != target || r.component != component {
        bail!("build receipt component/target mismatch");
    }
    crate::package::plain(&r.library)?;
    Ok(r)
}
pub fn cargo_metadata(inv: &Inventory, target: &str) -> Result<serde_json::Value> {
    Ok(serde_json::from_slice(&dependencies::capture(
        Command::new("cargo").current_dir(&inv.root).args([
            "metadata",
            "--locked",
            "--format-version",
            "1",
            "--filter-platform",
            target,
        ]),
    )?)?)
}
pub fn execute(
    inv: &Inventory,
    target: &str,
    component: &str,
    action: &str,
    release: bool,
) -> Result<Option<BuildResult>> {
    inv.require_buildable(component, target)?;
    dependencies::doctor(inv, target, Some(component).filter(|id| *id != "manager"))?;
    let core = if component == "manager" {
        None
    } else {
        Some(inv.core(component)?)
    };
    let manifest = core
        .map(|c| inv.root.join(&c.manifest))
        .unwrap_or_else(|| inv.root.join("Cargo.toml"));
    if !manifest.is_file() {
        bail!("adapter manifest missing: {}", manifest.display());
    }
    let source_state = if action == "build" {
        let directory = if let Some(core) = core {
            core.source_directory(&inv.root)?
        } else {
            inv.root.clone()
        };
        let state = sources::state(&directory)?;
        Some((directory, state))
    } else {
        None
    };
    let package = core
        .map(|c| c.package.as_str())
        .unwrap_or("shinki-emu-manager");
    let features = core.map(|c| c.features.clone()).unwrap_or_default();
    let target_dir = target_directory(inv, target, component)?;
    let metadata: serde_json::Value = serde_json::from_slice(&dependencies::capture(
        Command::new("cargo")
            .current_dir(&inv.root)
            .args([
                "metadata",
                "--locked",
                "--format-version",
                "1",
                "--no-deps",
                "--manifest-path",
            ])
            .arg(&manifest),
    )?)?;
    let package_metadata = metadata["packages"]
        .as_array()
        .context("cargo packages")?
        .iter()
        .find(|p| p["name"] == package)
        .with_context(|| format!("package {package} absent from {}", manifest.display()))?;
    let package_id = package_metadata["id"]
        .as_str()
        .context("cargo package id")?;
    let version_inputs = (component == "manager").then(|| {
        [
            std::env::var_os("SHINKI_EMU_PRODUCT_VERSION"),
            std::env::var_os("SHINKI_EMU_CI_VERSION"),
        ]
    });
    let product_version = version_inputs
        .as_ref()
        .map(|[explicit, ci]| {
            manager_product_version(
                package_metadata["version"]
                    .as_str()
                    .context("Manager package version")?,
                explicit
                    .as_deref()
                    .map(|v| v.to_str().context("non-UTF-8 SHINKI_EMU_PRODUCT_VERSION"))
                    .transpose()?,
                ci.as_deref()
                    .map(|v| v.to_str().context("non-UTF-8 SHINKI_EMU_CI_VERSION"))
                    .transpose()?,
            )
        })
        .transpose()?;
    let android_abi = if target.contains("android") {
        Some(if target.starts_with("aarch64") {
            "arm64-v8a"
        } else {
            "x86_64"
        })
    } else {
        None
    };
    let mut cmd = if crate::apple::supported(target) {
        crate::apple::command(&inv.root, target)?
    } else {
        Command::new("cargo")
    };
    cmd.current_dir(&inv.root);
    cmd.args([action, "--locked", "--manifest-path"])
        .arg(&manifest)
        .args([
            "-p",
            package,
            "--message-format=json-render-diagnostics",
            "--target-dir",
        ])
        .arg(&target_dir);
    cmd.args(["--target", target]);
    if release {
        cmd.arg("--release");
    }
    // Android and OHOS load the Manager as a cdylib from their app shells.
    let library_host = target.contains("android") || target.ends_with("ohos");
    if core.is_some() || library_host {
        cmd.arg("--lib");
    } else {
        cmd.args(["--bin", "shinki-emu-manager"]);
    }
    if !features.is_empty() {
        cmd.arg("--features").arg(features.join(","));
    }
    dependencies::apply_environment(inv, target, component, &mut cmd)?;
    if let Some(abi) = android_abi {
        // cargo-ndk consumes build-script JSON. Import its toolchain environment
        // and invoke Cargo directly so every embedded manifest stays observable.
        let mut environment = Command::new("cargo");
        environment
            .current_dir(&inv.root)
            .args(["ndk-env", "--target", target, "-P", "26", "--json"]);
        dependencies::apply_environment(inv, target, component, &mut environment)?;
        let environment: std::collections::BTreeMap<String, String> =
            serde_json::from_slice(&dependencies::capture(&mut environment)?)?;
        let clang = environment
            .get("CLANG_PATH")
            .context("cargo ndk-env omitted CLANG_PATH")?;
        // ndk-env omits the two private variables its linker wrapper requires.
        cmd.envs(&environment)
            .env("_CARGO_NDK_LINK_CLANG", clang)
            .env("_CARGO_NDK_LINK_TARGET", format!("--target={target}26"))
            .env("ANDROID_PLATFORM", "26")
            .env("ANDROID_ABI", abi)
            .env("CARGO_NDK_ANDROID_PLATFORM", abi);
    }
    if let Some(version) = product_version {
        cmd.env("SHINKI_EMU_PRODUCT_VERSION", version);
    }
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;
    let output = child.stdout.take().context("Cargo stdout")?;
    let mut libraries = Vec::new();
    let mut manifests = Vec::new();
    let mut parse_error = None;
    let mut compiled_product_version = None;
    for line in BufReader::new(output).lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                parse_error = Some(anyhow::Error::from(e));
                break;
            }
        };
        let value: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => {
                eprintln!("{line}");
                continue;
            }
        };
        if value["reason"] == "compiler-message"
            && let Some(s) = value["message"]["rendered"].as_str()
        {
            eprint!("{s}");
        }
        if value["reason"] == "build-script-executed"
            && (value["package_id"] == package_id || component == "manager")
            && let Some(dir) = value["out_dir"].as_str()
        {
            if value["package_id"] == package_id
                && let Some(expected) = product_version
            {
                let path = Path::new(dir).join("manager-build-version.json");
                match manager_build_version(&path, expected) {
                    Ok(version) => compiled_product_version = Some(version),
                    Err(error) => parse_error = Some(error),
                }
            }
            let path = Path::new(dir).join("core-build.json");
            if path.is_file() {
                match fs::read(&path)
                    .map_err(anyhow::Error::from)
                    .and_then(|bytes| {
                        serde_json::from_slice::<CoreBuildManifest>(&bytes)
                            .map_err(anyhow::Error::from)
                    }) {
                    Ok(m) => manifests.push(m),
                    Err(e) => parse_error = Some(e),
                }
            }
        }
        if value["package_id"] != package_id {
            continue;
        }
        if value["reason"] == "compiler-artifact" {
            if core.is_none() && !library_host {
                if let Some(s) = value["executable"].as_str() {
                    libraries.push(PathBuf::from(s));
                }
            } else if value["target"]["crate_types"]
                .as_array()
                .is_some_and(|a| a.iter().any(|v| v == "cdylib"))
            {
                for f in value["filenames"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|v| v.as_str())
                {
                    let p = Path::new(f);
                    if p.extension()
                        .is_some_and(|s| s == "dll" || s == "so" || s == "dylib")
                    {
                        libraries.push(p.to_owned());
                    }
                }
            }
        }
    }
    let status = child.wait()?;
    if let Some(e) = parse_error {
        return Err(e);
    }
    if !status.success() {
        bail!("Cargo {action} failed for {component}/{target}: {status}");
    }
    if action != "build" {
        return Ok(None);
    }
    if component == "manager" && compiled_product_version.is_none() {
        bail!("Cargo did not report the Manager build-script version identity");
    }
    let mut embedded_core_builds = Vec::new();
    if component == "manager" {
        for manifest in manifests.drain(..) {
            let embedded = inv.core(&manifest.plugin_id)?;
            manifest.validate()?;
            if !embedded
                .static_manager_targets
                .iter()
                .any(|triple| triple == target)
                || manifest.target != target
                || manifest.profile != if release { "release" } else { "debug" }
                || !matches!(
                    manifest.artifact_kind.as_str(),
                    "native-static" | "rust-cdylib"
                )
            {
                bail!(
                    "native build manifest does not match embedded core {}",
                    embedded.id
                );
            }
            embedded_core_builds.push(manifest);
        }
    }
    libraries.sort();
    libraries.dedup();
    let core_build = if core.is_some() {
        if manifests.len() != 1 {
            bail!(
                "expected one core-build.json from {package}'s build-script-executed, found {}",
                manifests.len()
            );
        }
        let m = manifests.remove(0);
        m.validate()?;
        if m.plugin_id != component
            || m.target != target
            || m.profile != if release { "release" } else { "debug" }
            || Some(m.artifact_kind.as_str()) != core.map(|c| c.artifact_kind.as_str())
        {
            bail!("core-build manifest does not match requested build");
        }
        Some(m)
    } else {
        None
    };
    let library = if let Some(native) = core_build.as_ref().and_then(|m| m.native_library.as_ref())
    {
        native.clone()
    } else {
        if libraries.len() != 1 {
            bail!(
                "Cargo did not report exactly one dynamic library/executable for {package}: {libraries:?}"
            );
        }
        libraries.remove(0)
    };
    crate::binary::inspect(&library, target, core.is_some())?;
    let (source_directory, before) = source_state.context("build source state missing")?;
    let after = sources::state(&source_directory)?;
    let source_commit = before.commit;
    let source_dirty = before.dirty || after.dirty || after.commit != source_commit;
    let toolchain = String::from_utf8(dependencies::capture(Command::new("rustc").arg("-Vv"))?)?;
    let result = BuildResult {
        component: component.into(),
        target: target.into(),
        profile: if release { "release" } else { "debug" }.into(),
        library,
        core_build,
        embedded_core_builds,
        product_version: compiled_product_version,
        source_commit,
        source_dirty,
        toolchain,
        features,
        target_dir,
    };
    if component == "manager" && target.contains("android") {
        let generated = result.target_dir.join("android-inputs");
        if generated.exists() {
            fs::remove_dir_all(&generated)?;
        }
        crate::package::atomic_directory(&generated, |stage| {
            crate::package::android_inputs(inv, &result, stage)
        })?;
        println!("Android JNI/assets: {}", generated.display());
    }
    let path = receipt_path(inv, target, component);
    fs::create_dir_all(path.parent().unwrap())?;
    fs::write(path, serde_json::to_vec_pretty(&result)?)?;
    Ok(Some(result))
}
pub fn tests(inv: &Inventory, core: Option<&str>, manager: bool) -> Result<()> {
    let mut cmd = Command::new("cargo");
    cmd.current_dir(&inv.root).args(["test", "--locked"]);
    if let Some(c) = core {
        let c = inv.core(c)?;
        cmd.arg("--manifest-path")
            .arg(inv.root.join(&c.manifest))
            .args(["-p", &c.package]);
        if !c.features.is_empty() {
            cmd.arg("--features").arg(c.features.join(","));
        }
        cmd.arg("--target-dir")
            .arg(inv.root.join(".work/test").join(&c.id));
    } else if manager {
        cmd.args(["-p", "shinki-emu-manager"])
            .arg("--target-dir")
            .arg(inv.root.join(".work/test/manager"));
    } else {
        cmd.args(["-p", "xtask", "-p", "shinki-emu-build"]);
    }
    if core.is_some() || manager {
        let version = String::from_utf8(dependencies::capture(Command::new("rustc").arg("-vV"))?)?;
        let host = version
            .lines()
            .find_map(|s| s.strip_prefix("host: "))
            .context("rustc host triple")?;
        dependencies::apply_environment(inv, host, core.unwrap_or("manager"), &mut cmd)?;
    }
    dependencies::run(&mut cmd)
}

#[cfg(test)]
mod version_tests {
    use super::*;

    #[test]
    fn manager_compile_inputs_reject_conflicting_and_invalid_versions() {
        assert!(
            manager_product_version("0.1.0-beta.6", Some("0.1.0-beta.8"), Some("0.1.0-beta.7"))
                .is_err()
        );
        for invalid in ["", "beta.8", "v0.1.0-beta.8", "0.1.0-beta.8 "] {
            assert!(manager_product_version("0.1.0-beta.6", Some(invalid), None).is_err());
            assert!(manager_product_version("0.1.0-beta.6", None, Some(invalid)).is_err());
        }
    }

    #[test]
    fn manager_build_sidecar_rejects_an_old_or_missing_compile_identity() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("manager-build-version.json");
        assert!(manager_build_version(&path, "0.1.0-beta.8").is_err());
        fs::write(&path, r#"{"version":"0.1.0-beta.6"}"#).unwrap();
        assert!(manager_build_version(&path, "0.1.0-beta.8").is_err());
        fs::write(&path, r#"{"package_version":"0.1.0-beta.8"}"#).unwrap();
        assert!(manager_build_version(&path, "0.1.0-beta.8").is_err());
    }
}
