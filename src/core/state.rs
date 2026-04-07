use mem_core::Context;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Goal {
    pub description: String,
}

pub struct AgentState {
    pub goal: Goal,
    pub context: Context,
    pub is_complete: bool,
}

impl AgentState {
    pub fn new(goal: &str, context: Context) -> Self {
        Self {
            goal: Goal { description: goal.to_string() },
            context,
            is_complete: false,
        }
    }
}
