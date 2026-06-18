//! Tests for author-scoped commits and the long-term memory store (no LLM).

use std::path::PathBuf;

use swarm_core::coordinator::Coordinator;
use swarm_core::memory::MemoryStore;
use swarm_core::skills::SkillStore;

fn temp_workspace(tag: &str) -> PathBuf {
    use std::time::{SystemTime, UNIX_EPOCH};
    let n = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("swarm-{tag}-{n}"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[tokio::test]
async fn commit_is_author_scoped() {
    let ws = temp_workspace("commit");
    let coord = Coordinator::new(ws.clone());

    coord.buffer.stage_write("a.txt", "from A".into(), "A");
    coord.buffer.stage_write("b.txt", "from B".into(), "B");

    // A commits: only a.txt lands; B's work is untouched and still pending.
    let (paths, _) = coord.commit("A").await.unwrap();
    assert_eq!(paths, vec![PathBuf::from("a.txt")]);
    assert_eq!(std::fs::read_to_string(ws.join("a.txt")).unwrap(), "from A");
    assert!(!ws.join("b.txt").exists());
    assert!(coord.buffer.has_pending("B"));
    assert!(!coord.buffer.has_pending("A"));

    // B commits independently.
    let (paths, _) = coord.commit("B").await.unwrap();
    assert_eq!(paths, vec![PathBuf::from("b.txt")]);
    assert_eq!(std::fs::read_to_string(ws.join("b.txt")).unwrap(), "from B");

    std::fs::remove_dir_all(&ws).ok();
}

#[test]
fn memory_store_persists_and_recalls() {
    let ws = temp_workspace("mem");

    {
        let store = MemoryStore::load(&ws);
        store.remember("auth lives in crates/auth/src/lib.rs", vec!["auth".into()], "A");
        store.remember("the project uses tokio for async", vec!["async".into()], "A");
        assert!(!store.is_empty());

        // Keyword recall ranks the relevant note first.
        let hits = store.recall(Some("auth"), 8);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].text.contains("auth lives in"));

        assert!(store.digest(10).contains("tokio"));
    }

    // A fresh load sees the persisted entries.
    let reloaded = MemoryStore::load(&ws);
    assert_eq!(reloaded.recall(None, 10).len(), 2);

    std::fs::remove_dir_all(&ws).ok();
}

#[test]
fn skills_can_be_created_and_used_at_runtime() {
    let ws = temp_workspace("skills");
    let store = SkillStore::new(&ws);
    assert!(store.is_empty());

    store
        .create("Make Release", "How to cut a release", "1. bump version\n2. tag")
        .unwrap();

    let list = store.list();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].name, "make-release");
    assert_eq!(list[0].description, "How to cut a release");

    let skill = store.get("make-release").expect("skill");
    assert!(skill.instructions.contains("bump version"), "{}", skill.instructions);

    // A fresh store sees it (persisted as a file).
    assert!(!SkillStore::new(&ws).is_empty());

    std::fs::remove_dir_all(&ws).ok();
}
