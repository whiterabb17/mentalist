use clap::Parser;
use colored::*;
use mentalist::{Harness, Request, Response, DeepAgent, DeepAgentState, ModelProvider};
use mentalist::middleware::{MindPalaceMiddleware, todo::TodoMiddleware};
use brain::Brain;
use mem_core::Context;
use std::sync::Arc;
use std::path::PathBuf;
use anyhow::Result;
use async_trait::async_trait;
use tokio::time::{sleep, Duration};

#[derive(Parser)]
struct Args {
    /// Enter interactive mode to talk to the agent
    #[arg(short, long)]
    interactive: bool,
}

/// A Mock LLM Provider for the demo to ensure it runs without API keys.
pub struct MockProvider;

#[async_trait]
impl ModelProvider for MockProvider {
    async fn complete(&self, req: Request) -> Result<Response> {
        // Mocking reasoning based on prompt keywords to simulate intelligence
        let content = if req.prompt.contains("Analyze") {
            "Assessment complete. Found a modular 7-layer memory architecture with SHA-256 offloading and Zstd archival."
        } else if req.prompt.contains("plan") {
            "Strategy formulated: 1. Scan CAS registry, 2. Run Layer 3 Summarizer, 3. Execute Fact Extraction."
        } else if req.prompt.contains("Standardize") {
            "Instruction received. I will now enforce consistency across all internal fact extraction tools."
        } else {
            "I am the Mentalist DeepAgent. How can I assist with your memory optimization today?"
        }.to_string();

        Ok(Response {
            content,
            tool_calls: vec![],
        })
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    
    println!("\n{}", "===================================================".bold().cyan());
    println!("{}", "   🧠 MINDPALACE & MENTALIST SYSTEM DEMO 🧠   ".bold().cyan());
    println!("{}\n", "===================================================".bold().cyan());
    
    // 1. Initialize Brain (7-Layer Memory Core)
    let brain = Arc::new(Brain::default());
    
    // 2. Setup the Harness with Mock Provider
    let provider = Box::new(MockProvider);
    let mut harness = Harness::new(provider);
    
    // Ensure agent directory exists
    let agent_dir = PathBuf::from(".agent");
    if !agent_dir.exists() {
        std::fs::create_dir_all(&agent_dir)?;
    }
    
    // 3. Register Middlewares
    harness.add_middleware(Box::new(MindPalaceMiddleware::new(brain.clone())));
    harness.add_middleware(Box::new(TodoMiddleware::new(agent_dir.join("todo.md"))));
    
    // 4. Initialize the DeepAgent Orchestrator
    let state = DeepAgentState {
        session_id: "demo_session_001".to_string(),
        context: Context { items: vec![] },
        sandbox_root: std::env::current_dir()?,
    };
    
    let executor = mentalist::executor::SandboxedExecutor::new(
        mentalist::executor::ExecutionMode::Local,
        std::env::current_dir()?
    );

    let mut agent = DeepAgent::new(harness, state, executor);

    if args.interactive {
        run_interactive(&mut agent).await?;
    } else {
        run_scripted(&mut agent).await?;
    }

    Ok(())
}

/// Scripted Demo Mode (Default)
async fn run_scripted(agent: &mut DeepAgent) -> Result<()> {
    let script = vec![
        "Analyze the current MindPalace project structure.",
        "Update the plan for memory extraction tasks.",
        "Standardize the fact extraction tools.",
    ];

    println!("{}", ">>> Starting Scripted Verification...".bold().white());

    for (i, prompt) in script.into_iter().enumerate() {
        println!("\n{}: {}", format!("TURN {}", i + 1).on_blue().white().bold(), prompt.bold());
        
        // Visualizing the DeepAgent Hooks in action
        println!("{}", " 🟦 [Hook] before_ai_call: Optimizing Context (Layers 1-7)...".blue());
        sleep(Duration::from_millis(200)).await;
        println!("{}", " 🟦 [Hook] before_ai_call: Injecting Planning state (TODO.md)...".blue());
        sleep(Duration::from_millis(200)).await;
        
        let response = agent.step(prompt.to_string()).await?;
        
        println!("{}", " 🟩 [Hook] after_ai_call: Response Received & Analyzing Intent...".green());
        sleep(Duration::from_millis(200)).await;
        println!("{}: {}", " 🤖 Agent Response".bold().cyan(), response);
        
        // Simulated tool hook visualization
        println!("{}", " 🟪 [Hook] after_tool_call: Extracting Durable Facts (Layer 5)...".magenta());
        sleep(Duration::from_millis(200)).await;
    }
    
    println!("\n{}", "===================================================".bold().green());
    println!("{}", "   ✅ SYSTEM DEMO COMPLETED SUCCESSFULLY ✅   ".bold().green());
    println!("{}\n", "===================================================".bold().green());
    
    println!("{}", "Check '.agent/' for generated session, todo, and knowledge files.".italic().white());
    Ok(())
}

/// Interactive REPL Mode
async fn run_interactive(agent: &mut DeepAgent) -> Result<()> {
    use std::io::{self, Write};

    println!("{}", ">>> Entering Interactive Mode (type 'exit' or 'quit' to stop)".bold().yellow());
    
    loop {
        print!("\n{} ", "User >".bold().white());
        io::stdout().flush()?;
        
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim();
        
        if input.is_empty() { continue; }
        if input == "exit" || input == "quit" { break; }
        
        println!("{}", " 🟦 [Hook] before_ai_call: Optimizing Context...".blue());
        sleep(Duration::from_millis(200)).await;
        let response = agent.step(input.to_string()).await?;
        println!("{}", " 🟩 [Hook] after_ai_call: Processing response...".green());
        sleep(Duration::from_millis(200)).await;
        
        println!("{}: {}", " 🤖 Agent Response".bold().cyan(), response);
    }
    
    Ok(())
}
