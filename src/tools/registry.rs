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
        guard.values().map(|t| {
            let mut schema = t.schema();
            schema.source = t.source();
            schema
        }).collect()
    }

    pub async fn unregister_by_prefix(&self, prefix: &str) {
        let mut guard = self.tools.lock().await;
        guard.retain(|_, t| t.source() != prefix);
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
