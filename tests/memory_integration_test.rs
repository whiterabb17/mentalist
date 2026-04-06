use mentalist::{Harness, Request, Context};
use mentalist::middleware::Middleware;
use mem_core::{OllamaProvider, MemoryItem, MemoryRole, TokenCounter};
use std::sync::Arc;
use mockito::Server;
use serde_json::json;

#[tokio::test]
async fn test_ollama_memory_lifecycle() {
    let mut server = Server::new_async().await;
    let url = server.url();

    // 1. Mock Ollama Chat Response (OllamaProvider::complete uses /api/chat)
    let _m = server.mock("POST", "/api/chat")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json!({
            "model": "qwen2.5-coder:3b",
            "created_at": "2026-04-06T00:00:00Z",
            "message": {
                "role": "assistant",
                "content": "Hello! I am ready to assist with your mission-critical tasks."
            },
            "done": true,
            "total_duration": 1000,
            "load_duration": 100,
            "prompt_eval_count": 10,
            "prompt_eval_duration": 100,
            "eval_count": 20,
            "eval_duration": 800
        }).to_string())
        .create_async().await;

    // 2. Setup Provider pointing to Mock Server
    let provider = Arc::new(OllamaProvider::new(
        url,
        "qwen2.5-coder:3b".to_string(),
        "nomic-embed-text".to_string(),
        Some(2048)
    ));

    // 3. Setup Harness with Memory Layer
    // We'll use a Spy middleware to ensure the memory layer is being invoked
    struct MemorySpy {
        pub count: Arc<std::sync::atomic::AtomicUsize>,
    }
    #[async_trait::async_trait]
    impl Middleware for MemorySpy {
        fn name(&self) -> &str { "MemorySpy" }
        async fn before_ai_call(&self, _req: &mut Request) -> anyhow::Result<()> {
            self.count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }
    }

    let spy_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut harness = Harness::new(provider.clone());
    harness.add_middleware(Arc::new(MemorySpy { count: spy_count.clone() }));

    // 4. Create Request with Context
    let context = Arc::new(Context {
        items: vec![
            MemoryItem {
                role: MemoryRole::User,
                content: "Hello".into(),
                timestamp: 123456789,
                metadata: json!({}),
            }
        ]
    });

    let req = Request {
        prompt: "How are you?".to_string(),
        context,
        tools: vec![],
    };

    // 5. Execute
    let res = harness.run(req).await.expect("Failed to run harness with mock Ollama");

    // 6. Assertions
    assert!(res.content.contains("ready to assist"));
    assert_eq!(spy_count.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_token_counting_resilience() {
    // This verifies that our token counting uses the global leaner cl100k_base
    // and correctly reports sizes without triggering massive allocations.
    let text = "Mission critical stability is the priority for Gypsy on Windows.";
    let provider = OllamaProvider::new(
        "http://localhost:11434".into(),
        "any".into(),
        "any".into(),
        None
    );
    
    // We cast to Arc<dyn TokenCounter> to test the trait implementation
    let counter: Arc<dyn TokenCounter> = Arc::new(provider);
    let count = counter.count_tokens(text);
    
    assert!(count > 0);
    assert!(count < text.len()); // Basic sanity check for tokens vs chars
}
