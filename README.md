# Mentalist (DeepAgent Middleware)

![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg) ![Rust](https://img.shields.io/badge/language-Rust-orange.svg) ![Status: Active](https://img.shields.io/badge/Status-Active-brightgreen.svg) ![Methodology: DeepAgent](https://img.shields.io/badge/Methodology-DeepAgent-blue.svg)

![Mentalist & MindPalace Demo](./examples/mentalist_mindpalace.gif)

The **Mentalist** is a high-performance, production-ready execution environment for autonomous agents in Rust. It implements the **Agent = Model + Harness** paradigm from the **DeepAgent methodology**, providing the necessary infrastructure to make LLM-based agents reliable, stateful, and secure.

## 🚀 Key Pillars

### 1. Execution Lifecycle Hooks (Interceptors)
The harness provides a strict "Interceptor" pattern through four primary hooks:
- **`before_ai_call`**: Optimize the prompt context (integrated with MindPalace) and inject current planning state.
- **`after_ai_call`**: Distill intent and capture model discoveries before processing tool calls.
- **`before_tool_call`**: A security gate that performs sandbox validation and resource checks.
- **`after_tool_call`**: Updates the context with tool results and triggers long-term fact extraction.

### 2. Explicit Planning (TODO.md)
Adopts the DeepAgent "Planning" pillar. The `TodoMiddleware` ensures the agent maintains objective-coherence by automatically injecting and persisting a stateful `TODO.md` file throughout the session.

### 3. Native Docker Isolation
Includes a `SandboxedExecutor` using the **native `bollard` Rust SDK** for high-security isolation:
- **Local Restricted Shell**: Environment variable stripping and directory-level isolation.
- **Docker Isolation**: (High-security) Wraps tool calls in isolated containers using a direct connection to the Docker daemon. Supports **Auto-Pull** of missing images for seamless UX.

### 4. Full Session Serialization
Enables **State Recovery**. The entire `DeepAgent` state (Plan + History + Knowledge) is serializable to JSON, allowing agents to be stopped and resumed across restarts.

## 🛠️ Usage Example

```rust
use mentalist::{Harness, Request, DeepAgent, DeepAgentState};
use mentalist::executor::{SandboxedExecutor, ExecutionMode};
use mentalist::middleware::{MindPalaceMiddleware, todo::TodoMiddleware};
use std::sync::Arc;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. Initialize the 7-Layer Brain (MindPalace)
    let brain = Arc::new(Brain::default());
    
    // 2. Configure the Sandboxed Executor (Wasm mode - Native & Secure)
    let executor = SandboxedExecutor::new(
        ExecutionMode::Wasm { 
            module_path: "./tools/python.wasm".into(), 
            mount_root: true 
        },
        PathBuf::from("./")
    );

    // 3. Setup the Harness and Model Provider
    let provider = Box::new(MockProvider); // Implement ModelProvider
    let mut harness = Harness::new(provider);
    
    // 4. Add Middlewares (Memory + Planning)
    harness.add_middleware(Box::new(MindPalaceMiddleware::new(brain)));
    harness.add_middleware(Box::new(TodoMiddleware::new(".agent/todo.md".into())));
    
    // 5. Initialize the DeepAgent
    let mut agent = DeepAgent::new(harness, DeepAgentState::default(), executor);
    
    // 6. Run a turn
    let response = agent.step("Analyze the project structure".to_string()).await?;
    println!("Agent: {}", response);
    
    Ok(())
}
```

### 🛡️ Sandboxed Tool Execution

DeepAgent implements a "Deny-by-Default" execution model. Tools run in an isolated environment where they cannot access host secrets or the entire filesystem.

#### 🌐 Cross-Platform Native (Wasm) - Recommended
Uses **Wasmtime** to run tool modules with capability-based security. This is the fastest and most portable way to run tools without Docker.

```rust
ExecutionMode::Wasm { 
    module_path: "./tools/python.wasm".into(), 
    mount_root: true 
}
```

#### 🐳 High-Security Isolation (Docker)
Uses a native connection to the Docker daemon via `bollard`. Best for complex tool environments or legacy code.

```rust
ExecutionMode::Docker { 
    image: "python:3.11-slim".into() 
}
```

#### 🐍 Running Python via Wasm (No Docker)

To run Python tools natively and securely on any platform (Windows, Linux, macOS), you can use a `python.wasm` build:

1. **Download a WASI-compatible Python**: e.g., from [VMware's Wasm Labs](https://github.com/vmware-labs/webassembly-language-runtimes) or the official CPython Wasm builds.
2. **Place in tools/**: Save the module as `./tools/python.wasm`.
3. **Configure the Executor**: Map your project root to `/sandbox` for safe access.
4. **Concrete Usage Example**:

```rust
let executor = SandboxedExecutor::new(
    ExecutionMode::Wasm { 
        module_path: "./tools/python.wasm".into(), 
        mount_root: true 
    },
    PathBuf::from("./")
);

// Invoke a script inside the sandbox
let output = executor.execute("python", vec![
    "-c".to_string(), 
    "print('Hello from the Sandbox!')".to_string()
]).await?;
```

Take a look at [examples/python_wasm_demo.rs](./examples/python_wasm_demo.rs) for a complete, runnable example.

- **`provider`**: Traits and adapters for native LLMs (Anthropic/OpenAI) and SDK bridges.
- **`executor`**: Security-first tool execution (Sandboxing).
- **`middleware`**: Stateful logic for Planning, Memory, and Safety.
- **`agent`**: High-level orchestrator with session recovery.

---

*Part of the MindPalace Agent Memory ecosystem.*
