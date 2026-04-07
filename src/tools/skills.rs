use async_trait::async_trait;
use crate::tools::{Tool, ToolSchema};
use serde_json::Value;

pub struct Skill {
    pub name: String,
    pub description: String,
    pub parameters: Value,
    pub handler: std::sync::Arc<dyn Fn(Value) -> futures_util::future::BoxFuture<'static, anyhow::Result<Value>> + Send + Sync>,
}

#[async_trait]
impl Tool for Skill {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name.clone(),
            description: self.description.clone(),
            parameters: self.parameters.clone(),
        }
    }

    async fn execute(&self, input: Value) -> anyhow::Result<Value> {
        (self.handler)(input).await
    }
}
