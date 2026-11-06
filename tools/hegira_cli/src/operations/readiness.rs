//! Read-only diagnostics, not execution authority or application hook dispatch.

use std::path::PathBuf;

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ReadinessStatus {
    Pass,
    Warning,
    Failure,
}

#[derive(Debug, Serialize)]
pub(crate) struct ReadinessCheck {
    pub(crate) code: &'static str,
    pub(crate) status: ReadinessStatus,
    pub(crate) message: &'static str,
    pub(crate) action: Option<&'static str>,
}

impl ReadinessCheck {
    pub(crate) fn new(
        code: &'static str,
        status: ReadinessStatus,
        message: &'static str,
        action: Option<&'static str>,
    ) -> Self {
        Self {
            code,
            status,
            message,
            action,
        }
    }
}

/// An explicit narrow tool-probe selection, never ambient application PATH.
pub(crate) struct ReadinessTools {
    pub(crate) cargo: PathBuf,
    pub(crate) directories: Vec<PathBuf>,
    pub(crate) wasm_bindgen: Option<PathBuf>,
    pub(crate) wasm_opt: Option<PathBuf>,
}
