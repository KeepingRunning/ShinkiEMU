use crate::{MetadataError, fields::*};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const PATCH_CATALOG_SCHEMA_VERSION: &str = "shinki.emu.patches.v2";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PatchRecipe {
    pub script: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PatchDownload {
    pub url: String,
    pub sha256: String,
    pub size: u64,
}
impl PatchDownload {
    pub fn validate(&self) -> Result<(), MetadataError> {
        if self.url.len() > 2048
            || !https(&self.url)
            || self.sha256.len() != 64
            || !self
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            || self.size == 0
            || self.size > 1024 * 1024 * 1024
        {
            return Err(MetadataError::SchemaMismatch("patch.download"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PatchFileFingerprint {
    pub path: String,
    pub size: u64,
    #[schemars(regex(pattern = r"^[0-9a-f]{64}$"))]
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PatchRelease {
    #[schemars(regex(pattern = r"^r[0-9]{1,63}$"))]
    pub release_id: String,
    #[schemars(regex(pattern = r"^v[0-9]{1,63}$"))]
    pub vn_id: String,
    pub title: BTreeMap<String, String>,
    /// Actual medium/channel label, not a patch category.
    pub edition: String,
    /// The game's own version, when recorded; never the recipe/runtime version.
    pub game_version: Option<String>,
    pub fingerprints: Vec<PatchFileFingerprint>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PatchCatalogEntry {
    pub id: String,
    pub title: BTreeMap<String, String>,
    pub version: String,
    pub emulators: Vec<String>,
    pub source_url: Option<String>,
    pub license: String,
    pub authorization_url: Option<String>,
    pub summary: BTreeMap<String, String>,
    pub issue_url: Option<String>,
    pub recipe: PatchRecipe,
    pub releases: Vec<PatchRelease>,
    #[serde(default)]
    pub downloads: Vec<PatchDownload>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PatchCatalog {
    #[schemars(regex(pattern = r"^shinki\.emu\.patches\.v2$"))]
    pub schema: String,
    pub entries: Vec<PatchCatalogEntry>,
}

/// A repository-relative path: no links, drive names, ADS or traversal components.
pub fn is_safe_metadata_relative_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 256
        && !path.contains('\\')
        && !path.contains("..")
        && path.split('/').all(|p| {
            !p.is_empty()
                && p != "."
                && p != ".."
                && !p.ends_with('.')
                && !p.ends_with(' ')
                && p.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'+'))
        })
}

fn https(value: &str) -> bool {
    url::Url::parse(value).is_ok_and(|u| {
        u.scheme() == "https"
            && u.host_str().is_some()
            && u.username().is_empty()
            && u.password().is_none()
            && u.port().is_none()
    })
}
fn localized(values: &BTreeMap<String, String>, max: usize) -> bool {
    ["zh-CN", "en"].iter().all(|l| values.contains_key(*l))
        && values
            .iter()
            .all(|(l, v)| matches!(l.as_str(), "zh-CN" | "en" | "ja") && is_bounded_text(v, 1, max))
}
fn unique(values: &[String], valid: impl Fn(&str) -> bool) -> bool {
    let mut seen = BTreeSet::new();
    !values.is_empty() && values.len() <= 4096 && values.iter().all(|v| valid(v) && seen.insert(v))
}

impl PatchRelease {
    pub fn validate(&self) -> Result<(), MetadataError> {
        let invalid = || MetadataError::SchemaMismatch("patch.release");
        if !is_vndb_release_id(&self.release_id)
            || !is_vndb_vn_id(&self.vn_id)
            || !localized(&self.title, 256)
            || !is_bounded_text(&self.edition, 1, 128)
            || self
                .game_version
                .as_ref()
                .is_some_and(|version| !is_bounded_text(version, 1, 64))
            || self.fingerprints.is_empty()
            || self.fingerprints.len() > 64
        {
            return Err(invalid());
        }
        let mut paths = BTreeSet::new();
        for file in &self.fingerprints {
            // Source game paths may contain Unicode, unlike repository paths.
            // The patch executor additionally applies its cross-platform path policy.
            if file.path.is_empty()
                || file.path.len() > 1024
                || file.path.contains(['\\', ':', '\0'])
                || file
                    .path
                    .split('/')
                    .any(|p| p.is_empty() || p == "." || p == ".." || p.ends_with(['.', ' ']))
                || !is_sha256_hex(&file.sha256)
                || !paths.insert(file.path.to_lowercase())
            {
                return Err(invalid());
            }
        }
        Ok(())
    }
}
impl PatchCatalogEntry {
    pub fn validate(&self) -> Result<(), MetadataError> {
        let invalid = || MetadataError::SchemaMismatch("patch.entry");
        if !is_plugin_id(&self.id)
            || matches!(self.id.as_str(), "." | "..")
            || !localized(&self.title, 256)
            || !localized(&self.summary, 2048)
            || self.version.len() > 64
            || semver::Version::parse(&self.version).is_err()
            || !is_bounded_text(&self.license, 1, 256)
            || !unique(&self.emulators, is_known_core_id)
            || !is_safe_metadata_relative_path(&self.recipe.script)
            || !self.recipe.script.ends_with(".luau")
            || self.releases.is_empty()
            || self.releases.len() > 256
        {
            return Err(invalid());
        }
        for url in [&self.source_url, &self.authorization_url, &self.issue_url]
            .into_iter()
            .flatten()
        {
            if url.len() > 2048 || !https(url) {
                return Err(invalid());
            }
        }
        let mut releases = BTreeSet::new();
        for release in &self.releases {
            release.validate()?;
            if !releases.insert(&release.release_id) {
                return Err(invalid());
            }
        }
        if self.downloads.len() > 256 {
            return Err(invalid());
        }
        let mut downloads = BTreeSet::new();
        for download in &self.downloads {
            download.validate()?;
            if !downloads.insert(&download.url) {
                return Err(invalid());
            }
        }
        Ok(())
    }
    pub fn applies_to_work(&self, vn_id: &str) -> bool {
        self.releases.iter().any(|release| release.vn_id == vn_id)
    }
}
impl PatchCatalog {
    pub fn validate(&self) -> Result<(), MetadataError> {
        if self.schema != PATCH_CATALOG_SCHEMA_VERSION || self.entries.len() > 4096 {
            return Err(MetadataError::SchemaMismatch("patches"));
        }
        let mut ids = BTreeSet::new();
        let mut releases = BTreeMap::new();
        for entry in &self.entries {
            entry.validate()?;
            if !ids.insert(&entry.id) {
                return Err(MetadataError::Duplicate(entry.id.clone()));
            }
            for release in &entry.releases {
                if let Some(previous) = releases.insert(&release.release_id, release)
                    && previous != release
                {
                    return Err(MetadataError::SchemaMismatch("patch.release_conflict"));
                }
            }
        }
        Ok(())
    }
}
pub fn parse_patch_catalog(bytes: &[u8]) -> Result<PatchCatalog, MetadataError> {
    let catalog: PatchCatalog = serde_json::from_value(
        crate::parse_unique_json(bytes)
            .map_err(|_| MetadataError::SchemaMismatch("patches-v2.json"))?,
    )
    .map_err(|_| MetadataError::SchemaMismatch("patches-v2.json"))?;
    catalog.validate()?;
    Ok(catalog)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entry() -> PatchCatalogEntry {
        serde_json::from_value(serde_json::json!({
            "id":"public-test","title":{"zh-CN":"公开测试","en":"Public test"},"version":"1.0.0",
            "emulators":["ReMinori"],"source_url":null,"license":"MIT","authorization_url":null,
            "summary":{"zh-CN":"测试配方","en":"Test recipe"},"issue_url":null,"recipe":{"script":"public.luau"},
            "releases":[{"release_id":"r1","vn_id":"v1","title":{"zh-CN":"盒装版","en":"Boxed edition"},"edition":"boxed","game_version":null,
                "fingerprints":[{"path":"game.dat","size":8,"sha256":"a".repeat(64)}]}]
        })).unwrap()
    }
    #[test]
    fn release_identity_and_fingerprints_are_required() {
        entry().validate().unwrap();
        for mutate in [
            (|entry: &mut PatchCatalogEntry| entry.releases.clear()) as fn(&mut PatchCatalogEntry),
            |entry| entry.releases[0].fingerprints.clear(),
            |entry| entry.releases[0].release_id = "v1".into(),
            |entry| entry.releases[0].fingerprints[0].path = "../private".into(),
            |entry| entry.releases[0].fingerprints[0].sha256 = "bad".into(),
        ] {
            let mut value = entry();
            mutate(&mut value);
            assert!(value.validate().is_err());
        }
    }
    #[test]
    fn localized_text_requires_zh_cn_and_en_and_allows_only_ja_extra() {
        let mut value = entry();
        value.title.insert("ja".into(), "公開テスト".into());
        value.validate().unwrap();
        for mutate in [
            (|entry: &mut PatchCatalogEntry| {
                entry.title.remove("en");
            }) as fn(&mut PatchCatalogEntry),
            |entry| {
                entry.summary.remove("zh-CN");
            },
            |entry| {
                entry.title.insert("ko".into(), "공개 테스트".into());
            },
            |entry| {
                entry.title.insert("ja".into(), String::new());
            },
        ] {
            let mut value = entry();
            mutate(&mut value);
            assert!(value.validate().is_err());
        }
    }
    #[test]
    fn legacy_categories_are_not_accepted_by_v2() {
        let mut value = serde_json::to_value(entry()).unwrap();
        value["kind"] = "recipe".into();
        assert!(serde_json::from_value::<PatchCatalogEntry>(value).is_err());
    }
}
