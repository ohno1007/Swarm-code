//! Skills: reusable, user/agent-authored capability packs.
//!
//! A skill is a markdown file with a small frontmatter, stored under
//! `<workspace>/.swarm/skills/<name>.md`:
//!
//! ```text
//! ---
//! name: changelog
//! description: How to update the CHANGELOG for this repo
//! ---
//! When asked to update the changelog, edit CHANGELOG.md, add an entry under
//! "Unreleased", keep entries terse, ...
//! ```
//!
//! Skills are pure data, so the agent can create and use them at runtime
//! (no restart): `create_skill` writes one, `use_skill` loads its instructions
//! into the conversation, `list_skills` enumerates them.

use std::path::PathBuf;

/// Lightweight summary for listing.
#[derive(Debug, Clone)]
pub struct SkillMeta {
    pub name: String,
    pub description: String,
}

/// A full skill (with instruction body).
#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub instructions: String,
}

/// Reads/writes skills under `<workspace>/.swarm/skills`.
pub struct SkillStore {
    dir: PathBuf,
}

impl SkillStore {
    pub fn new(workspace: &std::path::Path) -> Self {
        Self {
            dir: workspace.join(".swarm").join("skills"),
        }
    }

    /// Enumerate skills (re-reads the directory, so freshly created skills show
    /// up immediately).
    pub fn list(&self) -> Vec<SkillMeta> {
        let mut out = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&self.dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("md") {
                    continue;
                }
                if let Some(skill) = parse(&path) {
                    out.push(SkillMeta {
                        name: skill.name,
                        description: skill.description,
                    });
                }
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    pub fn get(&self, name: &str) -> Option<Skill> {
        let path = self.dir.join(format!("{}.md", sanitize(name)));
        parse(&path)
    }

    /// Create (or overwrite) a skill. Returns the file path.
    pub fn create(
        &self,
        name: &str,
        description: &str,
        instructions: &str,
    ) -> std::io::Result<PathBuf> {
        std::fs::create_dir_all(&self.dir)?;
        let name = sanitize(name);
        let path = self.dir.join(format!("{name}.md"));
        let body = format!(
            "---\nname: {name}\ndescription: {description}\n---\n{}\n",
            instructions.trim()
        );
        std::fs::write(&path, body)?;
        Ok(path)
    }

    pub fn is_empty(&self) -> bool {
        self.list().is_empty()
    }

    /// `- name: description` lines for prompt injection.
    pub fn digest(&self) -> String {
        self.list()
            .into_iter()
            .map(|s| format!("- {}: {}", s.name, s.description))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Slugify a skill name to a safe filename stem.
fn sanitize(name: &str) -> String {
    name.trim()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_lowercase()
}

/// Parse a skill file's frontmatter + body.
fn parse(path: &std::path::Path) -> Option<Skill> {
    let text = std::fs::read_to_string(path).ok()?;
    let stem = path.file_stem()?.to_str()?.to_string();

    let (mut name, mut description, body) = if let Some(rest) = text.strip_prefix("---\n") {
        if let Some(end) = rest.find("\n---") {
            let (front, after) = rest.split_at(end);
            let body = after.trim_start_matches("\n---").trim_start_matches('\n');
            let mut name = stem.clone();
            let mut description = String::new();
            for line in front.lines() {
                if let Some(v) = line.strip_prefix("name:") {
                    name = v.trim().to_string();
                } else if let Some(v) = line.strip_prefix("description:") {
                    description = v.trim().to_string();
                }
            }
            (name, description, body.to_string())
        } else {
            (stem.clone(), String::new(), text.clone())
        }
    } else {
        (stem.clone(), String::new(), text.clone())
    };

    if name.is_empty() {
        name = stem;
    }
    if description.is_empty() {
        description = "(no description)".to_string();
    }
    Some(Skill {
        name,
        description,
        instructions: body,
    })
}
