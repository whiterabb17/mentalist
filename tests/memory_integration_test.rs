use mentalist::memory::{MemoryStore, MindPalaceMemory, MemoryEvent};
use std::sync::Arc;
use brain::Brain;
use mem_core::{MindPalaceConfig, OllamaProvider};

#[tokio::test]
async fn test_mindpalace_memory_adapter() {
    let brain = Arc::new(Brain::new(MindPalaceConfig::default(), None, None));
    let storage = mem_core::FileStorage::new(std::path::PathBuf::from("/tmp/mentalist_memory_test"));
    let ollama = Arc::new(OllamaProvider::new("http://localhost:11434".into(), "qwen2.5-coder:7b".into(), "".into(), None));
    let retriever = mem_retriever::MemoryRetriever::legacy(storage, ollama.clone(), ollama.clone());
    let adapter = MindPalaceMemory::new(brain, retriever);

    let event = MemoryEvent {
        content: "Significant event for test".into(),
        timestamp: 123456,
        metadata: serde_json::json!({ "type": "test_marker" }),
    };

    let res = adapter.store(event).await;
    assert!(res.is_ok());
}
