use mentalist::execution::executor::Executor;
use mentalist::tools::ToolRegistry;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("Mentalist Python/WASM Execution Demo (v0.3.3)");
    
    // 1. Setup Tool Registry
    let tools = ToolRegistry::new();
    // In a real demo, we'd add PyTool here for WASM execution
    let tools = Arc::new(tools);

    // 2. Setup Executor
    let _executor = Executor::new(tools);

    println!("WASM tools integration is now managed via the Skill/Tool interface.");
    Ok(())
}
