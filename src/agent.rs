use serde::{Serialize, Deserialize};
use crate::{Harness, Request, executor::SandboxedExecutor};
use mem_core::Context;
use std::fs;
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
}

impl DeepAgent {
    pub fn new(harness: Harness, state: DeepAgentState, executor: SandboxedExecutor) -> Self {
        Self { harness, state, executor }
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
            // Simplified tool argument parsing
            let args = tool.arguments
                .as_object()
                .map(|obj| obj.values().map(|v| v.as_str().unwrap_or("").to_string()).collect())
                .unwrap_or_default();

            // Run Tool Hooks (Safety check happens here)
            let mut result = "In-progress".to_string();
            self.harness.run_tool_hooks(&mut tool, &mut result).await?;

            // Execute in the sandbox
            result = self.executor.execute(&tool.name, args).await?;
            
            // Post-tool hooks (Offloading/Extraction happens here)
            self.harness.run_tool_hooks(&mut tool, &mut result).await?;
        }

        // 3. Save State (Session Serialization Pillar)
        self.save_state().await?;

        Ok(response.content)
    }

    /// Persists the entire agent state (History + Memory Snapshots) for crash recovery.
    pub async fn save_state(&self) -> anyhow::Result<()> {
        let root = PathBuf::from(".agent/sessions");
        if !root.exists() {
            fs::create_dir_all(&root)?;
        }
        
        let path = root.join(format!("session_{}.json", self.state.session_id));
        let data = serde_json::to_vec_pretty(&self.state)?;
        fs::write(path, data)?;
        Ok(())
    }
}
