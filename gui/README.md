# Swarm-code Desktop (Tauri + Material 3)

A graphical front-end for the Swarm-code agent. The Rust backend is the same
`swarm-core` used by the CLI (multi-agent coordination, change buffer, symbol
locks, memory, skills, MCP, …); the UI is a Material Design 3 web app rendered
in a native Tauri window.

Features: streaming chat with **markdown**, inline **tool-call cards**, live
**sub-agent** output, a **context-memory ring**, model switch (`deepseek-chat` /
`deepseek-reasoner`), a thinking-intensity slider, multi-session rail, and an
in-app **Settings** dialog for the DeepSeek key and workspace folder.

## Get a Windows build (no toolchain needed)

Tauri builds Windows installers with MSVC + WebView2, so they're produced on a
**Windows runner** via GitHub Actions, not cross-compiled from Linux.

1. Push to the repo (or run the **Build Windows GUI** workflow from the Actions
   tab → *Run workflow*).
2. Open the finished run → **Artifacts** → download `swarm-code-windows`.
3. Unzip and run the `.msi`/NSIS `.exe` installer (or the raw `swarm-gui.exe`).
   WebView2 is built into Windows 10/11.

## Build locally

### Windows
Install [Rust](https://rustup.rs), [Node 20+](https://nodejs.org), and the
*Visual Studio Build Tools* (Desktop C++). Then:

```powershell
cd gui
npm install
npm run tauri build     # installer in src-tauri\target\release\bundle\
# or for development:
npm run tauri dev
```

### Linux (for development/verification)
```bash
sudo apt-get install -y libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libsoup-3.0-dev
cd gui && npm install && npm run tauri dev
```

## Architecture

```
 Material 3 web UI (gui/src, Vite + @material/web)
        │  invoke() / Channel  (Tauri IPC)
 gui/src-tauri (Rust commands, event streaming)
        │
 swarm-core  ── DeepSeek, tree-sitter, coordinator, memory, skills, MCP
```

First run: open **Settings** (gear icon), paste your DeepSeek API key and pick a
workspace folder. The key is saved to `~/.config/swarm-code/config` (shared with
the CLI).
