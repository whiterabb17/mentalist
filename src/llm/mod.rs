use async_trait::async_trait;
pub use mem_core::{Request as LlmRequest, Response as LlmResponse, ResponseChunk, ToolCall, ToolCallDelta};

#[async_trait]
pub trait LLMProvider: Send + Sync {
    async fn generate(&self, req: LlmRequest) -> anyhow::Result<LlmResponse>;
    async fn generate_stream(&self, req: LlmRequest) -> anyhow::Result<futures_util::stream::BoxStream<'static, anyhow::Result<ResponseChunk>>>;
}

pub struct LLMRouter {
    pub providers: Vec<std::sync::Arc<dyn LLMProvider>>,
}

impl LLMRouter {
    pub fn new() -> Self {
        Self { providers: Vec::new() }
    }

    pub fn add_provider(&mut self, provider: std::sync::Arc<dyn LLMProvider>) {
        self.providers.push(provider);
    }
}

pub struct MindPalaceLLM {
    pub inner: std::sync::Arc<dyn mem_core::ModelProvider>,
}

impl MindPalaceLLM {
    pub fn new(inner: std::sync::Arc<dyn mem_core::ModelProvider>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl LLMProvider for MindPalaceLLM {
    async fn generate(&self, req: LlmRequest) -> anyhow::Result<LlmResponse> {
        Ok(self.inner.complete(req).await?)
    }

    async fn generate_stream(&self, req: LlmRequest) -> anyhow::Result<futures_util::stream::BoxStream<'static, anyhow::Result<ResponseChunk>>> {
        self.inner.stream_complete(req).await
    }
}
