//! HarmonyOS/OpenHarmony Manager HAP: stage the Rust cdylib and bundled
//! resources into `Platforms/OHOS`, build with hvigor, sign with the SDK's
//! hap-sign-tool and verify the signed archive.
use crate::{
    binary,
    build::BuildResult,
    cores::Inventory,
    dependencies,
    package::{self, atomic_directory, copy_entries, sha256, tree},
};
use anyhow::{Context, Result, bail};
use std::{
    fs, io,
    path::{Path, PathBuf},
    process::Command,
};

const ABI: &str = "arm64-v8a";
const BUNDLE_NAME: &str = "org.shinkirou.shinkiemu";
const MANAGER: &str = "libshinki_emu_manager.so";
/// Public password of the OpenHarmony SDK test keystore (`OpenHarmony.p12`).
/// The generated debug key lives in a copy of that test keystore.
const TEST_KEYSTORE_PASSWORD: &str = "123456";
const DEBUG_KEY_ALIAS: &str = "shinkiemu debug";
const PROFILE_KEY_ALIAS: &str = "openharmony application profile debug";
const APP_CA_KEY_ALIAS: &str = "openharmony application ca";
const APP_CA: &str = "C=CN,O=OpenHarmony,OU=OpenHarmony Team,CN=OpenHarmony Application CA";
const DEBUG_SUBJECT: &str = "C=CN,O=Project-Shinkirou,OU=ShinkiEMU,CN=ShinkiEMU OHOS Debug";

/// DevEco Studio tools, located from the native SDK
/// (`<DevEco>/sdk/default/openharmony/native`).
pub(crate) struct Tools {
    sdk_home: PathBuf,
    node: PathBuf,
    hvigor: PathBuf,
    ohpm: PathBuf,
    java: PathBuf,
    sign_tool: PathBuf,
    test_pki: PathBuf,
    readelf: PathBuf,
    notices: [(PathBuf, &'static str); 2],
}

pub(crate) fn tools() -> Result<Tools> {
    let native = dependencies::ohos_sdk_native()?;
    let openharmony = native.parent().context("OHOS SDK layout")?;
    let sdk_home = openharmony
        .parent()
        .and_then(Path::parent)
        .context("OHOS SDK layout: expected <DevEco>/sdk/default/openharmony/native")?
        .to_owned();
    let deveco = sdk_home
        .parent()
        .context("DevEco Studio directory")?
        .to_owned();
    let exe = if cfg!(windows) { ".exe" } else { "" };
    let node = if cfg!(windows) {
        deveco.join("tools/node/node.exe")
    } else {
        deveco.join("tools/node/bin/node")
    };
    let jbr = deveco.join("jbr/bin").join(format!("java{exe}"));
    let java = if jbr.is_file() {
        jbr
    } else {
        PathBuf::from(std::env::var_os("JAVA_HOME").context(
            "hap-sign-tool needs Java: DevEco Studio's jbr is missing and JAVA_HOME is unset",
        )?)
        .join("bin")
        .join(format!("java{exe}"))
    };
    let lib = openharmony.join("toolchains/lib");
    let tools = Tools {
        sdk_home,
        node,
        hvigor: deveco.join("tools/hvigor/bin/hvigorw.js"),
        ohpm: deveco.join("tools/ohpm/bin/pm-cli.js"),
        java,
        sign_tool: lib.join("hap-sign-tool.jar"),
        test_pki: lib,
        readelf: native.join("llvm/bin").join(format!("llvm-readelf{exe}")),
        notices: [
            (native.join("NOTICE.txt"), "licenses/ohos-sdk/NOTICE.txt"),
            (native.join("llvm/NOTICE"), "licenses/ohos-sdk/llvm-NOTICE"),
        ],
    };
    for path in [
        &tools.node,
        &tools.hvigor,
        &tools.ohpm,
        &tools.java,
        &tools.sign_tool,
        &tools.readelf,
        &tools.test_pki.join("OpenHarmony.p12"),
        &tools.test_pki.join("OpenHarmonyProfileDebug.pem"),
        &tools.test_pki.join("UnsgnedDebugProfileTemplate.json"),
    ]
    .into_iter()
    .chain(tools.notices.iter().map(|(path, _)| path))
    {
        if !path.is_file() {
            bail!(
                "DevEco Studio/OpenHarmony SDK file missing: {}",
                path.display()
            );
        }
    }
    Ok(tools)
}

/// Build, sign and verify the debug-test HAP from a release Manager receipt.
pub(crate) fn package(
    inv: &Inventory,
    result: &BuildResult,
    version: &str,
    out: &Path,
) -> Result<PathBuf> {
    if result.target != "aarch64-unknown-linux-ohos" {
        bail!("unsupported OHOS Manager target {}", result.target);
    }
    let tools = tools()?;
    let signing = signing_material(&tools)?;
    let project = inv.root.join("Platforms/OHOS");
    atomic_directory(out, |stage| {
        stage_inputs(inv, result, &tools, &project)?;
        let unsigned = build_hap(&tools, &project, version)?;
        let hap = stage.join(format!(
            "ShinkiEMU-{version}-ohos-{ABI}-debug-test-signed.hap"
        ));
        dependencies::run(
            sign_tool(&tools)
                .args([
                    "sign-app",
                    "-mode",
                    "localSign",
                    "-keyAlias",
                    DEBUG_KEY_ALIAS,
                ])
                .args(["-keyPwd", TEST_KEYSTORE_PASSWORD, "-appCertFile"])
                .arg(signing.join("app-cert.cer"))
                .arg("-profileFile")
                .arg(signing.join("profile.p7b"))
                .arg("-inFile")
                .arg(&unsigned)
                .args(["-signAlg", "SHA256withECDSA", "-keystoreFile"])
                .arg(signing.join("debug.p12"))
                .args(["-keystorePwd", TEST_KEYSTORE_PASSWORD, "-signCode", "1"])
                .arg("-outFile")
                .arg(&hap),
        )?;
        verify_hap(&tools, &hap, &result.target)?;
        fs::write(
            stage.join("ohos.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "schema": "shinki.emu.ohos_package.v1",
                "target": result.target,
                "abi": ABI,
                "bundle_name": BUNDLE_NAME,
                "signing": "openharmony-sdk-test-pki-debug",
                "hap": hap.file_name().unwrap().to_string_lossy(),
                "sha256": sha256(&hap)?,
                "size": fs::metadata(&hap)?.len()
            }))?,
        )?;
        package::write_provenance(result, &stage.join("build.json"))?;
        Ok(())
    })?;
    Ok(out.to_owned())
}

