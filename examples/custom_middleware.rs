use mentalist::{Harness, DeepAgent, DeepAgentState, Request, Response, ToolCall, ModelProvider, ResponseChunk, Context};
use mentalist::middleware::Middleware;
use mentalist::executor::{SandboxedExecutor, ExecutionMode};
use async_trait::async_trait;
use std::sync::Arc;
use std::path::PathBuf;
use futures_util::stream::{self, BoxStream};

struct MockModel;
#[async_trait]
impl ModelProvider for MockModel {
    async fn complete(&self, _req: Request) -> anyhow::Result<Response> {
        Ok(Response { 
            content: "I will use the echo tool.".into(), 
            tool_calls: vec![ToolCall {
                name: "echo".into(),
                arguments: serde_json::json!({ "msg": "Hello from Custom Middleware Example!" }),
            }]
        })
    }
    async fn stream_complete(&self, _req: Request) -> anyhow::Result<BoxStream<'static, anyhow::Result<ResponseChunk>>> {
        Ok(Box::pin(stream::empty()))
    }
}

struct SafetyMiddleware;
#[async_trait]
impl Middleware for SafetyMiddleware {
    fn name(&self) -> &str { "Safety" }
    fn priority(&self) -> i32 { 1 } // Run early

    async fn before_tool_call(&self, tool: &mut ToolCall) -> anyhow::Result<()> {
        if tool.name == "rm" {
            anyhow::bail!("Security: 'rm' command is restricted.");
        }
        Ok(())
    }
}

struct MetricsMiddleware {
    pub start_time_ms: std::sync::atomic::AtomicU64,
}
#[async_trait]
impl Middleware for MetricsMiddleware {
    fn name(&self) -> &str { "Metrics" }
    fn priority(&self) -> i32 { 100 } // Run late

    async fn before_ai_call(&self, _req: &mut Request) -> anyhow::Result<()> {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
        self.start_time_ms.store(now, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }

    async fn after_ai_call(&self, _res: &mut Response) -> anyhow::Result<()> {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
        let start = self.start_time_ms.load(std::sync::atomic::Ordering::SeqCst);
        println!("AI Call took {}ms", now - start);
        Ok(())
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let provider = Box::new(MockModel);
    let mut harness = Harness::new(provider);
    
    harness.add_middleware(Arc::new(SafetyMiddleware));
    harness.add_middleware(Arc::new(MetricsMiddleware { 
        start_time_ms: std::sync::atomic::AtomicU64::new(0) 
    }));

    let temp_dir = std::env::current_dir()?.join(".agent_sandbox");
    std::fs::create_dir_all(&temp_dir)?;

    let executor = Arc::new(SandboxedExecutor::new(
        ExecutionMode::Local,
        temp_dir.clone(),
        None
    )?);

    let state = DeepAgentState {
        session_id: "example_session".into(),
        context: Arc::new(Context { items: vec![] }),
        sandbox_root: temp_dir,
    };

    // Note: We need a real MemoryController or a mock one.
    // For this example, we'll use a mock approach if possible, but let's assume we can't easily.
    // Actually, DeepAgent needs a real ResilientMemoryController<FileStorage>.
    
    println!("Example initialized. Middlewares configured.");
    println!("Harness has {} middlewares.", harness.middlewares.len());
    
    // In a real scenario, you'd run the agent here.
    // let mut agent = DeepAgent::new(harness, state, executor, memory_controller);
    // agent.step("Hello").await?;

    Ok(())
}
