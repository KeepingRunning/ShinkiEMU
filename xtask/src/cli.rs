use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "cargo xtask",
    about = "ShinkiEMU build, packaging, release and metadata tools",
    disable_help_subcommand = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Args)]
pub struct TargetCore {
    #[arg(long)]
    pub target: String,
    #[arg(long)]
    pub core: Option<String>,
}

#[derive(Args)]
pub struct DependencyOptions {
    #[arg(long)]
    pub target: String,
    #[arg(long, conflicts_with = "all_cores")]
    pub core: Option<String>,
    #[arg(long)]
    pub all_cores: bool,
    /// Reuse unpublished fork edits only when HEAD exactly matches its source pin.
    #[arg(long)]
    pub allow_dirty_sources: bool,
}

#[derive(Args)]
#[group(required = true, multiple = false)]
pub struct Selection {
    #[arg(long)]
    pub core: Option<String>,
    #[arg(long)]
    pub manager: bool,
}
impl Selection {
    pub fn component(&self) -> &str {
        self.core.as_deref().unwrap_or("manager")
    }
}

#[derive(Args)]
pub struct Build {
    #[arg(long)]
    pub target: String,
    #[arg(long, conflicts_with_all=["manager", "all_cores"])]
    pub core: Option<String>,
    #[arg(long, conflicts_with = "all_cores")]
    pub manager: bool,
    #[arg(long)]
    pub all_cores: bool,
    #[arg(long)]
    pub release: bool,
}
#[derive(Subcommand)]
pub enum Command {
    /// Build a private Apple cross image from SHINKI_APPLE_SDK_DIR.
    AppleImage {
        #[arg(long)]
        target: String,
    },
    Doctor(TargetCore),
    Deps(DependencyOptions),
    /// List every combination; `--json` emits the job matrix of supported rows only.
    Matrix {
        #[arg(long)]
        json: bool,
        /// Drop rows whose CI build another row already performs (the universal Android Manager).
        #[arg(long, requires = "json")]
        ci: bool,
        /// Filter to components affected by changes since this Git ref.
        #[arg(long, requires = "json")]
        since: Option<String>,
    },
    Build(Build),
    Check {
        #[arg(long)]
        target: String,
        #[command(flatten)]
        selection: Selection,
    },
    Test {
        #[arg(long, conflicts_with = "manager")]
        core: Option<String>,
        #[arg(long)]
        manager: bool,
    },
    Package {
        #[arg(long)]
        target: String,
        #[command(flatten)]
        selection: Selection,
        #[arg(long, default_value = "")]
        version: String,
        #[arg(long)]
        out: PathBuf,
    },
    /// Prepare one target's release files, or publish every prepared target.
    Release {
        #[arg(long)]
        component: String,
        #[arg(long, required_unless_present_any = ["publish", "verify_handoff", "dry_run"], conflicts_with_all = ["publish", "verify_handoff", "dry_run"])]
        target: Option<String>,
        #[arg(long, default_value = "")]
        version: String,
        #[arg(long)]
        publish: bool,
        /// Verify the staged OHOS HAP and corresponding source without DevEco.
        #[arg(long, conflicts_with = "publish")]
        verify_handoff: bool,
        /// Resolve and verify upstream/adapter identities without building or publishing.
        #[arg(long, conflicts_with_all = ["publish", "verify_handoff"])]
        dry_run: bool,
    },
    Metadata {
        #[command(subcommand)]
        cmd: crate::metadata::MetadataCommand,
    },
    Ci {
        #[arg(long)]
        target: String,
        #[arg(long)]
        component: String,
        #[arg(long)]
        report: PathBuf,
    },
}
