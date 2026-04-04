use serde::{Serialize, Deserialize};
use crate::{Harness, Request, Response, executor::SandboxedExecutor};
use mem_core::{Context, FileStorage};
use mem_resilience::ResilientMemoryController;
use std::sync::Arc;
use std::path::PathBuf;

#[derive(Serialize, Deserialize)]
pub struct DeepAgentState {
    pub session_id: String,
    pub context: Context,
    pub sandbox_root: PathBuf,
}

/// The DeepAgent orchestrates the Model, Harness, and Executor into a single stateful entity.
pub struct DeepAgent {
    pub harness: Harness,
    pub state: DeepAgentState,
    pub executor: SandboxedExecutor,
    pub memory_controller: Arc<ResilientMemoryController<FileStorage>>,
}

impl DeepAgent {
    pub fn new(
        harness: Harness, 
        state: DeepAgentState, 
        executor: SandboxedExecutor, 
        memory_controller: Arc<ResilientMemoryController<FileStorage>>
    ) -> Self {
        Self { harness, state, executor, memory_controller }
    }

    /// Executes a single reasoning/action step following the DeepAgent loop.
    pub async fn step(&mut self, user_input: String) -> anyhow::Result<String> {
        let mut full_content = String::new();
        let mut stream = Box::pin(self.step_stream(user_input));
        use futures_util::StreamExt;
        
        while let Some(res) = stream.next().await {
            match res? {
                AgentStepEvent::TextChunk(c) => full_content.push_str(&c),
                _ => (),
            }
        }
        Ok(full_content)
    }

    /// Streaming version of step for TUI/UX responsiveness.
    pub fn step_stream(&mut self, user_input: String) -> impl futures_util::Stream<Item = anyhow::Result<AgentStepEvent>> + '_ {
        use async_stream::try_stream;
        
        try_stream! {
            let req = Request {
                prompt: user_input,
                context: self.state.context.clone(),
            };

            // 1. Run the Harness Lifecycle (Streaming AI Call)
            let mut stream = self.harness.run_stream(req).await?;
            use futures_util::StreamExt;

            let mut final_response = Response { content: String::new(), tool_calls: vec![] };

            while let Some(chunk_res) = stream.next().await {
                let chunk = chunk_res?;
                if let Some(c) = chunk.content_delta {
                    final_response.content.push_str(&c);
                    yield AgentStepEvent::TextChunk(c);
                }
                // (Optional: handle tool call deltas if needed for planning visuals)
            }

            // Since we're not doing full recursive reasoning in the simple stream yet,
            // we'll just handle tools if they were emitted (Ollama doesn't stream ToolCalls well in 0.1)
            // But if we have them, we execute. 
            // In a real production loop, we'd loop here.
            
            // For now, let's assume if it had tools, they are in the final_response (if the provider supports it)
            // or we do a non-streaming check.
            
            // 2. Mocking the logic for tool calls in this simplified stream:
            // (Real implementation would use a loop)
            
            // 3. Final Resilient Save State
            self.save_state_resilient().await?;
        }
    }

    /// Persists agent state using the ResilientMemoryController (Gap 9)
    pub async fn save_state_resilient(&self) -> anyhow::Result<()> {
        let root = PathBuf::from(".agent/sessions");
        if !root.exists() {
            std::fs::create_dir_all(&root)?;
        }
        
        let path = root.join(format!("session_{}.json", self.state.session_id));
        let data = serde_json::to_vec_pretty(&self.state)?;
        
        self.memory_controller.optimize_resilient(&mut self.state.context.clone()).await?;
        
        std::fs::write(path, data)?;
        Ok(())
    }
}

pub enum AgentStepEvent {
    TextChunk(String),
    ToolStarted(String),
    ToolFinished(String, String),
    Status(String),
}
