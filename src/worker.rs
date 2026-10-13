use crate::storage::{SpaceStore, try_lock_file, unix_now};
use crate::{
    AppResult, daily,
    participation::{self, Lease},
    retry, summarizer,
};
use std::process::{Command, Stdio};
use std::time::Instant;
pub(crate) enum Attempt {
    Ran,
    Wait,
    None,
}

fn work_key(store: &SpaceStore) -> AppResult<Option<String>> {
    if !participation::load(store)?.enabled
        || store.meta()?.is_none_or(|m| m.archived)
        || store.legacy_with_tabs()?
    {
        return Ok(None);
    }
    daily::key(store)
}
pub(crate) fn next_attempt_at(store: &SpaceStore) -> AppResult<Option<u64>> {
    let Some(key) = work_key(store)? else {
        return Ok(None);
    };
    let retry = retry::load(store, &key)?;
    Ok((!retry.exhausted()).then_some(retry.next_at))
}
pub(crate) fn attempt_space(store: &SpaceStore) -> AppResult<Attempt> {
    let Some(mut lease) = Lease::new(store)? else {
        return Ok(Attempt::None);
    };
    let Some(key) = work_key(store)? else {
        return Ok(Attempt::None);
    };
    let mut budget = retry::load(store, &key)?;
    if budget.exhausted() {
        store.set_status("error", &budget.stop_message())?;
        return Ok(Attempt::None);
    }
    if budget.next_at > unix_now() {
        return Ok(Attempt::Wait);
    }
    let (events, manual) = daily::selected(store, budget.attempts > 0)?;
    if events.is_empty() {
        return Ok(Attempt::None);
    }
    if !manual {
        let selected_day = events
            .first()
            .and_then(|e| daily::day_at(e.recorded_at_unix))
            .ok_or("automatic batch has no source date")?;
        if selected_day != daily::today()? {
            return Ok(Attempt::Wait);
        }
        lease.automatic_until = Some(daily::end_of_day(&selected_day)?);
    }
    let Some(_branch) = crate::branch_context::try_reduce_lock(store, &events)? else {
        budget.next_at = unix_now() + 1;
        retry::save(store, &budget)?;
        return Ok(Attempt::Wait);
    };
    let original = budget.clone();
    budget.begin(unix_now());
    retry::save(store, &budget)?;
    let started = Instant::now();
    let result = reduce(store, &events, &lease);
    let _guard = participation::lock(store)?;
    if lease.cancelled() {
        retry::save(store, &original)?;
        store.set_status(
            "idle",
            crate::tr!(
                "날짜 또는 Space 설정 변경 · 원문 보존 · 자동 소급 안 함",
                "Date or Space settings changed · Original retained · No automatic backfill"
            ),
        )?;
        return Ok(Attempt::Ran);
    }
    record_attempt(
        store,
        serde_json::json!({"mode":if manual{"manual_day"}else{"today"},"attempt":budget.attempts,"event_count":events.len(),"event_chars":events.iter().map(|e|e.content.chars().count()).sum::<usize>(),"first_event_seq":events.first().map(|e|e.seq),"last_event_seq":events.last().map(|e|e.seq),"elapsed_ms":started.elapsed().as_millis(),"outcome":if result.is_ok(){"success"}else{"failed"}}),
    )?;
    if let Err(error) = result {
        if error
            .downcast_ref::<crate::corrections::StaleCorrections>()
            .is_some()
        {
            let mut restored = original;
            restored.next_at = unix_now() + 1;
            retry::save(store, &restored)?;
            store.set_status("idle", &error.to_string())?;
            return Ok(Attempt::Wait);
        }
        if error
            .downcast_ref::<crate::reducer_diagnostics::RejectedResponse>()
            .is_some()
        {
            budget.validation_failed(&error.to_string());
        } else {
            budget.validation_error = None;
            budget.repeated_validation = 0;
        }
        budget.failed(unix_now());
        retry::save(store, &budget)?;
        store.set_status(
            if budget.exhausted() {
                "error"
            } else {
                "retry_wait"
            },
            &crate::tr!("요약 실패 · 원문 보존 · {}", "Summary failed · Original retained · {}";
                error.to_string().chars().take(300).collect::<String>()
            ),
        )?;
    }
    Ok(Attempt::Ran)
}
fn reduce(
    store: &SpaceStore,
    events: &[crate::model::TurnEvent],
    lease: &Lease<'_>,
) -> AppResult<()> {
    let guard = lease.guard()?;
    let current = store.current()?;
    // Journal catalog supplies bounded facts. Never feed a newer Space brief or
    // bootstrap transcript to an older selected batch.
    let base = crate::model::CurrentState {
        revision: current.revision,
        source_seq: current.source_seq,
        timing: crate::model::ContextTiming {
            partial_history: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let repo = crate::branch_context::model_repo(store, events)?;
    store.set_status(
        "summarizing",
        crate::tr!(
            "선택된 완료 대화 요약 중 · 다른 날짜 자동 소급 안 함",
            "Summarizing selected completed conversations · No backfill of other dates"
        ),
    )?;
    if std::env::var_os("MAEKLOG_DISABLE_LUNA").is_some() {
        return Err("Luna execution is disabled by MAEKLOG_DISABLE_LUNA".into());
    }
    drop(guard);
    let input = if lease.automatic_until.is_some() {
        daily::model_events(store, events)?
    } else {
        events.to_vec()
    };
    let result = summarizer::summarize(
        lease,
        &base,
        &input,
        &repo,
        None,
        summarizer::Mode::Incremental,
    );
    if let Err(error) = &result
        && let Some(rejected) = error.downcast_ref::<crate::reducer_diagnostics::RejectedResponse>()
    {
        // Serialize only after confirming this generation still participates.
        // A concurrent clear/opt-out must never resurrect private diagnostics.
        let _guard = lease.guard()?;
        if crate::reducer_diagnostics::record(store, rejected, events).is_err() {
            return Err(format!("{error} · failure diagnostics could not be saved").into());
        }
    }
    let summary = result?;
    let timing = crate::timing::resolve(&base, &summary, events, None);
    let _guard = lease.guard()?;
    store.commit_selected(current.revision, events, summary, repo, timing)?;
    Ok(())
}
fn record_attempt(store: &SpaceStore, sample: serde_json::Value) -> AppResult<()> {
    let path = store.path("processing.json");
    let mut samples: Vec<serde_json::Value> = crate::storage::read_json_or_default(&path)?;
    samples.push(sample);
    if samples.len() > 50 {
        samples.drain(..samples.len() - 50);
    }
    crate::storage::atomic_write_json(&path, &samples)
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RefreshOutcome {
    Requested {
        today_turns: usize,
    },
    NoTodayWork {
        held_turns: usize,
        open_events: usize,
    },
}

pub fn request_refresh(store: &SpaceStore) -> AppResult<RefreshOutcome> {
    request_refresh_with(store, spawn_detached)
}

fn request_refresh_with(
    store: &SpaceStore,
    spawn: impl FnOnce() -> AppResult<()>,
) -> AppResult<RefreshOutcome> {
    let guard = participation::lock(store)?;
    participation::ensure_enabled(store)?;
    // r does not revive a historical manual request or authorize another date.
    crate::storage::atomic_write_json(&store.path("manual-day.json"), &daily::Manual::default())?;
    let queue = daily::queue(store)?;
    retry::reset(store)?;
    drop(guard);
    if queue.today_turns > 0 {
        spawn()?;
        Ok(RefreshOutcome::Requested {
            today_turns: queue.today_turns,
        })
    } else {
        Ok(RefreshOutcome::NoTodayWork {
            held_turns: queue.manual.values().sum(),
            open_events: queue.open_events,
        })
    }
}
pub fn request_rebuild(store: &SpaceStore) -> AppResult<()> {
    participation::ensure_enabled(store)?;
    Err(crate::tr!("전체 대화 자동 복원은 사용하지 않습니다. 패널 b에서 날짜를 선택하거나 daily request YYYY-MM-DD를 실행하세요.", "Automatic full-history restore is disabled. Select a date with b or run daily request YYYY-MM-DD.").into())
}
pub fn request_day(store: &SpaceStore, day: &str) -> AppResult<()> {
    daily::request(store, day)?;
    spawn_detached()
}
pub fn spawn_detached() -> AppResult<()> {
    use std::os::unix::process::CommandExt;
    let mut child = Command::new(std::env::current_exe()?)
        .arg("worker")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}
pub fn resume_pending(store: &SpaceStore) -> AppResult<()> {
    if pending_recovery_needed(store)? {
        spawn_detached()?;
    }
    Ok(())
}
fn pending_recovery_needed(store: &SpaceStore) -> AppResult<bool> {
    if try_lock_file(&store.path(".worker.lock"))?.is_none() {
        return Ok(false);
    }
    Ok(crate::scheduler::next_attempt_at(store)?.is_some()
        && try_lock_file(&store.state_root.join("worker.lock"))?.is_some())
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::lock_file;

    struct RefreshFixture {
        store: SpaceStore,
    }

    impl RefreshFixture {
        fn new() -> Self {
            use std::sync::atomic::{AtomicU64, Ordering};
            static SERIAL: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "pm-refresh-{}-{}",
                std::process::id(),
                SERIAL.fetch_add(1, Ordering::Relaxed)
            ));
            let store = SpaceStore::for_workspace(
                &root,
                "refresh-test",
                "workspace",
                root.to_str().unwrap(),
                None,
            )
            .unwrap();
            Self { store }
        }

        fn event(&self, kind: crate::model::EventKind, turn: &str, at: u64) -> u64 {
            let event = self
                .store
                .append_event(crate::storage::NewTurnEvent {
                    kind,
                    session_id: "refresh-thread".into(),
                    turn_id: Some(turn.into()),
                    model: None,
                    content: turn.into(),
                    cwd: self.store.state_root.to_string_lossy().into_owned(),
                })
                .unwrap();
            let mut events = self.store.events().unwrap();
            events
                .iter_mut()
                .find(|candidate| candidate.seq == event.seq)
                .unwrap()
                .recorded_at_unix = at;
            crate::storage::atomic_write_json_lines(&self.store.path("inbox.jsonl"), &events)
                .unwrap();
            let mut sources = crate::branch_context::sources(&self.store).unwrap();
            sources.get_mut(&event.seq).unwrap().at_unix_utc = Some(at);
            crate::storage::atomic_write_json(&self.store.path("event-sources.json"), &sources)
                .unwrap();
            event.seq
        }

        fn completed(&self, turn: &str, at: u64) -> [u64; 2] {
            [
                self.event(crate::model::EventKind::UserPrompt, turn, at),
                self.event(crate::model::EventKind::AssistantFinal, turn, at),
            ]
        }
    }

    impl Drop for RefreshFixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.store.state_root);
        }
    }

    #[test]
    fn refresh_retries_collected_today_without_reviving_a_requested_past_day() {
        let fixture = RefreshFixture::new();
        let today_seqs = fixture.completed("today", unix_now());
        let old_at = unix_now() - 86_400;
        fixture.completed("past", old_at);
        let old_day = daily::day_at(old_at).unwrap();
        participation::set_enabled(&fixture.store, false).unwrap();
        participation::set_enabled(&fixture.store, true).unwrap();
        let today_key = daily::key(&fixture.store).unwrap().unwrap();
        daily::request(&fixture.store, &old_day).unwrap();
        let mut budget = retry::load(&fixture.store, &today_key).unwrap();
        budget.attempts = retry::MAX_ATTEMPTS;
        retry::save(&fixture.store, &budget).unwrap();
        let scope = crate::branch_context::detect_for_store(
            &fixture.store,
            fixture.store.state_root.to_str().unwrap(),
        )
        .unwrap();
        let journal_path = crate::branch_context::directory(&fixture.store.state_root, &scope)
            .join("journal.json");
        let gap_proof = std::fs::read(&journal_path).unwrap();
        let raw_before = std::fs::read(fixture.store.path("inbox.jsonl")).unwrap();
        let source_before = std::fs::read(fixture.store.path("event-sources.json")).unwrap();
        let mut calls = 0;
        let outcome = request_refresh_with(&fixture.store, || {
            calls += 1;
            let (events, manual) = daily::eligible(&fixture.store)?;
            assert!(!manual);
            assert_eq!(
                events.iter().map(|event| event.seq).collect::<Vec<_>>(),
                today_seqs
            );
            Ok(())
        })
        .unwrap();
        assert_eq!(outcome, RefreshOutcome::Requested { today_turns: 1 });
        assert_eq!(calls, 1);
        assert!(daily::manual(&fixture.store).unwrap().seqs.is_empty());
        let key = daily::key(&fixture.store).unwrap().unwrap();
        assert_eq!(key, today_key);
        assert_eq!(retry::load(&fixture.store, &key).unwrap().attempts, 0);
        assert_eq!(std::fs::read(journal_path).unwrap(), gap_proof);
        assert_eq!(
            std::fs::read(fixture.store.path("inbox.jsonl")).unwrap(),
            raw_before
        );
        assert_eq!(
            std::fs::read(fixture.store.path("event-sources.json")).unwrap(),
            source_before
        );
    }

    #[test]
    fn refresh_reports_held_and_open_input_without_scheduling_a_model() {
        let fixture = RefreshFixture::new();
        let old_at = unix_now() - 86_400;
        fixture.completed("past", old_at);
        let restricted = fixture.completed("today-held", unix_now());
        fixture.event(crate::model::EventKind::UserPrompt, "open", unix_now());
        crate::storage::atomic_write_json(
            &fixture.store.path("daily-restrictions.json"),
            &daily::Restrictions {
                manual_only: restricted.into_iter().collect(),
                ..Default::default()
            },
        )
        .unwrap();
        daily::request(&fixture.store, &daily::day_at(old_at).unwrap()).unwrap();
        let key = daily::key(&fixture.store).unwrap().unwrap();
        let mut budget = retry::load(&fixture.store, &key).unwrap();
        budget.attempts = retry::MAX_ATTEMPTS;
        retry::save(&fixture.store, &budget).unwrap();
        let raw_before = std::fs::read(fixture.store.path("inbox.jsonl")).unwrap();
        let outcome = request_refresh_with(&fixture.store, || {
            panic!("held dates and unfinished input must never spawn a worker")
        })
        .unwrap();
        assert_eq!(
            outcome,
            RefreshOutcome::NoTodayWork {
                held_turns: 2,
                open_events: 1,
            }
        );
        assert!(daily::manual(&fixture.store).unwrap().seqs.is_empty());
        assert!(daily::eligible(&fixture.store).unwrap().0.is_empty());
        assert_eq!(retry::load(&fixture.store, &key).unwrap().attempts, 0);
        assert_eq!(
            std::fs::read(fixture.store.path("inbox.jsonl")).unwrap(),
            raw_before
        );
    }

    #[test]
    fn disabled_refresh_does_not_clear_requests_reset_budgets_or_reenable() {
        let fixture = RefreshFixture::new();
        fixture.completed("today", unix_now());
        let key = daily::key(&fixture.store).unwrap().unwrap();
        let mut budget = retry::load(&fixture.store, &key).unwrap();
        budget.attempts = retry::MAX_ATTEMPTS;
        retry::save(&fixture.store, &budget).unwrap();
        crate::storage::atomic_write_json(
            &fixture.store.path("manual-day.json"),
            &daily::Manual {
                day: "older-request".into(),
                ..Default::default()
            },
        )
        .unwrap();
        participation::set_enabled(&fixture.store, false).unwrap();
        let names = [
            "manual-day.json",
            "daily-retries.json",
            "participation.json",
        ];
        let before: Vec<_> = names
            .iter()
            .map(|name| std::fs::read(fixture.store.path(name)).unwrap())
            .collect();
        assert!(request_refresh_with(&fixture.store, || panic!("disabled spawn")).is_err());
        for (name, expected) in names.into_iter().zip(before) {
            assert_eq!(std::fs::read(fixture.store.path(name)).unwrap(), expected);
        }
        assert!(!participation::load(&fixture.store).unwrap().enabled);
    }

    #[test]
    fn unreadable_input_returns_failure_without_spawning_or_erasing_raw() {
        let fixture = RefreshFixture::new();
        std::fs::write(fixture.store.path("inbox.jsonl"), "malformed raw\n").unwrap();
        assert!(request_refresh_with(&fixture.store, || panic!("unreadable spawn")).is_err());
        assert_eq!(
            std::fs::read_to_string(fixture.store.path("inbox.jsonl")).unwrap(),
            "malformed raw\n"
        );
        assert!(participation::load(&fixture.store).unwrap().enabled);
    }

    #[test]
    fn opt_out_blocks_rebuild_and_scheduler_without_erasing_the_request() {
        let root = std::env::temp_dir().join(format!("pm-optout-rebuild-{}", std::process::id()));
        let store = SpaceStore::for_workspace(&root, "test", "w1", "/repo", None).unwrap();
        store.request_rebuild("session").unwrap();
        assert!(next_attempt_at(&store).unwrap().is_none()); // Old automatic rebuild request is not permission.
        participation::set_enabled(&store, false).unwrap();
        assert!(next_attempt_at(&store).unwrap().is_none());
        assert!(!pending_recovery_needed(&store).unwrap());
        assert!(matches!(attempt_space(&store).unwrap(), Attempt::None));
        assert!(request_refresh(&store).is_err());
        participation::set_enabled(&store, true).unwrap();
        assert!(store.rebuild_request().unwrap().is_some());
        assert!(next_attempt_at(&store).unwrap().is_none());
        assert!(request_rebuild(&store).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn processing_diagnostics_are_bounded_and_private() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("pm-processing-test-{}", std::process::id()));
        let store = SpaceStore::for_workspace(&root, "test", "w1", "/repo", None).unwrap();
        for i in 0..55 {
            record_attempt(
                &store,
                serde_json::json!({"first_event_seq":i,"outcome":"success"}),
            )
            .unwrap();
        }
        let path = store.path("processing.json");
        let samples: Vec<serde_json::Value> = crate::storage::read_json_or_default(&path).unwrap();
        assert_eq!(samples.len(), 50);
        assert_eq!(samples[0]["first_event_seq"], 5);
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn interrupted_rebuild_resumes_but_active_and_failed_work_does_not() {
        let root =
            std::env::temp_dir().join(format!("maeklog-recovery-test-{}", std::process::id()));
        let store = SpaceStore::for_workspace(&root, "test", "w1", "/repo", None).unwrap();
        assert!(!pending_recovery_needed(&store).unwrap());
        store.request_rebuild("session-a").unwrap();
        assert!(!pending_recovery_needed(&store).unwrap());
        for kind in [
            crate::model::EventKind::UserPrompt,
            crate::model::EventKind::AssistantFinal,
        ] {
            store
                .append_event(crate::storage::NewTurnEvent {
                    kind,
                    session_id: "daily-fixture".into(),
                    turn_id: Some("one".into()),
                    model: None,
                    content: "today".into(),
                    cwd: "/repo".into(),
                })
                .unwrap();
        }
        store
            .set_status("summarizing", "Interrupted before commit")
            .unwrap();
        assert!(pending_recovery_needed(&store).unwrap());

        let running = lock_file(&root.join("worker.lock"), false).unwrap();
        assert!(!pending_recovery_needed(&store).unwrap());
        drop(running);
        assert!(pending_recovery_needed(&store).unwrap());

        let space_running = lock_file(&store.path(".worker.lock"), false).unwrap();
        assert!(!pending_recovery_needed(&store).unwrap());
        drop(space_running);
        assert!(pending_recovery_needed(&store).unwrap());

        store
            .set_status("error", "Explicit retry required")
            .unwrap();
        assert!(pending_recovery_needed(&store).unwrap()); // legacy errors get bounded recovery
        let mut budget = retry::load(&store, &work_key(&store).unwrap().unwrap()).unwrap();
        budget.attempts = retry::MAX_ATTEMPTS;
        retry::save(&store, &budget).unwrap();
        assert!(!pending_recovery_needed(&store).unwrap());
        store.set_status("queued", "").unwrap();
        store.archive().unwrap();
        assert!(!pending_recovery_needed(&store).unwrap());
        std::fs::remove_dir_all(root).unwrap();
    }
}
