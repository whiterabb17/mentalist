use clap::Parser;
use colored::*;
use mentalist::{Harness, Request, Response, ResponseChunk, DeepAgent, DeepAgentState, ModelProvider};
use mentalist::middleware::{MindPalaceMiddleware, todo::TodoMiddleware};
// removed unused brain::Brain
use mem_core::{Context, FileStorage, EmbeddingProvider, LlmClient, TokenCounter};
use mem_resilience::ResilientMemoryController;
use std::sync::Arc;
// removed unused std::path::PathBuf
use anyhow::Result;
use async_trait::async_trait;
// removed unused sleep, Duration
use futures_util::stream::BoxStream;

#[derive(Parser)]
struct Args {
    #[arg(short, long)]
    interactive: bool,
}

pub struct MockProvider;

#[async_trait]
impl ModelProvider for MockProvider {
    async fn complete(&self, req: Request) -> Result<Response> {
        Ok(Response { content: format!("Mock result for: {}", req.prompt), tool_calls: vec![] })
    }

    async fn stream_complete(&self, _req: Request) -> Result<BoxStream<'static, Result<ResponseChunk>>> {
        let stream = async_stream::try_stream! {
            yield ResponseChunk { 
                content_delta: Some("Streaming... ".to_string()), 
                tool_call_delta: None, 
                usage: None,
                is_final: false 
            };
            yield ResponseChunk { 
                content_delta: Some("Done!".to_string()), 
                tool_call_delta: None, 
                usage: None,
                is_final: true 
            };
        };
        Ok(Box::pin(stream))
    }
}

#[async_trait]
impl LlmClient for MockProvider {
    async fn completion(&self, _prompt: &str) -> Result<String> {
        Ok("Mock completion result.".to_string())
    }
}

pub struct MockEmbeddingProvider;

#[async_trait]
impl EmbeddingProvider for MockEmbeddingProvider {
    async fn embed(&self, _text: &str) -> Result<Vec<f32>> {
        Ok(vec![0.0; 384]) // Mocked 384-dim vector
    }
}

pub struct MockTokenCounter;

impl TokenCounter for MockTokenCounter {
    fn count_tokens(&self, text: &str) -> usize {
        text.len() / 4
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let _args = Args::parse();
    
    println!("\n{}", "===================================================".bold().cyan());
    println!("{}", "   🧠 MINDPALACE & MENTALIST: 7-LAYER DEMO 🧠   ".bold().cyan());
    println!("{}\n", "===================================================".bold().cyan());
    
    // 0. Infrastructure Setup
    let storage_root = std::env::current_dir()?.join(".agent/storage");
    let storage = FileStorage::new(storage_root.clone());
    let mock_llm = Arc::new(MockProvider);
    let mock_embeddings = Arc::new(MockEmbeddingProvider);
    let mock_tokens = Arc::new(MockTokenCounter);
    let session_id = "demo_sota_001".to_string();

    // 1. Initialize SOTA Hardened Middleware (7-Layers!)
    let mp_middleware = MindPalaceMiddleware::hardened(
        storage.clone(),
        mock_llm.clone(),
        mock_embeddings.clone(),
        mock_tokens.clone(),
        session_id.clone()
    );

    // 2. Resilience Setup
    let brain = Arc::clone(&mp_middleware.brain);
    let memory_controller = Arc::new(ResilientMemoryController::new(
        brain,
        storage.clone(),
        3 // Failure threshold
    ));

    // 3. Harness & Middlewares
    let mut harness = Harness::new(Box::new(MockProvider));
    harness.add_middleware(Box::new(mp_middleware));
    harness.add_middleware(Box::new(TodoMiddleware::new(std::env::current_dir()?.join(".agent/todo.md"))));
    
    // 4. Initial Agent Configuration
    let state = DeepAgentState {
        session_id,
        context: Context { items: vec![] },
        sandbox_root: std::env::current_dir()?,
    };
    
    let executor = mentalist::executor::SandboxedExecutor::new(
        mentalist::executor::ExecutionMode::Local,
        std::env::current_dir()?,
        None
    );

    let mut agent = DeepAgent::new(harness, state, executor, memory_controller);

    println!("{}", ">>> Starting Hardened Verification Loop...".bold().white());
    
    let script = vec![
        "Analyze the project structure and remember we use 7-layer memory.", // Should trigger Reflection
        "Update the plan for total integration.",
    ];

    for (i, prompt) in script.into_iter().enumerate() {
        println!("\n{}: {}", format!("TURN {}", i + 1).on_blue().white().bold(), prompt.bold());
        let response = agent.step(prompt.to_string()).await?;
        println!("{}: {}", " 🤖 Agent Response".bold().cyan(), response);
    }

    println!("\n{}", "===================================================".bold().green());
    println!("{}", "   ✅ HARDENED SOTA DEMO COMPLETED SUCCESSFULLY ✅   ".bold().green());
    println!("{}\n", "===================================================".bold().green());
    
    Ok(())
}
