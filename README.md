# Swarm-code

A multi-agent AI coding CLI, written in Rust. Like Claude Code, but built around
**multi-agent coordination** and **concurrent multi-session** work, with a
built-in **tree-sitter code analyzer** so agents can understand a project's
structure fast instead of reading every file.

Currently targets the **DeepSeek** API (OpenAI-compatible).

> Status: early skeleton. The architecture and plumbing are in place; expect
> rough edges and missing features.

## Architecture

A Cargo workspace of four crates:

| Crate | Responsibility |
|-------|----------------|
| `swarm-llm` | LLM provider abstraction + DeepSeek backend (chat + tool calling). |
| `swarm-analyzer` | tree-sitter syntax & scope analysis (Rust, Python, JS, Go). |
| `swarm-core` | Agents, tools, the `Swarm` coordinator, concurrent sessions. |
| `swarm-cli` | The `swarm` binary: REPL + `run` / `analyze` subcommands. |

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

## Usage

```bash
# 1. Configure your key
cp .env.example .env && $EDITOR .env   # set DEEPSEEK_API_KEY

# 2. Analyze a file (no API key needed)
cargo run -p swarm-cli -- analyze src/main.rs
cargo run -p swarm-cli -- analyze src/main.rs --json

# 3. One-shot task
cargo run -p swarm-cli -- run "summarize the architecture of this repo"

# 4. Interactive multi-session REPL
cargo run -p swarm-cli -- chat
```

REPL commands: `/new [title]`, `/sessions`, `/switch <n>`, `/help`, `/quit`.

## Roadmap

- Streaming responses (the `stream` plumbing exists in `swarm-llm`).
- Shell/exec and search tools.
- Richer scope queries (symbol-at-position, references).
- Provider plugins beyond DeepSeek.
- True parallel sub-agent fan-out with result aggregation.

## License

MIT
