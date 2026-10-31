use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::MetadataError;
use crate::fields::{MAX_ENTRIES, is_bounded_text, is_plugin_id, is_sha256_hex};

pub const CORE_CATALOG_SCHEMA_VERSION: &str = "shinki.emu.cores.v2";
pub const LEGACY_CORE_CATALOG_SCHEMA_VERSION: &str = "shinki.emu.cores.v1";
pub const CORE_RELEASE_URL_PREFIX: &str =
    "https://github.com/Project-Shinkirou/ShinkiEMU/releases/download/";
pub const MAX_CORE_PACKAGE_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoreCatalog {
    #[schemars(regex(pattern = r"^shinki\.emu\.cores\.v[12]$"))]
    pub schema: String,
    pub entries: Vec<CoreCatalogEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoreCatalogEntry {
    #[schemars(regex(pattern = r"^[A-Za-z0-9._-]{1,128}$"))]
    pub plugin_id: String,
    #[schemars(length(min = 1, max = 64))]
    pub display_name: String,
    #[schemars(length(min = 1, max = 64))]
    pub family_name: String,
    #[schemars(length(min = 1, max = 88))]
    pub version: String,
    #[schemars(length(min = 1, max = 128))]
    pub abi: String,
    #[schemars(regex(pattern = r"^[a-z0-9_]{1,32}-[a-z0-9_]{1,32}$"))]
    pub platform: String,
    #[schemars(regex(
        pattern = r"^https://github\.com/Project-Shinkirou/ShinkiEMU/releases/download/[A-Za-z0-9._+-]+/[A-Za-z0-9._+-]+$"
    ))]
    pub url: String,
    #[schemars(regex(pattern = r"^[0-9a-f]{64}$"))]
    pub sha256: String,
    #[schemars(range(min = 1, max = 536870912))]
    pub size: u64,
    #[schemars(length(min = 1, max = 128))]
    pub license: String,
    #[schemars(regex(pattern = r"^[A-Za-z0-9._+-]{1,128}$"))]
    pub release_tag: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<crate::CoreIdentity>,
}

impl CoreCatalogEntry {
    pub fn validate(&self) -> Result<(), MetadataError> {
        if !is_plugin_id(&self.plugin_id) {
            return Err(MetadataError::SchemaMismatch("core.plugin_id"));
        }
        if !is_bounded_text(&self.display_name, 1, 64) {
            return Err(MetadataError::ResponseBounds("core.display_name"));
        }
        if !is_bounded_text(&self.family_name, 1, 64) {
            return Err(MetadataError::ResponseBounds("core.family_name"));
        }
        crate::validate_core_version(&self.version, self.identity.as_ref())?;
        if self.identity.is_some() {
            let slug = self.plugin_id.to_ascii_lowercase().replace(['.', '_'], "-");
            if slug.len() > 32 || self.release_tag != format!("core-{slug}-{}", self.version) {
                return Err(MetadataError::SchemaMismatch("core.release_tag_identity"));
            }
        }
        if !is_bounded_text(&self.abi, 1, 128) {
            return Err(MetadataError::SchemaMismatch("core.abi"));
        }
        if !is_platform_triple(&self.platform) {
            return Err(MetadataError::SchemaMismatch("core.platform"));
        }
        if !is_release_tag(&self.release_tag) {
            return Err(MetadataError::SchemaMismatch("core.release_tag"));
        }
        if !is_release_asset_url(&self.url, &self.release_tag) {
            return Err(MetadataError::SchemaMismatch("core.url"));
        }
        if !is_sha256_hex(&self.sha256) {
            return Err(MetadataError::SchemaMismatch("core.sha256"));
        }
        if self.size == 0 || self.size > MAX_CORE_PACKAGE_BYTES {
            return Err(MetadataError::ResponseBounds("core.size"));
        }
        if !is_bounded_text(&self.license, 1, 128) {
            return Err(MetadataError::ResponseBounds("core.license"));
        }
        Ok(())
    }

    pub fn display_version(&self) -> String {
        self.identity.as_ref().map_or_else(
            || self.version.clone(),
            crate::CoreIdentity::display_version,
        )
    }

