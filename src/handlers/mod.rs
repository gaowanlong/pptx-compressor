//! Media compression handlers.
//!
//! Every handler writes its result to a *candidate* file outside the extracted
//! PPTX tree and returns a [`CompressCandidate`]. The pipeline alone decides
//! whether to replace the original, so "compressed is larger" and failures can
//! never leave a worse or broken file behind.

use std::path::PathBuf;

pub mod gif_handler;
pub mod image;
pub mod video;
pub mod xml_clean;

/// Result of a compression attempt. The original file is never touched here.
#[derive(Debug, Clone)]
pub struct CompressCandidate {
    /// Path of the newly encoded file (inside the pipeline work directory).
    pub path: PathBuf,
    /// Set when the container format changed, e.g. `wmv` -> `mp4`.
    /// The value is the new extension without a leading dot.
    pub new_extension: Option<String>,
}

/// Sentinel error text used when the user cancels a long-running encode.
pub const CANCELLED: &str = "__PPTX_COMPRESSOR_CANCELLED__";
