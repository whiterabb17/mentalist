use mem_core::Context;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Goal {
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentState {
    pub session_id: String,
    pub goal: Option<Goal>,
    pub context: Arc<Context>,
    pub sandbox_root: PathBuf,
    pub is_complete: bool,
}

impl Default for AgentState {
    fn default() -> Self {
        Self {
            session_id: "default".into(),
            goal: None,
            context: Arc::new(Context::default()),
            sandbox_root: std::env::current_dir().unwrap_or_default(),
            is_complete: false,
        }
    }
}

impl AgentState {
    pub fn new(session_id: String, context: Arc<Context>, sandbox_root: PathBuf) -> Self {
        Self {
            session_id,
            goal: None,
            context,
            sandbox_root,
            is_complete: false,
        }
    }
}
