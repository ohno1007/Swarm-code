# Swarm-code — Quickstart (Windows)

A multi-agent AI coding CLI (Rust, DeepSeek) with a full-screen, Claude-Code-style
interface, plus self-configurable **skills** and **MCP**.

## 1. Run it (no install)

`swarm.exe` is a **self-contained** Windows x64 binary — no DLLs, no runtime to
install. Put it anywhere, then open a terminal there.

> **Use Windows Terminal** (or PowerShell in it) for the best experience — it
> renders the colors, box-drawing and icons the TUI uses. The old `cmd.exe`
> works but shows some glyphs poorly.

```powershell
# in the folder containing swarm.exe
.\swarm.exe --help
```

To run it from anywhere, drop `swarm.exe` in a folder on your `PATH`
(e.g. `C:\Users\<you>\bin`, then add that folder to PATH), or just `cd` to it.

## 2. Configure your DeepSeek key (in the CLI)

```powershell
.\swarm.exe config set-key      # paste your key; saved to %USERPROFILE%\.config\swarm-code\config
.\swarm.exe config show         # verify (masked)
```

The first `chat`/`run` also prompts automatically. Key is read from
`DEEPSEEK_API_KEY` → `%USERPROFILE%\.config\swarm-code\config` → prompt.

## 3. Use it

```powershell
# Understand a file's structure (no API key needed) — tree-sitter outline
.\swarm.exe analyze src\main.rs
.\swarm.exe analyze src\main.rs --json

# One-shot task in the current folder
.\swarm.exe run "add a Display impl for the Config struct and run cargo check"

# Full-screen interactive UI (the main way to use it)
.\swarm.exe chat
.\swarm.exe chat --plain        # simple line-based mode, if you prefer
```

Point at a specific project with `-w <path>` (default: current folder).

### The chat UI

A header bar (model • session • status), a scrolling transcript that streams the
assistant's reply with **inline tool/command feedback** (`⚙ tool … ✓ result`),
and an input box at the bottom.

| Key | Action |
|-----|--------|
| Enter | send message |
| ↑ / ↓ , PgUp / PgDn | scroll the transcript |
| Esc | clear the input line |
| Ctrl+C | quit |

Slash commands (type in the input box): `/new [title]`, `/sessions`,
`/switch <n>`, `/help`, `/quit`. Sessions run concurrently, so you can start a
task, `/new` another, and switch between them.

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

## 6. Where state lives (Windows)

| Path | What |
|------|------|
| `%USERPROFILE%\.config\swarm-code\config` | API key & model |
| `<project>\.swarm\memory.json` | long-term memory (`remember`/`recall`) |
| `<project>\.swarm\skills\*.md` | skills |
| `<project>\.swarm\mcp.json` | MCP server configs |
| `%TEMP%\swarm-code.log` | logs (TUI mode logs here, not the screen) |

`.swarm/` is gitignored by default.

## Build from source (optional)

```powershell
# needs the Rust toolchain (https://rustup.rs)
cargo build --release          # target\release\swarm.exe
```

## 7. Env knobs (PowerShell)

```powershell
$env:DEEPSEEK_MODEL = "deepseek-reasoner"   # or deepseek-chat (default)
$env:DEEPSEEK_BASE_URL = "https://api.deepseek.com"
$env:RUST_LOG = "swarm_core=debug"          # verbose logging (-> %TEMP%\swarm-code.log)
.\swarm.exe chat
```
