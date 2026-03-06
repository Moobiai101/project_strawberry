//! Render pipeline

use thiserror::Error;

#[derive(Error, Debug)]
pub enum RenderError {
    #[error("GPU error: {0}")]
    Gpu(String),

    #[error("Shader error: {0}")]
    Shader(String),
}

/// Main render pipeline
pub struct RenderPipeline {
    // TODO: wgpu device, queue, etc.
}

impl RenderPipeline {
    pub async fn new() -> Result<Self, RenderError> {
        // TODO: Initialize wgpu
        Ok(Self {})
    }
}
