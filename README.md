# Mentalist: Cognitive AI Agent Runtime

Mentalist is a production-grade cognitive runtime for autonomous AI agents, powered by the **MindPalace** memory ecosystem. It implements a multi-phase cognitive loop designed for safety, scalability, and deep reasoning.

## 🚀 Key Features

*   **Multi-Phase Cognitive Loop**: Orchestrates `PLAN -> EXECUTE -> CRITIQUE -> STORE -> ADAPT` cycles for robust goal achievement.
*   **Parallel Execution Engine**: Uses a Directed Acyclic Graph (DAG) based `TaskGraph` to resolve independent tasks in parallel using `tokio`.
*   **Security Gates**: Capability-based security engine with tool allowlisting and prompt injection sanitization.
*   **MindPalace Integration**: Deeply integrated with the MindPalace 7-layer memory architecture for persistent context and fact-based reasoning.
*   **Unified Tool Interface**: Standardized `Tool` and `Skill` traits for seamless integration of MCP servers and internal logic.
*   **Observability**: Full execution traceability via `tracing`.

## 🏗 Architecture

Mentalist is organized into modular functional areas:

*   **`core`**: The `AgentRuntime` orchestrator.
*   **`cognition`**: Planning and Critique engines (powered by `mem-planner`).
*   **`execution`**: Parallel task resolution and dependency management.
*   **`memory`**: Unified adapter for MindPalace `Brain` and `Retriever`.
*   **`security`**: Policy-based guardrails and validation.
*   **`tools`**: Registry for local skills and MCP adapters.
*   **`llm`**: Provider-agnostic LLM interface.

## 🏁 Getting Started

### Prerequisites

*   Rust 1.75+
*   Ollama (for local LLM support)

### Run the Demo

```bash
cargo run --example full_system_demo
```

## 🧪 Testing

The suite includes hardened functional tests for the cognitive core:

```bash
cargo test
```

*   `test_runtime_multi_phase_reasoning`: Verifies state persistence and retry logic.
*   `test_parallel_executor_complex_dag`: Validates dependency resolution and parallel task execution.
*   `test_security_violation_caught`: Ensures policy enforcement.

## 📄 License

MIT
