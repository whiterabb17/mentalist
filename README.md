# Mentalist (v0.3.8): Cognitive AI Agent Runtime

![Rust](https://img.shields.io/badge/language-Rust-orange.svg) ![Status: Production-Ready](https://img.shields.io/badge/Status-Production--Ready-brightgreen.svg) ![Ecosystem: MindPalace](https://img.shields.io/badge/Ecosystem-MindPalace-magenta.svg)

**Mentalist** is a high-performance, production-grade cognitive runtime for autonomous AI agents. It acts as the orchestration layer for the **MindPalace** memory ecosystem, implementing a resilient multi-phase cognitive loop designed for high-agency tasks, safety, and deep reasoning.

---

## 🧠 The 5-Phase Cognitive Loop

Mentalist orchestrates agent behavior through a continuous state machine, ensuring every action is planned, executed, and validated:

1.  **PLAN**: Decomposes the high-level goal into a Directed Acyclic Graph (DAG) of actionable tasks using `mem-planner`.
2.  **EXECUTE**: Resolves the DAG in parallel, dispatching tool calls to the `ToolRegistry` with granular security monitoring.
3.  **CRITIQUE**: Evaluates the results of the execution phase against the original goal using an independent LLM or heuristic critic.
4.  **STORE**: Distills durable facts from the interaction and commits them to the MindPalace `FactGraph`.
5.  **ADAPT**: Adjusts the remaining plan based on the critic's feedback, identifying if a goal has been reached or if a retry with a new strategy is required.

---

## 🏰 Deep MindPalace Integration

Mentalist v0.3.8 is built on a "Defense-in-Depth" memory strategy. It handles the complexity of the MindPalace 7-layer pipeline through its **Middleware System**:

-   **Hardened Middleware**: The `MindPalaceMiddleware::hardened` constructor automatically initializes a `Brain` with all 7 layers (Identity, Offloading, Compaction, Extraction, etc.).
-   **Proactive Knowledge Injection**: During the `before_ai_call` hook, Mentalist queries the `MemoryRetriever` and injects high-precision RAG facts directly into the system context.
-   **Deductive Fact Learning**: Facts are extracted in real-time from three sources: User prompts, AI responses, and Tool execution results.
-   **Multi-Agent Coordination**: Built-in support for `mem-bridge` (context forking) and `mem-broker` (collective learning) via the runtime metadata.

---

## 🔮 Aggregator API (v0.3.8)

Mentalist serves as the primary entry point for the entire ecosystem, re-exporting core types to simplify agent development:

```rust
use mentalist::{
    AgentRuntime, RuntimeEvent, ExecutionLimits,
    MindPalacePlanner, MindPalaceMemory, SecurityEngine,
    MindPalaceLLM, ToolRegistry, Policy
};

// Foundational types from mem-core
use mentalist::{
    Context, MemoryItem, MemoryRole, 
    ModelProvider, EmbeddingProvider, TokenCounter
};
```

---

## 🚀 Runtime Integration Example

This example demonstrates initializing the Mentalist runtime with a **Hardened MindPalace Middleware** suite.

```rust
use std::sync::Arc;
use std::path::PathBuf;
use mentalist::{
    AgentRuntime, ExecutionLimits, MindPalacePlanner, 
    MindPalaceMemory, MindPalaceLLM, ToolRegistry, 
    SecurityEngine, DefaultCritic, Policy
};
use mentalist::middleware::{MindPalaceMiddleware, LoggingMiddleware};
use mem_core::{MindPalaceConfig, FileStorage, OllamaProvider};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. Providers & Storage
    let storage = FileStorage::new(PathBuf::from("./storage"));
    let llm = Arc::new(OllamaProvider::new(
        "http://localhost:11434".into(),
        "qwen2.5-coder:7b".into(),
        "mxbai-embed-large".into(),
        None,
    ));

    // 2. Setup Hardened MindPalace Middleware (7-Layer Memory)
    let mp_middleware = Arc::new(MindPalaceMiddleware::hardened(
        storage.clone(),
        llm.clone(),        // LlmClient
        llm.clone(),        // EmbeddingProvider
        llm.clone(),        // TokenCounter
        "session_001".into(),
        1024,                // Embedding Dimension
        PathBuf::from("./vault"),
        None,               // Use default config
    ));

    // 3. Runtime Construction
    let runtime = AgentRuntime {
        planner: Arc::new(MindPalacePlanner::new(llm.clone())),
        executor: Arc::new(mentalist::execution::executor::Executor::new(Arc::new(ToolRegistry::new()))),
        memory: Arc::new(MindPalaceMemory::new(mp_middleware.brain.clone(), mp_middleware.retriever.clone())),
        llm: Arc::new(MindPalaceLLM::new(llm.clone())),
        tools: Arc::new(ToolRegistry::new()),
        security: Arc::new(SecurityEngine::new(Policy::default())),
        critic: Arc::new(DefaultCritic),
        limits: ExecutionLimits { max_steps: 5, timeout_seconds: 300 },
        middlewares: vec![mp_middleware, Arc::new(LoggingMiddleware)],
    };

    // 4. Run Cognitive Loop
    let goal = "Analyze the project structure and suggest three improvements.";
    let result = runtime.run(goal, mem_core::Context::default(), None, None).await?;
    
    println!("Final Result: {}", result);
    Ok(())
}
```

---

*Powered by the [MindPalace](https://github.com/whiterabb17/mindpalace) Memory Architecture.*
