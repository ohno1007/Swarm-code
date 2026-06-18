# Swarm-code

A multi-agent AI coding CLI, written in Rust. Like Claude Code, but built around
**multi-agent coordination** and **concurrent multi-session** work, with a
built-in **tree-sitter code analyzer** and a **git-like change-management layer**
so a swarm of agents can edit one codebase without stepping on each other.

Currently targets the **DeepSeek** API (OpenAI-compatible).

> Status: working skeleton. Core plumbing is in place and tested; expect rough
> edges and missing features.

## Highlights

- **Change Buffer** — agents never write to disk directly; edits are staged into
  a shared overlay (like a git index) and applied atomically on commit.
- **Symbol-level locks** — finer-grained than file locks: two agents can edit
  two different functions in the same file at once, but not the same one.
- **Compile validation** — `commit_changes` runs `cargo check` (fast) and
  reports failures so the swarm self-corrects.
- **Event bus** — staged/committed/locked/validated events propagate to all
  agents and to the REPL live.
- **Agent terminal** — `run_command` gives agents a sandboxed shell.
- **Streaming + live feedback** — responses stream token-by-token, and every
  tool/command call is shown as it runs (`⚙ name … ✓ result`).
- **Memory system** — two layers: *working memory* auto-compacts the
  conversation when it grows too large (older turns → an LLM summary), and
  *long-term memory* persists durable project facts to `.swarm/memory.json`
  (`remember`/`recall`/`forget`), injected into each new session's prompt.
- **In-CLI key setup** — first run prompts for your DeepSeek key and saves it.

## Architecture

A Cargo workspace of four crates:

| Crate | Responsibility |
|-------|----------------|
| `swarm-llm` | LLM provider abstraction + DeepSeek backend (chat, tool calling, streaming). |
| `swarm-analyzer` | tree-sitter syntax & scope analysis (Rust, Python, JS, Go). |
| `swarm-core` | Agents, tools, the `Swarm`/`Coordinator`, change buffer, locks, validation, events, concurrent sessions. |
| `swarm-cli` | The `swarm` binary: REPL + `run` / `analyze` / `config` subcommands. |

```
swarm-cli ──> swarm-core ──> swarm-llm  (DeepSeek)
                  └────────> swarm-analyzer (tree-sitter)
```

### Multi-agent coordination
The lead **orchestrator** agent runs a tool-calling loop. One of its tools is
`spawn_agent`, which delegates a self-contained subtask to a **worker** agent
that runs in its own context window and returns a result. Delegation is one
level deep by design (workers can't spawn), keeping things bounded.

### Concurrent sessions
`SessionManager` holds many sessions, each behind its own async mutex. Different
sessions make progress in parallel; turns within a session stay ordered. In the
REPL, `/new` and `/switch` move between them.

### Code analysis as a tool
`analyze_code` gives an agent a tree-sitter outline of a file — its symbols
(functions, types, modules) and lexical scopes (parameters, `let` bindings) —
so it can build a mental model without reading the whole file. Add a language by
dropping its grammar crate and a rule table into `swarm-analyzer`.

### Git-like change management
All agents in a workspace share one `Coordinator`:

```
write_file / edit_symbol ─▶ Change Buffer ─(commit_changes)─▶ disk ─▶ cargo check
        │                       (overlay)                                  │
        └── edit_symbol takes a symbol-level lock                          ▼
                                                                       Event Bus ─▶ all agents
```

The agent tools for this workflow: `write_file`, `edit_symbol`, `view_changes`,
`commit_changes`, `discard_changes`, `list_locks`, `run_command`, `cargo_check`.
`edit_symbol` locates a symbol via tree-sitter, locks it, and stages a
replacement; `commit_changes` flushes **your** staged files (commits are
author-scoped), releases your locks and validates.

### Memory
- **Working memory** (`WorkingMemory`): per-agent conversation buffer. Past a
  token budget it summarizes the oldest whole turns into one note and keeps
  recent turns verbatim — long sessions don't blow the context window.
- **Long-term memory** (`MemoryStore`): workspace-scoped, persisted to
  `.swarm/memory.json`. Tools `remember` / `recall` / `forget`; a digest of
  recent notes is injected into the lead agent's prompt at session start, so the
  swarm carries knowledge across sessions.

## Usage

```bash
# 1. Configure your key (first `chat`/`run` also prompts automatically)
cargo run -p swarm-cli -- config set-key
cargo run -p swarm-cli -- config show

# 2. Analyze a file (no API key needed)
cargo run -p swarm-cli -- analyze src/main.rs
cargo run -p swarm-cli -- analyze src/main.rs --json

# 3. One-shot task
cargo run -p swarm-cli -- run "add a Display impl for the Config struct"

# 4. Interactive multi-session REPL
cargo run -p swarm-cli -- chat
```

Keys are read from `DEEPSEEK_API_KEY`, then `~/.config/swarm-code/config`, then
an interactive prompt. REPL commands: `/new [title]`, `/sessions`,
`/switch <n>`, `/help`, `/quit`.

## Roadmap

- ~~Streaming responses~~ ✓
- ~~Shell/exec tool~~ ✓ (`run_command`)
- ~~Change buffer, symbol locks, compile validation, event bus~~ ✓
- ~~In-CLI key configuration~~ ✓
- ~~Context compaction + tool/command feedback~~ ✓
- ~~Author-scoped commits~~ ✓
- ~~Memory system (working + long-term)~~ ✓
- Search/grep tool and richer scope queries (symbol-at-position, references).
- True parallel sub-agent fan-out with result aggregation.
- Provider plugins beyond DeepSeek.

## License

MIT
