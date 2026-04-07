use async_trait::async_trait;
use crate::llm::{LLMProvider, LlmRequest, LlmResponse, ResponseChunk};
use mem_core::ModelProvider;
use std::sync::Arc;
use futures_util::stream::BoxStream;

pub struct MindPalaceLLM {
    pub provider: Arc<dyn ModelProvider>,
}

impl MindPalaceLLM {
    pub fn new(provider: Arc<dyn ModelProvider>) -> Self {
        Self { provider }
    }
}

#[async_trait]
impl LLMProvider for MindPalaceLLM {
    async fn generate(&self, req: LlmRequest) -> anyhow::Result<LlmResponse> {
        let res = self.provider.complete(req).await?;
        Ok(LlmResponse {
            content: res.content,
            tool_calls: res.tool_calls.into_iter().map(|tc| mem_core::ToolCall {
                name: tc.name,
                arguments: tc.arguments,
            }).collect(),
        })
    }

    async fn generate_stream(&self, req: LlmRequest) -> anyhow::Result<BoxStream<'static, anyhow::Result<ResponseChunk>>> {
        self.provider.stream_complete(req).await
    }
}
