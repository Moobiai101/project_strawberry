//! AI Agent

use serde::{Deserialize, Serialize};

/// AI Agent that processes natural language and executes commands
pub struct AIAgent {
    // TODO: LLM client, context builder
}

impl AIAgent {
    pub fn new() -> Self {
        Self {}
    }

    /// Process a natural language request
    pub async fn process(&self, _input: &str) -> Result<AIResponse, AIError> {
        // TODO: Parse intent, generate commands
        todo!("Implement AI processing")
    }
}

impl Default for AIAgent {
    fn default() -> Self {
        Self::new()
    }
}

/// AI response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIResponse {
    pub message: String,
    pub commands: Vec<String>,
}

/// AI errors
#[derive(Debug, thiserror::Error)]
pub enum AIError {
    #[error("LLM error: {0}")]
    LlmError(String),

    #[error("Parse error: {0}")]
    ParseError(String),
}
