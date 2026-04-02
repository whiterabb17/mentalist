use async_trait::async_trait;
use std::sync::Arc;
use crate::{Request, Response, ToolCall};
use brain::Brain;

#[async_trait]
pub trait Middleware: Send + Sync {
    /// Fires before the prompt reaches the LLM.
    async fn before_ai_call(&self, _req: &mut Request) -> anyhow::Result<()> {
        Ok(())
    }

    /// Fires after the LLM responds, before processing tool calls.
    async fn after_ai_call(&self, _res: &mut Response) -> anyhow::Result<()> {
        Ok(())
    }

    /// Fires before a specific tool execution (Safety gate).
    async fn before_tool_call(&self, _tool: &mut ToolCall) -> anyhow::Result<()> {
        Ok(())
    }

    /// Fires after a tool result is returned.
    async fn after_tool_call(&self, _result: &mut String) -> anyhow::Result<()> {
        Ok(())
    }
}

pub struct MindPalaceMiddleware {
    pub brain: Arc<Brain>,
}

impl MindPalaceMiddleware {
    pub fn new(brain: Arc<Brain>) -> Self {
        Self { brain }
    }
}

#[async_trait]
impl Middleware for MindPalaceMiddleware {
    async fn before_ai_call(&self, req: &mut Request) -> anyhow::Result<()> {
        // Pillar: Stateful Context Defense
        self.brain.optimize(&mut req.context).await?;
        Ok(())
    }
}

pub mod todo;