/// Replace the generated `entry/libs` and `rawfile/bundled` trees (both
/// ignored by Git) with this build's runtime closure and bundled files.
fn stage_inputs(
    inv: &Inventory,
    result: &BuildResult,
    tools: &Tools,
    project: &Path,
) -> Result<()> {
    let libs = project.join("entry/libs");
    let bundled = project.join("entry/src/main/resources/rawfile/bundled");
    for generated in [&libs, &bundled] {
        match fs::remove_dir_all(generated) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    binary::inspect(&result.library, &result.target, false)?;
    let mut native = vec![(result.library.clone(), MANAGER.to_owned())];
    for dependency in binary::closure(
        &result.library,
        &package::search_dirs(inv, result)?,
        &result.target,
    )? {
        native.push(dependency);
    }
    fs::create_dir_all(libs.join(ABI))?;
    copy_entries(&native, &libs.join(ABI))?;

    let temp = crate::temporary::directory()?;
    let mut entries = vec![(
        inv.root.join("THIRD_PARTY_NOTICES.md"),
        "THIRD_PARTY_NOTICES.md".to_owned(),
    )];
    entries.extend(
        tools
            .notices
            .iter()
            .map(|(path, name)| (path.clone(), (*name).to_owned())),
    );
    entries.extend(package::metadata_entries(inv, temp.path())?);
    entries.extend(package::licenses(inv, result, temp.path())?);
    entries.extend(package::static_core_entries(inv, result, temp.path())?);
    fs::create_dir_all(&bundled)?;
    copy_entries(&entries, &bundled)?;
    let mut names = entries
        .into_iter()
        .map(|(_, name)| name)
        .collect::<Vec<_>>();
    names.sort();
    // Read by ManagerBridge.ets `extractBundled`.
    fs::write(bundled.join("bundled.manifest"), names.join("\n") + "\n")?;
    Ok(())
}

fn build_hap(tools: &Tools, project: &Path, version: &str) -> Result<PathBuf> {
    let output = project.join("entry/build/default/outputs/default");
    match fs::remove_dir_all(&output) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let hvigor_home = project
        .parent()
        .and_then(Path::parent)
        .context("repository root")?
        .join(".work/ohos/hvigor-home");
    fs::create_dir_all(&hvigor_home)?;
    let node = |script: &Path| {
        let mut command = Command::new(&tools.node);
        command
            .arg(script)
            .current_dir(project)
            .env("DEVECO_SDK_HOME", &tools.sdk_home)
            .env("HVIGOR_USER_HOME", &hvigor_home)
            .env("SHINKI_EMU_OHOS_VERSION_NAME", version);
        command
    };
    dependencies::run(node(&tools.ohpm).args(["install", "--all"]))?;
    dependencies::run(
        node(&tools.hvigor)
            .args([
                "--mode",
                "module",
                "-p",
                "product=default",
                "-p",
                "buildMode=release",
            ])
            .args(["assembleHap", "--no-daemon"]),
    )?;
    // Only the top level: hvigor links symbol/ to its native cache.
    let mut haps = Vec::new();
    for entry in fs::read_dir(&output)? {
        let path = entry?.path();
        if path.is_file()
            && path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().ends_with("-unsigned.hap"))
        {
            package::plain(&path)?;
            haps.push(path);
        }
    }
    if haps.len() != 1 {
        bail!(
            "hvigor must produce exactly one unsigned HAP in {}",
            output.display()
        );
    }
    Ok(haps.remove(0))
}

