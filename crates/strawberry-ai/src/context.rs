//! AI Context builder

/// Builds context for LLM from ECS state
pub struct ContextBuilder {
    // TODO: Tiered context system
}

impl ContextBuilder {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for ContextBuilder {
    fn default() -> Self {
        Self::new()
    }
}
