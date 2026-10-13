//! Local calendar policy. Dates/selection never cause a model or GitHub call.

use crate::{
    AppResult,
    model::{CurrentState, EventKind, TurnEvent},
    storage::{SpaceStore, atomic_write_json, lock_file, read_json_or_default, unix_now},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;
use std::time::Duration;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Selection {
    pub current: CurrentState,
    pub done: BTreeSet<u64>,
    pub display_seq: u64,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Manual {
    pub day: String,
    pub seqs: BTreeSet<u64>,
    pub generation: u64,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Restrictions {
    pub manual_only: BTreeSet<u64>,
    pub restart_capped: BTreeSet<u64>,
}
#[derive(Clone, Debug)]
pub struct Turn {
    pub day: String,
    pub events: Vec<TurnEvent>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Queue {
    pub today: String,
    pub today_turns: usize,
    pub manual: BTreeMap<String, usize>,
    pub open_events: usize,
    pub requested: Option<String>,
}
pub fn day_at(at: u64) -> Option<String> {
    if at == 0 {
        return None;
    }
    let value = crate::local_time::LocalTimes::new([at]).display(Some(at));
    let date = value.get(..10)?;
    (date.as_bytes().get(4) == Some(&b'/')).then(|| date.replace('/', "-"))
}
pub fn today() -> AppResult<String> {
    day_at(unix_now()).ok_or_else(|| {
        crate::tr!(
            "로컬 날짜 확인 실패 · 자동 요약 중지",
            "Local date unavailable · Automatic summary stopped"
        )
        .into()
    })
}
pub fn end_of_day(day: &str) -> AppResult<u64> {
    let mut command = Command::new("date");
    #[cfg(not(target_os = "macos"))]
    command.args(["-d", &format!("{day} tomorrow"), "+%s"]);
    #[cfg(target_os = "macos")]
    command.args(["-j", "-f", "%Y-%m-%d", "-v+1d", day, "+%s"]);
    let out = crate::process::run_with_timeout(&mut command, None, Duration::from_secs(3), 1024)?;
    if !out.success {
        return Err("local midnight unavailable; automatic processing paused".into());
    }
    Ok(out.stdout.trim().parse()?)
}
pub fn turns(events: &[TurnEvent]) -> Vec<Turn> {
    let times = crate::local_time::LocalTimes::new(
        events
            .iter()
            .map(|e| e.recorded_at_unix)
            .filter(|at| *at > 0),
    );
    let mut turns = Vec::new();
    let mut pending: Vec<TurnEvent> = Vec::new();
    for event in events {
        if pending
            .last()
            .is_some_and(|p| p.session_id != event.session_id)
        {
            pending.clear();
        }
        pending.push(event.clone());
        if matches!(
            event.kind,
            EventKind::AssistantFinal | EventKind::Interrupted
        ) {
            let dates: BTreeSet<_> = pending
                .iter()
                .filter_map(|e| {
                    let value =
                        times.display((e.recorded_at_unix > 0).then_some(e.recorded_at_unix));
                    let day = value.get(..10)?;
                    (day.as_bytes().get(4) == Some(&b'/')).then(|| day.replace('/', "-"))
                })
                .collect();
            let day = if dates.len() == 1 && pending.iter().all(|e| e.recorded_at_unix > 0) {
                dates.first().cloned().unwrap_or_default()
            } else {
                "unknown".into()
            };
            turns.push(Turn {
                day,
                events: std::mem::take(&mut pending),
            });
        }
    }
    turns
}
pub fn request(store: &SpaceStore, day: &str) -> AppResult<()> {
    let _participation = crate::participation::lock(store)?;
    crate::participation::ensure_enabled(store)?;
    let _events = lock_file(&store.path(".event.lock"), false)?;
    let queue = queue(store)?;
    if !queue.manual.contains_key(day) {
        return Err(crate::tr!(
            "선택한 날짜에 수동 처리할 완료 대화가 없습니다",
            "No completed conversations to process for the selected date"
        )
        .into());
    }
    let restrictions: Restrictions = read_json_or_default(&store.path("daily-restrictions.json"))?;
    let seqs = turns(&store.unprocessed_events()?)
        .into_iter()
        .filter(|t| {
            t.day == day
                && (day != queue.today
                    || t.events
                        .iter()
                        .any(|e| restrictions.manual_only.contains(&e.seq)))
        })
        .flat_map(|t| t.events.into_iter().map(|e| e.seq))
        .collect();
    atomic_write_json(
        &store.path("manual-day.json"),
        &Manual {
            day: day.into(),
            seqs,
            generation: crate::participation::load(store)?.generation,
        },
    )?;
    crate::retry::reset(store)?;
    store.set_status(
        "queued",
        crate::tr!(
            "선택한 날짜만 수동 요약 대기",
            "Selected date queued for manual summary"
        ),
    )
}
pub fn manual(store: &SpaceStore) -> AppResult<Manual> {
    let mut value: Manual = read_json_or_default(&store.path("manual-day.json"))?;
    if value.generation != crate::participation::load(store)?.generation {
        return Ok(Manual::default());
    }
    let pending: BTreeSet<_> = store
        .unprocessed_events()?
        .into_iter()
        .map(|e| e.seq)
        .collect();
    value.seqs.retain(|seq| pending.contains(seq));
    Ok(value)
}
pub fn queue(store: &SpaceStore) -> AppResult<Queue> {
    let today = today()?;
    let events = store.unprocessed_events()?;
    let restrictions: Restrictions = read_json_or_default(&store.path("daily-restrictions.json"))?;
    let turns = turns(&events);
    let complete: usize = turns.iter().map(|t| t.events.len()).sum();
    let mut today_turns = 0;
    let mut manual_counts = BTreeMap::new();
    for t in turns {
        if t.day == today
            && !t
                .events
                .iter()
                .any(|e| restrictions.manual_only.contains(&e.seq))
        {
            today_turns += 1;
        } else {
            *manual_counts.entry(t.day).or_insert(0) += 1;
        }
    }
    let request = manual(store)?;
    Ok(Queue {
        today,
        today_turns,
        manual: manual_counts,
        open_events: events.len().saturating_sub(complete),
        requested: (!request.seqs.is_empty()).then_some(request.day),
    })
}
pub fn eligible(store: &SpaceStore) -> AppResult<(Vec<TurnEvent>, bool)> {
    let today = today()?;
    let all = turns(&store.unprocessed_events()?);
    let restrictions: Restrictions = read_json_or_default(&store.path("daily-restrictions.json"))?;
    let mut automatic: Vec<_> = all
        .iter()
        .filter(|t| {
            t.day == today
                && !t
                    .events
                    .iter()
                    .any(|e| restrictions.manual_only.contains(&e.seq))
        })
        .cloned()
        .collect();
    let requested = manual(store)?;
    let manual = if requested.seqs.is_empty() {
        false
    } else {
        automatic = all
            .into_iter()
            .filter(|t| t.events.iter().all(|e| requested.seqs.contains(&e.seq)))
            .collect();
        true
    };
    // Newest completed turn first; keep message order inside each bounded batch.
    automatic.sort_by_key(|t| t.events.last().map(|e| (e.recorded_at_unix, e.seq)));
    Ok((
        automatic.into_iter().flat_map(|t| t.events).collect(),
        manual,
    ))
}
pub fn selected(store: &SpaceStore, small: bool) -> AppResult<(Vec<TurnEvent>, bool)> {
    let (events, manual) = eligible(store)?;
    Ok((
        crate::branch_context::same_context_batch(
            store,
            crate::batch::newest(&events, small),
            true,
        )?,
        manual,
    ))
}
pub fn key(store: &SpaceStore) -> AppResult<Option<String>> {
    let (events, manual) = selected(store, false)?;
    Ok(events.last().map(|last| {
        format!(
            "daily:{}:{}",
            if manual { "manual" } else { "today" },
            last.seq
        )
    }))
}

pub fn status(store: &SpaceStore) -> AppResult<crate::model::WorkerStatus> {
    let mut status = store.status()?;
    let Some(key) = key(store)? else {
        status.state = "idle".into();
        status.message.clear();
        return Ok(status);
    };
    let budget = crate::retry::load(store, &key)?;
    if budget.exhausted() {
        status.state = "error".into();
        status.message = budget.stop_message();
    } else if status.state != "summarizing" {
        status.state = if budget.attempts > 0 && budget.next_at > unix_now() {
            "retry_wait"
        } else {
            "queued"
        }
        .into();
    }
    Ok(status)
}
pub fn resume_three(store: &SpaceStore) -> AppResult<()> {
    let _worker = lock_file(&store.path(".worker.lock"), false)?;
    let _participation = crate::participation::lock(store)?;
    if crate::participation::load(store)?.enabled {
        return Err("recent restart requires a stopped Space".into());
    }
    let _events = lock_file(&store.path(".event.lock"), false)?;
    let today = today()?;
    let all = turns(&store.unprocessed_events()?);
    let mut today_turns: Vec<_> = all.into_iter().filter(|t| t.day == today).collect();
    today_turns.sort_by_key(|t| t.events.last().map(|e| (e.recorded_at_unix, e.seq)));
    let keep = today_turns.len().saturating_sub(3);
    let mut restrictions: Restrictions =
        read_json_or_default(&store.path("daily-restrictions.json"))?;
    restrictions.restart_capped.clear();
    for (index, t) in today_turns.into_iter().enumerate() {
        for e in t.events {
            if index < keep {
                restrictions.manual_only.insert(e.seq);
            } else {
                restrictions.manual_only.remove(&e.seq);
                restrictions.restart_capped.insert(e.seq);
            }
        }
    }
    atomic_write_json(&store.path("daily-restrictions.json"), &restrictions)?;
    atomic_write_json(&store.path("manual-day.json"), &Manual::default())?;
    crate::retry::reset(store)?;
    store.set_status(
        "queued",
        crate::tr!(
            "오늘 최근 완료 최대 3턴 · 나머지는 사용자 요청 대기",
            "Up to 3 recent completed turns today · Others await your request"
        ),
    )
}

pub fn model_events(store: &SpaceStore, events: &[TurnEvent]) -> AppResult<Vec<TurnEvent>> {
    let restrictions: Restrictions = read_json_or_default(&store.path("daily-restrictions.json"))?;
    let capped = events
        .iter()
        .any(|e| restrictions.restart_capped.contains(&e.seq));
    let limit = 2400.min(14400 / events.len().max(1));
    Ok(events
        .iter()
        .map(|event| {
            let mut value = event.clone();
            if capped {
                value.content = event.content.chars().take(limit).collect();
                value.truncated |= value.content != event.content;
            }
            value
        })
        .collect())
}

pub fn cli(args: &[String]) -> AppResult<()> {
    let root = crate::config::state_root()?;
    let store = SpaceStore::for_caller(&root)?;
    match args.first().map_or("list", String::as_str) {
        "list" => println!("{}", serde_json::to_string_pretty(&queue(&store)?)?),
        "request" => crate::worker::request_day(
            &store,
            args.get(1).ok_or("choose an exact date from daily list")?,
        )?,
        "resume" => {
            if !crate::participation::load(&store)?.enabled {
                resume_three(&store)?;
                crate::participation::set_enabled(&store, true)?;
            }
        }
        _ => {
            return Err(
                "use daily list | daily request YYYY-MM-DD (or unknown) | daily resume".into(),
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ContextTiming, RepoSnapshot, Summary};
    use crate::storage::atomic_write_json_lines;
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    struct Fixture {
        root: std::path::PathBuf,
        store: SpaceStore,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "pm-daily-test-{}-{}",
                std::process::id(),
                SEQ.fetch_add(1, Ordering::Relaxed)
            ));
            let store = SpaceStore::for_workspace(&root, "test", "one", "/fixture", None).unwrap();
            Self { root, store }
        }
        fn write(&self, events: &[TurnEvent]) {
            atomic_write_json_lines(&self.store.path("inbox.jsonl"), events).unwrap();
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.root).unwrap();
        }
    }
    fn event(seq: u64, at: u64) -> TurnEvent {
        TurnEvent {
            seq,
            kind: if seq % 2 == 1 {
                EventKind::UserPrompt
            } else {
                EventKind::AssistantFinal
            },
            session_id: "fixture".into(),
            turn_id: Some(format!("turn-{}", seq.div_ceil(2))),
            model: None,
            content: format!("event {seq}"),
            truncated: false,
            recorded_at_unix: at,
            cwd: "/fixture".into(),
        }
    }
    #[test]
    fn midnight_unknown_and_cross_day_never_become_automatic() {
        let fixture = Fixture::new();
        let now = unix_now();
        let end = end_of_day(&today().unwrap()).unwrap();
        assert_ne!(day_at(end - 1), day_at(end));
        fixture.write(&[
            event(1, now - 3 * 86400),
            event(2, now - 3 * 86400),
            event(3, 0),
            event(4, 0),
            event(5, end - 1),
            event(6, end),
            event(7, now),
            event(8, now),
        ]);
        let (eligible, manual) = selected(&fixture.store, false).unwrap();
        assert!(!manual);
        assert_eq!(
            eligible.iter().map(|e| e.seq).collect::<Vec<_>>(),
            vec![7, 8]
        );
        let state = queue(&fixture.store).unwrap();
        assert_eq!(state.today_turns, 1);
        assert_eq!(state.manual["unknown"], 2);
    }
    #[test]
    fn newest_commit_never_erases_older_input_and_manual_does_not_regress_snapshot() {
        let fixture = Fixture::new();
        let now = unix_now();
        let events: Vec<_> = (1..=14)
            .map(|seq| event(seq, if seq <= 4 { now - 3 * 86400 } else { now }))
            .collect();
        fixture.write(&events);
        let (first, _) = selected(&fixture.store, false).unwrap();
        assert_eq!(
            first.iter().map(|e| e.seq).collect::<Vec<_>>(),
            (7..=14).collect::<Vec<_>>()
        );
        fixture
            .store
            .commit_selected(
                0,
                &first,
                Summary {
                    objective: "newest".into(),
                    ..Default::default()
                },
                RepoSnapshot::default(),
                ContextTiming::default(),
            )
            .unwrap();
        assert_eq!(fixture.store.current().unwrap().source_seq, 0);
        assert_eq!(fixture.store.unprocessed_events().unwrap().len(), 6);
        let (second, _) = selected(&fixture.store, false).unwrap();
        assert_eq!(second.iter().map(|e| e.seq).collect::<Vec<_>>(), vec![5, 6]);
        fixture
            .store
            .commit_selected(
                1,
                &second,
                Summary {
                    objective: "older today".into(),
                    ..Default::default()
                },
                RepoSnapshot::default(),
                ContextTiming::default(),
            )
            .unwrap();
        assert!(selected(&fixture.store, false).unwrap().0.is_empty());
        let day = day_at(now - 3 * 86400).unwrap();
        request(&fixture.store, &day).unwrap();
        let (older, is_manual) = selected(&fixture.store, false).unwrap();
        assert!(is_manual);
        assert_eq!(older.len(), 4);
        fixture
            .store
            .commit_selected(
                2,
                &older,
                Summary {
                    objective: "previous day".into(),
                    ..Default::default()
                },
                RepoSnapshot::default(),
                ContextTiming::default(),
            )
            .unwrap();
        assert_eq!(fixture.store.current().unwrap().summary.objective, "newest");
        assert_eq!(fixture.store.current().unwrap().source_seq, 14);
        assert!(fixture.store.unprocessed_events().unwrap().is_empty());
    }
    #[test]
    fn sparse_receipts_survive_stale_raw_file_and_requests_freeze_event_ids() {
        let fixture = Fixture::new();
        let now = unix_now();
        let old = now - 3 * 86400;
        let events = vec![event(1, old), event(2, old), event(3, now), event(4, now)];
        fixture.write(&events);
        fixture
            .store
            .commit_selected(
                0,
                &events[2..],
                Summary::default(),
                RepoSnapshot::default(),
                ContextTiming::default(),
            )
            .unwrap();
        fixture.write(&events); // crash after atomic selection state, before inbox rewrite
        assert_eq!(fixture.store.unprocessed_events().unwrap().len(), 2);
        request(&fixture.store, &day_at(old).unwrap()).unwrap();
        let mut late = events;
        late.extend([event(5, old), event(6, old)]);
        fixture.write(&late);
        assert_eq!(
            selected(&fixture.store, false)
                .unwrap()
                .0
                .iter()
                .map(|e| e.seq)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        crate::participation::set_enabled(&fixture.store, false).unwrap();
        crate::participation::set_enabled(&fixture.store, true).unwrap();
        assert!(
            selected(&fixture.store, false).unwrap().0.is_empty(),
            "opt-out invalidates manual authorization"
        );
    }
    #[test]
    fn reactivation_keeps_three_today_turns_without_deleting_other_dates() {
        let fixture = Fixture::new();
        let now = unix_now();
        let events: Vec<_> = (1..=16)
            .map(|seq| event(seq, if seq <= 4 { now - 3 * 86400 } else { now }))
            .collect();
        fixture.write(&events);
        crate::participation::set_enabled(&fixture.store, false).unwrap();
        resume_three(&fixture.store).unwrap();
        assert_eq!(fixture.store.events().unwrap().len(), 16);
        assert_eq!(queue(&fixture.store).unwrap().today_turns, 3);
        let long: Vec<_> = events[10..]
            .iter()
            .map(|event| {
                let mut event = event.clone();
                event.content = "한".repeat(5000);
                event
            })
            .collect();
        let input = model_events(&fixture.store, &long).unwrap();
        assert!(
            input
                .iter()
                .all(|event| event.truncated && event.content.chars().count() <= 2400)
        );
        assert!(
            input
                .iter()
                .map(|event| event.content.chars().count())
                .sum::<usize>()
                <= 14400
        );
        assert_eq!(
            selected(&fixture.store, false)
                .unwrap()
                .0
                .iter()
                .map(|e| e.seq)
                .collect::<Vec<_>>(),
            (11..=16).collect::<Vec<_>>()
        );
        assert_eq!(
            queue(&fixture.store)
                .unwrap()
                .manual
                .values()
                .sum::<usize>(),
            5
        );
    }
    #[test]
    fn midnight_cancels_automatic_lease_and_retry_budgets_survive_new_tasks() {
        let fixture = Fixture::new();
        let mut lease = crate::participation::Lease::new(&fixture.store)
            .unwrap()
            .unwrap();
        lease.automatic_until = Some(unix_now());
        assert!(lease.cancelled());
        let mut budget = crate::retry::RetryState::default().for_key("daily:today:2");
        budget.attempts = 3;
        crate::retry::save(&fixture.store, &budget).unwrap();
        crate::retry::save(
            &fixture.store,
            &crate::retry::RetryState::default().for_key("daily:today:4"),
        )
        .unwrap();
        assert!(
            crate::retry::load(&fixture.store, "daily:today:2")
                .unwrap()
                .exhausted()
        );
    }
}