/// Debug signing material derived from the OpenHarmony SDK test PKI, kept in
/// `SHINKI_EMU_OHOS_SIGNING_DIR` outside the checkout. Created once: a copy of
/// the SDK test keystore with a new ECC key, an app certificate issued by the
/// SDK's test application CA, and a debug provision profile for this bundle
/// signed by the SDK's profile-debug key.
fn signing_material(tools: &Tools) -> Result<PathBuf> {
    let directory = PathBuf::from(std::env::var_os("SHINKI_EMU_OHOS_SIGNING_DIR").context(
        "set SHINKI_EMU_OHOS_SIGNING_DIR to a directory outside the checkout for OHOS debug signing material",
    )?);
    let files = ["debug.p12", "app-cert.cer", "profile.p7b"];
    if files.iter().all(|name| directory.join(name).is_file()) {
        for name in files {
            package::plain(&directory.join(name))?;
        }
        return Ok(directory);
    }
    if files.iter().any(|name| directory.join(name).exists()) {
        bail!(
            "incomplete OHOS signing material in {}; remove it to regenerate",
            directory.display()
        );
    }
    fs::create_dir_all(&directory)?;
    let temp = tempfile::tempdir_in(&directory)?;
    let work = temp.path();
    fs::copy(
        tools.test_pki.join("OpenHarmony.p12"),
        work.join("debug.p12"),
    )?;
    // hap-sign-tool accepts CA certificates only from .cer files: split the
    // SDK chain (root CA, application CA, profile-debug certificate).
    let chain = fs::read_to_string(tools.test_pki.join("OpenHarmonyProfileDebug.pem"))?;
    let certificates = pem_certificates(&chain);
    if certificates.len() != 3 {
        bail!(
            "OpenHarmonyProfileDebug.pem must hold root CA, application CA and profile certificate"
        );
    }
    fs::write(work.join("root-ca.cer"), certificates[0])?;
    fs::write(work.join("sub-ca.cer"), certificates[1])?;
    let keystore = |command: &mut Command| {
        command
            .arg("-keystoreFile")
            .arg(work.join("debug.p12"))
            .args(["-keystorePwd", TEST_KEYSTORE_PASSWORD])
            .current_dir(work);
    };
    let mut command = sign_tool(tools);
    command.args(["generate-keypair", "-keyAlias", DEBUG_KEY_ALIAS, "-keyPwd"]);
    command.args([
        TEST_KEYSTORE_PASSWORD,
        "-keyAlg",
        "ECC",
        "-keySize",
        "NIST-P-256",
    ]);
    keystore(&mut command);
    dependencies::run(&mut command)?;
    let mut command = sign_tool(tools);
    command
        .args(["generate-app-cert", "-keyAlias", DEBUG_KEY_ALIAS])
        .args([
            "-keyPwd",
            TEST_KEYSTORE_PASSWORD,
            "-signAlg",
            "SHA256withECDSA",
        ])
        .args(["-issuer", APP_CA, "-issuerKeyAlias", APP_CA_KEY_ALIAS])
        .args([
            "-issuerKeyPwd",
            TEST_KEYSTORE_PASSWORD,
            "-subject",
            DEBUG_SUBJECT,
        ])
        .args(["-validity", "3650", "-rootCaCertFile", "root-ca.cer"])
        .args(["-subCaCertFile", "sub-ca.cer", "-outForm", "certChain"])
        .args(["-outFile", "app-cert.cer"]);
    keystore(&mut command);
    dependencies::run(&mut command)?;
    let app_chain = fs::read_to_string(work.join("app-cert.cer"))?;
    let leaf = pem_certificates(&app_chain)
        .first()
        .copied()
        .context("generated app certificate chain is empty")?;
    let mut profile: serde_json::Value = serde_json::from_slice(&fs::read(
        tools.test_pki.join("UnsgnedDebugProfileTemplate.json"),
    )?)?;
    let now = chrono::Utc::now().timestamp();
    profile["uuid"] = profile_uuid(leaf, now).into();
    profile["validity"] = serde_json::json!({
        "not-before": now,
        "not-after": now + 3650 * 86_400,
    });
    let bundle = profile["bundle-info"]
        .as_object_mut()
        .context("profile template bundle-info")?;
    bundle.insert("developer-id".into(), "Project-Shinkirou".into());
    bundle.insert("bundle-name".into(), BUNDLE_NAME.into());
    bundle.insert("development-certificate".into(), format!("{leaf}\n").into());
    // The template's device list names two OpenHarmony test devices; an
    // unrestricted debug profile is valid on any device that trusts the PKI.
    profile
        .as_object_mut()
        .context("profile template")?
        .remove("debug-info");
    fs::write(
        work.join("profile.json"),
        serde_json::to_vec_pretty(&profile)?,
    )?;
    let mut command = sign_tool(tools);
    command
        .args([
            "sign-profile",
            "-mode",
            "localSign",
            "-keyAlias",
            PROFILE_KEY_ALIAS,
        ])
        .args(["-keyPwd", TEST_KEYSTORE_PASSWORD, "-profileCertFile"])
        .arg(tools.test_pki.join("OpenHarmonyProfileDebug.pem"))
        .args(["-inFile", "profile.json", "-signAlg", "SHA256withECDSA"])
        .args(["-outFile", "profile.p7b"]);
    keystore(&mut command);
    dependencies::run(&mut command)?;
    dependencies::run(
        sign_tool(tools)
            .args(["verify-profile", "-inFile", "profile.p7b"])
            .current_dir(work),
    )?;
    for name in files {
        fs::rename(work.join(name), directory.join(name))?;
    }
    Ok(directory)
}

