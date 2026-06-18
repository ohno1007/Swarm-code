//! Interactive REPL with multi-session support.
//!
//! Slash commands:
//!   /new [title]      create a new session and switch to it
//!   /sessions         list live sessions
//!   /switch <n>       switch to session number n (from /sessions)
//!   /help             show commands
//!   /quit             exit
//!
//! Sessions run concurrently under the hood (see `SessionManager`), so you can
//! kick off long tasks in one and keep working in another.

use std::path::PathBuf;

use swarm_core::SessionManager;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use uuid::Uuid;

pub async fn run(workspace: PathBuf) -> anyhow::Result<()> {
    let manager = swarm_core::build_manager(workspace.clone())?;

    println!("Swarm-code — multi-agent AI coding CLI");
    println!("workspace: {}", workspace.display());
    println!("type /help for commands, /quit to exit\n");

    let mut current = manager.create("main").await;

    let stdin = tokio::io::stdin();
    let mut lines = BufReader::new(stdin).lines();
    let mut stdout = tokio::io::stdout();

    prompt(&mut stdout, current).await?;
    while let Some(line) = lines.next_line().await? {
        let line = line.trim().to_string();
        if line.is_empty() {
            prompt(&mut stdout, current).await?;
            continue;
        }

        if let Some(cmd) = line.strip_prefix('/') {
            match handle_command(cmd, &manager, &mut current).await? {
                Flow::Quit => break,
                Flow::Continue => {}
            }
            prompt(&mut stdout, current).await?;
            continue;
        }

        match manager.send(current, line).await {
            Ok(answer) => println!("\n{answer}\n"),
            Err(e) => eprintln!("\nerror: {e}\n"),
        }
        prompt(&mut stdout, current).await?;
    }

    Ok(())
}

enum Flow {
    Continue,
    Quit,
}

async fn handle_command(
    cmd: &str,
    manager: &SessionManager,
    current: &mut Uuid,
) -> anyhow::Result<Flow> {
    let mut parts = cmd.split_whitespace();
    let name = parts.next().unwrap_or("");
    let rest = parts.collect::<Vec<_>>().join(" ");

    match name {
        "quit" | "exit" | "q" => return Ok(Flow::Quit),
        "help" | "h" => {
            println!(
                "commands:\n  \
                 /new [title]   new session\n  \
                 /sessions      list sessions\n  \
                 /switch <n>    switch session by number\n  \
                 /help          this help\n  \
                 /quit          exit"
            );
        }
        "new" => {
            let title = if rest.is_empty() { "session".to_string() } else { rest };
            *current = manager.create(title).await;
            println!("created session {}", short(*current));
        }
        "sessions" => {
            let sessions = manager.list().await;
            for (i, (id, title, turns)) in sessions.iter().enumerate() {
                let marker = if *id == *current { "*" } else { " " };
                println!("{marker} [{i}] {} \"{title}\" ({turns} msgs)", short(*id));
            }
        }
        "switch" => match rest.trim().parse::<usize>() {
            Ok(n) => {
                let sessions = manager.list().await;
                match sessions.get(n) {
                    Some((id, title, _)) => {
                        *current = *id;
                        println!("switched to [{n}] \"{title}\"");
                    }
                    None => println!("no session #{n} (see /sessions)"),
                }
            }
            Err(_) => println!("usage: /switch <number>"),
        },
        other => println!("unknown command: /{other} (try /help)"),
    }
    Ok(Flow::Continue)
}

async fn prompt(stdout: &mut tokio::io::Stdout, current: Uuid) -> anyhow::Result<()> {
    stdout
        .write_all(format!("swarm:{}> ", short(current)).as_bytes())
        .await?;
    stdout.flush().await?;
    Ok(())
}

fn short(id: Uuid) -> String {
    id.to_string()[..8].to_string()
}
