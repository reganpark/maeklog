//! A provisional view never advances the chronological cursor or consumes its queue.
// v0.12 only reads legacy previews; the old producer is retained as a regression fixture.
#![cfg_attr(not(test), allow(unused_imports))]
use crate::AppResult;
use crate::model::{CurrentState, TurnEvent, WorkerStatus};
use crate::storage::{SpaceStore, atomic_write_json, lock_file, read_json_or_default, unix_now};
use crate::{batch, retry};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LatestContext {
    pub first_event_seq: u64,
    pub cumulative_base_revision: u64,
    pub state: CurrentState,
}

pub fn visible(store: &SpaceStore, current: &CurrentState) -> AppResult<Option<LatestContext>> {
    let latest: Option<LatestContext> = read_json_or_default(&store.path("latest.json"))?;
    Ok(latest.filter(|latest| latest.state.source_seq > current.source_seq))
}

#[cfg(test)]
pub fn pending(store: &SpaceStore) -> AppResult<Vec<TurnEvent>> {
    if crate::restart::boundary(store)?.is_some() {
        return Ok(Vec::new());
    }
    if !crate::participation::load(store)?.enabled || store.meta()?.is_none_or(|meta| meta.archived)
    {
        return Ok(Vec::new());
    }
    let current = store.current()?;
    let events = store.processable_events()?;
    let seen = visible(store, &current)?.map_or(current.source_seq, |v| v.state.source_seq);
    if events.last().is_none_or(|event| event.seq <= seen) {
        return Ok(Vec::new());
    }
    // Avoid a second model call when the entire queue fits one healthy regular reduction.
    let blocked = retry::load(store, &format!("events:{}", current.source_seq))?.exhausted();
    if !blocked
        && (crate::participation::load(store)?.history_blocked
            || store.rebuild_request()?.is_none())
        && batch::select(&events, false).len() == events.len()
    {
        return Ok(Vec::new());
    }
    Ok(events
        .into_iter()
        .filter(|event| event.seq > seen)
        .collect())
}

#[cfg(test)]
pub fn set_status(store: &SpaceStore, state: &str, message: &str) -> AppResult<()> {
    atomic_write_json(
        &store.path("latest-status.json"),
        &WorkerStatus {
            state: state.into(),
            message: message.into(),
            updated_at_unix: Some(unix_now()),
        },
    )
}

#[cfg(test)]
pub fn commit(store: &SpaceStore, latest: &LatestContext) -> AppResult<()> {
    let _lock = lock_file(&store.path(".commit.lock"), false)?;
    let _events = lock_file(&store.path(".event.lock"), false)?;
    let current = store.current()?;
    if current.revision != latest.cumulative_base_revision
        || latest.state.source_seq <= current.source_seq
        || visible(store, &current)?
            .is_some_and(|old| old.state.source_seq >= latest.state.source_seq)
    {
        return Err("stale latest context cannot replace newer memory".into());
    }
    atomic_write_json(&store.path("latest.json"), latest)?;
    set_status(store, "idle", "")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ContextTiming, EventKind, RepoSnapshot, Summary};
    use crate::storage::NewTurnEvent;

    #[test]
    fn preview_survives_restart_and_old_commits_without_consuming_any_input() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("pm-latest-test-{}", std::process::id()));
        let store = SpaceStore::for_workspace(&root, "test", "w1", "/repo", None).unwrap();
        for seq in 1..=10 {
            store
                .append_event(NewTurnEvent {
                    kind: if seq % 2 == 0 {
                        EventKind::AssistantFinal
                    } else {
                        EventKind::UserPrompt
                    },
                    session_id: "session".into(),
                    turn_id: None,
                    model: None,
                    content: format!("record {seq}"),
                    cwd: "/repo".into(),
                })
                .unwrap();
        }
        assert_eq!(pending(&store).unwrap().len(), 10);
        let value = LatestContext {
            first_event_seq: 9,
            cumulative_base_revision: 0,
            state: CurrentState {
                revision: 1,
                source_seq: 10,
                summary: Summary {
                    objective: "new decision B".into(),
                    ..Summary::default()
                },
                ..CurrentState::default()
            },
        };
        commit(&store, &value).unwrap();
        assert_eq!(store.cursor().unwrap(), 0);
        assert_eq!(store.unprocessed_events().unwrap().len(), 10);
        assert_eq!(
            std::fs::metadata(store.path("latest.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert!(commit(&store, &value).is_err());
        let reopened = SpaceStore::for_workspace(&root, "test", "w1", "/repo", None).unwrap();
        assert_eq!(
            visible(&reopened, &reopened.current().unwrap())
                .unwrap()
                .unwrap()
                .state
                .summary
                .objective,
            "new decision B"
        );
        assert!(pending(&reopened).unwrap().is_empty());
        store
            .commit_summary(
                0,
                4,
                Summary {
                    objective: "old decision A".into(),
                    ..Summary::default()
                },
                RepoSnapshot::default(),
                ContextTiming::default(),
            )
            .unwrap();
        assert_eq!(
            visible(&store, &store.current().unwrap())
                .unwrap()
                .unwrap()
                .state
                .summary
                .objective,
            "new decision B"
        );
        assert_eq!(store.unprocessed_events().unwrap().len(), 6);
        assert!(commit(&store, &value).is_err());
        store
            .commit_summary(
                1,
                10,
                value.state.summary,
                RepoSnapshot::default(),
                ContextTiming::default(),
            )
            .unwrap();
        assert!(
            visible(&store, &store.current().unwrap())
                .unwrap()
                .is_none()
        );
        assert!(store.unprocessed_events().unwrap().is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }
}
