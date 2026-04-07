# Mentalist: Cognitive AI Agent Runtime

Mentalist is a production-grade cognitive runtime for autonomous AI agents, powered by the **MindPalace** memory ecosystem. It implements a multi-phase cognitive loop designed for safety, scalability, and deep reasoning.

## 🚀 Key Features

*   **Multi-Phase Cognitive Loop**: Orchestrates `PLAN -> EXECUTE -> CRITIQUE -> STORE -> ADAPT` cycles for robust goal achievement. Each phase is independently verifiable and retry-aware.
*   **Parallel Execution Engine**: Uses a Directed Acyclic Graph (DAG) based `TaskGraph` to resolve independent tasks in parallel using `tokio`, maximizing performance for complex agentic workflows.
*   **Security Gates**: Capability-based security engine (`SecurityEngine`) with granular tool allowlisting, prompt injection sanitization, and execution sandboxing.
*   **MindPalace Integration**: Deeply integrated with the MindPalace 7-layer memory architecture for persistent context, semantic fact-based reasoning, and long-term skill storage.
*   **Unified Tool Interface**: Standardized `Tool` and `Skill` traits for seamless integration of MCP (Model Context Protocol) servers and internal logic.
*   **Real-time Observability**: Stream core execution events via `RuntimeEvent` for TUI/Web UI monitoring, with full tracing support.

## 🏗 Architecture

Mentalist is organized into modular functional areas:

*   **`core`**: The `AgentRuntime` orchestrator managing the cognitive loop.
*   **`cognition`**: Planning and Critique engines powered by the `mem-planner` crate.
*   **`execution`**: The `TaskGraph` engine for dependency resolution and `MultiExecutor` for tool dispatch.
*   **`memory`**: Unified adapter for MindPalace `Brain`, `Retriever`, and `MindPalaceMemory` implementations.
*   **`security`**: Policy-based guardrails, capability management, and sanitization logic.
*   **`tools`**: Registry and adapters for MCP servers (filesystem, firecrawl, duckduckgo, etc.).
*   **`llm`**: Provider-agnostic LLM interface supporting Anthropic, OpenAI, Gemini, and Ollama.

## 🔮 Standardized API (Aggregator)

Mentalist v0.3.5 serves as a comprehensive aggregator for MindPalace crates, simplifying the dependency stack for downstream agents like **Gypsy**.

### Common Imports
```rust
use mentalist::{
    AgentRuntime, RuntimeEvent, ExecutionLimits,
    MindPalacePlanner, MindPalaceMemory, SecurityEngine,
    MindPalaceLLM, ToolRegistry, Policy
};

// Re-exports from mem-core
use mentalist::{ModelProvider, EmbeddingProvider, TokenCounter, Context};
```

### Compatibility Bridge (v0.3.3 -> v0.3.5)
To assist with migrations, Mentalist includes a compatibility layer for legacy patterns:
- `mentalist::executor::MultiExecutor`: Wraps the new `ToolRegistry`.
- `mentalist::DeepAgentState`: Legacy alias for `AgentState`.
- `mentalist::mcp::McpExecutor`: Re-export for standard MCP adapters.

## 🏁 Execution Observability

The `AgentRuntime::run` method accepts an optional `UnboundedSender<RuntimeEvent>` to provide real-time updates:

- `RuntimeEvent::Status(String)`: High-level phase updates (e.g., "Planning...", "Executing...").
- `RuntimeEvent::TextChunk(String)`: Incremental output from the LLM.
- `RuntimeEvent::ToolStarted(String)`: Notification that a tool execution has begun.
- `RuntimeEvent::ToolFinished(String, String, bool)`: Results and success status of tool calls.
- `RuntimeEvent::MetricUpdate`: Latency and token utilization statistics per step.

## 📦 Usage

Add Mentalist to your `Cargo.toml`:

```toml
[dependencies]
mentalist = { git = "https://github.com/whiterabb17/mentalist.git", version = "0.3.5" }
```

### Initializing the Runtime
```rust
let runtime = Arc::new(AgentRuntime {
    planner: Arc::new(MindPalacePlanner::new(planner_engine)),
    executor: Arc::new(Executor::new(registry)),
    memory: Arc::new(MindPalaceMemory::new(brain, retriever)),
    llm: Arc::new(MindPalaceLLM::new(provider)),
    tools: registry,
    security: Arc::new(SecurityEngine::new(Policy::default())),
    critic: Arc::new(DefaultCritic),
    limits: ExecutionLimits::default(),
});
```

## 🏁 Getting Started

### Prerequisites

*   Rust 1.75+
*   Ollama (for local LLM support)

### Run the Demo

```bash
cargo run --example full_system_demo
```

## 🧪 Testing

```bash
cargo test
```

## 📄 License

MIT
