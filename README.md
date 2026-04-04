# Mentalist (v0.2.7)

The **Mentalist** is a high-performance, production-ready execution environment for autonomous agents in Rust. It implements the **Agent = Model + Harness** paradigm, providing the infrastructure to make LLM-based agents reliable, stateful, and context-aware through the integrated **MindPalace** memory ecosystem.

## 🚀 Key Pillars

### 1. Hardened Execution Lifecycle
The harness provides a strict Interceptor pattern integrated with the resilient MindPalace pipeline. Every turn is protected by:
- **`before_ai_call`**: Triggers **Layer 6 (HNSW Retrieval)** and **Layer 4 (Structural Compaction)** to optimize the prompt.
- **`after_tool_call`**: Triggers **Layer 5 (Fact Extraction)** into the **RuVector-Graph** and ensures **Layer 1 (Offloading)**.

### 2. Resilience Pillar
The **DeepAgent** handles all state saves through the **ResilientMemoryController**. This ensures:
- **Circuit Breaker Coverage**: LLM errors or storage latencies are isolated.
- **Safe Persistence**: Integrated JSON checkpointing before and after heavy task execution.

### 3. Explicit Planning (TODO.md)
Adopts the "Planning" pillar. The `TodoMiddleware` ensures the agent maintains objective-coherence by automatically injecting and persisting a stateful `TODO.md` file.

### 4. Native & Docker Isolation
Includes a `SandboxedExecutor` for high-security tool execution:
- **Wasmtime (Natively Secure)**: Capability-based security for running tools like Python via Wasm.
- **Docker (Native `bollard`)**: Full container isolation with auto-pull support.

## 🛠️ Usage Example (v1.0.0)

Integrated with the hardened MindPalace memory architecture:

```rust
use mentalist::{Harness, DeepAgent, DeepAgentState};
use mentalist::executor::{SandboxedExecutor, ExecutionMode};
use mem_resilience::{ResilientMemoryController, CircuitBreaker};
use brain::Brain;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. Initialize the Hardened Memory Brain
    let mut brain = Brain::new(Some(metrics), Some(token_counter));
    brain.add_layer(Arc::new(FactExtractor::new(...)));
    
    // 2. Setup the Resilience Controller
    let controller = Arc::new(ResilientMemoryController::new(
        Arc::new(brain), 
        storage, 
        5 // failure threshold
    ));

    // 3. Initialize the DeepAgent with Sandboxed Execution
    let executor = SandboxedExecutor::new(
        ExecutionMode::Docker { image: "python:3.10-slim".into() },
        PathBuf::from("./workdir")
    );
    
    let mut agent = DeepAgent::new(
        harness, 
        state, 
        executor, 
        controller
    );
    
    // 4. Integrated Step with Resilience & Planning
    let response = agent.step("Initiate project architecture audit").await?;
    println!("Agent Analysis: {}", response);
    
    Ok(())
}
```

## 📂 Repository Structure

- `mentalist`: High-level agent harness and executor.
- `mindpalace`: 7-layer memory ecosystem (HNSW, GraphDB, Adaptive TTL).

---

*Part of the MindPalace Agent Memory ecosystem.*
