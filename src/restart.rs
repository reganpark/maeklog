//! User-triggered activation is bounded; focus/startup never re-enable a Space.
use crate::model::{CurrentState, EventKind, TurnEvent};
use crate::storage::{
    SpaceStore, atomic_write_json, atomic_write_json_lines, lock_file, read_json_optional,
};
use crate::{AppResult, config, participation, retry, transcript};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Boundary {
    pub first_seq: u64,
    pub last_seq: Option<u64>,
}

pub fn boundary(store: &SpaceStore) -> AppResult<Option<Boundary>> {
    let value: Option<Boundary> = read_json_optional(&store.path("restart.json"))?;
    let cursor = store.current()?.source_seq;
    Ok(value.filter(|b| b.last_seq.is_none_or(|last| last > cursor)))
}

pub fn prepare(store: &SpaceStore, session_id: Option<&str>) -> AppResult<()> {
    // Off first cancels old work. Wait for its lease before replacing the queue.
    let _worker = lock_file(&store.path(".worker.lock"), false)?;
    let _participation = participation::lock(store)?;
    if participation::load(store)?.enabled {
        return Err("recent restart requires a stopped Space".into());
    }
    let mut selected = if let Some(session) = session_id {
        let meta = store.meta()?.ok_or("missing Space metadata")?;
        transcript::load_recent_turns(&config::codex_home()?, session)?
            .into_iter()
            .flat_map(|(user, assistant, at, user_at)| {
                [
                    (EventKind::UserPrompt, user, user_at),
                    (EventKind::AssistantFinal, assistant, at),
                ]
                .map(|(kind, content, at)| TurnEvent {
                    seq: 0,
                    kind,
                    session_id: session.into(),
                    turn_id: None,
                    model: None,
                    content,
                    truncated: false,
                    recorded_at_unix: at.unwrap_or(0),
                    cwd: meta.cwd.clone(),
                })
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    if selected.is_empty() {
        selected = recent_collected(&store.events()?);
    }
    let _commit = lock_file(&store.path(".commit.lock"), false)?;
    let _events = lock_file(&store.path(".event.lock"), false)?;
    crate::legacy::preserve(store)?;
    let current = store.current()?;
    let last_seq = store
        .events()?
        .last()
        .map_or(current.source_seq, |e| e.seq.max(current.source_seq));
    let first_seq = last_seq.checked_add(1).ok_or("event sequence overflow")?;
    let sources = remap_sources(store, &mut selected, first_seq)?;
    let boundary = Boundary {
        first_seq,
        last_seq: selected.last().map(|event| event.seq),
    };
    // Persist boundary first. A crash leaves the Space off, never processing an old backlog.
    atomic_write_json(&store.path("restart.json"), &boundary)?;
    atomic_write_json(&store.path("event-sources.json"), &sources)?;
    atomic_write_json_lines(&store.path("inbox.jsonl"), &selected)?;
    atomic_write_json(
        &store.path("sequence.json"),
        &serde_json::json!({"next_seq":first_seq+u64::try_from(selected.len())?}),
    )?;
    // Keep revision monotonic, but do not send/display the previous long brief on restart.
    atomic_write_json(
        &store.path("current.json"),
        &CurrentState {
            revision: current.revision + 1,
            source_seq: last_seq,
            timing: crate::model::ContextTiming {
                partial_history: true,
                ..Default::default()
            },
            ..CurrentState::default()
        },
    )?;
    atomic_write_json(
        &store.path("cursor.json"),
        &serde_json::json!({"last_processed_seq":last_seq}),
    )?;
    for file in ["latest.json", "latest-status.json", "rebuild.json"] {
        match std::fs::remove_file(store.path(file)) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    retry::reset(store)?;
    store.set_status(
        "queued",
        if selected.is_empty() {
            crate::tr!(
                "최근 완료 대화 없음 · 다음 대화부터 수집",
                "No recent completed conversations · Collecting future conversations"
            )
        } else {
            crate::tr!(
                "최근 완료 대화 최대 3턴만 정리 · 이전 대기 기록 생략",
                "Summarizing up to 3 recent completed turns · Earlier pending records omitted"
            )
        },
    )
}

fn recent_collected(events: &[TurnEvent]) -> Vec<TurnEvent> {
    let mut turns = Vec::new();
    let mut prompt: Option<TurnEvent> = None;
    for event in events {
        match event.kind {
            EventKind::UserPrompt => prompt = Some(event.clone()),
            EventKind::AssistantFinal => {
                if let Some(user) = prompt.take()
                    && user.session_id == event.session_id
                {
                    turns.push(vec![user, event.clone()]);
                }
            }
            EventKind::Interrupted => prompt = None,
        }
    }
    if let Some(last_session) = turns.last().map(|turn| turn[0].session_id.clone()) {
        turns.retain(|turn| turn[0].session_id == last_session);
    }
    if turns.len() > 3 {
        turns.drain(..turns.len() - 3);
    }
    turns.into_iter().flatten().collect()
}

fn remap_sources(
    store: &SpaceStore,
    selected: &mut [TurnEvent],
    first_seq: u64,
) -> AppResult<std::collections::BTreeMap<u64, crate::branch_context::Source>> {
    let mut sources = crate::branch_context::sources(store)?;
    for (index, event) in selected.iter_mut().enumerate() {
        let original = event.seq;
        let mut source = crate::branch_context::source(store, event)?;
        event.seq = first_seq + u64::try_from(index)?;
        if original == 0 {
            // Transcript imports have no captured Git context. Never label them
            // with the branch currently checked out during reactivation.
            source.uid = format!(
                "import-{}",
                crate::branch_context::hash(&format!(
                    "{}:{:?}:{}:{}",
                    event.session_id, event.kind, event.recorded_at_unix, event.content
                ))
            );
            source.order = event.seq;
        }
        sources.insert(event.seq, source);
        let original_chars = event.content.chars().count();
        event.content = event.content.chars().take(2400).collect();
        event.truncated |= original_chars > 2400;
    }
    Ok(sources)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::NewTurnEvent;

    #[test]
    fn recent_queue_is_bounded_fresh_and_crash_after_commit_cannot_block_new_work() {
        let root = std::env::temp_dir().join(format!("pm-restart-test-{}", std::process::id()));
        let store = SpaceStore::for_workspace(&root, "test", "w1", "/repo", None).unwrap();
        for i in 1..=11 {
            store
                .append_event(NewTurnEvent {
                    kind: if i % 2 == 1 {
                        EventKind::UserPrompt
                    } else {
                        EventKind::AssistantFinal
                    },
                    session_id: "test".into(),
                    turn_id: None,
                    model: None,
                    content: format!("event {i} {}", "x".repeat(4000)),
                    cwd: "/repo".into(),
                })
                .unwrap();
        }
        atomic_write_json(
            &store.path("current.json"),
            &CurrentState {
                revision: 7,
                summary: crate::model::Summary {
                    objective: "old expensive brief".into(),
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .unwrap();
        participation::set_enabled(&store, false).unwrap();
        prepare(&store, None).unwrap();
        assert!(!participation::load(&store).unwrap().enabled);
        let events = store.events().unwrap();
        assert_eq!(events.len(), 6);
        assert!(events[0].content.starts_with("event 5 "));
        assert!(events[5].content.starts_with("event 10 "));
        assert!(
            events
                .iter()
                .all(|e| e.content.chars().count() <= 2400 && e.truncated)
        );
        assert_eq!(store.current().unwrap().revision, 8);
        assert!(store.current().unwrap().summary.objective.is_empty());
        let end = boundary(&store).unwrap().unwrap().last_seq.unwrap();
        let mut current = store.current().unwrap();
        current.source_seq = end;
        atomic_write_json(&store.path("current.json"), &current).unwrap();
        assert!(boundary(&store).unwrap().is_none());
        std::fs::remove_dir_all(root).unwrap();
    }
}
