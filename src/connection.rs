//! Hook session ids are authoritative; stale Herdr resume arguments must not replace them.
use crate::AppResult;
use crate::storage::{SpaceStore, atomic_write_json, lock_file, read_json_or_default, unix_now};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Connection {
    pub session_id: String,
    pub prior_sessions: Vec<String>,
    pub started_at: u64,
    pub last_event: String,
    // Distinguish an actual SessionStart delivery from UserPrompt/Stop fallback reconnection.
    pub session_start_id: Option<String>,
    pub session_start_at: Option<u64>,
}

pub fn record(store: &SpaceStore, session_id: &str, event: &str) -> AppResult<()> {
    if session_id.is_empty() || session_id.len() > 128 {
        return Err("invalid hook session id".into());
    }
    let _lock = lock_file(&store.path(".connection.lock"), false)?;
    let mut connection: Connection = read_json_or_default(&store.path("connection.json"))?;
    if event != "SessionStart"
        && connection.session_id != session_id
        && connection
            .prior_sessions
            .iter()
            .any(|old| old == session_id)
    {
        return Ok(()); // A late old-session Stop must not reconnect the panel backwards.
    }
    if connection.session_id != session_id {
        if !connection.session_id.is_empty() {
            connection
                .prior_sessions
                .push(connection.session_id.clone());
        } else if let Some(previous) = store.latest_session_id()?
            && previous != session_id
        {
            connection.prior_sessions.push(previous);
        }
        let count = connection.prior_sessions.len();
        if count > 8 {
            connection.prior_sessions.drain(..count - 8);
        }
        connection.session_id = session_id.into();
        connection.prior_sessions.retain(|old| old != session_id);
        connection.started_at = unix_now();
    }
    if event == "SessionStart" {
        connection.session_start_id = Some(session_id.into());
        connection.session_start_at = Some(unix_now());
    }
    connection.last_event = event.into();
    atomic_write_json(&store.path("connection.json"), &connection)?;
    store.remember_session(session_id)
}

pub fn notice(store: &SpaceStore, live_session: Option<&str>) -> AppResult<Option<String>> {
    let connection: Connection = read_json_or_default(&store.path("connection.json"))?;
    let remembered = store.latest_session_id()?;
    let known = if connection.session_id.is_empty() {
        remembered.as_deref()
    } else {
        Some(connection.session_id.as_str())
    };
    if live_session.is_some_and(|live| {
        Some(live) != known && !connection.prior_sessions.iter().any(|old| old == live)
    }) {
        return Ok(Some(
            crate::tr!(
                "[연결 확인] 새 세션의 대화 수신 미확인 · Codex /hooks 확인",
                "[Check connection] New-session conversation not received · Check Codex /hooks"
            )
            .into(),
        ));
    }
    if connection.last_event == "SessionStart" {
        return Ok(Some(
            crate::tr!("[새 세션] 연결됨 · 기존 맥락 유지 · 첫 대화 대기", "[New session] Connected · Existing context retained · Waiting for first conversation").into(),
        ));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::EventKind;
    use crate::storage::NewTurnEvent;

    #[test]
    fn new_session_keeps_memory_and_beats_old_queued_events() {
        let root = std::env::temp_dir().join(format!("pm-connection-{}", std::process::id()));
        let store = SpaceStore::for_workspace(&root, "test", "w1", "/repo", None).unwrap();
        store
            .commit_rebuild(
                0,
                crate::model::Summary {
                    objective: "프로젝트 목표".into(),
                    ..Default::default()
                },
                crate::model::RepoSnapshot::default(),
                crate::model::ContextTiming::default(),
            )
            .unwrap();
        record(&store, "old", "UserPromptSubmit").unwrap();
        store
            .append_event(NewTurnEvent {
                kind: EventKind::UserPrompt,
                session_id: "old".into(),
                turn_id: None,
                model: None,
                content: "old work".into(),
                cwd: "/repo".into(),
            })
            .unwrap();
        record(&store, "new", "SessionStart").unwrap();
        let start: Connection = read_json_or_default(&store.path("connection.json")).unwrap();
        assert_eq!(start.session_start_id.as_deref(), Some("new"));
        assert!(start.session_start_at.is_some());
        assert_eq!(store.latest_session_id().unwrap().as_deref(), Some("new"));
        assert_eq!(store.current().unwrap().summary.objective, "프로젝트 목표");
        assert_eq!(store.events().unwrap().len(), 1);
        assert!(
            notice(&store, Some("old"))
                .unwrap()
                .unwrap()
                .contains(crate::tr!("첫 대화 대기", "Waiting for first conversation"))
        );
        record(&store, "old", "Stop").unwrap();
        assert_eq!(store.latest_session_id().unwrap().as_deref(), Some("new"));
        record(&store, "new", "UserPromptSubmit").unwrap();
        let after: Connection = read_json_or_default(&store.path("connection.json")).unwrap();
        assert_eq!(after.session_start_id, start.session_start_id);
        assert_eq!(after.session_start_at, start.session_start_at);
        assert!(notice(&store, Some("new")).unwrap().is_none());
        record(&store, "old", "SessionStart").unwrap();
        record(&store, "old", "UserPromptSubmit").unwrap();
        assert!(notice(&store, Some("old")).unwrap().is_none());
        assert!(
            notice(&store, Some("third"))
                .unwrap()
                .unwrap()
                .contains(crate::tr!("수신 미확인", "conversation not received"))
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
