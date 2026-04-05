# API Reference

The `mentalist` workspace follows highly decoupled async patterns. Here are the core structures natively mapping boundaries strictly securely.

## 1. ToolExecutor trait
The universal abstraction interface handling functional endpoints automatically mapping schemas defensively recursively matching boundaries.
```rust
#[async_trait]
pub trait ToolExecutor: Send + Sync {
    /// Executes a tool by name with specified JSON arguments.
    async fn execute(&self, name: &str, args: serde_json::Value) -> Result<String>;
    
    /// Lists all tools currently supported by this executor.
    async fn list_tools(&self) -> Result<Vec<ToolDefinition>>;
}
```

## 2. MultiExecutor
A native mapping structurally binding vectors resolving bounded dependencies cleanly efficiently mapping multiple `ToolExecutor` dependencies concurrently automatically handling nested searches gracefully efficiently mapping lists structurally seamlessly successfully.
```rust
let mut multi = MultiExecutor::new();
multi.add_executor(Arc::new(SandboxedExecutor::new(...)?));
multi.add_executor(Arc::new(McpExecutor::new(...)));
```

## 3. DeepAgent 
The primary recursive pipeline orchestrator bounds logic securely automatically storing boundaries strictly against persistent session states.
```rust
let agent = DeepAgent::new(
    harness, // Handles provider hooks
    state, // `DeepAgentState` with Context arrays
    executor, // The `MultiExecutor` or standard single instance natively
    memory_controller // Extends arrays optimizing `Context`
);

let result = agent.step("What is the current time?".to_string()).await?;
```
> [!NOTE]
> Advanced applications can invoke `step_stream` directly bounding generators tracking specific logic tokens efficiently mapping streaming arrays recursively updating boundaries seamlessly natively rendering streams securely accurately gracefully cleanly.
