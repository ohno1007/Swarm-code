//! Tools for self-configuring and using skills.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::tool::{Tool, ToolContext};

/// List available skills.
pub struct ListSkills;

#[async_trait]
impl Tool for ListSkills {
    fn name(&self) -> &str {
        "list_skills"
    }
    fn description(&self) -> &str {
        "List available skills (reusable instruction packs) by name and description."
    }
    fn parameters(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }
    async fn execute(&self, _args: Value, ctx: &ToolContext) -> anyhow::Result<String> {
        let skills = ctx.coordinator.skills.list();
        if skills.is_empty() {
            return Ok("no skills defined yet (create one with create_skill)".to_string());
        }
        Ok(skills
            .into_iter()
            .map(|s| format!("- {}: {}", s.name, s.description))
            .collect::<Vec<_>>()
            .join("\n"))
    }
}

/// Load a skill's instructions into context.
pub struct UseSkill;

#[async_trait]
impl Tool for UseSkill {
    fn name(&self) -> &str {
        "use_skill"
    }
    fn description(&self) -> &str {
        "Load a skill's full instructions so you can follow them for the current \
         task. Call list_skills first if unsure of the name."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "name": { "type": "string", "description": "Skill name." } },
            "required": ["name"]
        })
    }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> anyhow::Result<String> {
        let name = args["name"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing required argument: name"))?;
        match ctx.coordinator.skills.get(name) {
            Some(skill) => Ok(format!(
                "Skill '{}' — {}\n\nInstructions:\n{}",
                skill.name, skill.description, skill.instructions
            )),
            None => Ok(format!("no skill named '{name}' (see list_skills)")),
        }
    }
}

/// Create or overwrite a skill — the agent configuring its own capabilities.
pub struct CreateSkill;

#[async_trait]
impl Tool for CreateSkill {
    fn name(&self) -> &str {
        "create_skill"
    }
    fn description(&self) -> &str {
        "Create (or overwrite) a reusable skill: a named, described set of \
         instructions saved to .swarm/skills. Use this to teach yourself a \
         repeatable procedure so future sessions can `use_skill` it."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": { "type": "string", "description": "Short skill name (slug)." },
                "description": { "type": "string", "description": "One-line summary of when to use it." },
                "instructions": { "type": "string", "description": "The full procedure/guidance the skill teaches." }
            },
            "required": ["name", "description", "instructions"]
        })
    }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> anyhow::Result<String> {
        let name = args["name"].as_str().ok_or_else(|| anyhow::anyhow!("missing name"))?;
        let description = args["description"].as_str().unwrap_or("");
        let instructions = args["instructions"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing instructions"))?;
        let path = ctx.coordinator.skills.create(name, description, instructions)?;
        Ok(format!("created skill '{name}' at {}", path.display()))
    }
}
