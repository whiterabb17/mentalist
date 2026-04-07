use async_trait::async_trait;
use mem_core::Context;
use std::sync::Arc;
use brain::Brain;

/// Persistent memory event for the store.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MemoryEvent {
    pub content: String,
    pub timestamp: u64,
    pub metadata: serde_json::Value,
}

/// Query for recalling memories.
pub struct MemoryQuery {
    pub text: String,
    pub limit: usize,
}

#[async_trait]
pub trait MemoryStore: Send + Sync {
    async fn store(&self, event: MemoryEvent) -> anyhow::Result<()>;
    async fn recall(&self, query: MemoryQuery) -> anyhow::Result<Vec<MemoryEvent>>;
    async fn summarize(&self, ctx: &mut Context) -> anyhow::Result<String>;
}

pub struct MindPalaceMemory {
    pub brain: Arc<Brain>,
    pub retriever: mem_retriever::MemoryRetriever<mem_core::FileStorage>,
}

impl MindPalaceMemory {
    pub fn new(brain: Arc<Brain>, retriever: mem_retriever::MemoryRetriever<mem_core::FileStorage>) -> Self {
        Self { brain, retriever }
    }
}

#[async_trait]
impl MemoryStore for MindPalaceMemory {
    async fn store(&self, event: MemoryEvent) -> anyhow::Result<()> {
        // Logic to ingest into mindpalace
        tracing::debug!("Storing event in MindPalace: {}", event.content);
        Ok(())
    }

    async fn recall(&self, query: MemoryQuery) -> anyhow::Result<Vec<MemoryEvent>> {
        let facts = self.retriever.retrieve_relevant_facts(&query.text, query.limit, None).await?;
        Ok(facts.into_iter().map(|(f, _)| MemoryEvent {
            content: f.content,
            timestamp: f.timestamp,
            metadata: serde_json::json!({"category": f.category}),
        }).collect())
    }

    async fn summarize(&self, ctx: &mut Context) -> anyhow::Result<String> {
        self.brain.optimize(ctx).await?;
        Ok("Context optimized".into())
    }
}
