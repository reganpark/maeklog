use super::*;
use crate::storage::NewTurnEvent;
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};

pub(super) fn fixture() -> SpaceStore {
    static SERIAL: AtomicU64 = AtomicU64::new(0);
    let root = std::env::temp_dir().join(format!(
        "pm-live-turn-{}-{}",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    ));
    let store = SpaceStore::for_workspace(&root, "test", "w1", "/missing-live-repo", None).unwrap();
    let prompt = store
        .append_event(NewTurnEvent {
            kind: EventKind::UserPrompt,
            session_id: "thread-1".into(),
            turn_id: Some("turn-1".into()),
            model: None,
            content: "수전 모델과 렌더 확인".into(),
            cwd: "/missing-live-repo".into(),
        })
        .unwrap();
    record_hook(&store, &prompt).unwrap();
    store
}

#[allow(
    clippy::needless_pass_by_value,
    reason = "The fixture builds an owned notification from each inline JSON value"
)]
pub(super) fn notify(store: &SpaceStore, seq: u64, method: &str, params: Value) {
    apply(
        store,
        "thread-1",
        "turn-1",
        42,
        &Envelope {
            sequence: seq,
            cwd: "/missing-live-repo".into(),
            notification: json!({"method":method,"params":params}),
        },
        unix_now(),
    )
    .unwrap();
}

#[test]
fn progress_is_only_a_projection_and_duplicate_items_keep_content_revision() {
    let store = fixture();
    let inbox = fs_bytes(&store.path("inbox.jsonl"));
    notify(&store, 1, "turn/started", json!({"turn":{"id":"turn-1"}}));
    let item = json!({"item":{"type":"agentMessage","id":"report-1","phase":"commentary","text":"몸체를 생성하고 치수를 확인하고 있습니다"}});
    notify(&store, 2, "item/completed", item.clone());
    let first = load(&store).unwrap().unwrap();
    notify(&store, 3, "item/completed", item);
    let repeated = load(&store).unwrap().unwrap();
    assert_eq!(first.revision, repeated.revision);
    assert_eq!(repeated.commentary.len(), 1);
    assert_eq!(fs_bytes(&store.path("inbox.jsonl")), inbox);
    assert!(!store.path("current.json").exists());
    assert!(!store.path("live-summary.json").exists());
    std::fs::remove_dir_all(&store.state_root).unwrap();
}

fn fs_bytes(path: &std::path::Path) -> Vec<u8> {
    std::fs::read(path).unwrap()
}

#[test]
fn legacy_command_arguments_never_reach_saved_progress_or_summary_evidence() {
    let store = fixture();
    for (index, command) in [
        "TOKEN=SECRET_ENV curl -H 'Authorization: Bearer SECRET_TOKEN'",
        "sh -c 'login --password SECRET_PASSWORD'",
        "/private/SECRET_PATH/tool --config=SECRET_CONFIG",
    ]
    .iter()
    .enumerate()
    {
        notify(
            &store,
            index as u64 + 1,
            "item/completed",
            json!({"item":{"type":"commandExecution","id":format!("cmd-{index}"),
                "command":command,"status":"completed","exitCode":1}}),
        );
    }
    let state = load(&store).unwrap().unwrap();
    assert_eq!(state.activities.len(), 3);
    assert!(
        state.activities.iter().all(
            |a| a.status == "failed" && a.label == crate::tr!("명령 실행", "Command execution")
        )
    );
    let saved = std::fs::read_to_string(store.path("live-turn.json")).unwrap();
    assert!(!saved.contains("SECRET"));
    // Queue through the public API without starting a model worker.
    let _worker = lock_file(&store.state_root.join("worker.lock"), false).unwrap();
    crate::live_summary::request(&store).unwrap();
    let summary = std::fs::read_to_string(store.path("live-summary.json")).unwrap();
    assert!(!summary.contains("SECRET"));
    std::fs::remove_dir_all(&store.state_root).unwrap();
}

#[test]
fn late_tool_start_and_late_progress_cannot_reopen_completed_work() {
    let store = fixture();
    notify(
        &store,
        1,
        "item/completed",
        json!({"item":{"type":"commandExecution","id":"tool-1","command":"render","status":"completed","exitCode":0}}),
    );
    let revision = load(&store).unwrap().unwrap().revision;
    notify(
        &store,
        2,
        "item/started",
        json!({"item":{"type":"commandExecution","id":"tool-1","command":"render","status":"inProgress"}}),
    );
    assert_eq!(load(&store).unwrap().unwrap().revision, revision);
    notify(
        &store,
        3,
        "turn/completed",
        json!({"turn":{"id":"turn-1","status":"completed"}}),
    );
    let ended = load(&store).unwrap().unwrap();
    notify(
        &store,
        4,
        "item/completed",
        json!({"item":{"type":"agentMessage","id":"late","text":"late"}}),
    );
    assert_eq!(load(&store).unwrap().unwrap(), ended);
    std::fs::remove_dir_all(&store.state_root).unwrap();
}