    pub fn release_cmp(&self, other: &Self) -> std::cmp::Ordering {
        match (&self.identity, &other.identity) {
            (Some(a), Some(b)) => a.release_sequence.cmp(&b.release_sequence),
            (Some(_), None) => std::cmp::Ordering::Greater,
            (None, Some(_)) => std::cmp::Ordering::Less,
            (None, None) => semver::Version::parse(&self.version)
                .ok()
                .cmp(&semver::Version::parse(&other.version).ok()),
        }
    }

    /// Compare installation identity, never the plugin's independent crate version.
    pub fn is_update_for(&self, version: &str, identity: Option<&crate::CoreIdentity>) -> bool {
        if self.version == version {
            return false;
        }
        match (&self.identity, identity) {
            (Some(candidate), Some(installed)) => {
                candidate.release_sequence > installed.release_sequence
            }
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (None, None) => match (
                semver::Version::parse(&self.version),
                semver::Version::parse(version),
            ) {
                (Ok(candidate), Ok(installed)) => candidate > installed,
                _ => false,
            },
        }
    }
}

impl CoreCatalog {
    pub fn validate(&self) -> Result<(), MetadataError> {
        if self.schema != CORE_CATALOG_SCHEMA_VERSION
            && self.schema != LEGACY_CORE_CATALOG_SCHEMA_VERSION
        {
            return Err(MetadataError::SchemaMismatch("cores.schema"));
        }
        if self.entries.len() > MAX_ENTRIES {
            return Err(MetadataError::ResponseBounds("cores.entries"));
        }
        let mut keys = BTreeSet::new();
        let mut sequences = std::collections::BTreeMap::new();
        for entry in &self.entries {
            entry.validate()?;
            if self.schema == LEGACY_CORE_CATALOG_SCHEMA_VERSION && entry.identity.is_some() {
                return Err(MetadataError::SchemaMismatch("cores.legacy_identity"));
            }
            if let Some(identity) = &entry.identity {
                let key = (&entry.plugin_id, identity.release_sequence);
                if sequences
                    .insert(key, &entry.version)
                    .is_some_and(|old| old != &entry.version)
                {
                    return Err(MetadataError::SchemaMismatch("cores.release_sequence"));
                }
            }
            if !keys.insert((
                entry.plugin_id.as_str(),
                entry.platform.as_str(),
                entry.version.as_str(),
            )) {
                return Err(MetadataError::Duplicate(format!(
                    "{}/{}/{}",
                    entry.plugin_id, entry.platform, entry.version
                )));
            }
        }
        Ok(())
    }

    pub fn latest(&self, plugin_id: &str, platform: &str, abi: &str) -> Option<&CoreCatalogEntry> {
        self.entries
            .iter()
            .filter(|entry| {
                entry.plugin_id == plugin_id && entry.platform == platform && entry.abi == abi
            })
            .max_by(|left, right| left.release_cmp(right))
    }
}

pub fn parse_core_catalog(bytes: &[u8]) -> Result<CoreCatalog, MetadataError> {
    let catalog: CoreCatalog = serde_json::from_value(
        crate::parse_unique_json(bytes).map_err(|_| MetadataError::SchemaMismatch("cores.json"))?,
    )
    .map_err(|_| MetadataError::SchemaMismatch("cores.json"))?;
    catalog.validate()?;
    Ok(catalog)
}

fn is_platform_triple(value: &str) -> bool {
    let segment = |part: &str| {
        (1..=32).contains(&part.len())
            && part
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    };
    value
        .split_once('-')
        .is_some_and(|(os, arch)| segment(os) && segment(arch))
}

pub(crate) fn is_release_tag(value: &str) -> bool {
    (1..=128).contains(&value.len())
        && value != "."
        && value != ".."
        && value.bytes().all(is_asset_byte)
}

fn is_asset_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'+')
}

