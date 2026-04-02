use async_trait::async_trait;
use serde::{Serialize, Deserialize};
use mem_core::Context;

pub mod provider;
pub mod executor;
pub mod middleware;
pub mod agent;
pub use agent::{DeepAgent, DeepAgentState};

use futures_util::stream::BoxStream;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub prompt: String,
    pub context: Context,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseChunk {
    pub content_delta: Option<String>,
    pub tool_call_delta: Option<ToolCallDelta>,
    pub is_final: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallDelta {
    pub name: Option<String>,
    pub arguments_delta: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub name: String,
    pub arguments: serde_json::Value,
}

#[async_trait]
pub trait ModelProvider: Send + Sync {
    /// Non-streaming completion.
    async fn complete(&self, req: Request) -> anyhow::Result<Response>;
    
    /// Streaming completion for real-time observability.
    async fn stream_complete(&self, req: Request) -> anyhow::Result<BoxStream<'static, anyhow::Result<ResponseChunk>>>;
}

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
    /// Note: Interceptors like after_ai_call should be triggered by the caller
    /// once the stream is fully collected into a Response.
    pub async fn run_stream(&self, mut req: Request) -> anyhow::Result<BoxStream<'static, anyhow::Result<ResponseChunk>>> {
        // 1. Hook: before_ai_call (Context Optimization/Planning)
        for mw in &self.middlewares {
            mw.before_ai_call(&mut req).await?;
        }

        // 2. Execute AI reasoning (Streaming)
        self.provider.stream_complete(req).await
    }

    /// Helper for executed tool hooks (to be used by the Agent loop).
    pub async fn run_tool_hooks(&self, tool: &mut ToolCall, result: &mut String) -> anyhow::Result<()> {
        for mw in &self.middlewares {
            mw.before_tool_call(tool).await?;
        }
        
        // Tool execution happens in the Sandbox (Agent-level)
        
        for mw in &self.middlewares {
            mw.after_tool_call(result).await?;
        }
        Ok(())
    }
}
