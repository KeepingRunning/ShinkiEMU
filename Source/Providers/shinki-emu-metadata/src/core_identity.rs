//! Reproducible external-core identity, independent of Manager SemVer.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use crate::MetadataError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum UpstreamRefKind { Tag, Branch }
impl UpstreamRefKind {
    pub fn as_str(self) -> &'static str {
        match self { Self::Tag => "tag", Self::Branch => "branch" }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpstreamIdentity {
    #[schemars(regex(pattern = r"^https://github\.com/[A-Za-z0-9._-]{1,100}/[A-Za-z0-9._-]{1,100}$"))]
    pub repo: String,
    pub ref_kind: UpstreamRefKind,
    #[schemars(length(min = 1, max = 256), regex(pattern = r"^(?!.*\.\.)(?!.*@\{)[A-Za-z0-9._+/-]+$"))]
    pub ref_name: String,
    #[schemars(regex(pattern = r"^[0-9a-f]{40}$"))]
    pub commit: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AdapterIdentity {
    #[schemars(regex(pattern = r"^https://github\.com/Project-Shinkirou/[A-Za-z0-9._-]{1,100}$"))]
    pub repo: String,
    #[schemars(length(min = 1, max = 256), regex(pattern = r"^(?!.*\.\.)(?!.*@\{)[A-Za-z0-9._+/-]+$"))]
    pub branch: String,
    #[schemars(regex(pattern = r"^[0-9a-f]{40}$"))]
    pub commit: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoreIdentity {
    pub upstream: UpstreamIdentity,
    pub adapter: AdapterIdentity,
    #[schemars(range(min = 1, max = 9007199254740991_u64))]
    pub release_sequence: u64,
}

pub fn is_git_ref(value: &str) -> bool {
    !value.is_empty() && value.len() <= 256 && !value.contains("..") && !value.contains("@{")
        && value != "@" && !value.ends_with('.')
        && value.split('/').all(|s| !s.is_empty() && !s.starts_with('.') && !s.ends_with(".lock")
            && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-+".contains(&b)))
}
fn is_commit(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn repository(value: &str) -> Option<&str> {
    let path = value.strip_prefix("https://github.com/")?;
    let (owner, repo) = path.split_once('/')?;
    let safe = |s: &str| !s.is_empty() && s != "." && s != ".." && s.len() <= 100
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b));
    (safe(owner) && safe(repo) && !repo.get(repo.len().saturating_sub(4)..).is_some_and(|suffix| suffix.eq_ignore_ascii_case(".git"))).then_some(owner)
}
impl CoreIdentity {
    pub fn validate(&self) -> Result<(), MetadataError> {
        if repository(&self.upstream.repo).is_none_or(|owner| owner.eq_ignore_ascii_case("Project-Shinkirou"))
            || repository(&self.adapter.repo) != Some("Project-Shinkirou")
            || !is_git_ref(&self.upstream.ref_name) || !is_git_ref(&self.adapter.branch)
            || !is_commit(&self.upstream.commit) || !is_commit(&self.adapter.commit)
            || !(1..=9_007_199_254_740_991).contains(&self.release_sequence)
        { return Err(MetadataError::SchemaMismatch("core.identity")); }
        Ok(())
    }
    /// SHA256 over UTF-8 fields, each terminated by LF. Refs cannot contain LF.
    /// The sequence is included so a deliberate rebuild has a distinct identity.
    pub fn version(&self) -> String {
        let mut hash = Sha256::new();
        for field in [&self.upstream.repo, self.upstream.ref_kind.as_str(), &self.upstream.ref_name,
            &self.upstream.commit, &self.adapter.repo, &self.adapter.branch, &self.adapter.commit,
            &self.release_sequence.to_string()] {
            hash.update(field.as_bytes()); hash.update(b"\n");
        }
        let readable: String = self.upstream.ref_name.bytes().take(16).map(|b| {
            if b.is_ascii_alphanumeric() { char::from(b.to_ascii_lowercase()) } else { '-' }
        }).collect();
        format!("{}-{}-{}", self.upstream.ref_kind.as_str(), readable, hex::encode(hash.finalize()))
    }
    pub fn display_version(&self) -> String {
        format!("{}@{} (adapter {})", self.upstream.ref_name, &self.upstream.commit[..12], &self.adapter.commit[..12])
    }
}

/// Legacy packages retain authentic SemVer; new packages use a validated identity.
pub fn validate_core_version(version: &str, identity: Option<&CoreIdentity>) -> Result<(), MetadataError> {
    if let Some(identity) = identity {
        identity.validate()?;
        if version != identity.version() { return Err(MetadataError::SchemaMismatch("core.version_identity")); }
    } else if version.len() > 64 || semver::Version::parse(version).is_err() {
        return Err(MetadataError::SchemaMismatch("core.version"));
    }
    Ok(())
}
