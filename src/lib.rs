// unused import removed
pub use mem_core::{Context, Request, Response, ResponseChunk, ToolCall, ToolCallDelta, ModelProvider};

pub mod provider;
pub mod executor;
pub mod middleware;
pub mod agent;
pub use agent::{DeepAgent, DeepAgentState};

use futures_util::stream::BoxStream;

pub struct Harness {
    pub provider: Box<dyn ModelProvider>,
    pub middlewares: Vec<Box<dyn middleware::Middleware>>,
}

impl Harness {
    pub fn new(provider: Box<dyn ModelProvider>) -> Self {
        Self {
            provider,
            middlewares: Vec::new(),
        }
    }

    pub fn add_middleware(&mut self, middleware: Box<dyn middleware::Middleware>) {
        self.middlewares.push(middleware);
    }

    /// Orchestrated Execution Loop following DeepAgent methodology.
    pub async fn run(&self, mut req: Request) -> anyhow::Result<Response> {
        // 1. Hook: before_ai_call (Context Optimization/Planning)
        for mw in &self.middlewares {
            mw.before_ai_call(&mut req).await?;
        }

        // 2. Execute AI reasoning
        let mut res = self.provider.complete(req).await?;

        // 3. Hook: after_ai_call (Response Parsing/Intent Extraction)
        for mw in &self.middlewares {
            mw.after_ai_call(&mut res).await?;
        }

        Ok(res)
    }

    /// Orchestrated Streaming Loop.
    pub async fn run_stream(&self, mut req: Request) -> anyhow::Result<BoxStream<'static, anyhow::Result<ResponseChunk>>> {
        // 1. Hook: before_ai_call (Context Optimization/Planning)
        for mw in &self.middlewares {
            mw.before_ai_call(&mut req).await?;
        }

        // 2. Execute AI reasoning (Streaming)
        self.provider.stream_complete(req).await
    }

    /// Helper for executed tool hooks (before).
    pub async fn run_before_tool_hooks(&self, tool: &mut ToolCall) -> anyhow::Result<()> {
        for mw in &self.middlewares {
            mw.before_tool_call(tool).await?;
        }
        Ok(())
    }

    /// Helper for executed tool hooks (after).
    pub async fn run_after_tool_hooks(&self, tool: &ToolCall, result: &mut String) -> anyhow::Result<()> {
        for mw in &self.middlewares {
            mw.after_tool_call(tool, result).await?;
        }
        Ok(())
    }
}
