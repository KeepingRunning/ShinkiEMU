use crate::{build, cores::Inventory, package};
use anyhow::{Context, Result};
use serde::Serialize;
use std::{fs, io::Write, path::Path};
#[derive(Serialize)]
struct Report<'a> {
    schema: &'static str,
    component: &'a str,
    target: &'a str,
    status: &'static str,
    reason: Option<String>,
    artifact: Option<std::path::PathBuf>,
}
pub fn run(inv: &Inventory, target: &str, component: &str, path: &Path) -> Result<()> {
    let mut blocked = inv.blocked(component, target)?.map(str::to_owned);
    if target.contains("ohos")
        && std::env::var_os("OHOS_SDK_NATIVE").is_none()
        && let Some(reason) = blocked.as_mut()
    {
        reason.push_str(" Missing prerequisite: OHOS_SDK_NATIVE.");
    }
    let mut report = Report {
        schema: "shinki.emu.ci.v1",
        component,
        target,
        status: "blocked",
        reason: blocked.clone(),
        artifact: None,
    };
    let result = if blocked.is_some() {
        Ok(())
    } else {
        (|| -> Result<()> {
            if component == "manager" && target.contains("android") {
                let other = if target == "aarch64-linux-android" {
                    "x86_64-linux-android"
                } else {
                    "aarch64-linux-android"
                };
                build::execute(inv, other, component, "build", true)?;
            }
            let built = build::execute(inv, target, component, "build", true)?
                .expect("build returns artifact");
            report.status = "built";
            report.artifact = Some(built.library);
            let version = if component != "manager" && inv.core(component)?.source.is_some() {
                crate::release::resolve_version(inv, component, "")?
            } else {
                std::env::var("SHINKI_EMU_CI_VERSION")
                    .context("SHINKI_EMU_CI_VERSION required for Manager/self-owned CI package")?
            };
            // Restricted binaries are not uploaded, even as Actions artifacts.
            if component != "manager" && !inv.core(component)?.publish_blocked.is_empty() {
                report.status = "publish_blocked";
                report.reason = Some(inv.core(component)?.publish_blocked.clone());
                report.artifact = None;
                return Ok(());
            }
            let out = inv.root.join(".work/ci").join(target).join(component);
            let staged = package::package(inv, target, component, &version, &out)?;
            if component == "manager" && target.contains("android") {
                let record: serde_json::Value =
                    serde_json::from_slice(&fs::read(staged.join("android.json"))?)?;
                let apk = record["apk"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Android package APK missing"))?;
                package::portable(apk)?;
                report.artifact = Some(staged.join(apk));
            } else if component == "manager" && target == "aarch64-apple-ios" {
                report.artifact =
                    Some(staged.join(format!("ShinkiEMU-{version}-{target}-device-test.ipa")));
            } else {
                report.artifact = Some(archive_package(&staged, &out)?);
            }
            Ok(())
        })()
    };
    if let Err(e) = &result {
        report.status = "failed";
        report.reason = Some(format!("{e:#}"));
        report.artifact = None;
    }
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec_pretty(&report)?)?;
    if let Some(summary) = std::env::var_os("GITHUB_STEP_SUMMARY") {
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(summary)?;
        writeln!(
            f,
            "### {component} / {target}\n\n**{}**\n\n{}\n\nArtifact: {}\n",
            report.status,
            report.reason.as_deref().unwrap_or(""),
            report
                .artifact
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "null".into())
        )?;
    }
    println!("{}", serde_json::to_string(&report)?);
    result
}

fn archive_package(staged: &Path, out: &Path) -> Result<std::path::PathBuf> {
    let mut name = out
        .file_name()
        .context("CI package output name")?
        .to_os_string();
    name.push(".zip");
    let archive = out.with_file_name(name);
    package::zip_entries(&package::tree(staged)?, &archive)?;
    Ok(archive)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn dotted_core_ids_keep_independent_archive_contents() {
        let temp = tempfile::tempdir().unwrap();
        let mut artifacts = Vec::new();
        for (core, contents) in [
            ("shinki.emu.musica", b"musica".as_slice()),
            ("shinki.emu.cmvs", b"cmvs".as_slice()),
        ] {
            let staged = temp.path().join(core);
            fs::create_dir(&staged).unwrap();
            fs::write(staged.join("core.bin"), contents).unwrap();
            artifacts.push((archive_package(&staged, &staged).unwrap(), contents));
        }
        for (artifact, expected) in artifacts {
            let mut archive = zip::ZipArchive::new(fs::File::open(artifact).unwrap()).unwrap();
            let mut actual = Vec::new();
            archive
                .by_name("core.bin")
                .unwrap()
                .read_to_end(&mut actual)
                .unwrap();
            assert_eq!(actual, expected);
        }
    }
}