fn is_release_asset_url(value: &str, release_tag: &str) -> bool {
    value
        .strip_prefix(CORE_RELEASE_URL_PREFIX)
        .and_then(|rest| rest.split_once('/'))
        .is_some_and(|(tag, asset)| {
            tag == release_tag
                && !asset.is_empty()
                && asset.len() <= 256
                && asset.bytes().all(is_asset_byte)
                && asset != "."
                && asset != ".."
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(version: &str) -> CoreCatalogEntry {
        CoreCatalogEntry {
            plugin_id: "shinki.emu.musica".into(),
            display_name: "Shinki Musica".into(),
            family_name: "Musica".into(),
            version: version.into(),
            abi: "shinki.emu.native.c.v3".into(),
            platform: "windows-x86_64".into(),
            url: format!(
                "{CORE_RELEASE_URL_PREFIX}emu-v{version}/shinki-emu-core-shinki_emu_musica-{version}-windows-x86_64.zip"
            ),
            sha256: "b".repeat(64),
            size: 1024,
            license: "MIT OR Apache-2.0".into(),
            release_tag: format!("emu-v{version}"),
            identity: None,
        }
    }

    fn catalog(entries: Vec<CoreCatalogEntry>) -> CoreCatalog {
        CoreCatalog {
            schema: CORE_CATALOG_SCHEMA_VERSION.into(),
            entries,
        }
    }

    #[test]
    fn latest_picks_highest_semver_for_platform_and_abi() {
        let catalog = catalog(vec![
            entry("0.2.0"),
            entry("0.10.0+public.1"),
            entry("0.9.1"),
        ]);
        let parsed = parse_core_catalog(&serde_json::to_vec(&catalog).unwrap()).unwrap();
        let latest = parsed
            .latest(
                "shinki.emu.musica",
                "windows-x86_64",
                "shinki.emu.native.c.v3",
            )
            .unwrap();
        assert_eq!(latest.version, "0.10.0+public.1");
        assert!(
            parsed
                .latest(
                    "shinki.emu.musica",
                    "windows-x86_64",
                    "shinki.emu.native.c.v2"
                )
                .is_none()
        );
    }

    #[test]
    fn preserves_case_sensitive_emulator_identity() {
        let mut reminori = entry("0.2.0");
        reminori.plugin_id = "ReMinori".into();
        let parsed =
            parse_core_catalog(&serde_json::to_vec(&catalog(vec![reminori])).unwrap()).unwrap();
        assert_eq!(
            parsed
                .latest("ReMinori", "windows-x86_64", "shinki.emu.native.c.v3")
                .unwrap()
                .plugin_id,
            "ReMinori"
        );
        assert!(
            parsed
                .latest("reminori", "windows-x86_64", "shinki.emu.native.c.v3")
                .is_none()
        );
    }

    #[test]
    fn rejects_foreign_urls_duplicates_and_bad_versions() {
        let mut foreign = entry("0.2.0");
        foreign.url = "https://example.com/core.zip".into();
        assert!(catalog(vec![foreign]).validate().is_err());
        let mut mismatched_tag = entry("0.2.0");
        mismatched_tag.release_tag = "emu-v0.3.0".into();
        assert!(catalog(vec![mismatched_tag]).validate().is_err());
        let mut traversal = entry("0.2.0");
        traversal.url = format!("{CORE_RELEASE_URL_PREFIX}emu-v0.2.0/../x.zip");
        assert!(catalog(vec![traversal]).validate().is_err());
        assert!(matches!(
            catalog(vec![entry("0.2.0"), entry("0.2.0")]).validate(),
            Err(MetadataError::Duplicate(_))
        ));
        let mut bad_version = entry("0.2.0");
        bad_version.version = "latest".into();
        assert!(catalog(vec![bad_version]).validate().is_err());
        let mut oversized = entry("0.2.0");
        oversized.size = MAX_CORE_PACKAGE_BYTES + 1;
        assert!(catalog(vec![oversized]).validate().is_err());
    }

    fn external(
        kind: crate::UpstreamRefKind,
        reference: &str,
        sequence: u64,
        adapter: char,
    ) -> CoreCatalogEntry {
        let identity = crate::CoreIdentity {
            upstream: crate::UpstreamIdentity {
                repo: "https://github.com/example/engine".into(),
                ref_kind: kind,
                ref_name: reference.into(),
                commit: "a".repeat(40),
            },
            adapter: crate::AdapterIdentity {
                repo: "https://github.com/Project-Shinkirou/engine".into(),
                branch: "shinki/integration".into(),
                commit: adapter.to_string().repeat(40),
            },
            release_sequence: sequence,
        };
        let mut result = entry("1.0.0");
        result.plugin_id = "engine".into();
        result.version = identity.version();
        result.release_tag = format!("core-engine-{}", result.version);
        result.url = format!("{CORE_RELEASE_URL_PREFIX}{}/engine.zip", result.release_tag);
        result.identity = Some(identity);
        result
    }

    #[test]
    fn tagged_and_slash_branch_consumers_use_provenance_not_semver_or_hash_order() {
        use crate::UpstreamRefKind::{Branch, Tag};
        let tagged = external(Tag, "v1.2.3", 1, 'f');
        let branch = external(Branch, "release/next", 2, '1');
        let revision = external(Tag, "v1.2.3", 3, '2');
        assert!(semver::Version::parse(&branch.version).is_err());
        assert!(!branch.version.contains('/'));
        assert_ne!(tagged.version, revision.version);
        let same_slug = external(Branch, "release-next", 2, '1');
        assert_ne!(same_slug.version, branch.version);
        assert_eq!(branch.identity.as_ref().unwrap().version(), branch.version);
        for entries in [
            vec![tagged.clone(), revision.clone(), branch.clone()],
            vec![branch.clone(), tagged.clone(), revision.clone()],
        ] {
            let parsed =
                parse_core_catalog(&serde_json::to_vec(&catalog(entries)).unwrap()).unwrap();
            assert_eq!(
                parsed.latest("engine", "windows-x86_64", "shinki.emu.native.c.v3"),
                Some(&revision)
            );
        }
        assert!(revision.is_update_for(&tagged.version, tagged.identity.as_ref()));
        assert!(!tagged.is_update_for(&revision.version, revision.identity.as_ref()));
        assert_eq!(
            branch.display_version(),
            "release/next@aaaaaaaaaaaa (adapter 111111111111)"
        );
    }

    #[test]
    fn legacy_published_entries_survive_and_new_identity_wins_without_relabeling() {
        let mut old = entry("0.1.0-beta.3");
        old.plugin_id = "engine".into();
        let legacy = CoreCatalog {
            schema: LEGACY_CORE_CATALOG_SCHEMA_VERSION.into(),
            entries: vec![old.clone()],
        };
        assert_eq!(
            parse_core_catalog(&serde_json::to_vec(&legacy).unwrap())
                .unwrap()
                .entries,
            vec![old.clone()]
        );
        let new = external(crate::UpstreamRefKind::Branch, "main", 1, '1');
        let parsed = catalog(vec![new.clone(), old.clone()]);
        parsed.validate().unwrap();
        assert_eq!(
            parsed.latest("engine", "windows-x86_64", "shinki.emu.native.c.v3"),
            Some(&new)
        );
        assert!(new.is_update_for(&old.version, None));
        assert_eq!(parsed.entries[1], old);
    }

    #[test]
    fn rejects_forged_versions_traversal_provenance_and_ambiguous_sequence() {
        let valid = external(crate::UpstreamRefKind::Branch, "feature/next", 1, 'b');
        for bad_ref in [
            "../next",
            "main/../../other",
            "/main",
            "main.lock",
            "main\\\\next",
            "main@{1}",
            "main\\nnext",
        ] {
            let bad = external(crate::UpstreamRefKind::Branch, bad_ref, 1, 'b');
            assert!(bad.validate().is_err(), "{bad_ref}");
        }
        let mut forged = valid.clone();
        forged.version = "0.1.0-beta.9".into();
        assert!(forged.validate().is_err());
        let mut reused = external(crate::UpstreamRefKind::Tag, "v1.0", 1, 'c');
        reused.platform = "linux-x86_64".into();
        assert!(catalog(vec![valid.clone(), reused]).validate().is_err());
        for repository in [
            "https://github.com/project-shinkirou/engine",
            "https://github.com/./engine",
            "https://github.com/example/..",
            "https://github.com/example/engine.git",
            "https://github.com/example/engine.GIT",
        ] {
            let mut wrong_upstream = valid.clone();
            wrong_upstream.identity.as_mut().unwrap().upstream.repo = repository.into();
            wrong_upstream.version = wrong_upstream.identity.as_ref().unwrap().version();
            wrong_upstream.release_tag = format!("core-engine-{}", wrong_upstream.version);
            wrong_upstream.url = format!(
                "{CORE_RELEASE_URL_PREFIX}{}/engine.zip",
                wrong_upstream.release_tag
            );
            assert!(wrong_upstream.validate().is_err(), "{repository}");
        }
        let boundary = external(crate::UpstreamRefKind::Branch, &"x".repeat(256), 2, 'b');
        boundary.validate().unwrap();
        assert!(boundary.version.len() <= 88);
        assert!(
            external(crate::UpstreamRefKind::Branch, &"x".repeat(257), 2, 'b')
                .validate()
                .is_err()
        );
    }
}
