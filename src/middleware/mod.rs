use crate::tools::registry::ToolRegistry;
pub use mem_core::{Request, Response, ToolCall, Context};
use async_trait::async_trait;
use brain::Brain;
use mem_bridge::AgentBridge;
use mem_compactor::IntelligentFullCompactor;
use mem_core::{FileStorage, MemoryItem, MemoryRole, MindPalaceConfig};
use mem_dreamer::DreamWorker;
use mem_extractor::{FactExtractor, ReflectionLayer};
use mem_micro::{AdaptiveMicroCompactor, TTLDecayStrategy};
use mem_offloader::{OffloaderConfig, ToolOffloader};
use mem_personality::PersonalityGuard;
use mem_retriever::{MemoryRetriever, RuVectorStore};
use mem_session::SessionSummarizer;
use ruvector_core::types::DistanceMetric;
use std::path::PathBuf;
use std::sync::Arc;

#[async_trait]
pub trait Middleware: Send + Sync {
    /// Returns a human-readable name for the middleware, used in diagnostic contexts.
    fn name(&self) -> &str {
        "Middleware"
    }

    /// Whether failure of this middleware should abort the entire request chain.
    /// Security-critical middlewares should return true (default).
    fn is_critical(&self) -> bool {
        true
    }

    /// Returns the execution priority. Lower values run first. Default is 10.
    fn priority(&self) -> i32 {
        10
    }

    /// Called when the Harness is initialized or when the middleware is added.
    async fn initialize(&self) -> anyhow::Result<()> {
        Ok(())
    }

    /// Called when the Harness or Agent is being shut down.
    async fn shutdown(&self) -> anyhow::Result<()> {
        Ok(())
    }

    /// Fires before the prompt reaches the LLM.
    async fn before_ai_call(&self, _req: &mut Request) -> anyhow::Result<()> {
        Ok(())
    }

    /// Fires after the LLM responds, before processing tool calls.
    async fn after_ai_call(&self, _res: &mut Response) -> anyhow::Result<()> {
        Ok(())
    }

    /// Fires before a specific tool execution (Safety gate).
    async fn before_tool_call(&self, _tool: &mut ToolCall) -> anyhow::Result<()> {
        Ok(())
    }

    /// Fires after a tool result is returned.
    async fn after_tool_call(&self, _tool: &ToolCall, _result: &mut String) -> anyhow::Result<()> {
        Ok(())
    }

    /// Fires to manually request optimization/summarization of context.
    async fn optimize_context(&self, _ctx: &mut Context) -> anyhow::Result<()> {
        Ok(())
    }
}

pub struct SafetyMiddleware {
    pub forbidden_tools: Vec<String>,
}

impl SafetyMiddleware {
    pub fn new(forbidden: Vec<String>) -> Self {
        Self {
            forbidden_tools: forbidden,
        }
    }
}

#[async_trait]
impl Middleware for SafetyMiddleware {
    fn name(&self) -> &str {
        "Safety"
    }

    async fn before_tool_call(&self, tool: &mut ToolCall) -> anyhow::Result<()> {
        if self.forbidden_tools.contains(&tool.name) {
            anyhow::bail!(
                "Security: Tool '{}' is forbidden by SafetyMiddleware.",
                tool.name
            );
        }
        Ok(())
    }
}

pub struct MindPalaceMiddleware {
    pub brain: Arc<Brain>,
    pub extractor: Arc<FactExtractor<FileStorage>>,
    pub retriever: MemoryRetriever<FileStorage>,
    pub bridge: Arc<AgentBridge<FileStorage>>,
    pub dreamer: Option<Arc<DreamWorker<FileStorage>>>,
    pub session_id: String,
    pub token_budget: usize,
}

