use async_trait::async_trait;
use mem_core::Context;
use mem_planner::ExecutionPlan;
use mentalist::cognition::{Critic, Feedback, Planner};
use mentalist::core::{AgentRuntime, ExecutionLimits};
use mentalist::execution::executor::{ExecutionResult, Executor};
use mentalist::llm::{LLMProvider, LlmRequest, LlmResponse};
use mentalist::memory::{MemoryEvent, MemoryQuery, MemoryStore};
use mentalist::security::{Policy, SecurityEngine};
use mentalist::tools::ToolRegistry;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;

struct ReasoningLLM;
#[async_trait]
impl LLMProvider for ReasoningLLM {
    async fn generate(&self, _req: LlmRequest) -> anyhow::Result<LlmResponse> {
        Ok(LlmResponse {
            content: "Mock plan JSON...".to_string(),
            tool_calls: vec![],
            usage: None,
        })
    }
    async fn generate_stream(
        &self,
        _req: LlmRequest,
    ) -> anyhow::Result<
        futures_util::stream::BoxStream<'static, anyhow::Result<mentalist::llm::ResponseChunk>>,
    > {
        anyhow::bail!("Streaming not implemented")
    }
}

struct MockPlanner {
    pub call_count: Arc<AtomicUsize>,
}
#[async_trait]
impl Planner for MockPlanner {
    async fn create_plan(
        &self,
        _goal: &str,
        _context: &Context,
        _tools: Vec<mentalist::tools::ToolSchema>,
        _todo: Option<&str>,
    ) -> anyhow::Result<ExecutionPlan> {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        let mut plan = ExecutionPlan::new();
        plan.add_task(mem_planner::TaskNode {
            id: mem_planner::TaskId::new(),
            name: "Dummy task".into(),
            description: "A dummy task to trigger execution".into(),
            tool_name: None,
            tool_args: None,
            dependencies: vec![],
            metadata: serde_json::Value::Null,
        });
        Ok(plan)
    }
}

struct PersistentMemory {
    pub events: Arc<tokio::sync::Mutex<Vec<MemoryEvent>>>,
}
#[async_trait]
impl MemoryStore for PersistentMemory {
    async fn store(&self, event: MemoryEvent) -> anyhow::Result<()> {
        let mut events = self.events.lock().await;
        events.push(event);
        Ok(())
    }
    async fn recall(&self, _query: MemoryQuery) -> anyhow::Result<Vec<MemoryEvent>> {
        let events = self.events.lock().await;
        Ok(events.clone())
    }
    async fn summarize(&self, _ctx: &mut Context) -> anyhow::Result<String> {
        Ok("Summary".into())
    }
}

struct MultiStepCritic {
    pub call_count: Arc<AtomicUsize>,
}
#[async_trait]
impl Critic for MultiStepCritic {
    async fn evaluate(
        &self,
        _goal: &str,
        _context: &Context,
        _results: &std::collections::HashMap<mem_planner::TaskId, ExecutionResult>,
    ) -> anyhow::Result<Feedback> {
        let count = self.call_count.fetch_add(1, Ordering::SeqCst);
        if count == 0 {
            Ok(Feedback {
                score: 0.5,
                critique: "Retry needed.".into(),
                suggests_retry: true,
            })
        } else {
            Ok(Feedback {
                score: 1.0,
                critique: "Perfect.".into(),
                suggests_retry: false,
            })
        }
    }
}

#[tokio::test]
async fn test_runtime_multi_phase_reasoning() {
    let planner_count = Arc::new(AtomicUsize::new(0));
    let critic_count = Arc::new(AtomicUsize::new(0));
    let memory_events = Arc::new(tokio::sync::Mutex::new(vec![]));

    let llm = Arc::new(ReasoningLLM);
    let memory = Arc::new(PersistentMemory {
        events: Arc::clone(&memory_events),
    });
    let tools = Arc::new(ToolRegistry::new());
    let security = Arc::new(SecurityEngine::new(Policy {
        allowed_capabilities: vec![],
        tool_allowlist: vec![],
    }));
    let planner = Arc::new(MockPlanner {
        call_count: Arc::clone(&planner_count),
    });
    let executor = Arc::new(Executor::new(Arc::clone(&tools)));
    let critic = Arc::new(MultiStepCritic {
        call_count: Arc::clone(&critic_count),
    });

    let runtime = AgentRuntime {
        planner,
        executor,
        memory,
        llm,
        tools,
        security,
        critic,
        limits: ExecutionLimits {
            max_steps: 3,
            timeout_seconds: 60,
        },
        middlewares: vec![],
    };

    let result = runtime
        .run("Complex multi-phase goal", Context::default(), None, None)
        .await;
    assert!(result.is_ok(), "Runtime failed: {:?}", result.err());

    // Core Verification: Ensure we actually went through multiple phases.
    assert!(
        planner_count.load(Ordering::SeqCst) >= 2,
        "Should have called Planner for at least 2 phases"
    );
    assert_eq!(
        critic_count.load(Ordering::SeqCst),
        2,
        "Critic should have been called twice"
    );
}

struct ConversationalPlanner {
    pub content: String,
}
#[async_trait]
impl Planner for ConversationalPlanner {
    async fn create_plan(
        &self,
        _goal: &str,
        _context: &Context,
        _tools: Vec<mentalist::tools::ToolSchema>,
        _todo: Option<&str>,
    ) -> anyhow::Result<ExecutionPlan> {
        Ok(ExecutionPlan {
            tasks: std::collections::HashMap::new(),
            content: self.content.clone(),
            requires_approval: false,
            usage: None,
        })
    }
}

#[tokio::test]
async fn test_runtime_conversational_flow() {
    let memory_events = Arc::new(tokio::sync::Mutex::new(vec![]));
    let llm = Arc::new(ReasoningLLM);
    let memory = Arc::new(PersistentMemory {
        events: Arc::clone(&memory_events),
    });
    let tools = Arc::new(ToolRegistry::new());
    let security = Arc::new(SecurityEngine::new(Policy {
        allowed_capabilities: vec![],
        tool_allowlist: vec![],
    }));
    let planner = Arc::new(ConversationalPlanner {
        content: "Hello! I am a test agent.".into(),
    });
    let executor = Arc::new(Executor::new(Arc::clone(&tools)));
    let critic = Arc::new(MultiStepCritic {
        call_count: Arc::new(AtomicUsize::new(0)),
    });

    let runtime = AgentRuntime {
        planner,
        executor,
        memory,
        llm,
        tools,
        security,
        critic,
        limits: ExecutionLimits {
            max_steps: 3,
            timeout_seconds: 60,
        },
        middlewares: vec![],
    };

    let (tx, mut rx) = mpsc::unbounded_channel();
    let result = runtime.run("Say hello", Context::default(), Some(tx), None).await;
    assert!(result.is_ok());

    let mut received_text = String::new();
    while let Ok(event) = rx.try_recv() {
        if let mentalist::core::runtime::RuntimeEvent::TextChunk(c) = event {
            received_text.push_str(&c);
        }
    }

    assert_eq!(received_text, "Hello! I am a test agent.");
}
