//! # Mentalist
//!
//! A production-grade agentic framework for building resilient, stateful, and sandboxed AI agents.

pub use mem_core::{Context, Request, Response, ResponseChunk, ToolCall, ToolCallDelta, ModelProvider};

pub mod provider;
pub mod executor;
pub mod mcp;
pub mod skills;
pub mod middleware;
pub mod agent;
pub mod config;
pub mod error;
pub use agent::{DeepAgent, DeepAgentState};

use std::sync::Arc;
use futures_util::stream::{BoxStream, StreamExt};

/// The central orchestration engine that manages the AI interaction lifecycle.
///
/// `Harness` wraps a `ModelProvider` and a sequence of `Middleware` hooks.
/// It is responsible for orchestrating the reasoning loop and tool execution callbacks.
pub struct Harness {
    pub provider: Arc<dyn ModelProvider>,
    pub middlewares: Vec<Arc<dyn middleware::Middleware>>,
    pub config: config::MentalistConfig,
}

impl Harness {
    pub fn new(provider: Arc<dyn ModelProvider>) -> Self {
        Self {
            provider,
            middlewares: Vec::new(),
            config: config::MentalistConfig::default(),
        }
    }

    pub fn with_config(mut self, config: config::MentalistConfig) -> Self {
        self.config = config;
        self
    }

    pub fn add_middleware(&mut self, middleware: Arc<dyn middleware::Middleware>) {
        self.middlewares.push(middleware);
        self.middlewares.sort_by_key(|mw| mw.priority());
    }

    /// Orchestrated Execution Loop following DeepAgent methodology.
    #[tracing::instrument(skip(self, req), fields(prompt_len = req.prompt.len()))]
    pub async fn run(&self, mut req: Request) -> crate::error::Result<Response> {
        // 1. Hook: before_ai_call (Context Optimization/Planning)
        for mw in &self.middlewares {
            if let Err(e) = mw.before_ai_call(&mut req).await {
                let mw_name = mw.name();
                if mw.is_critical() {
                    tracing::error!(middleware = mw_name, error = %e, "Critical middleware failure in before_ai_call");
                    return Err(crate::error::MentalistError::MiddlewareError {
                        middleware: mw_name.to_string(),
                        source: e,
                    });
                } else {
                    tracing::warn!(middleware = mw_name, error = %e, "Non-critical middleware failure in before_ai_call. Continuing.");
                }
            }
        }

        // 2. Execute AI reasoning
        let mut res = self.provider.complete(req).await
            .map_err(crate::error::MentalistError::ProviderError)?;

        // 3. Hook: after_ai_call (Response Parsing/Intent Extraction)
        for mw in &self.middlewares {
            if let Err(e) = mw.after_ai_call(&mut res).await {
                let mw_name = mw.name();
                if mw.is_critical() {
                    tracing::error!(middleware = mw_name, error = %e, "Critical middleware failure in after_ai_call");
                    return Err(crate::error::MentalistError::MiddlewareError {
                        middleware: mw_name.to_string(),
                        source: e,
                    });
                } else {
                    tracing::warn!(middleware = mw_name, error = %e, "Non-critical middleware failure in after_ai_call. Continuing.");
                }
            }
        }

        Ok(res)
    }

    /// Orchestrated Streaming Loop with Post-Hook Support.
    #[tracing::instrument(skip(self, req), fields(prompt_len = req.prompt.len()))]
    pub async fn run_stream(&self, mut req: Request) -> crate::error::Result<BoxStream<'static, crate::error::Result<ResponseChunk>>> {
        for mw in &self.middlewares {
            if let Err(e) = mw.before_ai_call(&mut req).await {
                let mw_name = mw.name();
                if mw.is_critical() {
                    tracing::error!(middleware = mw_name, error = %e, "Critical middleware failure in before_ai_call");
                    return Err(crate::error::MentalistError::MiddlewareError {
                        middleware: mw_name.to_string(),
                        source: e,
                    });
                } else {
                    tracing::warn!(middleware = mw_name, error = %e, "Non-critical middleware failure in before_ai_call. Continuing.");
                }
            }
        }

