//! `matrix --json --since <ref>`: the job-matrix rows whose inputs changed since a Git ref.
use crate::{cores::Inventory, dependencies};
use anyhow::{Context, Result, bail};
use std::{
    collections::{BTreeMap, BTreeSet},
    process::Command,
};

/// Inputs of every row.
const ALL_FILES: &[&str] = &[
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain.toml",
    "build/targets.toml",
];
const ALL_DIRS: &[&str] = &[
    "xtask/",
    "Source/SDK/",
    "Source/FamilyApi/",
    "Source/Tools/",
    "build/vcpkg/",
    "build/ci/",
];
const MANAGER_DIRS: &[&str] = &["Source/Manager/", "Source/Providers/"];

#[derive(Debug, Default)]
pub struct Affected {
    all: bool,
    manager: bool,
    /// Manager targets that statically embed a changed core.
    manager_targets: BTreeSet<String>,
    cores: BTreeSet<String>,
}

impl Affected {
    pub fn row(&self, component: &str, target: &str) -> bool {
        self.all
            || if component == "manager" {
                self.manager || self.manager_targets.contains(target)
            } else {
                self.cores.contains(component)
            }
    }
    fn core<'a>(&mut self, id: &str, static_targets: impl IntoIterator<Item = &'a str>) {
        self.cores.insert(id.to_owned());
        self.manager_targets
            .extend(static_targets.into_iter().map(str::to_owned));
    }
}

/// Compare `HEAD` with `reference`, including the paths on both sides of a rename.
pub fn since(inv: &Inventory, reference: &str) -> Result<Affected> {
    if reference.starts_with('-') {
        bail!("--since takes a Git ref, not an option: {reference}");
    }
    let git = |args: &[&str]| {
        dependencies::capture(Command::new("git").arg("-C").arg(&inv.root).args(args))
    };
    let base = String::from_utf8(git(&[
        "rev-parse",
        "--verify",
        "--end-of-options",
        &format!("{reference}^{{commit}}"),
    ])?)?
    .trim()
    .to_owned();
    let names = String::from_utf8(git(&[
        "diff",
        "--name-only",
        "--no-renames",
        "-z",
        &base,
        "HEAD",
        "--",
    ])?)?;
    let paths: Vec<&str> = names.split('\0').filter(|p| !p.is_empty()).collect();
    let (old_cores, new_cores) = if paths.contains(&"build/cores.toml") {
        (
            git(&["show", &format!("{base}:build/cores.toml")])
                .ok()
                .and_then(|bytes| String::from_utf8(bytes).ok()),
            String::from_utf8(git(&["show", "HEAD:build/cores.toml"])?)?,
        )
    } else {
        (None, String::new())
    };
    classify(inv, &paths, old_cores.as_deref(), &new_cores)
}

/// `old_cores`/`new_cores` are `build/cores.toml` at the ref and at `HEAD`; they are
/// read only when that file changed. A missing or unreadable old inventory affects all rows.
fn classify(
    inv: &Inventory,
    paths: &[&str],
    old_cores: Option<&str>,
    new_cores: &str,
) -> Result<Affected> {
    let mut affected = Affected::default();
    for &path in paths {
        if path.ends_with(".md") || path.starts_with("docs/") || path.starts_with("Metadata/") {
            continue;
        }
        if ALL_FILES.contains(&path) || ALL_DIRS.iter().any(|dir| path.starts_with(dir)) {
            affected.all = true;
        } else if MANAGER_DIRS.iter().any(|dir| path.starts_with(dir)) {
            affected.manager = true;
        } else if path == "build/cores.toml" {
            core_table_changes(&mut affected, old_cores, new_cores)?;
        } else if let Some(core) = family_core(inv, path) {
            // A statically embedded core is also an input of those Manager builds.
            affected.core(
                &core.id,
                core.static_manager_targets.iter().map(String::as_str),
            );
        } else {
            affected.all = true;
        }
        if affected.all {
            break;
        }
    }
    Ok(affected)
}

