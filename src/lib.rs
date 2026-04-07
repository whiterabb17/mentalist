pub mod core;
pub mod cognition;
pub mod memory;
pub mod execution;
pub mod tools;
pub mod llm;
pub mod security;
pub mod telemetry;
pub mod config;
pub mod middleware;

// Re-exports for a clean public API
pub use core::runtime::{AgentRuntime, ExecutionLimits, RuntimeEvent};
pub use core::state::{AgentState, AgentState as DeepAgentState, Goal};
pub use cognition::{Planner, MindPalacePlanner, Critic, DefaultCritic};
pub use memory::{MemoryStore, MindPalaceMemory};
pub use execution::executor::{Executor, ExecutionResult};
pub use execution::graph::TaskGraph;
pub use tools::registry::ToolRegistry;
pub use tools::Tool;
pub use llm::{LLMProvider, LLMRouter, MindPalaceLLM};
pub use security::{SecurityEngine, Capability, Policy};
pub use telemetry::init_telemetry;
pub use config::{RuntimeConfig, SecurityConfig, AgentConfig};

// Core types from mem-core for convenience
pub use mem_core::{
    Request, Response, ToolCall, Context, MemoryItem, MemoryRole, 
    ModelProvider, EmbeddingProvider, TokenCounter
};

// Re-export mem-planner for easier dependency management in agents
pub use mem_planner;

// Error handling
pub mod error {
    pub use anyhow::Error as MentalistError;
}

// --- Compatibility Layer for Gypsy (v0.3.3 -> v0.3.5) ---

pub mod mcp {
    pub use crate::tools::mcp_adapter::{McpServer as McpExecutor, BuiltinMcp};
}

pub mod executor {
    pub use crate::execution::executor::{Executor, ExecutionResult};
    use crate::tools::registry::ToolRegistry;
    use crate::tools::mcp_adapter::{McpServer, McpTool};
    use std::sync::Arc;

    /// Bridge for V0.3.3 -> V0.3.5. Wraps ToolRegistry.
    #[derive(Clone)]
    pub struct MultiExecutor {
        pub registry: Arc<ToolRegistry>,
    }

    impl MultiExecutor {
        pub fn new() -> Self {
            Self { registry: Arc::new(ToolRegistry::new()) }
        }

        pub async fn add_executor(&self, _name: String, server: Arc<McpServer>) {
            match server.list_tools().await {
                Ok(tools) => {
                    for (name, desc, params) in tools {
                        let tool = McpTool {
                            server: Arc::clone(&server),
                            name,
                            description: desc,
                            parameters: params,
                        };
                        self.registry.register(Arc::new(tool)).await;
                    }
                }
                Err(e) => tracing::error!("Failed to list tools for MCP server: {}", e),
            }
        }
        
        pub async fn add_tool(&self, tool: Arc<dyn crate::tools::Tool>) {
            self.registry.register(tool).await;
        }

        pub async fn list_executors(&self) -> Vec<(String, bool, String)> {
             // Return dummy info for TUI compatibility
             vec![("mcp:filesystem".into(), true, "Running".into())]
        }
    }

    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    pub enum ExecutionMode {
        Local,
        Docker { image: String },
        Wasm { module_path: String, mount_root: bool, env_vars: std::collections::HashMap<String, String> },
    }
}

pub mod skills {
    pub type SkillExecutor = crate::tools::registry::ToolRegistry;
}

pub type Result<T> = anyhow::Result<T>;
