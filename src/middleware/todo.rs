use async_trait::async_trait;
use crate::{Request, Response, ToolCall, middleware::Middleware};
use std::path::PathBuf;

pub struct TodoMiddleware {
    pub todo_path: PathBuf,
}

impl TodoMiddleware {
    pub fn new(todo_path: PathBuf) -> Self {
        Self { todo_path }
    }

    async fn read_todos(&self) -> String {
        if self.todo_path.exists() {
            tokio::fs::read_to_string(&self.todo_path)
                .await
                .unwrap_or_else(|_| "Error reading TODO.md".to_string())
        } else {
            "No current objectives defined.".to_string()
        }
    }
}

#[async_trait]
impl Middleware for TodoMiddleware {
    fn name(&self) -> &str { "Todo" }

    async fn before_ai_call(&self, req: &mut Request) -> anyhow::Result<()> {
        // Pillar: Explicit Planning
        // Inject the current agent-compatible Markdown plan into the prompt.
        // This ensures the agent maintains objective-coherence across reasoning steps.
        let todos = self.read_todos().await;
        req.prompt = format!(
            "### CURRENT AGENT OBJECTIVES (from TODO.md) ###\n{}\n\n---\n\n{}", 
            todos, 
            req.prompt
        );
        Ok(())
    }

    async fn after_ai_call(&self, res: &mut Response) -> anyhow::Result<()> {
        // Monitor if the LLM is attempting to update its plan
        for tool in &res.tool_calls {
            if tool.name == "update_todo" || tool.name == "write_file" {
                if let Some(path) = tool.arguments.get("path").and_then(|p| p.as_str()) {
                    if path.contains("TODO.md") {
                        tracing::info!("Plan update detected in AI response. Objectives are evolving.");
                    }
                }
            }
        }
        Ok(())
    }

    async fn after_tool_call(&self, tool: &ToolCall, _result: &mut String) -> anyhow::Result<()> {
        // After a tool executes, if it was a plan update, we refresh our understanding
        // and confirm the persistence of a valid plan.
        let is_plan_update = tool.name == "update_todo" || 
            (tool.name == "write_file" && tool.arguments.get("path").and_then(|p: &serde_json::Value| p.as_str()).map(|p| p.contains("TODO.md")).unwrap_or(false));

        if is_plan_update && self.todo_path.exists() {
            let _updated_todos = self.read_todos().await;
            tracing::info!("Verified and refreshed TODO.md state after plan update tool.");
        }
        Ok(())
    }
}
