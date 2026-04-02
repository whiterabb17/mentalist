use async_trait::async_trait;
use crate::{Request, Response, ModelProvider};

/// Native Anthropic Provider. Industry standard for Claude-based agents.
pub struct AnthropicProvider {
    pub api_key: String,
}

impl AnthropicProvider {
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }
}

#[async_trait]
impl ModelProvider for AnthropicProvider {
    async fn complete(&self, req: Request) -> anyhow::Result<Response> {
        // Implementation: Direct HTTP or SDK call to Anthropic Messages API
        // For the harness, we provide the template for production integration.
        Ok(Response {
            content: format!("Replied to: {}", req.prompt),
            tool_calls: Vec::new(),
        })
    }
}

/// Generic SDK Bridge to allow integration with existing libraries like langchain-rs.
pub struct SdkBridge<T: ModelProvider> {
    pub inner: T,
}

#[async_trait]
impl<T: ModelProvider> ModelProvider for SdkBridge<T> {
    async fn complete(&self, req: Request) -> anyhow::Result<Response> {
        self.inner.complete(req).await
    }
}
