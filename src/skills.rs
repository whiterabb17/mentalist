use crate::executor::{ToolExecutor, CommandValidator};
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
    pub validator: CommandValidator,
}

impl SkillExecutor {
    pub async fn new(root: PathBuf) -> Result<Self> {
        let mut executor = Self {
            skills_root: root,
            skills: HashMap::new(),
            validator: CommandValidator::new_default(),
        };
        executor.reload().await?;
        Ok(executor)
    }

    pub async fn reload(&mut self) -> Result<()> {
        if !self.skills_root.exists() {
            return Ok(());
        }

        let mut next_skills = HashMap::new();
        let mut read_dir = fs::read_dir(&self.skills_root).await?;
        while let Some(entry) = read_dir.next_entry().await? {
            let path = entry.path();
            if path.is_dir() {
                let skill_file = path.join("SKILL.md");
                if skill_file.exists() {
                    match self.load_skill(&path).await {
                        Ok(skill) => {
                            next_skills.insert(skill.metadata.name.clone(), Arc::new(skill));
                        }
                        Err(e) => {
                            tracing::error!("Failed to load skill at {:?}: {}", path, e);
                        }
                    }
                }
            }
        }
        self.skills = next_skills;
        Ok(())
    }

    async fn load_skill(&self, path: &Path) -> Result<Skill> {
        let content = fs::read_to_string(path.join("SKILL.md")).await?;
        
        let (frontmatter, instructions) = if content.starts_with("---") {
            let parts: Vec<&str> = content.splitn(3, "---").collect();
            match parts.len() {
                3 => (parts[1], parts[2].trim()),
                _ => anyhow::bail!("Invalid SKILL.md frontmatter separator"),
            }
        } else {
            anyhow::bail!("SKILL.md must start with --- for frontmatter");
        };

        if frontmatter.trim().is_empty() {
            anyhow::bail!("Empty frontmatter in SKILL.md");
        }

        let metadata: SkillMetadata = serde_yaml::from_str(frontmatter)
            .context("Failed to parse SKILL.md frontmatter")?;

        if metadata.name.is_empty() {
            anyhow::bail!("Skill name is required");
        }
        if metadata.description.is_empty() {
            anyhow::bail!("Skill description is required");
        }

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
        // Validate skill name
        if !name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-') {
            anyhow::bail!("Invalid skill name: {} (only alphanumeric, _, - allowed)", name);
        }
        
        let skill = self.skills.get(name).context(format!("Skill '{}' not found", name))?;
        
        let canonical_skill_path = skill.path.canonicalize()?;
        let canonical_skills_root = self.skills_root.canonicalize()?;
        
        if !canonical_skill_path.starts_with(&canonical_skills_root) {
            anyhow::bail!("Skill path escapes skills directory");
        }

        let scripts_dir = canonical_skill_path.join("scripts");
        if !scripts_dir.exists() {
            return Ok(format!("Skill '{}' activated. Instructions loaded into context.", name));
        }

        let allowed_scripts = ["run.sh", "run.py", "run.js"];

        for script_name in allowed_scripts {
            let script_path = scripts_dir.join(script_name);
            if !script_path.exists() || !script_path.is_file() {
                continue;
            }
            
            // Verify it's not a symlink
            if script_path.symlink_metadata()?.file_type().is_symlink() {
                tracing::warn!("Skipping symlink script: {:?}", script_path);
                continue;
            }

            // POSIX Permission Check: Ensure script is executable
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let metadata = std::fs::metadata(&script_path)?;
                if metadata.permissions().mode() & 0o111 == 0 {
                    tracing::warn!("Script exists but is not executable: {:?}", script_path);
                    anyhow::bail!("Skill script {:?} is not executable. Please run 'chmod +x' on it.", script_name);
                }
            }
            
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
                continue;
            };

            let args_str = serde_json::to_string(&args)?;
            // Validate the arguments before spawning the script
            self.validator.validate(name, &[args_str.clone()], &self.skills_root)?;

            cmd.arg(args_str);
            cmd.current_dir(&canonical_skill_path);
            
            let output = tokio::time::timeout(std::time::Duration::from_secs(30), cmd.output())
                .await
                .map_err(|_| anyhow!("Skill script execution timed out after 30s"))??;

            if !output.status.success() {
                return Err(anyhow!("Skill script failed: {}", String::from_utf8_lossy(&output.stderr)));
            }
            return Ok(String::from_utf8_lossy(&output.stdout).to_string());
        }

        anyhow::bail!(
            "No executable script found in {:?}. Looked for: {:?}",
            scripts_dir,
            allowed_scripts
        );
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