impl MindPalaceMiddleware {
    pub fn new(
        brain: Arc<Brain>,
        extractor: Arc<FactExtractor<FileStorage>>,
        retriever: MemoryRetriever<FileStorage>,
        bridge: Arc<AgentBridge<FileStorage>>,
        dreamer: Option<Arc<DreamWorker<FileStorage>>>,
        session_id: String,
    ) -> Self {
        Self {
            brain,
            extractor,
            retriever,
            bridge,
            dreamer,
            session_id,
            token_budget: 4096,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn hardened(
        storage: FileStorage,
        llm: Arc<dyn mem_core::LlmClient>,
        embeddings: Arc<dyn mem_core::EmbeddingProvider>,
        token_counter: Arc<dyn mem_core::TokenCounter>,
        session_id: String,
        dimension: usize,
        vault_path: PathBuf,
        config_override: Option<MindPalaceConfig>,
    ) -> Self {
        let config = config_override.unwrap_or_else(|| MindPalaceConfig {
            max_context_items: 50, // Standard threshold for deep focus
            ..Default::default()
        });

        let mut brain = Brain::new(config.clone(), None, Some(token_counter));

        // Create directories for the session vault
        let checkpoint_dir = vault_path.join("checkpoints");
        let narrative_dir = vault_path.join("narratives");
        let knowledge_dir = vault_path.join("knowledge");
        std::fs::create_dir_all(&checkpoint_dir).ok();
        std::fs::create_dir_all(&narrative_dir).ok();
        std::fs::create_dir_all(&knowledge_dir).ok();

        // Initialize Shared Infrastructure (Extractor)
        let extractor = Arc::new(FactExtractor::new(
            llm.clone(),
            embeddings.clone(),
            storage.clone(),
            config.clone(),
            "knowledge.json".to_string(),
            session_id.clone(),
        ));

        // 1. Identity & Ethics: Personality Guard (Priority 1)
        brain.add_layer(Arc::new(PersonalityGuard::new(
            "Gypsy: A high-agency agentic AI focused on filesystem excellence and pair programming.".to_string(),
            Some(llm.clone()),
        )));

        // 2. Efficiency: Tool Offloader (Priority 1)
        brain.add_layer(Arc::new(ToolOffloader::new(
            storage.clone(),
            OffloaderConfig::default(),
        )));

        // 3. Noise Reduction: Adaptive Micro Compactor (Priority 2)
        let relevance_analyzer = Arc::clone(&extractor) as Arc<dyn mem_core::RelevanceAnalyzer>;
        brain.add_layer(Arc::new(AdaptiveMicroCompactor::new(
            config.clone(),
            TTLDecayStrategy::AdaptiveByType,
            relevance_analyzer,
        )));

        // 4. Summarization: Session Summarizer (Priority 3)
        // Hardcoded 80% threshold as per user request
        let mut session_config = config.clone();
        session_config.summary_interval = (config.max_context_items as f32 * 0.8) as usize;
        brain.add_layer(Arc::new(SessionSummarizer::new(
            llm.clone(),
            storage.clone(),
            session_config,
            narrative_dir.to_string_lossy().to_string(),
            true, // Validation mode on
        )));

        // 5. Intelligence: Reflection & Fact Extraction (Priority 4/5)
        brain.add_layer(Arc::new(ReflectionLayer::new(extractor.clone())));
        brain.add_layer(extractor.clone() as Arc<dyn mem_core::MemoryLayer>);

        // 6. Emergency Pruning: Intelligent Full Compactor (Priority 4)
        let importance_analyzer = Arc::clone(&extractor) as Arc<dyn mem_core::ImportanceAnalyzer>;
        brain.add_layer(Arc::new(IntelligentFullCompactor::new(
            llm.clone(),
            importance_analyzer,
            storage.clone(),
            config.clone(),
            checkpoint_dir.to_string_lossy().to_string(),
        )));

        // 7. Background Synthesis: Dream Worker (Priority 6)
        let dreamer = Arc::new(DreamWorker::new(
            llm.clone(),
            storage.clone(),
            config.clone(),
            vault_path.join("dream.lock"),
        ));
        brain.add_layer(dreamer.clone());

        // 8. Coordination: Agent Bridge (Priority 7)
        let bridge = Arc::new(AgentBridge::new(storage.clone()));
        brain.add_layer(bridge.clone());

        // Persistence Layer (Memory Retriever)
        let graph = Arc::new(mem_core::FactGraph::new(None).expect("Failed to init fact graph"));
        let store = Arc::new(RuVectorStore::new(
            dimension,
            DistanceMetric::Cosine,
            graph.clone(),
        ));
        let retriever = MemoryRetriever::new(storage, embeddings, llm.clone(), store, graph);

        Self::new(
            Arc::new(brain),
            extractor,
            retriever,
            bridge,
            Some(dreamer),
            session_id,
        )
    }
}

#[async_trait]
impl Middleware for MindPalaceMiddleware {
    fn name(&self) -> &str {
        "MindPalace"
    }

    async fn before_ai_call(&self, req: &mut Request) -> anyhow::Result<()> {
        // 1. Proactive Extraction: Learn from User input immediately
        let user_context = Context {
            items: vec![MemoryItem {
                role: MemoryRole::User,
                content: req.prompt.clone(),
                timestamp: chrono::Utc::now().timestamp() as u64,
                metadata: serde_json::json!({}),
            }],
        };
        let user_facts = self.extractor.extract_facts(&user_context).await?;
        if !user_facts.is_empty() {
            self.extractor.commit_knowledge(user_facts).await?;
            self.retriever
                .hydrate_from_kb(&self.extractor.knowledge_path)
                .await?;
        }

        // 2. High-Precision RAG: Use recent context + prompt for query
        let facts = self
            .retriever
            .retrieve_relevant_facts(&req.prompt, 5, None)
            .await?;

        // Standard DeepAgent Methodology: Clone context for enrichment/optimization (Arc is immutable)
        let mut current_context = (*req.context).clone();

        if !facts.is_empty() {
            let mut fact_content = String::from("### RELEVANT KNOWLEDGE ###\n");
            for (fact, score) in facts {
                fact_content.push_str(&format!(
                    "- [{}] {} (similarity: {:.2})\n",
                    fact.category, fact.content, score
                ));
            }

            current_context.items.push(MemoryItem {
                role: MemoryRole::System,
                content: fact_content,
                timestamp: chrono::Utc::now().timestamp() as u64,
                metadata: serde_json::json!({"rag": true}),
            });
        }

        // 3. Orchestrated 7-Layer Optimization (Hardened Logic)
        self.brain.optimize(&mut current_context).await?;

        // Replace with optimized Arc
        req.context = Arc::new(current_context);

        // 4. Proactive token budget compaction check
        if let Some(counter) = &self.brain.token_counter {
            let current_tokens: usize = req
                .context
                .items
                .iter()
                .map(|i| counter.count_tokens(&i.content))
                .sum();
            if current_tokens > (self.token_budget as f32 * 0.8) as usize {
                tracing::warn!(
                    "Token budget critical ({}%). Performance may degrade.",
                    (current_tokens as f32 / self.token_budget as f32) * 100.0
                );
            }
        }

        Ok(())
    }

    async fn after_ai_call(&self, res: &mut Response) -> anyhow::Result<()> {
        // Deductive Fact Extraction from AI's own response
        let ai_context = Context {
            items: vec![MemoryItem {
                role: MemoryRole::Assistant,
                content: res.content.clone(),
                timestamp: chrono::Utc::now().timestamp() as u64,
                metadata: serde_json::json!({}),
            }],
        };

        let new_facts = self.extractor.extract_facts(&ai_context).await?;
        if !new_facts.is_empty() {
            tracing::info!("Extracted {} facts from AI response.", new_facts.len());
            self.extractor.commit_knowledge(new_facts).await?;
            self.retriever
                .hydrate_from_kb(&self.extractor.knowledge_path)
                .await?;
        }
        Ok(())
    }

    async fn after_tool_call(&self, _tool: &ToolCall, result: &mut String) -> anyhow::Result<()> {
        // Deductive Fact Extraction from tool results
        let temp_context = Context {
            items: vec![MemoryItem {
                role: MemoryRole::Tool,
                content: result.clone(),
                timestamp: chrono::Utc::now().timestamp() as u64,
                metadata: serde_json::json!({}),
            }],
        };

        let new_facts = self.extractor.extract_facts(&temp_context).await?;
        if !new_facts.is_empty() {
            tracing::info!("Extracted {} facts from tool output.", new_facts.len());
            self.extractor.commit_knowledge(new_facts).await?;
            self.retriever
                .hydrate_from_kb(&self.extractor.knowledge_path)
                .await?;
        }

        Ok(())
    }

    async fn optimize_context(&self, ctx: &mut Context) -> anyhow::Result<()> {
        self.brain.optimize(ctx).await?;
        Ok(())
    }
}

pub mod todo;

/// Automatically discovers and injects tools from the registry into the AI request.
pub struct ToolDiscoveryMiddleware {
    pub tools: Arc<ToolRegistry>,
}

impl ToolDiscoveryMiddleware {
    pub fn new(tools: Arc<ToolRegistry>) -> Self {
        Self { tools }
    }
}

#[async_trait]
impl Middleware for ToolDiscoveryMiddleware {
    fn name(&self) -> &str {
        "ToolDiscovery"
    }

    fn is_critical(&self) -> bool {
        false
    }

    async fn before_ai_call(&self, req: &mut Request) -> anyhow::Result<()> {
        let tools = self.tools.list_tools().await;
        // Map ToolSchema to mem_core::ToolDefinition (they are structurally identical)
        for t in tools {
            req.tools.push(mem_core::ToolDefinition {
                name: t.name,
                description: t.description,
                parameters: t.parameters,
            });
        }
        Ok(())
    }
}

/// Comprehensive, production-ready logging middleware that uses the tracing crate.
pub struct LoggingMiddleware;

#[async_trait]
impl Middleware for LoggingMiddleware {
    fn name(&self) -> &str {
        "Logging"
    }

    fn is_critical(&self) -> bool {
        false
    }

    async fn before_ai_call(&self, req: &mut Request) -> anyhow::Result<()> {
        tracing::info!(
            target: "mentalist::logging_mw",
            "AI Call Starting | Prompt: {:.50}... | Context Items: {}",
            req.prompt, req.context.items.len()
        );
        Ok(())
    }

    async fn after_ai_call(&self, res: &mut Response) -> anyhow::Result<()> {
        tracing::info!(
            target: "mentalist::logging_mw",
            "AI Call Finished | Response: {:.50}... | Tool Calls: {}",
            res.content, res.tool_calls.len()
        );
        Ok(())
    }

    async fn before_tool_call(&self, tool: &mut ToolCall) -> anyhow::Result<()> {
        let mut args = tool.arguments.clone();
        
        // Redact potentially sensitive keys
        if let Some(obj) = args.as_object_mut() {
            let sensitive_keys = ["key", "token", "secret", "password", "auth", "credential", "api_key"];
            for key in obj.keys().cloned().collect::<Vec<_>>() {
                if sensitive_keys.iter().any(|&sk| key.to_lowercase().contains(sk)) {
                    obj.insert(key, serde_json::Value::String("[REDACTED]".to_string()));
                }
            }
        }

        let args_json =
            serde_json::to_string(&args).unwrap_or_else(|_| "INVALID_ARGS".into());
        tracing::info!(
            target: "mentalist::logging_mw",
            "Tool Call Starting | Name: {} | Args: {}",
            tool.name, args_json
        );
        Ok(())
    }

    async fn after_tool_call(&self, tool: &ToolCall, result: &mut String) -> anyhow::Result<()> {
        tracing::info!(
            target: "mentalist::logging_mw",
            "Tool Call Finished | Name: {} | Result size: {} chars",
            tool.name, result.len()
        );
        Ok(())
    }

    async fn optimize_context(&self, ctx: &mut Context) -> anyhow::Result<()> {
        tracing::info!(
            target: "mentalist::logging_mw",
            "Context Optimization Requested | Current Items: {}",
            ctx.items.len()
        );
        Ok(())
    }
}
