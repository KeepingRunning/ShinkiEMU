use crate::affected::Affected;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Deserialize)]
pub struct Core {
    pub id: String,
    pub display_name: String,
    pub family_name: String,
    pub license: String,
    pub manifest: PathBuf,
    pub package: String,
    pub features: Vec<String>,
    pub dependencies: Vec<String>,
    pub artifact_kind: String,
    pub resources: Vec<String>,
    pub licenses: Vec<String>,
    pub supported_targets: Vec<String>,
    #[serde(default)]
    pub static_manager_targets: Vec<String>,
    #[serde(default)]
    pub static_features: Vec<String>,
    pub blocked: Vec<Blocked>,
    pub publish_blocked: String,
    pub source: Option<CoreSource>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CoreSource {
    pub repo: String,
    pub branch: String,
    pub rev: String,
    /// Original upstream project this organization fork tracks.
    pub upstream: String,
    pub upstream_identity: shinki_emu_metadata::UpstreamIdentity,
    pub release_sequence: u64,
}

impl Core {
    pub fn source_directory(&self, repository: &Path) -> Result<PathBuf> {
        if self.source.is_none() {
            return Ok(repository.to_owned());
        }
        let relative = self
            .manifest
            .strip_prefix("ThirdParty")
            .context("external core manifests must be under ThirdParty")?;
        let directory = relative
            .components()
            .next()
            .context("external core directory missing")?;
        Ok(repository.join("ThirdParty").join(directory.as_os_str()))
    }
}

impl CoreSource {
    pub fn identity(&self) -> shinki_emu_metadata::CoreIdentity {
        shinki_emu_metadata::CoreIdentity {
            upstream: self.upstream_identity.clone(),
            adapter: shinki_emu_metadata::AdapterIdentity {
                repo: self.repo.clone(), branch: self.branch.clone(), commit: self.rev.clone(),
            },
            release_sequence: self.release_sequence,
        }
    }
    fn validate(&self) -> Result<()> {
        self.identity().validate()?;
        if self.upstream != self.upstream_identity.repo {
            bail!("upstream identity must name the original upstream repository");
        }
        let repository = self
            .repo
            .strip_prefix("https://github.com/Project-Shinkirou/")
            .context("external core source must be an organization GitHub repository")?;
        crate::package::portable(repository)?;
        if repository.contains('/') {
            bail!("core source must name one GitHub repository");
        }
        crate::package::portable(&self.branch)?;
        if self.branch.contains("..") || self.branch.split('/').any(|part| part.ends_with(".lock"))
        {
            bail!("invalid source branch");
        }
        if self.rev.len() != 40
            || !self
                .rev
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            bail!("core source revision must be an exact lowercase 40-hex commit");
        }
        let upstream = self
            .upstream
            .strip_prefix("https://github.com/")
            .context("core upstream must be a GitHub repository URL")?;
        let mut parts = upstream.split('/');
        let (Some(owner), Some(name), None) = (parts.next(), parts.next(), parts.next()) else {
            bail!("core upstream must name one GitHub owner/repository");
        };
        crate::package::portable(owner)?;
        crate::package::portable(name)?;
        if owner.eq_ignore_ascii_case("Project-Shinkirou") {
            bail!("core upstream must be the original project, not the organization fork");
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Deserialize)]
pub struct Blocked {
    pub target: String,
    pub reason: String,
}
#[derive(Clone, Debug, Deserialize)]
pub struct Target {
    pub triple: String,
    pub runner: String,
    pub platform: String,
    pub architecture: String,
    pub variant: String,
    pub tools: Vec<String>,
    pub manager_supported: bool,
    pub manager_blocked: String,
    /// Buildable and publishable on a host with the native SDK, but never
    /// scheduled on a GitHub runner. Publication requires an exact-head
    /// local artifact and successful remote CI.
    #[serde(default)]
    pub manager_local_build: bool,
    /// Non-empty: every component on this target is blocked with this reason,
    /// overriding core `supported_targets` and `manager_supported`.
    #[serde(default)]
    pub blocked: String,
}
#[derive(Deserialize)]
struct CoreFile {
    schema: String,
    cores: Vec<Core>,
}
#[derive(Deserialize)]
struct TargetFile {
    schema: String,
    targets: Vec<Target>,
}
#[derive(Clone, Debug)]
pub struct Inventory {
    pub root: PathBuf,
    pub cores: Vec<Core>,
    pub targets: Vec<Target>,
}
#[derive(Serialize)]
pub struct Matrix {
    pub include: Vec<MatrixRow>,
}
#[derive(Serialize)]
pub struct MatrixRow {
    pub target: String,
    pub component: String,
    pub external: bool,
    pub runner: String,
    pub platform: String,
    pub architecture: String,
    pub variant: String,
    /// Declared reason this combination is not built; never part of a job matrix.
    #[serde(skip)]
    pub blocked: Option<String>,
    /// Distribution restriction of the core; the release workflow refuses these components.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub publish_blocked: String,
}

pub fn slug(id: &str) -> String {
    id.to_ascii_lowercase().replace(['.', '_'], "-")
}
impl Inventory {
    pub fn load(root: &Path) -> Result<Self> {
        let c: CoreFile = toml::from_str(&std::fs::read_to_string(root.join("build/cores.toml"))?)?;
        let t: TargetFile =
            toml::from_str(&std::fs::read_to_string(root.join("build/targets.toml"))?)?;
        if c.schema != "shinki.emu.build_cores.v1" || t.schema != "shinki.emu.build_targets.v1" {
            bail!("unsupported build inventory schema");
        }
        let result = Self {
            root: root.to_owned(),
            cores: c.cores,
            targets: t.targets,
        };
        result.validate()?;
        Ok(result)
    }
    fn validate(&self) -> Result<()> {
        if self.cores.is_empty() || self.targets.is_empty() {
            bail!("empty build matrix");
        }
        let mut ids = BTreeSet::new();
        let mut slugs = BTreeSet::new();
        let mut triples = BTreeSet::new();
        for t in &self.targets {
            if !triples.insert(&t.triple) {
                bail!("duplicate target {}", t.triple);
            }
            if !t.manager_supported && t.manager_blocked.is_empty() {
                bail!("target needs a Manager blocked reason");
            }
            if t.manager_local_build && (t.runner != "local-only" || !t.manager_supported) {
                bail!(
                    "{}: locally built Manager must be supported and use a local-only runner",
                    t.triple
                );
            }
        }
        for c in &self.cores {
            if c.id == "manager" || !ids.insert(&c.id) || !slugs.insert(slug(&c.id)) {
                bail!("duplicate core id or release slug: {}", c.id);
            }
            crate::package::portable(&c.id)?;
            crate::package::portable(&c.manifest.to_string_lossy().replace('\\', "/"))?;
            if c.manifest.starts_with("ThirdParty") != c.source.is_some() {
                bail!(
                    "external core {} needs a pinned source declaration; in-repo cores must not have one",
                    c.id
                );
            }
            if let Some(source) = &c.source {
                if slug(&c.id).len() > 32 || c.id.contains('/') {
                    bail!("external plugin slug must be one segment of at most 32 bytes");
                }
                source.validate()?;
                c.source_directory(&self.root)?;
            }
            let mut seen = BTreeSet::new();
            for t in &c.supported_targets {
                self.target(t)?;
                if !seen.insert(t) {
                    bail!("duplicate supported target");
                }
            }
            for b in &c.blocked {
                self.target(&b.target)?;
                if b.reason.is_empty() || !seen.insert(&b.target) {
                    bail!("invalid blocked target for {}", c.id);
                }
            }
            if seen != triples {
                bail!("core {} does not describe every target", c.id);
            }
            for t in &c.static_manager_targets {
                let target = self.target(t)?;
                if !(target.manager_supported || target.manager_local_build)
                    || !matches!(target.platform.as_str(), "android" | "ios" | "ohos")
                {
                    bail!("invalid static Manager integration");
                }
            }
        }
        self.validate_static_dependencies()?;
        Ok(())
    }
    fn validate_static_dependencies(&self) -> Result<()> {
        let manifest: toml::Value = toml::from_str(&std::fs::read_to_string(
            self.root
                .join("Source/Manager/shinki-emu-manager/Cargo.toml"),
        )?)?;
        for (platform, key) in [
            ("android", "cfg(target_os = \"android\")"),
            ("ios", "cfg(target_os = \"ios\")"),
            ("ohos", "cfg(target_env = \"ohos\")"),
        ] {
            let dependencies = manifest["target"][key]["dependencies"]
                .as_table()
                .context("Manager target dependencies")?;
            for core in &self.cores {
                let embedded = self.targets.iter().any(|target| {
                    target.platform == platform
                        && core.static_manager_targets.contains(&target.triple)
                });
                let dependency = dependencies.get(&core.package);
                if !embedded {
                    if dependency.is_some() {
                        bail!("Manager {platform} depends on non-static core {}", core.id);
                    }
                    continue;
                }
                let dependency = dependency.with_context(|| {
                    format!("Manager {platform} is missing static core {}", core.id)
                })?;
                let mut expected_features = core.static_features.clone();
                expected_features.sort();
                expected_features.dedup();
                let mut actual_features = dependency
                    .get("features")
                    .and_then(toml::Value::as_array)
                    .map(|features| {
                        features
                            .iter()
                            .filter_map(toml::Value::as_str)
                            .map(str::to_owned)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                actual_features.sort();
                actual_features.dedup();
                let features_match = actual_features == expected_features;
                let defaults_ok = dependency
                    .get("default-features")
                    .and_then(toml::Value::as_bool)
                    != Some(true);
                let source_match = if let Some(source) = &core.source {
                    dependency.get("git").and_then(toml::Value::as_str) == Some(&source.repo)
                        && dependency.get("rev").and_then(toml::Value::as_str) == Some(&source.rev)
                } else if let Some(path) = dependency.get("path").and_then(toml::Value::as_str) {
                    let manager_dir = self.root.join("Source/Manager/shinki-emu-manager");
                    let package_dir = self.root.join(
                        core.manifest
                            .parent()
                            .context("in-tree core manifest has no package directory")?,
                    );
                    std::fs::canonicalize(manager_dir.join(path)).ok()
                        == std::fs::canonicalize(package_dir).ok()
                } else {
                    false
                };
                if !source_match || !features_match || !defaults_ok {
                    bail!(
                        "Manager {platform} static core {} pin/path/features differ from inventory",
                        core.id
                    );
                }
            }
        }
        Ok(())
    }
    pub fn core(&self, id: &str) -> Result<&Core> {
        self.cores
            .iter()
            .find(|c| c.id == id)
            .with_context(|| format!("unknown core {id}"))
    }
    pub fn target(&self, triple: &str) -> Result<&Target> {
        self.targets
            .iter()
            .find(|t| t.triple == triple)
            .with_context(|| format!("unknown target {triple}"))
    }
    pub fn blocked(&self, component: &str, triple: &str) -> Result<Option<&str>> {
        let t = self.target(triple)?;
        if !t.blocked.is_empty() {
            return Ok(Some(t.blocked.as_str()));
        }
        if component == "manager" {
            return Ok((!t.manager_supported).then_some(t.manager_blocked.as_str()));
        }
        let c = self.core(component)?;
        Ok(c.blocked
            .iter()
            .find(|b| b.target == triple)
            .map(|b| b.reason.as_str()))
    }
    pub fn require_supported(&self, component: &str, triple: &str) -> Result<()> {
        if let Some(reason) = self.blocked(component, triple)? {
            bail!("{component}/{triple} blocked: {reason}");
        }
        Ok(())
    }
    /// Like [`Self::require_supported`], but admits a Manager whose target
    /// allows local builds of an unsupported Manager.
    pub fn require_buildable(&self, component: &str, triple: &str) -> Result<()> {
        let t = self.target(triple)?;
        if component == "manager" && t.blocked.is_empty() && t.manager_local_build {
            return Ok(());
        }
        self.require_supported(component, triple)
    }
    /// Every Manager/core combination on every declared target, with its blocked reason.
    pub fn rows(&self) -> Result<Vec<MatrixRow>> {
        let mut rows = Vec::new();
        for t in &self.targets {
            for component in
                std::iter::once("manager").chain(self.cores.iter().map(|c| c.id.as_str()))
            {
                rows.push(MatrixRow {
                    target: t.triple.clone(),
                    component: component.into(),
                    runner: t.runner.clone(),
                    platform: t.platform.clone(),
                    architecture: t.architecture.clone(),
                    variant: t.variant.clone(),
                    external: component != "manager" && self.core(component)?.source.is_some(),
                    blocked: self.blocked(component, &t.triple)?.map(str::to_owned),
                    publish_blocked: if component == "manager" {
                        String::new()
                    } else {
                        self.core(component)?.publish_blocked.clone()
                    },
                });
            }
        }
        Ok(rows)
    }
    /// GitHub job matrix: supported combinations with hosted runners only.
    /// Local-only packages join at the exact-head publication handoff. With
    /// `ci`, Android Manager rows after the first are dropped because either
    /// row builds the same universal APK.
    pub fn job_matrix(&self, ci: bool, affected: Option<&Affected>) -> Result<Matrix> {
        let mut android_manager = false;
        let include = self
            .rows()?
            .into_iter()
            .filter(|row| {
                if row.blocked.is_some()
                    || row.runner == "local-only"
                    || affected.is_some_and(|a| !a.row(&row.component, &row.target))
                {
                    return false;
                }
                if ci && row.component == "manager" && row.platform == "android" {
                    return !std::mem::replace(&mut android_manager, true);
                }
                true
            })
            .collect();
        Ok(Matrix { include })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_manager_pins_features_and_paths_cannot_drift() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let inventory = Inventory::load(root).unwrap();

        let mut changed = inventory.clone();
        changed
            .cores
            .iter_mut()
            .find(|core| core.id == "siglus_rs")
            .unwrap()
            .static_features = vec!["unexpected".into()];
        assert!(changed.validate_static_dependencies().is_err());

        let mut changed = inventory;
        changed
            .cores
            .iter_mut()
            .find(|core| core.id == "shinki.emu.musica")
            .unwrap()
            .manifest = "Source/Families/shinki-emu-cmvs/Cargo.toml".into();
        assert!(changed.validate_static_dependencies().is_err());
    }

    #[test]
    fn target_block_overrides_supported_manager_and_cores() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let mut inventory = Inventory::load(root).unwrap();
        let triple = "x86_64-pc-windows-msvc";
        assert!(inventory.blocked("manager", triple).unwrap().is_none());
        assert!(inventory.blocked("rfvp", triple).unwrap().is_none());
        inventory
            .targets
            .iter_mut()
            .find(|target| target.triple == triple)
            .unwrap()
            .blocked = "deferred".into();
        for component in ["manager", "rfvp"] {
            assert_eq!(
                inventory.blocked(component, triple).unwrap(),
                Some("deferred")
            );
            assert!(inventory.require_supported(component, triple).is_err());
        }
        assert!(
            inventory
                .rows()
                .unwrap()
                .iter()
                .filter(|row| row.target == triple)
                .all(|row| row.blocked.as_deref() == Some("deferred"))
        );
        assert!(
            !inventory
                .job_matrix(false, None)
                .unwrap()
                .include
                .iter()
                .any(|row| row.target == triple)
        );
        assert!(
            inventory
                .blocked("manager", "aarch64-linux-android")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn local_only_manager_joins_release_but_never_runs_on_hosted_ci() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let mut inventory = Inventory::load(root).unwrap();
        let triple = "aarch64-unknown-linux-ohos";
        assert!(inventory.require_supported("manager", triple).is_ok());
        assert!(inventory.require_buildable("manager", triple).is_ok());
        assert!(inventory.require_buildable("rfvp", triple).is_err());
        assert!(inventory.rows().unwrap().iter().any(|row| {
            row.target == triple && row.component == "manager" && row.blocked.is_none()
        }));
        assert!(
            !inventory
                .job_matrix(true, None)
                .unwrap()
                .include
                .iter()
                .any(|row| row.target == triple)
        );
        let index = inventory
            .targets
            .iter()
            .position(|target| target.triple == triple)
            .unwrap();
        inventory.targets[index].blocked = "deferred".into();
        assert!(inventory.require_buildable("manager", triple).is_err());
        inventory.targets[index].blocked.clear();
        inventory.targets[index].manager_supported = false;
        assert!(inventory.validate().is_err());
    }

    #[test]
    fn ci_matrix_builds_the_universal_android_manager_once() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let inventory = Inventory::load(root).unwrap();
        let android_managers = |ci| {
            inventory
                .job_matrix(ci, None)
                .unwrap()
                .include
                .iter()
                .filter(|row| row.component == "manager" && row.platform == "android")
                .count()
        };
        assert_eq!(android_managers(false), 2);
        assert_eq!(android_managers(true), 1);
    }
}
