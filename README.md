# Mentalist (v0.3.1)

The **Mentalist** is a high-performance, production-grade execution environment for autonomous agents in Rust. It implements the **Agent = Model + Harness** paradigm, providing the infrastructure to make LLM-based agents reliable, stateful, and context-aware through the integrated **MindPalace** memory ecosystem.

## 🚀 Key Pillars

### 1. Hardened & Secure Execution Lifecycle
The harness provides a strict Interceptor pattern integrated with the resilient MindPalace pipeline. Every turn is protected by:
- **`CommandValidator`**: Multi-layer security including command whitelisting, shell injection protection, and strict path traversal guards, now with runtime execution limits.
- **`is_critical` Policy**: Enhanced middleware safety where non-critical failures (like logging or planning) do not abort the core agent reasoning loop.
- **Structured Error Handling**: Categorized tool errors (Transient, Permission Denied, Tool Not Found, Security Violation) with exponential backoff retry logic, wrapped in a unified `MentalistError` framework.

### 2. Resilience & Stability Pillar
The **DeepAgent** handles all state lifecycle through the **ResilientMemoryController**.
- **JSON Auto-Fixer**: Heuristic recovery for malformed LLM tool arguments (balances unclosed braces and fixes trailing commas).
- **Context Management**: Automatic context optimization and compaction triggered when history reaches configurable bounds (default 50 items).
- **Infinite Cycle Protection**: Built-in guardrails against cyclical tool patterns (max 20 calls per turn) to prevent resource exhaustion.
- **Thread-Safe Persistence**: Mutex-guarded resilient state saving using atomic temp-file swaps.

### 3. Explicit Planning (TODO.md)
Adopts the "Planning" pillar. The `TodoMiddleware` ensures the agent maintains objective-coherence by automatically injecting and persisting a stateful `TODO.md` file, with hardened IO error handling for reliability.

### 4. Sandboxed Isolation
Includes a `SandboxedExecutor` for high-security tool execution:
- **Wasmtime (Natively Secure)**: Capability-based security for running tools like Python via Wasm with strict filesystem mounting.
- **Docker (Native `bollard`)**: Full container isolation with verified resource limits (CPU/Memory memory validation post-creation).
- **POSIX Permission Security**: Execution-bit validation for skill scripts to ensure diagnostic clarity.

### 5. Multi-Protocol Tool Support
Exposes a flexible executor architecture:
- **MCP (Model Context Protocol)**: Direct integration with domestic and remote MCP servers via the `McpExecutor`. Supports standard-compliant `initialize` handshakes and JSON-RPC.
- **Skills System**: A filesystem-based tool discovery system with validated script execution (Python, JS, Shell).
- **MultiExecutor & Dynamic Loading**: Advanced tool routing with registration collision detection. The `DynamicExecutorLoader` enables seamless instantiation from a unified `MentalistConfig`.
- **Sensitive Data Redaction**: Automatic masking of `api_key`, `token`, and `secret` values in diagnostic logs.

## 🛠️ Usage Example (v1.0.0)

Integrated with the hardened MindPalace memory architecture:

```rust
use mentalist::{Harness, DeepAgent, DeepAgentState, StepConfig};
use mentalist::executor::{SandboxedExecutor, ExecutionMode};
use mem_resilience::{ResilientMemoryController, CircuitBreaker};
use brain::Brain;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. Initialize the Hardened Memory Brain
    let brain = Arc::new(Brain::new(mem_core::MindPalaceConfig::default(), None, None));
    
    // 2. Setup the Resilience Controller
    let controller = Arc::new(ResilientMemoryController::new(
        brain, 
        storage, 
        5 // failure threshold
    ));

    // 3. Initialize the DeepAgent with Sandboxed Execution
    let executor = Arc::new(SandboxedExecutor::new(
        ExecutionMode::Docker { image: "python:3.10-slim".into() },
        PathBuf::from("./workdir"),
        None
    )?);
    
    let config = AgentConfig {
        max_turns: 15,
        max_tool_calls_per_turn: 10,
        ..Default::default()
    };
    
    let mut agent = DeepAgent::new(
        harness, 
        state, 
        executor, 
        controller,
        None, // scheduler
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
