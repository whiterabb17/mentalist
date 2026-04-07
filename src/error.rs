use thiserror::Error;
use crate::executor::ToolError;

#[derive(Debug, Error)]
pub enum MentalistError {
    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Executor error: {0}")]
    ExecutorError(#[from] ToolError),

    #[error("Agent error: {0}")]
    AgentError(String),

    #[error("Middleware '{middleware}' failure: {source}")]
    MiddlewareError {
        middleware: String,
        source: anyhow::Error,
    },

    #[error("Model provider error: {0}")]
    ProviderError(#[from] anyhow::Error),

    #[error("Internal error: {0}")]
    Internal(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, MentalistError>;