fn pem_certificates(text: &str) -> Vec<&str> {
    const END: &str = "-----END CERTIFICATE-----";
    let mut certificates = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("-----BEGIN CERTIFICATE-----") {
        let Some(end) = rest[start..].find(END) else {
            break;
        };
        certificates.push(&rest[start..start + end + END.len()]);
        rest = &rest[start + end + END.len()..];
    }
    certificates
}

/// RFC 4122 version-4 layout over a digest of the certificate and creation time.
fn profile_uuid(leaf: &str, now: i64) -> String {
    use sha2::{Digest, Sha256};
    let mut bytes: [u8; 16] = Sha256::digest(format!("{leaf}{now}{}", std::process::id()))[..16]
        .try_into()
        .expect("SHA-256 digest is longer than a UUID");
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex = hex::encode(bytes);
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

fn sign_tool(tools: &Tools) -> Command {
    let mut command = Command::new(&tools.java);
    command.arg("-jar").arg(&tools.sign_tool);
    command
}

/// `verify-app`, archive layout, and the dynamic section of every bundled
/// native library (all remaining NEEDED entries must be OHOS system libraries).
fn verify_hap(tools: &Tools, hap: &Path, target: &str) -> Result<()> {
    let evidence = crate::temporary::directory()?;
    dependencies::run(
        sign_tool(tools)
            .args(["verify-app", "-inFile"])
            .arg(hap)
            .arg("-outCertChain")
            .arg(evidence.path().join("chain.cer"))
            .arg("-outProfile")
            .arg(evidence.path().join("profile.p7b")),
    )?;
    let extracted = crate::temporary::directory()?;
    let mut archive = zip::ZipArchive::new(fs::File::open(hap)?)?;
    let mut names = Vec::new();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let name = entry.name()?.into_owned();
        println!("HAP {name} ({} bytes)", entry.size());
        if name.starts_with("libs/") && name.ends_with(".so") {
            package::portable(&name)?;
            let destination = extracted.path().join(&name);
            fs::create_dir_all(destination.parent().unwrap())?;
            io::copy(
                &mut entry,
                &mut fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(destination)?,
            )?;
        }
        names.push(name);
    }
    for required in [
        format!("libs/{ABI}/{MANAGER}"),
        "resources/rawfile/bundled/bundled.manifest".to_owned(),
        "resources/rawfile/bundled/metadata/index.json".to_owned(),
        "module.json".to_owned(),
    ] {
        if !names.contains(&required) {
            bail!("signed HAP lacks {required}");
        }
    }
    let dir = extracted.path().join("libs").join(ABI);
    for (library, name) in tree(&dir)? {
        let info = binary::inspect(&library, target, false)?;
        println!("HAP libs/{ABI}/{name} NEEDED: {:?}", info.dependencies);
        dependencies::run(Command::new(&tools.readelf).arg("-d").arg(&library))?;
        binary::closure(&library, std::slice::from_ref(&dir), target)?;
    }
    Ok(())
}
