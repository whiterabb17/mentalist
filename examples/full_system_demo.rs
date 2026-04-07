use mentalist::core::{AgentRuntime, ExecutionLimits};
use mentalist::cognition::MindPalacePlanner;
use mentalist::execution::executor::Executor;
use mentalist::memory::MindPalaceMemory;
use mentalist::tools::ToolRegistry;
use mentalist::security::{SecurityEngine, Policy};
use mentalist::llm::MindPalaceLLM;
use mentalist::telemetry::init_telemetry;
use std::sync::Arc;
use brain::Brain;
use mem_core::{MindPalaceConfig, Context};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. Initialize Telemetry
    init_telemetry();
    tracing::info!("Starting Mentalist v0.3.3 Demo");

    // 2. Setup LLM Provider (via Ollama)
    let ollama = Arc::new(mem_core::OllamaProvider::new("http://localhost:11434".into(), "qwen2.5-coder:7b".into(), "".into(), None));
    let llm = Arc::new(MindPalaceLLM::new(ollama.clone()));

    // 3. Setup MindPalace Memory Backend
    let brain = Arc::new(Brain::new(MindPalaceConfig::default(), None, None));
    let storage = mem_core::FileStorage::new(std::path::PathBuf::from(".mindpalace"));
    let retriever = mem_retriever::MemoryRetriever::legacy(storage, ollama.clone(), ollama.clone());
    let memory = Arc::new(MindPalaceMemory::new(brain.clone(), retriever));

    // 4. Setup Tools & Security
    let tools = Arc::new(ToolRegistry::new());
    let security = Arc::new(SecurityEngine::new(Policy {
        allowed_capabilities: vec![],
        tool_allowlist: vec![],
    }));

    // 5. Build Cognitive Core
    let planner = Arc::new(MindPalacePlanner::new(Arc::new(mem_planner::LlmPlanner::new(ollama))));
    let executor = Arc::new(Executor::new(Arc::clone(&tools)));
    let critic = Arc::new(mentalist::cognition::DefaultCritic);

    let runtime = AgentRuntime {
        planner,
        executor,
        memory,
        llm,
        tools,
        security,
        critic,
        limits: ExecutionLimits { max_steps: 10, timeout_seconds: 600 },
    };

    // 6. Run Mission
    let goal = "Analyze the repository for modularity gaps and propose a refactoring plan.";
    let result = runtime.run(goal, Context::default(), None).await?;

    println!("--- MISSION COMPLETE ---");
    println!("Final Result: {}", result);

    Ok(())
}
