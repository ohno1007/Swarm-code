# Swarm-code — Quickstart

A multi-agent AI coding CLI (Rust, DeepSeek). This guide gets you from zero to
driving the swarm, including the self-configurable **skills** and **MCP**.

## 1. Build

```bash
cargo build --release         # produces ./target/release/swarm
# optional: put it on your PATH
install -m755 target/release/swarm ~/.local/bin/swarm
```

(If you received the prebuilt `swarm` binary, just `chmod +x swarm` and run it.
It's a Linux x86-64 build; on macOS/Windows build from source as above.)

## 2. Configure your DeepSeek key (in the CLI)

```bash
swarm config set-key          # paste your key; saved to ~/.config/swarm-code/config (0600)
swarm config show             # verify (masked)
```

The first `swarm chat`/`swarm run` also prompts automatically if no key is set.
Key is read from `DEEPSEEK_API_KEY` → `~/.config/swarm-code/config` → prompt.

## 3. Use it

```bash
# Understand a file's structure (no API key needed) — tree-sitter outline
swarm analyze src/main.rs
swarm analyze src/main.rs --json

# One-shot task in the current repo
swarm run "add a Display impl for the Config struct and run cargo check"

# Interactive, multi-session REPL (streamed output + live tool feedback)
swarm chat
```

REPL commands: `/new [title]`, `/sessions`, `/switch <n>`, `/help`, `/quit`.
Point at another repo with `-w/--workspace <path>`.

## 4. How the agent works (what you'll see)

- It reads/searches code (`analyze_code`, `search`, `find_files`) to build a map.
- Edits are **staged**, not written directly: `edit_symbol` (symbol-level, with
  a lock) and `write_file` go into a change buffer. `commit_changes` flushes
  **your** staged work and runs `cargo check`.
- It can run a terminal (`run_command`) and delegate to workers — `spawn_agents`
  runs several in parallel.
- Each tool call is shown live as `⚙ name … ✓/✗ result`.

## 5. Self-configuration

### Skills (reusable instruction packs)
Ask the agent to create one, or drop a file in `.swarm/skills/<name>.md`:

```markdown
---
name: release
description: How to cut a release for this repo
---
1. Bump the version in Cargo.toml
2. Update CHANGELOG.md under "Unreleased"
3. Run cargo test, then tag vX.Y.Z
```

In chat: "create a skill called `release` that …", then later "use the release
skill". The agent uses `create_skill` / `list_skills` / `use_skill`. Available
skills are shown to it automatically at the start of each session.

### MCP servers (extend its tools)
Tell the agent to connect any stdio MCP server, e.g.:

> connect an MCP server named `fs` that runs
> `npx -y @modelcontextprotocol/server-filesystem /path/to/dir`

It calls `mcp_add_server`, which launches the server, discovers its tools and
saves the config to `.swarm/mcp.json`. Then it uses `mcp_list_tools` and
`mcp_call`. You can also pre-create `.swarm/mcp.json`:

```json
{
  "mcpServers": {
    "fs": { "command": "npx", "args": ["-y", "@modelcontextprotocol/server-filesystem", "/path"] }
  }
}
```

## 6. Where state lives

| Path | What |
|------|------|
| `~/.config/swarm-code/config` | API key & model |
| `<repo>/.swarm/memory.json` | long-term memory (`remember`/`recall`) |
| `<repo>/.swarm/skills/*.md` | skills |
| `<repo>/.swarm/mcp.json` | MCP server configs |

`.swarm/` is gitignored by default.

## 7. Env knobs

```bash
DEEPSEEK_MODEL=deepseek-reasoner   # or deepseek-chat (default)
DEEPSEEK_BASE_URL=https://api.deepseek.com
RUST_LOG=swarm_core=debug          # verbose logging
```
