use crate::tools::{Tool, ToolSchema};
use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::Mutex;

pub struct ToolRegistry {
    pub tools: Mutex<HashMap<String, Arc<dyn Tool>>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self { tools: Mutex::new(HashMap::new()) }
    }

    pub async fn register(&self, tool: Arc<dyn Tool>) {
        let mut guard = self.tools.lock().await;
        guard.insert(tool.schema().name.clone(), tool);
    }

    pub async fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        let guard = self.tools.lock().await;
        guard.get(name).cloned()
    }

    pub async fn list_tools(&self) -> Vec<ToolSchema> {
        let guard = self.tools.lock().await;
        guard.values().map(|t| t.schema()).collect()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
