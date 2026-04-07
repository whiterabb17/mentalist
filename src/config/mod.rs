use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeConfig {
    pub max_steps: usize,
    pub timeout_seconds: u64,
    pub session_id: String,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            max_steps: 10,
            timeout_seconds: 300,
            session_id: "default_session".to_string(),
        }
    }
}
