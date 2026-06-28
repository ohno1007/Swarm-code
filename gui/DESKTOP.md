# Swarm-code Desktop (`swarm-desktop.exe`)

The Material 3 GUI as a **single self-contained Windows .exe** — cross-compiled
from Linux (pure Rust, no Tauri/MSVC/WebView needed). It embeds the web UI and
runs a tiny local server, then opens your browser.

## Run

1. Double-click `swarm-desktop.exe` (or run it in a terminal).
2. A small console window shows a local URL like `http://127.0.0.1:54321` and
   your browser opens it automatically. If it doesn't, copy that URL into your
   browser.
3. Click the **gear icon** → paste your DeepSeek API key and pick a workspace
   folder → Save.
4. Chat. The console window keeps the app running; **close it or press Ctrl+C**
   to stop.

Notes:
- Nothing to install — only standard Windows system DLLs are used.
- The agent operates on the **workspace folder** you set (defaults to where the
  exe runs). Set it to your project folder in Settings.
- Your key is stored at `%USERPROFILE%\.config\swarm-code\config` (shared with
  the CLI).

## Build from source

Needs [Rust](https://rustup.rs) + [Node 20+](https://nodejs.org).

```bash
cd gui
npm install
npm run build            # produces gui/dist (embedded into the exe)
cd server
cargo build --release    # -> target/release/swarm-desktop(.exe)
```

Cross-compile to Windows from Linux (what produced the shipped binary):

```bash
rustup target add x86_64-pc-windows-gnu
sudo apt-get install -y gcc-mingw-w64-x86-64
cd gui && npm install && npm run build
cd server && cargo build --release --target x86_64-pc-windows-gnu
```

This is the only GUI variant that cross-compiles from Linux. The Tauri variant
(`gui/src-tauri`, native window) must be built on Windows — see `gui/README.md`.
