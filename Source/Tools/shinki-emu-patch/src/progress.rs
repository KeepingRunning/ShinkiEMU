#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatchVerification {
    Pending,
    Ok,
    Failed,
}

#[derive(Clone, Debug, Default)]
pub enum PatchActivity {
    #[default]
    Phase,
    Download {
        name: String,
        sha256: String,
        verification: PatchVerification,
    },
    Check {
        label: String,
        passed: Option<bool>,
        detail: String,
    },
}

#[derive(Clone, Debug, Default)]
pub struct PatchProgress {
    pub phase: String,
    pub completed: u64,
    pub total: Option<u64>,
    pub activity: PatchActivity,
}
impl PatchProgress {
    pub fn download(
        name: String,
        sha256: String,
        verification: PatchVerification,
        completed: u64,
        total: Option<u64>,
    ) -> Self {
        Self {
            phase: format!("Downloading {name}"),
            completed,
            total,
            activity: PatchActivity::Download {
                name,
                sha256,
                verification,
            },
        }
    }
    pub fn check(label: String, passed: Option<bool>, detail: String) -> Self {
        Self {
            phase: label.clone(),
            completed: u64::from(passed.is_some()),
            total: Some(1),
            activity: PatchActivity::Check {
                label,
                passed,
                detail,
            },
        }
    }
}
