use serde::{Serialize, Deserialize};
use crate::{Harness, Request, executor::SandboxedExecutor};
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
        let req = Request {
            prompt: user_input,
            context: self.state.context.clone(),
        };

        // 1. Run the Harness Lifecycle (Hooks + AI Call)
        let response = self.harness.run(req).await?;

        // 2. Process Recursive Tool Calls
        for mut tool in response.tool_calls {
            let args = tool.arguments
                .as_object()
                .map(|obj| {
                    obj.values()
                        .map(|v| {
                            if v.is_string() {
                                v.as_str().unwrap().to_string()
                            } else {
                                v.to_string()
                            }
                        })
                        .collect()
                })
                .unwrap_or_default();

            // Pre-tool state save (Resilient Pillar)
            self.save_state_resilient().await?;

            // Run Tool Hooks
            let mut result = "In-progress".to_string();
            self.harness.run_tool_hooks(&mut tool, &mut result).await?;
            
            // Execute in the sandbox
            result = self.executor.execute(&tool.name, args).await?;
            
            // Post-tool hooks (Offloading/Extraction happens here)
            self.harness.run_tool_hooks(&mut tool, &mut result).await?;

            // Post-tool state save (Resilient Pillar)
            self.save_state_resilient().await?;
        }

        // 3. Final Resilient Save State
        self.save_state_resilient().await?;

        Ok(response.content)
    }

    /// Persists agent state using the ResilientMemoryController (Gap 9)
    pub async fn save_state_resilient(&self) -> anyhow::Result<()> {
        let root = PathBuf::from(".agent/sessions");
        if !root.exists() {
            std::fs::create_dir_all(&root)?;
        }
        
        let path = root.join(format!("session_{}.json", self.state.session_id));
        let data = serde_json::to_vec_pretty(&self.state)?;
        
        // Resilience: Wrap saving in an emergency snapshot logic if possible, 
        // or just ensure the memory controller is aware of the context state.
        self.memory_controller.optimize_resilient(&mut self.state.context.clone()).await?;
        
        std::fs::write(path, data)?;
        Ok(())
    }
}
