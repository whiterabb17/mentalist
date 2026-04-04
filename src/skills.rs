use crate::executor::ToolExecutor;
use mem_core::ToolDefinition;
use async_trait::async_trait;
use anyhow::{Result, Context, anyhow};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::fs;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SkillMetadata {
    pub name: String,
    pub description: String,
    pub version: Option<String>,
    pub author: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_yaml::Value>,
}

pub struct Skill {
    pub path: PathBuf,
    pub metadata: SkillMetadata,
    pub instructions: String,
}

pub struct SkillExecutor {
    pub skills_root: PathBuf,
    pub skills: HashMap<String, Arc<Skill>>,
}

impl SkillExecutor {
    pub async fn new(root: PathBuf) -> Result<Self> {
        let mut executor = Self {
            skills_root: root,
            skills: HashMap::new(),
        };
        executor.reload().await?;
        Ok(executor)
    }

    pub async fn reload(&mut self) -> Result<()> {
        if !self.skills_root.exists() {
            return Ok(());
        }

        let mut read_dir = fs::read_dir(&self.skills_root).await?;
        while let Some(entry) = read_dir.next_entry().await? {
            let path = entry.path();
            if path.is_dir() {
                let skill_file = path.join("SKILL.md");
                if skill_file.exists() {
                    match self.load_skill(&path).await {
                        Ok(skill) => {
                            self.skills.insert(skill.metadata.name.clone(), Arc::new(skill));
                        }
                        Err(e) => {
                            tracing::error!("Failed to load skill at {:?}: {}", path, e);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    async fn load_skill(&self, path: &Path) -> Result<Skill> {
        let content = fs::read_to_string(path.join("SKILL.md")).await?;
        
        let (frontmatter, instructions) = if content.starts_with("---") {
            let parts: Vec<&str> = content.splitn(3, "---").collect();
            if parts.len() == 3 {
                (parts[1], parts[2].trim())
            } else {
                ("", content.trim())
            }
        } else {
            ("", content.trim())
        };

        let metadata: SkillMetadata = serde_yaml::from_str(frontmatter)
            .context("Failed to parse SKILL.md frontmatter")?;

        Ok(Skill {
            path: path.to_path_buf(),
            metadata,
            instructions: instructions.to_string(),
        })
    }
}

#[async_trait]
impl ToolExecutor for SkillExecutor {
    async fn execute(&self, name: &str, args: serde_json::Value) -> Result<String> {
        let skill = self.skills.get(name).context(format!("Skill '{}' not found", name))?;
        
        // According to agentskills.io, we prioritize executing scripts if they exist
        let scripts_dir = skill.path.join("scripts");
        if scripts_dir.exists() {
            // Check for common entry points: name.sh, name.py, name.js
            let possible_scripts = [
                format!("{}.sh", name),
                format!("{}.py", name),
                format!("{}.js", name),
                "run.sh".to_string(),
                "run.py".to_string(),
            ];

            for script_name in possible_scripts {
                let script_path = scripts_dir.join(&script_name);
                if script_path.exists() {
                    let mut cmd = if script_name.ends_with(".sh") {
                        let mut c = tokio::process::Command::new("bash");
                        c.arg(&script_path);
                        c
                    } else if script_name.ends_with(".py") {
                        let mut c = tokio::process::Command::new("python");
                        c.arg(&script_path);
                        c
                    } else if script_name.ends_with(".js") {
                        let mut c = tokio::process::Command::new("node");
                        c.arg(&script_path);
                        c
                    } else {
                        tokio::process::Command::new(&script_path)
                    };

                    // Pass arguments as JSON string
                    cmd.arg(serde_json::to_string(&args)?);
                    cmd.current_dir(&skill.path);
                    
                    let output = cmd.output().await?;
                    if !output.status.success() {
                        return Err(anyhow!("Skill script failed: {}", String::from_utf8_lossy(&output.stderr)));
                    }
                    return Ok(String::from_utf8_lossy(&output.stdout).to_string());
                }
            }
        }

        // If no script, returning instructions might be used to indicate "activation" 
        // leading to full injection by middleware.
        Ok(format!("Skill '{}' activated. Instructions loaded into context.", name))
    }

    async fn list_tools(&self) -> Result<Vec<ToolDefinition>> {
        let mut definitions = Vec::new();
        for skill in self.skills.values() {
            definitions.push(ToolDefinition {
                name: skill.metadata.name.clone(),
                description: skill.metadata.description.clone(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "input": { "type": "string" }
                    }
                }),
            });
        }
        Ok(definitions)
    }
}
