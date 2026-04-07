use async_trait::async_trait;
use serde_json::Value;

pub mod registry;
pub mod mcp_adapter;
pub mod skills;

pub use registry::ToolRegistry;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub parameters: Value,
    pub source: String,
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn schema(&self) -> ToolSchema;
    async fn execute(&self, input: Value) -> anyhow::Result<Value>;
    fn source(&self) -> String { "builtin".to_string() }
}
