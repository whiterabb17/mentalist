use async_trait::async_trait;
use crate::{Request, Response, middleware::Middleware};
use std::fs;
use std::path::PathBuf;

pub struct TodoMiddleware {
    pub todo_path: PathBuf,
}

impl TodoMiddleware {
    pub fn new(todo_path: PathBuf) -> Self {
        Self { todo_path }
    }
}

#[async_trait]
impl Middleware for TodoMiddleware {
    async fn before_ai_call(&self, req: &mut Request) -> anyhow::Result<()> {
        // Pillar: Explicit Planning
        // Before the LLM process, inject the current agent-compatible Markdown plan.
        // This ensures the agent maintains objective-coherence across tasks.
        if self.todo_path.exists() {
            if let Ok(todos) = fs::read_to_string(&self.todo_path) {
                req.prompt = format!("### CURRENT OBJECTIVES (TODO.md):\n{}\n\n---\n\n{}", todos, req.prompt);
            }
        }
        Ok(())
    }

    async fn after_ai_call(&self, _res: &mut Response) -> anyhow::Result<()> {
        // Implementation for identifying plan-update tool calls
        Ok(())
    }

    async fn after_tool_call(&self, _result: &mut String) -> anyhow::Result<()> {
        // Implementation for persisting plan changes after a 'write_todo' call
        Ok(())
    }
}
