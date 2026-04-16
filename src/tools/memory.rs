use crate::tools::{Tool, ToolSchema};
use mem_broker::tools::{execute_tool, get_progressive_disclosure_tools};
use mem_core::db::SqliteSearchEngine;
use mem_core::FactGraph;
use std::sync::Arc;
use async_trait::async_trait;
use serde_json::Value;

/// A wrapper that implements mentalist::tools::Tool for the Progressive Disclosure tools.
pub struct MemoryToolWrapper {
    pub name: String,
    pub description: String,
    pub parameters: Value,
    pub search_engine: Arc<SqliteSearchEngine>,
    pub graph: Arc<FactGraph>,
}

#[async_trait]
impl Tool for MemoryToolWrapper {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name.clone(),
            description: self.description.clone(),
            parameters: self.parameters.clone(),
            source: "memory".to_string(),
        }
    }

    async fn execute(&self, input: Value) -> anyhow::Result<Value> {
        let result = execute_tool(
            &self.name,
            &input,
            &self.search_engine,
            &self.graph,
        )?;
        Ok(Value::String(result))
    }
}

pub fn get_memory_tools(
    search_engine: Arc<SqliteSearchEngine>,
    graph: Arc<FactGraph>,
) -> Vec<Arc<dyn Tool>> {
    let definitions = get_progressive_disclosure_tools();
    definitions.into_iter().map(|d| {
        Arc::new(MemoryToolWrapper {
            name: d.name,
            description: d.description,
            parameters: d.parameters,
            search_engine: search_engine.clone(),
            graph: graph.clone(),
        }) as Arc<dyn Tool>
    }).collect()
}