        // 2. Execute AI reasoning (Streaming)
        let inner_stream = self.provider.stream_complete(req).await
            .map_err(crate::error::MentalistError::ProviderError)?;
        let middlewares = self.middlewares.clone();

        // Wrap stream to apply post-hooks after completion
        let wrapped = async_stream::try_stream! {
            let mut full_response = Response { 
                content: String::new(), 
                tool_calls: Vec::new() 
            };
            
            futures_util::pin_mut!(inner_stream);
            while let Some(chunk_res) = inner_stream.next().await {
                let chunk = chunk_res.map_err(crate::error::MentalistError::ProviderError)?;
                
                // Accumulate content and tool calls for post-processing
                if let Some(ref content) = chunk.content_delta {
                    full_response.content.push_str(content);
                }
                
                yield chunk;
            }

            // 3. Hook: after_ai_call (Response Processing)
            for mw in &middlewares {
                if let Err(e) = mw.after_ai_call(&mut full_response).await {
                    tracing::error!("Middleware '{}' failure in after_ai_call (streaming): {}", mw.name(), e);
                    // We don't bail the stream here as it's already finished, but we log
                }
            }
        };

        Ok(Box::pin(wrapped))
    }

    /// Helper for executed tool hooks (before).
    #[tracing::instrument(skip(self, tool), fields(tool_name = tool.name))]
    pub async fn run_before_tool_hooks(&self, tool: &mut ToolCall) -> crate::error::Result<()> {
        for mw in &self.middlewares {
            if let Err(e) = mw.before_tool_call(tool).await {
                let mw_name = mw.name();
                if mw.is_critical() {
                    tracing::error!(middleware = mw_name, error = %e, "Critical middleware failure in before_tool_call");
                    return Err(crate::error::MentalistError::MiddlewareError {
                        middleware: mw_name.to_string(),
                        source: e,
                    });
                } else {
                    tracing::warn!(middleware = mw_name, error = %e, "Non-critical middleware failure in before_tool_call. Continuing.");
                }
            }
        }
        Ok(())
    }

    /// Helper for executed tool hooks (after).
    #[tracing::instrument(skip(self, tool, result), fields(tool_name = tool.name, result_len = result.len()))]
    pub async fn run_after_tool_hooks(&self, tool: &ToolCall, result: &mut String) -> crate::error::Result<()> {
        for mw in &self.middlewares {
            if let Err(e) = mw.after_tool_call(tool, result).await {
                let mw_name = mw.name();
                if mw.is_critical() {
                    tracing::error!(middleware = mw_name, error = %e, "Critical middleware failure in after_tool_call");
                    return Err(crate::error::MentalistError::MiddlewareError {
                        middleware: mw_name.to_string(),
                        source: e,
                    });
                } else {
                    tracing::warn!(middleware = mw_name, error = %e, "Non-critical middleware failure in after_tool_call. Continuing.");
                }
            }
        }
        Ok(())
    }

    /// Triggers manual context optimization/summarization across all middlewares.
    #[tracing::instrument(skip(self, ctx), fields(ctx_items = ctx.items.len()))]
    pub async fn optimize_context(&self, ctx: &mut Context) -> crate::error::Result<()> {
        for mw in &self.middlewares {
            if let Err(e) = mw.optimize_context(ctx).await {
                let mw_name = mw.name();
                if mw.is_critical() {
                    tracing::error!(middleware = mw_name, error = %e, "Critical middleware failure in optimize_context");
                    return Err(crate::error::MentalistError::MiddlewareError {
                        middleware: mw_name.to_string(),
                        source: e,
                    });
                } else {
                    tracing::warn!(middleware = mw_name, error = %e, "Non-critical middleware failure in optimize_context. Continuing.");
                }
            }
        }
        Ok(())
    }

    /// Gets a middleware by name.
    pub fn get_middleware_by_name(&self, name: &str) -> Option<Arc<dyn middleware::Middleware>> {
        self.middlewares.iter().find(|m| m.name() == name).cloned()
    }
}
