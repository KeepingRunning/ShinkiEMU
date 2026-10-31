mod compatibility;
mod core_identity;
mod cores;
mod cover;
mod fields;
mod index;
mod json;
mod license;
mod matcher;
mod model;
mod patches;
mod presets;
mod sync;
mod vndb;
pub use fields::{KNOWN_CORE_IDS, is_known_core_id};
pub use json::parse_unique_json;
pub use patches::*;
pub use core_identity::{AdapterIdentity, CoreIdentity, UpstreamIdentity, UpstreamRefKind, is_git_ref, validate_core_version};

/// The only schema export used by the build tool and checked-in schema files.
pub fn metadata_json_schemas() -> std::collections::BTreeMap<String, serde_json::Value> {
    std::collections::BTreeMap::from([
        (
            "compatibility.v4.schema.json".into(),
            compatibility_json_schema(),
        ),
        (
            "cores.v2.schema.json".into(),
            serde_json::to_value(schemars::schema_for!(CoreCatalog)).expect("schema"),
        ),
        (
            "cores.v1.schema.json".into(),
            serde_json::from_str(include_str!("../../../../Metadata/schema/cores.v1.schema.json")).expect("legacy schema"),
        ),
        (
            "presets.v1.schema.json".into(),
            serde_json::to_value(schemars::schema_for!(PresetDatabase)).expect("schema"),
        ),
        (
            "index.v1.schema.json".into(),
            serde_json::to_value(schemars::schema_for!(MetadataIndex)).expect("schema"),
        ),
        (
            "patches.v1.schema.json".into(),
            serde_json::to_value(schemars::schema_for!(PatchCatalog)).expect("schema"),
        ),
    ])
}

#[cfg(not(any(target_os = "android", target_env = "ohos")))]
mod bangumi;

#[cfg(not(any(target_os = "android", target_env = "ohos")))]
pub use bangumi::{BangumiProvider, BangumiProviderConfig};
pub use compatibility::{
    COMPATIBILITY_SCHEMA_VERSION, CompatibilityDatabase, CompatibilityEntry, CompatibilityError,
    CompatibilityStatus, compatibility_json_schema, parse_compatibility_database,
};
pub use cores::{
    CORE_CATALOG_SCHEMA_VERSION, LEGACY_CORE_CATALOG_SCHEMA_VERSION, CORE_RELEASE_URL_PREFIX, CoreCatalog, CoreCatalogEntry,
    MAX_CORE_PACKAGE_BYTES, parse_core_catalog,
};
pub use cover::{CoverFetcher, CoverPolicy};
pub use index::{
    IndexedFile, MAX_METADATA_FILE_BYTES, MAX_METADATA_TOTAL_BYTES, METADATA_INDEX_FILE,
    METADATA_INDEX_SCHEMA_VERSION, MetadataIndex, parse_metadata_index,
};
pub use license::{MetadataLicenseManifest, ReleaseUse};
pub use matcher::{
    MATCHER_VERSION, MatchInput, match_metadata, normalize_title, unique_exact_match,
};
pub use model::*;
pub use presets::{PRESET_DATABASE_SCHEMA_VERSION, Preset, PresetDatabase, parse_preset_database};
pub use sync::{DEFAULT_METADATA_BASE_URL, MetadataSyncClient, SyncOutcome};
pub use vndb::{VndbProvider, VndbProviderConfig, plain_description};

#[allow(async_fn_in_trait)]
pub trait MetadataProvider: Send + Sync {
    fn provider_id(&self) -> MetadataProviderId;
    async fn search(
        &self,
        query: &MetadataSearchQuery,
    ) -> Result<Vec<MetadataRecord>, MetadataError>;
    async fn fetch_by_id(&self, remote_id: &str) -> Result<MetadataRecord, MetadataError>;
    async fn fetch_cover(
        &self,
        record: &MetadataRecord,
        allow_sensitive: bool,
    ) -> Result<CoverAsset, MetadataError>;
    /// Fetch the concrete releases (versions) of a visual novel, keyed by the
    /// VNDB release id (`rID`). Used to pin a local installation to a specific
    /// game version so compatibility can be reported per-version. Only the VNDB
    /// provider returns releases; Bangumi is used solely for progress tracking
    /// and returns an empty list.
    async fn fetch_releases(&self, remote_id: &str) -> Result<Vec<MetadataRelease>, MetadataError>;
}
