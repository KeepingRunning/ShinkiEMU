use std::{error::Error, fmt};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PatchErrorKind {
    InvalidRequest,
    UnsupportedInput,
    ReleaseMismatch,
    OutputExists,
    Path,
    Bounds,
    Memory,
    Codec,
    Integrity,
    Changed,
    Cancelled,
    Script,
    Io,
    Publish,
}

impl PatchErrorKind {
    pub const fn code(self) -> &'static str {
        match self {
            Self::InvalidRequest => "SHINKI_EMU_PATCH_INVALID_REQUEST",
            Self::UnsupportedInput => "SHINKI_EMU_PATCH_UNSUPPORTED_INPUT",
            Self::ReleaseMismatch => "SHINKI_EMU_PATCH_RELEASE_MISMATCH",
            Self::OutputExists => "SHINKI_EMU_PATCH_OUTPUT_EXISTS",
            Self::Path => "SHINKI_EMU_PATCH_PATH",
            Self::Bounds => "SHINKI_EMU_PATCH_BOUNDS",
            Self::Memory => "SHINKI_EMU_PATCH_MEMORY",
            Self::Codec => "SHINKI_EMU_PATCH_CODEC",
            Self::Integrity => "SHINKI_EMU_PATCH_INTEGRITY",
            Self::Changed => "SHINKI_EMU_PATCH_CHANGED",
            Self::Cancelled => "SHINKI_EMU_PATCH_CANCELLED",
            Self::Script => "SHINKI_EMU_PATCH_SCRIPT",
            Self::Io => "SHINKI_EMU_PATCH_IO",
            Self::Publish => "SHINKI_EMU_PATCH_PUBLISH",
        }
    }
}

pub struct PatchError {
    kind: PatchErrorKind,
    message: String,
    cause: Option<Box<dyn Error + Send + Sync>>,
    rolled_back: bool,
}

impl PatchError {
    pub fn new(kind: PatchErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            cause: None,
            rolled_back: false,
        }
    }

    pub fn with_source(
        kind: PatchErrorKind,
        message: impl Into<String>,
        cause: impl Error + Send + Sync + 'static,
    ) -> Self {
        Self {
            kind,
            message: message.into(),
            cause: Some(Box::new(cause)),
            rolled_back: false,
        }
    }

    pub fn kind(&self) -> PatchErrorKind {
        self.kind
    }
    pub fn code(&self) -> &'static str {
        self.kind.code()
    }
    pub fn rolled_back(&self) -> bool {
        self.rolled_back
    }
    pub(crate) fn recovered(mut self) -> Self {
        self.rolled_back = true;
        self
    }
}

impl fmt::Display for PatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code(), self.message)
    }
}

impl fmt::Debug for PatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl Error for PatchError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.cause
            .as_deref()
            .map(|source| source as &(dyn Error + 'static))
    }
}

impl From<std::io::Error> for PatchError {
    fn from(source: std::io::Error) -> Self {
        Self::with_source(PatchErrorKind::Io, "file operation failed", source)
    }
}

impl From<PatchError> for mlua::Error {
    fn from(error: PatchError) -> Self {
        Self::external(error)
    }
}

pub(crate) fn check_cancelled(cancelled: &std::sync::atomic::AtomicBool) -> Result<(), PatchError> {
    if cancelled.load(std::sync::atomic::Ordering::Acquire) {
        Err(PatchError::new(
            PatchErrorKind::Cancelled,
            "conversion cancelled",
        ))
    } else {
        Ok(())
    }
}
