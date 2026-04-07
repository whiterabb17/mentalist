use tracing::{info, instrument};

pub fn init_telemetry() {
    // Subscriber initialization logic or tracing spans
}

#[instrument(skip(runtime, goal))]
pub async fn trace_agent_run(runtime: &crate::AgentRuntime, goal: &str) -> anyhow::Result<String> {
    info!(goal, "Agent run starting");
    let res = runtime.run(goal, mem_core::Context::default()).await?;
    info!("Agent run finished");
    Ok(res)
}