/// The core whose manifest lives in the `Source/Families/<dir>/` that holds `path`.
fn family_core<'a>(inv: &'a Inventory, path: &str) -> Option<&'a crate::cores::Core> {
    let (dir, _) = path.strip_prefix("Source/Families/")?.split_once('/')?;
    let dir = std::path::Path::new("Source/Families").join(dir);
    inv.cores
        .iter()
        .find(|core| core.manifest.starts_with(&dir))
}

/// A per-core table that differs affects that core and the Manager targets that embed
/// it (before or after the change); any other inventory change affects every row.
fn core_table_changes(affected: &mut Affected, old: Option<&str>, new: &str) -> Result<()> {
    let Some(Ok(mut old)) = old.map(toml::from_str::<toml::Table>) else {
        affected.all = true;
        return Ok(());
    };
    let mut new: toml::Table = toml::from_str(new).context("build/cores.toml at HEAD")?;
    let (Ok(old_cores), Ok(new_cores)) = (by_id(&mut old), by_id(&mut new)) else {
        affected.all = true;
        return Ok(());
    };
    if old != new {
        affected.all = true;
        return Ok(());
    }
    for id in old_cores.keys().chain(new_cores.keys()) {
        let (before, after) = (old_cores.get(id), new_cores.get(id));
        if before == after {
            continue;
        }
        let static_targets = [before, after]
            .into_iter()
            .flatten()
            .filter_map(|core| core.get("static_manager_targets")?.as_array())
            .flatten()
            .filter_map(toml::Value::as_str);
        affected.core(id, static_targets);
    }
    Ok(())
}