#[test]
fn normalized_long_labels_are_idempotent_and_the_latest_completion_is_visible() {
    let store = fixture();
    let tool = json!({"item":{"type":"mcpToolCall","id":"tool-a","server":"a".repeat(160),"tool":"b".repeat(160),"status":"inProgress"}});
    notify(&store, 1, "item/started", tool.clone());
    let revision = load(&store).unwrap().unwrap().revision;
    notify(&store, 2, "item/started", tool);
    assert_eq!(load(&store).unwrap().unwrap().revision, revision);
    notify(
        &store,
        3,
        "item/completed",
        json!({"item":{"type":"mcpToolCall","id":"tool-b","server":"Blender","tool":"preview","status":"completed"}}),
    );
    notify(
        &store,
        4,
        "item/completed",
        json!({"item":{"type":"mcpToolCall","id":"tool-a","server":"a".repeat(160),"tool":"b".repeat(160),"status":"completed"}}),
    );
    assert_eq!(
        load(&store).unwrap().unwrap().activities.last().unwrap().id,
        "tool-a"
    );
    std::fs::remove_dir_all(&store.state_root).unwrap();
}

#[test]
fn bounded_history_marks_omissions_and_never_collects_reasoning_or_final() {
    let store = fixture();
    for seq in 1..=30 {
        notify(
            &store,
            seq,
            "item/completed",
            json!({"item":{"type":"agentMessage","id":format!("report-{seq}"),"text":"progress"}}),
        );
    }
    notify(
        &store,
        31,
        "item/completed",
        json!({"item":{"type":"reasoning","id":"hidden","summary":"secret"}}),
    );
    notify(
        &store,
        32,
        "item/completed",
        json!({"item":{"type":"agentMessage","id":"final","phase":"final_answer","text":"final"}}),
    );
    let state = load(&store).unwrap().unwrap();
    assert_eq!(state.commentary.len(), MAX_REPORTS);
    assert!(state.truncated);
    assert!(
        state
            .commentary
            .iter()
            .all(|r| r.id != "hidden" && r.id != "final")
    );
    std::fs::remove_dir_all(&store.state_root).unwrap();
}

#[test]
fn opt_out_and_completed_summary_hide_the_transient_projection() {
    let store = fixture();
    participation::set_enabled(&store, false).unwrap();
    assert!(load(&store).unwrap().is_none());
    participation::set_enabled(&store, true).unwrap();
    assert!(load(&store).unwrap().is_none());
    let prompt = store
        .append_event(NewTurnEvent {
            kind: EventKind::UserPrompt,
            session_id: "thread-1".into(),
            turn_id: Some("turn-2".into()),
            model: None,
            content: "새 요청".into(),
            cwd: "/missing-live-repo".into(),
        })
        .unwrap();
    record_hook(&store, &prompt).unwrap();
    let final_event = store
        .append_event(NewTurnEvent {
            kind: EventKind::AssistantFinal,
            session_id: "thread-1".into(),
            turn_id: Some("turn-2".into()),
            model: None,
            content: "완료 보고".into(),
            cwd: "/missing-live-repo".into(),
        })
        .unwrap();
    record_hook(&store, &final_event).unwrap();
    assert!(load(&store).unwrap().unwrap().ended);
    let mut selection = crate::daily::Selection::default();
    selection.done.insert(final_event.seq);
    atomic_write_json(&store.path("selected-current.json"), &selection).unwrap();
    assert!(load(&store).unwrap().is_none());
    std::fs::remove_dir_all(&store.state_root).unwrap();
}

#[test]
fn duplicate_hooks_cannot_reopen_an_already_processed_terminal_projection() {
    for kind in [EventKind::AssistantFinal, EventKind::Interrupted] {
        let store = fixture();
        let append = || {
            store
                .append_event(NewTurnEvent {
                    kind: kind.clone(),
                    session_id: "thread-1".into(),
                    turn_id: Some("turn-1".into()),
                    model: None,
                    content: "終了報告".into(),
                    cwd: "/missing-live-repo".into(),
                })
                .unwrap()
        };
        let original = append();
        record_hook(&store, &original).unwrap();
        atomic_write_json(
            &store.path("selected-current.json"),
            &crate::daily::Selection {
                done: [original.seq].into(),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(load(&store).unwrap().is_none());
        let before = fs_bytes(&store.path("live-turn.json"));
        let duplicate = append();
        record_hook(&store, &duplicate).unwrap();
        assert_eq!(fs_bytes(&store.path("live-turn.json")), before);
        assert!(load(&store).unwrap().is_none());
        std::fs::remove_dir_all(&store.state_root).unwrap();
    }
}
