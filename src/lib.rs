// unused import removed
pub use mem_core::{Context, Request, Response, ResponseChunk, ToolCall, ToolCallDelta, ModelProvider};

pub mod provider;
pub mod executor;
pub mod middleware;
pub mod agent;
pub use agent::{DeepAgent, DeepAgentState};

use std::sync::Arc;
use futures_util::stream::{BoxStream, StreamExt};

pub struct Harness {
    pub provider: Box<dyn ModelProvider>,
    pub middlewares: Vec<Arc<dyn middleware::Middleware>>,
}

impl Harness {
    pub fn new(provider: Box<dyn ModelProvider>) -> Self {
        Self {
            provider,
            middlewares: Vec::new(),
        }
    }

    pub fn add_middleware(&mut self, middleware: Arc<dyn middleware::Middleware>) {
        self.middlewares.push(middleware);
    }

    /// Orchestrated Execution Loop following DeepAgent methodology.
    pub async fn run(&self, mut req: Request) -> anyhow::Result<Response> {
        // 1. Hook: before_ai_call (Context Optimization/Planning)
        for mw in &self.middlewares {
            if let Err(e) = mw.before_ai_call(&mut req).await {
                let _mw_name = "Middleware"; // Better: mw.name() if added to trait
                tracing::error!("Middleware failure in before_ai_call: {}", e);
                return Err(e.context(format!("Middleware failure in before_ai_call")));
            }
        }

        // 2. Execute AI reasoning
        let mut res = self.provider.complete(req).await?;

        // 3. Hook: after_ai_call (Response Parsing/Intent Extraction)
        for mw in &self.middlewares {
            if let Err(e) = mw.after_ai_call(&mut res).await {
                tracing::error!("Middleware failure in after_ai_call: {}", e);
                return Err(e.context(format!("Middleware failure in after_ai_call")));
            }
        }

        Ok(res)
    }

    /// Orchestrated Streaming Loop with Post-Hook Support.
    pub async fn run_stream(&self, mut req: Request) -> anyhow::Result<BoxStream<'static, anyhow::Result<ResponseChunk>>> {
        // 1. Hook: before_ai_call
        for mw in &self.middlewares {
            if let Err(e) = mw.before_ai_call(&mut req).await {
                tracing::error!("Middleware failure in before_ai_call: {}", e);
                return Err(e.context("Middleware failure in before_ai_call"));
            }
        }

        // 2. Execute AI reasoning (Streaming)
        let inner_stream = self.provider.stream_complete(req).await?;
        let middlewares = self.middlewares.clone();

        // Wrap stream to apply post-hooks after completion
        let wrapped = async_stream::try_stream! {
            let mut full_response = Response { 
                content: String::new(), 
                tool_calls: Vec::new() 
            };
            
            futures_util::pin_mut!(inner_stream);
            while let Some(chunk_res) = inner_stream.next().await {
                let chunk = chunk_res?;
                
                // Accumulate content and tool calls for post-processing
                if let Some(ref content) = chunk.content_delta {
                    full_response.content.push_str(content);
                }
                
                yield chunk;
            }

            // 3. Hook: after_ai_call (Response Processing)
            for mw in &middlewares {
                if let Err(e) = mw.after_ai_call(&mut full_response).await {
                    tracing::error!("Middleware failure in after_ai_call (streaming): {}", e);
                    // We don't bail the stream here as it's already finished, but we log
                }
            }
        };

        Ok(Box::pin(wrapped))
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

    /// Triggers manual context optimization/summarization across all middlewares.
    pub async fn optimize_context(&self, ctx: &mut Context) -> anyhow::Result<()> {
        for mw in &self.middlewares {
            mw.optimize_context(ctx).await?;
        }
        Ok(())
    }
}