/// Remove the `[[cores]]` array from `table`, keyed by core id.
fn by_id(table: &mut toml::Table) -> Result<BTreeMap<String, toml::Value>> {
    let Some(toml::Value::Array(cores)) = table.remove("cores") else {
        bail!("build/cores.toml has no [[cores]] array");
    };
    cores
        .into_iter()
        .map(|core| {
            let id = core
                .get("id")
                .and_then(toml::Value::as_str)
                .context("core without id")?
                .to_owned();
            Ok((id, core))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn inventory() -> (Inventory, String) {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let cores = std::fs::read_to_string(root.join("build/cores.toml")).unwrap();
        (Inventory::load(root).unwrap(), cores)
    }
    fn rows(inv: &Inventory, affected: &Affected) -> BTreeSet<(String, String)> {
        inv.job_matrix(false, Some(affected))
            .unwrap()
            .include
            .into_iter()
            .map(|row| (row.component, row.target))
            .collect()
    }
    fn every_row(inv: &Inventory) -> BTreeSet<(String, String)> {
        rows(
            inv,
            &Affected {
                all: true,
                ..Affected::default()
            },
        )
    }
    fn changed(inv: &Inventory, paths: &[&str]) -> BTreeSet<(String, String)> {
        rows(inv, &classify(inv, paths, None, "").unwrap())
    }
    /// Rows of `core`, plus the Manager rows on its static integration targets.
    fn core_rows(inv: &Inventory, core: &str) -> BTreeSet<(String, String)> {
        let embedded = &inv.core(core).unwrap().static_manager_targets;
        every_row(inv)
            .into_iter()
            .filter(|(component, target)| {
                component == core || component == "manager" && embedded.contains(target)
            })
            .collect()
    }

    #[test]
    fn documentation_and_catalog_paths_affect_no_rows() {
        let (inv, _) = inventory();
        assert!(
            changed(
                &inv,
                &[
                    "docs/en/build-and-release.md",
                    "docs/assets/diagram.svg",
                    "Metadata/engines/cores/rfvp/windows-x64/0.1.0.json",
                    "xtask/README.md",
                    "Source/Manager/shinki-emu-manager/NOTES.md",
                ],
            )
            .is_empty()
        );
    }

    #[test]
    fn shared_build_inputs_and_unknown_paths_affect_every_row() {
        let (inv, _) = inventory();
        let all = every_row(&inv);
        for path in [
            "Cargo.toml",
            "Cargo.lock",
            "rust-toolchain.toml",
            "build/targets.toml",
            "xtask/src/release.rs",
            "Source/SDK/shinki-emu-sdk/src/lib.rs",
            "Source/FamilyApi/src/lib.rs",
            "Source/Tools/shinki-emu-build/src/lib.rs",
            "build/vcpkg/subtitle/vcpkg.json",
            "build/ci/linux-manager-smoke.sh",
            "build/dependencies.toml",
            "Platforms/Android/app/build.gradle.kts",
            ".github/workflows/ci.yml",
            // A family directory without its own core manifest is shared code.
            "Source/Families/shinki-emu-musica-keys/src/lib.rs",
        ] {
            assert_eq!(changed(&inv, &[path]), all, "{path}");
        }
    }

    #[test]
    fn manager_sources_affect_only_manager_rows() {
        let (inv, _) = inventory();
        let manager: BTreeSet<_> = every_row(&inv)
            .into_iter()
            .filter(|(component, _)| component == "manager")
            .collect();
        assert!(!manager.is_empty());
        for path in [
            "Source/Manager/shinki-emu-manager/src/main.rs",
            "Source/Providers/shinki-emu-metadata/src/lib.rs",
        ] {
            assert_eq!(changed(&inv, &[path]), manager, "{path}");
        }
    }

    #[test]
    fn family_sources_affect_their_core_and_its_static_manager_rows() {
        let (inv, _) = inventory();
        let musica = changed(&inv, &["Source/Families/shinki-emu-musica/src/lib.rs"]);
        assert_eq!(musica, core_rows(&inv, "shinki.emu.musica"));
        assert!(musica.iter().any(|(component, _)| component == "manager"));
        assert!(!musica.contains(&("manager".into(), "x86_64-pc-windows-msvc".into())));
        let cmvs = changed(&inv, &["Source/Families/shinki-emu-cmvs/Cargo.toml"]);
        assert_eq!(cmvs, core_rows(&inv, "shinki.emu.cmvs"));
        assert!(cmvs.contains(&("manager".into(), "aarch64-linux-android".into())));
        assert!(!cmvs.contains(&("manager".into(), "x86_64-pc-windows-msvc".into())));
    }

    #[test]
    fn inventory_changes_affect_only_the_edited_core_tables() {
        let (inv, current) = inventory();
        let table: toml::Table = toml::from_str(&current).unwrap();
        let edit = |change: &dyn Fn(&mut toml::Table)| {
            let mut old = table.clone();
            change(&mut old);
            let old = toml::to_string(&old).unwrap();
            rows(
                &inv,
                &classify(&inv, &["build/cores.toml"], Some(&old), &current).unwrap(),
            )
        };
        let rfvp = edit(&|old| {
            let cores = old["cores"].as_array_mut().unwrap();
            let rfvp = cores
                .iter_mut()
                .find(|core| core["id"].as_str() == Some("rfvp"));
            rfvp.unwrap()["display_name"] = "RFVP (previous)".into();
        });
        assert_eq!(rfvp, core_rows(&inv, "rfvp"));
        assert!(!rfvp.iter().any(|(component, _)| component == "siglus_rs"));

        assert!(edit(&|_| {}).is_empty(), "formatting-only edits");
        assert_eq!(
            edit(&|old| {
                old["schema"] = "shinki.emu.build_cores.v0".into();
            }),
            every_row(&inv)
        );
        assert_eq!(
            rows(
                &inv,
                &classify(&inv, &["build/cores.toml"], None, &current).unwrap()
            ),
            every_row(&inv),
            "new inventory file"
        );
    }
}
