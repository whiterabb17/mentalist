use mentalist::executor::{SandboxedExecutor, ExecutionMode};
use std::path::PathBuf;
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    // 1. Configure the Sandboxed Executor for Wasm
    // You must have a python.wasm file in the ./tools directory.
    // Download from: https://github.com/vmware-labs/webassembly-language-runtimes
    let executor = SandboxedExecutor::new(
        ExecutionMode::Wasm { 
            module_path: PathBuf::from("./tools/python.wasm"), 
            mount_root: true 
        },
        PathBuf::from("./") // Project root
    );

    println!("🚀 Running Python via WebAssembly Sandbox...");

    // 2. Prepare the Python command
    // We pass -c to run a string, or you could pass a path to a file in /sandbox/
    let cmd = "python"; // This is just a label in Wasm mode, the actual module is module_path
    let args = vec![
        "-c".to_string(), 
        "print('Hello from Python.wasm! 🐍'); import sys; print(f'Platform: {sys.platform}'); print(f'Sandbox Path: {sys.path}')".to_string()
    ];

    // 3. Execute
    match executor.execute(cmd, args).await {
        Ok(output) => {
            println!("--- Tool Output ---");
            println!("{}", output);
            println!("-------------------");
        }
        Err(e) => {
            eprintln!("❌ Execution Failed: {}", e);
            eprintln!("Note: Ensure ./tools/python.wasm exists and is a valid WASI module.");
        }
    }

    Ok(())
}
