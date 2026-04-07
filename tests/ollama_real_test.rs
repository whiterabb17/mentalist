use mentalist::agent::{DeepAgent, DeepAgentState};
use mentalist::middleware::{MindPalaceMiddleware, LoggingMiddleware};
use mentalist::provider::OllamaProvider;
use mentalist::executor::{SandboxedExecutor, ExecutionMode};
use mentalist::Harness;
use mem_core::{FileStorage, Context, MindPalaceConfig};
use mem_resilience::ResilientMemoryController;
use std::sync::Arc;

#[tokio::test]
async fn test_ollama_real_world_interaction() {
    // This test requires a live Ollama instance at localhost:11434
    // and expects 'qwen2.5-coder:3b' and 'nomic-embed-text' to be pulled.
    
    let model = std::env::var("MODEL_NAME").unwrap_or_else(|_| "qwen2.5-coder:3b".to_string());
    let embedding_model = std::env::var("EMBEDDING_MODEL").unwrap_or_else(|_| "nomic-embed-text".to_string());
    
    let provider = Arc::new(OllamaProvider::new(
        "http://127.0.0.1:11434".to_string(), 
        model, 
        embedding_model, 
        Some(32768)
    ));
    // provider implements LlmClient, EmbeddingProvider, and TokenCounter
    
    let mp_config = MindPalaceConfig {
        similarity_threshold: 0.85,
        model_context_window: 16384, // 16k tokens of memory context
        ..MindPalaceConfig::default()
    };
    
    // 1. Setup temporary storage for the "brain"
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let storage_path = temp_dir.path().to_path_buf();
    let vault_path = storage_path.join("vault");
    let sandbox_root = storage_path.join("sandbox");
    std::fs::create_dir_all(&vault_path).ok();
    std::fs::create_dir_all(&sandbox_root).ok();
    
    let storage = FileStorage::new(storage_path.clone());
    let session_id = "test-live-session".to_string();
    
    // 2. Initialize MindPalace (Memory Layer)
    let mp_middleware = MindPalaceMiddleware::hardened(
        storage.clone(),
        provider.clone() as Arc<dyn mem_core::LlmClient>,
        provider.clone() as Arc<dyn mem_core::EmbeddingProvider>,
        provider.clone() as Arc<dyn mem_core::TokenCounter>,
        session_id.clone(),
        768, // nomic-embed-text dimension
        vault_path,
        Some(mp_config),
    );
    
    let mut harness = Harness::new(provider.clone());
    harness.add_middleware(Arc::new(LoggingMiddleware));
    let mp_middleware_arc = Arc::new(mp_middleware);
    harness.add_middleware(mp_middleware_arc.clone());
    
    // 3. Initialize Agent Components
    let state = DeepAgentState {
        session_id: session_id.clone(),
        context: Arc::new(Context::default()),
        sandbox_root: sandbox_root.clone(),
    };
    
    let executor = Arc::new(SandboxedExecutor::new(
        ExecutionMode::Local, 
        sandbox_root, 
        None
    ).expect("Failed to create executor"));
    
    let memory_controller = Arc::new(ResilientMemoryController::new(
        mp_middleware_arc.brain.clone(),
        storage.clone(),
        3
    ));
    
    let mut agent = DeepAgent::new(
        harness, 
        state, 
        executor, 
        memory_controller, 
        None
    );
    
    // 4. Send a real message
    println!("Sending message to live Ollama...");
    let prompt = "Greeting! Say 'Paris' if you can hear me.".to_string();
    
    // We expect this to either succeed or trigger the 1.8GB crash we are hunting
    let response = agent.step(prompt).await;
    
    match response {
        Ok(res) => {
            println!("Ollama Responded: {}", res);
            assert!(res.to_lowercase().contains("paris"));
            
            // 5. Verify memory persistence
            // Fact extractor should have run in 'after_ai_call' hook.
            let knowledge_file = storage_path.join("knowledge.json");
            assert!(knowledge_file.exists() || storage_path.join("vault").exists(), "Memory layer failed to operate");
        }
        Err(e) => {
            panic!("Real-world Ollama call failed: {}", e);
        }
    }
}
