//! Integration tests for the git-like coordination layer (no LLM required).

use std::path::PathBuf;
use std::sync::Arc;

use swarm_core::coordinator::Coordinator;
use swarm_core::tool::{Tool, ToolContext};
use swarm_core::tools::coord::EditSymbol;
use swarm_core::tools::search::Search;

fn temp_workspace() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("swarm-test-{}", uuid_like()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn uuid_like() -> u128 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
}

fn ctx(coord: &Arc<Coordinator>, agent: &str) -> ToolContext {
    ToolContext::for_agent(agent, coord.clone(), None, 0)
}

#[tokio::test]
async fn symbol_lock_blocks_second_agent_but_allows_other_symbols() {
    let ws = temp_workspace();
    std::fs::write(
        ws.join("lib.rs"),
        "fn foo() {\n    let a = 1;\n}\n\nfn bar() {\n    let b = 2;\n}\n",
    )
    .unwrap();

    let coord = Coordinator::new(ws.clone());
    let edit = EditSymbol;

    // Agent A edits foo -> acquires the lock.
    let a = ctx(&coord, "A");
    let r = edit
        .execute(
            serde_json::json!({"path": "lib.rs", "symbol": "foo", "new_source": "fn foo() {\n    let a = 42;\n}"}),
            &a,
        )
        .await
        .unwrap();
    assert!(r.contains("staged symbol edit"), "{r}");

    // Agent B cannot touch foo (locked by A).
    let b = ctx(&coord, "B");
    let err = edit
        .execute(
            serde_json::json!({"path": "lib.rs", "symbol": "foo", "new_source": "fn foo() {}"}),
            &b,
        )
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("locked by A"), "{err}");

    // But B can edit a different symbol concurrently.
    edit.execute(
        serde_json::json!({"path": "lib.rs", "symbol": "bar", "new_source": "fn bar() {\n    let b = 99;\n}"}),
        &b,
    )
    .await
    .unwrap();

    // Overlay read reflects the staged edit before commit.
    let overlay = coord.buffer.read("lib.rs").unwrap().unwrap();
    assert!(overlay.contains("let a = 42"), "{overlay}");
    assert!(overlay.contains("let b = 99"), "{overlay}");

    // Disk is still untouched.
    let on_disk = std::fs::read_to_string(ws.join("lib.rs")).unwrap();
    assert!(on_disk.contains("let a = 1"));

    // Commit flushes to disk and releases A's locks.
    let (paths, _validation) = coord.commit("A").await.unwrap();
    assert_eq!(paths, vec![PathBuf::from("lib.rs")]);
    let committed = std::fs::read_to_string(ws.join("lib.rs")).unwrap();
    assert!(committed.contains("let a = 42"), "{committed}");

    std::fs::remove_dir_all(&ws).ok();
}

#[tokio::test]
async fn symbol_level_buffer_commits_each_symbol_independently() {
    let ws = temp_workspace();
    std::fs::write(
        ws.join("lib.rs"),
        "fn foo() {\n    let a = 1;\n}\n\nfn bar() {\n    let b = 2;\n}\n",
    )
    .unwrap();
    let coord = Coordinator::new(ws.clone());

    // A edits foo, B edits bar — different symbols, same file.
    coord
        .buffer
        .stage_symbol_edit("lib.rs", "foo", "fn foo() {\n    let a = 42;\n}".into(), "A");
    coord
        .buffer
        .stage_symbol_edit("lib.rs", "bar", "fn bar() {\n    let b = 99;\n}".into(), "B");

    // A commits FIRST: only foo lands; bar stays at its original on disk.
    coord.commit("A").await.unwrap();
    let after_a = std::fs::read_to_string(ws.join("lib.rs")).unwrap();
    assert!(after_a.contains("let a = 42"), "{after_a}");
    assert!(after_a.contains("let b = 2"), "bar must be untouched:\n{after_a}");
    assert!(coord.buffer.has_pending("B"));

    // B commits: bar's edit re-locates against the shifted file and lands.
    coord.commit("B").await.unwrap();
    let after_b = std::fs::read_to_string(ws.join("lib.rs")).unwrap();
    assert!(after_b.contains("let a = 42"), "{after_b}");
    assert!(after_b.contains("let b = 99"), "{after_b}");

    std::fs::remove_dir_all(&ws).ok();
}

#[tokio::test]
async fn search_finds_matches_over_overlay() {
    let ws = temp_workspace();
    std::fs::write(ws.join("a.rs"), "fn alpha() {}\nfn beta() {}\n").unwrap();
    std::fs::create_dir_all(ws.join("target")).unwrap();
    std::fs::write(ws.join("target/ignored.rs"), "fn alpha_ignored() {}\n").unwrap();

    let coord = Coordinator::new(ws.clone());
    let c = ctx(&coord, "A");

    let out = Search
        .execute(serde_json::json!({"pattern": "fn alpha", "ext": "rs"}), &c)
        .await
        .unwrap();
    assert!(out.contains("a.rs:1"), "{out}");
    // target/ is ignored by the walker.
    assert!(!out.contains("ignored"), "{out}");

    std::fs::remove_dir_all(&ws).ok();
}
