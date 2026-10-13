//! Pure terminal rendering of shared branch memory. Folding performs no I/O.
use crate::journal::{Kind, Outcome, Record, StateKind, StateStatus};
use crate::local_time::LocalTimes;
use crate::panel_style::{Line, Role};
use crate::storage::SpaceStore;
use crate::{AppResult, branch_context as context, journal, legacy, task_links, work_links};
use std::fmt::Write;
const PAGE_SIZE: usize = 25;
mod compact;
mod details;
pub use compact::{render_current_references, render_preview};
pub use details::{RecordSection, record_sections};

#[cfg(test)]
mod link_tests {
    use super::*;

    fn snapshot() -> Snapshot {
        Snapshot {
            today: None,
            daily: None,
            scope: context::Context::default(),
            journal: journal::Journal::default(),
            links: work_links::Links::default(),
            task: None,
            legacy: Vec::new(),
            unlinked_records: Vec::new(),
            unlinked_states: Vec::new(),
            times: LocalTimes::default(),
            pending: 0,
            other_pending: 0,
            gap: false,
            history_limited: false,
            error: None,
        }
    }

    #[test]
    fn no_match_unavailable_and_explicit_none_are_distinct_and_do_not_look_connected() {
        let mut snapshot = snapshot();
        snapshot.links.pr_state = work_links::ResolutionState::None;
        let mut lines = Vec::new();
        link_section(&mut lines, &snapshot, work_links::Kind::Pr, 80);
        assert!(
            lines
                .join("\n")
                .contains(crate::tr!("열린 PR 없음", "No open PR"))
        );
        assert!(!lines.iter().any(|line| line.role == Role::Heading));
        snapshot.links.discovery_unavailable = true;
        snapshot.links.discovery_error = Some("GitHub 원격 저장소 없음".into());
        lines.clear();
        link_section(&mut lines, &snapshot, work_links::Kind::Pr, 80);
        assert!(
            lines
                .join("\n")
                .contains(crate::tr!("PR 자동 연결 불가", "PR auto-link unavailable"))
        );
        assert!(
            !lines
                .join("\n")
                .contains(crate::tr!("열린 PR 없음", "No open PR"))
        );
        snapshot.links.pr_not_created = true;
        lines.clear();
        link_section(&mut lines, &snapshot, work_links::Kind::Pr, 80);
        assert!(lines.join("\n").contains(crate::tr!(
            "PR 연결 안 함 · 사용자 지정",
            "No PR link · User selected"
        )));
        assert!(
            !lines
                .join("\n")
                .contains(crate::tr!("미생성", "Not created"))
        );
        assert!(!lines.iter().any(|line| line.role == Role::Heading));
    }

    #[test]
    fn primary_shows_number_and_canonical_url_and_failed_cache_uses_warning_role() {
        let mut snapshot = snapshot();
        let mut link = work_links::parse(work_links::Kind::Pr, "example/project#10").unwrap();
        link.title = Some("연결 메타데이터 표시".into());
        snapshot.links.primary_pr = Some(link.id());
        snapshot.links.entries.insert(link.id(), link.clone());
        let mut lines = Vec::new();
        link_section(&mut lines, &snapshot, work_links::Kind::Pr, 80);
        assert!(lines.join("\n").contains("PR example/project#10"));
        assert!(lines.join("\n").contains(&link.url));
        assert!(lines.iter().any(|line| line.role == Role::Heading));
        snapshot.links.entries.get_mut(&link.id()).unwrap().error =
            Some("인증을 확인하세요".into());
        lines.clear();
        link_section(&mut lines, &snapshot, work_links::Kind::Pr, 80);
        assert!(lines.iter().any(|line| line.role == Role::Warning));
        assert!(!lines.iter().any(|line| line.role == Role::Heading));
        assert!(
            lines
                .join("\n")
                .contains(crate::tr!("조회 실패", "Lookup failed"))
        );
        assert!(lines.join("\n").contains(&link.url));
    }

    fn reference(number: u64) -> task_links::TaskReference {
        serde_json::from_value(serde_json::json!({
            "id": format!("example/project#{number}"),
            "repository": "example/project", "number": number,
            "kind": null, "raw": format!("example/project#{number}"),
            "url": null, "link": null, "verification": "checking",
            "error": null, "provenance": "user_prompt", "source": {}
        }))
        .unwrap()
    }

    #[test]
    fn quiet_unlinked_dialogue_does_not_claim_github_absence_or_show_two_errors() {
        let mut snapshot = snapshot();
        snapshot.task = Some(task_links::TaskLinks::default());
        snapshot.links.discovery_unavailable = true;
        snapshot.links.discovery_error = Some("GitHub 원격 저장소 없음".into());
        let text = render_header(&snapshot, 30).join("\n");
        assert!(text.contains(crate::tr!("지정된 이슈·PR 없음", "No issue/PR selected")));
        assert!(!text.contains(crate::tr!("자동 연결 불가", "auto-link unavailable")));
        assert!(!text.contains(crate::tr!("열린 PR 없음", "No open PR")));
        assert!(!text.contains(crate::tr!("연결된 이슈 없음", "No linked issue")));
        assert!(text.contains(crate::tr!("l 연결 정정", "l edit links")));
    }

    #[test]
    fn failed_new_target_stays_current_and_branch_journal_remains_a_separate_scope() {
        let mut snapshot = snapshot();
        snapshot.scope.branch = Some("main".into());
        let saved = render_records(&snapshot, 80, &View::default()).join("\n");
        let mut task = task_links::TaskLinks::default();
        task.current_request = "#10 말고 #11 작업해줘".into();
        let mut next = reference(11);
        next.verification = task_links::VerificationState::Error;
        next.error = Some("GitHub 응답 실패 · v로 다시 확인".into());
        task.current.push(next);
        task.previous.push(task_links::TaskSnapshot {
            current: vec![reference(10)],
            ..task_links::TaskSnapshot::default()
        });
        snapshot.task = Some(task);
        let text = render(&snapshot, 80, &View::default()).join("\n");
        let current = text
            .find(crate::tr!(
                "참조 example/project#11 · 확인 실패",
                "Reference example/project#11 · Check failed"
            ))
            .unwrap();
        let previous = text
            .find(crate::tr!(
                "이전 연결 참조 example/project#10",
                "Previous link Reference example/project#10"
            ))
            .unwrap();
        let boundary = text
            .find(crate::tr!(
                "저장된 브랜치 기록 · main",
                "Saved branch records · main"
            ))
            .unwrap();
        assert!(current < previous && previous < boundary);
        assert_eq!(
            saved,
            render_records(&snapshot, 80, &View::default()).join("\n")
        );
    }

    #[test]
    fn long_request_and_multiple_targets_are_bounded_without_discarding_targets() {
        let mut snapshot = snapshot();
        let mut task = task_links::TaskLinks::default();
        task.current_request = "긴 요청 내용 ".repeat(1000);
        task.current = vec![reference(10), reference(11), reference(12), reference(13)];
        snapshot.task = Some(task);
        let lines = render_header(&snapshot, 30);
        let request = lines
            .iter()
            .position(|line| line.contains(crate::tr!("현재 요청", "Current request")))
            .unwrap();
        let card = lines
            .iter()
            .position(|line| {
                line.contains(crate::tr!("현재 대화의 작업", "Work in this conversation"))
            })
            .unwrap();
        let request_lines = lines[request + 1..card]
            .iter()
            .filter(|line| !line.trim().is_empty())
            .count();
        assert!(request_lines <= 2);
        let normalize = |value: &str| value.split_whitespace().collect::<String>();
        assert!(normalize(&lines.join("\n")).contains(&normalize(crate::tr!(
            "외 2개 · l로 전체 참조 확인",
            "2 more · l for all references"
        ))));
        assert_eq!(snapshot.task.as_ref().unwrap().current.len(), 4);
    }

    #[test]
    fn unresolved_qualifier_keeps_the_raw_reference_visible() {
        let mut reference = reference(142);
        reference.repository = None;
        reference.raw = "demo-space#142".into();
        assert!(task_reference_label(&reference).contains("demo-space#142"));
        assert!(
            task_reference_label(&reference)
                .contains(crate::tr!("저장소 미확인", "Repository unknown"))
        );
    }
}

#[derive(Clone, Debug, Default)]
pub struct View {
    pub expanded: bool,
    pub page: usize,
}
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub today: Option<String>,
    pub daily: Option<crate::daily::Queue>,
    pub scope: context::Context,
    pub journal: journal::Journal,
    pub links: work_links::Links,
    pub task: Option<task_links::TaskLinks>,
    pub legacy: Vec<legacy::Snapshot>,
    pub unlinked_records: Vec<Record>,
    pub unlinked_states: Vec<journal::State>,
    pub times: LocalTimes,
    pub pending: usize,
    pub other_pending: usize,
    /// Recorded collection interruptions; retained as historical evidence.
    pub gap: bool,
    /// A bounded collected history is not proof of a current collection failure.
    pub history_limited: bool,
    pub error: Option<String>,
}
pub fn saved(root: &std::path::Path, scope: context::Context) -> AppResult<Snapshot> {
    let journal = journal::load(root, &scope)?;
    let links = work_links::load(root, &scope)?;
    let times = LocalTimes::new(
        evidence_times(&journal)
            .into_iter()
            .chain(links.entries.values().filter_map(|l| l.checked_at_unix_utc))
            .chain(links.discovery_checked_at_unix_utc),
    );
    Ok(Snapshot {
        today: crate::daily::today().ok(),
        daily: None,
        gap: !journal.gaps.is_empty(),
        history_limited: false,
        scope,
        journal,
        links,
        task: None,
        times,
        legacy: Vec::new(),
        unlinked_records: Vec::new(),
        unlinked_states: Vec::new(),
        pending: 0,
        other_pending: 0,
        error: None,
    })
}
pub fn max_page(s: &Snapshot) -> usize {
    let previous = s
        .journal
        .records
        .values()
        .filter(|r| r.kind == Kind::Completion && !r.excluded() && !completed_today(r, s))
        .count();
    let decisions = s
        .journal
        .records
        .values()
        .filter(|r| r.kind == Kind::Decision && !r.active())
        .map(|r| r.versions.len())
        .sum::<usize>();
    (previous
        + decisions
        + s.legacy.len()
        + s.unlinked_records
            .iter()
            .map(|r| r.versions.len())
            .sum::<usize>()
        + s.unlinked_states.len())
    .div_ceil(PAGE_SIZE)
    .saturating_sub(1)
}
fn completed_today(record: &Record, snapshot: &Snapshot) -> bool {
    snapshot.today.as_ref().is_some_and(|today| {
        snapshot.times.day(record.latest().at_unix_utc).as_ref() == Some(today)
    })
}
fn latest_follow<'a>(record: &Record, journal: &'a journal::Journal) -> Option<&'a Record> {
    let mut descendants = std::collections::BTreeSet::from([record.id.clone()]);
    loop {
        let before = descendants.len();
        for next in journal.records.values() {
            if next
                .follows
                .as_ref()
                .is_some_and(|id| descendants.contains(id))
            {
                descendants.insert(next.id.clone());
            }
        }
        if descendants.len() == before {
            break;
        }
    }
    journal
        .records
        .values()
        .filter(|r| !r.excluded() && r.id != record.id && descendants.contains(&r.id))
        .max_by_key(|r| r.latest().order)
}
pub fn load(store: &SpaceStore) -> AppResult<Snapshot> {
    let meta = store.meta()?.ok_or("Space metadata missing")?;
    let scope = context::detect_for_store(store, &meta.cwd)?;
    let mut error = None;
    let journal = journal::load(&store.state_root, &scope).unwrap_or_else(|e| {
        error = Some(crate::tr!("원장 읽기 실패 · {e}", "Journal read failed · {e}";));
        journal::Journal::default()
    });
    let links = work_links::load(&store.state_root, &scope).unwrap_or_else(|e| {
        error = Some(crate::tr!("연결 캐시 읽기 실패 · {e}", "Link cache read failed · {e}";));
        work_links::Links::default()
    });
    let task = match task_links::load(store) {
        Ok(task) => Some(task),
        Err(e) => {
            let notice =
                crate::tr!("대화 연결 읽기 실패 · {e}", "Conversation links read failed · {e}";);
            error = Some(error.map_or_else(|| notice.clone(), |old| format!("{old} · {notice}")));
            None
        }
    };
    let old = legacy::read(store).unwrap_or_default();
    let private_scope = context::Context {
        key: format!("unlinked-{}", context::hash(&store.key)),
        ..Default::default()
    };
    let unlinked = if private_scope.key == scope.key {
        journal::Journal::default()
    } else {
        journal::load(&store.state_root, &private_scope).unwrap_or_default()
    };
    let mut pending = 0;
    let mut other_pending = 0;
    for candidate in SpaceStore::all(&store.state_root)? {
        for event in candidate.unprocessed_events().unwrap_or_default() {
            if context::source(&candidate, &event)?.context.key == scope.key {
                pending += 1;
            } else if candidate.key == store.key {
                other_pending += 1;
            }
        }
    }
    let times = evidence_times(&journal)
        .into_iter()
        .chain(evidence_times(&unlinked))
        .chain(links.entries.values().filter_map(|l| l.checked_at_unix_utc))
        .chain(links.discovery_checked_at_unix_utc);
    let times = LocalTimes::new(times);
    let gap = !journal.gaps.is_empty();
    let preference = crate::participation::load(store)?;
    let history_limited = store.current()?.timing.partial_history
        || preference.history_blocked
        || preference.generation > 0;
    Ok(Snapshot {
        today: crate::daily::today().ok(),
        scope,
        journal,
        links,
        task,
        legacy: old,
        daily: Some(crate::daily::queue(store)?),
        unlinked_records: unlinked.records.into_values().collect(),
        unlinked_states: unlinked.states.into_values().collect(),
        times,
        pending,
        other_pending,
        gap,
        history_limited,
        error,
    })
}
fn evidence_times(journal: &journal::Journal) -> Vec<u64> {
    let mut values = Vec::new();
    values.extend(journal.through_unix_utc);
    values.extend(journal.updated_at_unix_utc);
    for record in journal.records.values() {
        for version in &record.versions {
            values.extend(version.at_unix_utc);
            values.extend(
                version
                    .proof
                    .iter()
                    .filter_map(|proof| proof.source.at_unix_utc),
            );
        }
        values.extend(
            record
                .corrections
                .iter()
                .map(|correction| correction.at_unix_utc),
        );
    }
    for state in journal.states.values() {
        values.extend(state.at_unix_utc);
        values.extend(
            state
                .proof
                .iter()
                .filter_map(|proof| proof.source.at_unix_utc),
        );
    }
    values
}

#[cfg(test)]
mod collection_tests {
    use super::*;

    struct Fixture {
        store: SpaceStore,
    }

    impl Fixture {
        fn new() -> Self {
            use std::sync::atomic::{AtomicU64, Ordering};
            static SERIAL: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "pm-collection-kind-{}-{}",
                std::process::id(),
                SERIAL.fetch_add(1, Ordering::Relaxed)
            ));
            let store = SpaceStore::for_scope(
                &root,
                "collection-test",
                "workspace",
                Some("tab"),
                root.to_str().unwrap(),
                None,
            )
            .unwrap();
            Self { store }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.store.state_root);
        }
    }

    #[test]
    fn bounded_history_and_unlinked_scope_are_not_recorded_collection_interruptions() {
        let fixture = Fixture::new();
        crate::storage::atomic_write_json(
            &fixture.store.path("current.json"),
            &crate::model::CurrentState {
                timing: crate::model::ContextTiming {
                    partial_history: true,
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .unwrap();
        let snapshot = load(&fixture.store).unwrap();
        assert!(snapshot.history_limited);
        assert!(!snapshot.gap);
        assert!(snapshot.journal.gaps.is_empty());
        assert!(snapshot.scope.unlinked_reason.is_some());
        assert_eq!(
            crate::participation::load(&fixture.store)
                .unwrap()
                .generation,
            0
        );
    }

    #[test]
    fn preference_generation_alone_is_a_history_limit_not_a_current_gap() {
        let fixture = Fixture::new();
        crate::participation::save(
            &fixture.store,
            &crate::participation::Preference {
                generation: 2,
                history_blocked: true,
                ..Default::default()
            },
        )
        .unwrap();
        let before = std::fs::read(fixture.store.path("participation.json")).unwrap();
        let snapshot = load(&fixture.store).unwrap();
        assert!(snapshot.history_limited);
        assert!(!snapshot.gap);
        assert!(snapshot.journal.gaps.is_empty());
        assert_eq!(
            std::fs::read(fixture.store.path("participation.json")).unwrap(),
            before
        );
    }

    #[test]
    fn recorded_collection_interruptions_remain_in_saved_and_live_views() {
        let fixture = Fixture::new();
        let scope =
            context::detect_for_store(&fixture.store, fixture.store.state_root.to_str().unwrap())
                .unwrap();
        let journal_path =
            context::directory(&fixture.store.state_root, &scope).join("journal.json");
        crate::storage::atomic_write_json(
            &journal_path,
            &journal::Journal {
                revision: 7,
                gaps: vec![journal::Gap {
                    space: fixture.store.key.clone(),
                    generation: 1,
                    reason: "Space 수집 중지 · 마지막 확인 기록".into(),
                }],
                ..Default::default()
            },
        )
        .unwrap();
        let before = std::fs::read(&journal_path).unwrap();
        let snapshot = load(&fixture.store).unwrap();
        assert!(snapshot.gap);
        assert!(!snapshot.history_limited);
        let saved = saved(&fixture.store.state_root, scope).unwrap();
        assert!(saved.gap);
        assert_eq!(snapshot.journal.gaps, saved.journal.gaps);
        assert_eq!(std::fs::read(journal_path).unwrap(), before);
    }
}

fn put(lines: &mut Vec<Line>, text: &str, width: usize) {
    crate::panel_style::wrap(lines, text, width, Role::Body);
}
fn muted(lines: &mut Vec<Line>, text: &str, width: usize) {
    crate::panel_style::wrap(lines, text, width, Role::Muted);
}
fn heading(lines: &mut Vec<Line>, text: &str, width: usize) {
    lines.push(Line::default());
    crate::panel_style::wrap(lines, text, width, Role::Heading);
}
fn text_record(r: &Record, s: &Snapshot) -> String {
    let v = r.latest();
    let condition = r.condition().map_or(
        String::new(),
        |c| crate::tr!(" · 조건: {c}", " · Condition: {c}";),
    );
    let mut line = format!(
        "{}  {}{}{}",
        s.times.display(v.at_unix_utc),
        r.text(),
        condition,
        if r.kind == Kind::Completion {
            crate::tr!(" [보고]", " [Reported]")
        } else {
            ""
        }
    );
    if crate::corrections::current(r).is_some() {
        line.push_str(crate::tr!(" [사용자 정정]", " [User correction]"));
    }
    let follow = latest_follow(r, &s.journal);
    if let Some(f) = follow {
        let _ = write!(line, " → {}: {}", outcome(f.outcome), f.text());
    }
    line
}
fn outcome(value: Outcome) -> &'static str {
    match value {
        Outcome::Failed => crate::tr!("후속 검증 실패", "Follow-up verification failed"),
        Outcome::Rework => crate::tr!("재작업 필요", "Rework needed"),
        Outcome::Passed => crate::tr!("후속 통과 보고", "Follow-up passed: reported"),
        _ => crate::tr!("후속 보고", "Follow-up reported"),
    }
}
fn state_line(state: &journal::State, s: &Snapshot) -> String {
    let owner = state_owner(&state.owner);
    let condition = state.condition.as_ref().map_or(
        String::new(),
        |c| crate::tr!(" · 조건: {c}", " · Condition: {c}";),
    );
    let status = match state.status {
        StateStatus::Unconfirmed => crate::tr!(
            " · 해제/현재 상태 미확인",
            " · Resolution/current state unknown"
        ),
        StateStatus::Resolved => crate::tr!(" · 해결 근거 확인", " · Resolution evidence found"),
        StateStatus::Active => "",
    };
    format!(
        "{}  {}{}{}{}",
        s.times.display(state.at_unix_utc),
        if state.kind == StateKind::Action {
            format!("{owner} — ")
        } else {
            String::new()
        },
        state.text,
        condition,
        status
    )
}
fn state_owner(owner: &str) -> &str {
    match owner {
        "sol" | "codex" => "Codex",
        "user" => crate::tr!("사용자", "User"),
        "luna" => "Luna",
        "unknown" => crate::tr!("담당 미확인", "Owner unknown"),
        v => v,
    }
}
#[allow(
    clippy::too_many_lines,
    reason = "One pure link section orders primary metadata, association status and recovery guidance"
)]
fn link_section(lines: &mut Vec<Line>, s: &Snapshot, kind: work_links::Kind, width: usize) {
    let label = if kind == work_links::Kind::Issue {
        crate::tr!("이슈", "Issue")
    } else {
        "PR"
    };
    let candidate_count = s.links.candidates(kind).len();
    if kind == work_links::Kind::Pr && s.links.pr_not_created {
        lines.push(Line::default());
        muted(
            lines,
            crate::tr!("PR 연결 안 함 · 사용자 지정", "No PR link · User selected"),
            width,
        );
        return;
    }
    if kind == work_links::Kind::Issue && s.links.issue_not_linked {
        lines.push(Line::default());
        muted(
            lines,
            crate::tr!(
                "이슈 연결 안 함 · 사용자 지정",
                "No issue link · User selected"
            ),
            width,
        );
        return;
    }
    if let Some(link) = s.links.primary(kind) {
        lines.push(Line::default());
        let resolution = s.links.resolution(kind);
        crate::panel_style::wrap(
            lines,
            &format!(
                "{label} {}#{}{}",
                link.repository,
                link.number,
                match resolution {
                    work_links::ResolutionState::Error =>
                        crate::tr!(" · 조회 실패 · 마지막 연결", " · Lookup failed · Last link"),
                    work_links::ResolutionState::Checking =>
                        crate::tr!(" · 갱신 중", " · Updating"),
                    _ if s.links.is_manual(kind) =>
                        crate::tr!(" · 사용자 지정", " · User selected"),
                    _ => "",
                }
            ),
            width,
            match resolution {
                work_links::ResolutionState::Error => Role::Warning,
                work_links::ResolutionState::Checking => Role::Muted,
                _ => Role::Heading,
            },
        );
        put(
            lines,
            link.title
                .as_deref()
                .unwrap_or(crate::tr!("제목 조회 전", "Title not checked")),
            width,
        );
        muted(lines, &link.url, width);
        if let Some(purpose) = &link.purpose {
            put(
                lines,
                &crate::tr!("{} · 본문 발췌: {purpose}", "{} · Body excerpt: {purpose}";
                    if kind == work_links::Kind::Issue {
                        crate::tr!("문제/목표", "Problem/goal")
                    } else {
                        crate::tr!("변경 범위", "Change scope")
                    }
                ),
                width,
            );
        } else {
            put(
                lines,
                crate::tr!("본문 요약 미확인", "Body summary unknown"),
                width,
            );
        }
        if let Some(error) = &link.error {
            put(
                lines,
                &crate::tr!("조회 실패 · 캐시의 마지막 확인: {} · {error}", "Lookup failed · Last cached check: {} · {error}";
                    s.times.display(link.checked_at_unix_utc)
                ),
                width,
            );
        }
    } else {
        use work_links::ResolutionState;
        let state = if s.links.discovery_unavailable {
            crate::tr!("{label} 자동 연결 불가", "{label} auto-link unavailable";)
        } else {
            match s.links.resolution(kind) {
                ResolutionState::Unchecked => {
                    crate::tr!("{label} 자동 연결 대기", "{label} auto-link pending";)
                }
                ResolutionState::Checking => {
                    crate::tr!("{label} 연결 조회 중…", "{label} link lookup…";)
                }
                ResolutionState::None if kind == work_links::Kind::Pr => {
                    crate::tr!("열린 PR 없음", "No open PR").into()
                }
                ResolutionState::None => crate::tr!("연결된 이슈 없음", "No linked issue").into(),
                ResolutionState::Ambiguous => {
                    crate::tr!("{label} 후보 {candidate_count}건 · 선택 필요", "{label} candidates: {candidate_count} · Selection needed";)
                }
                ResolutionState::Error => {
                    crate::tr!("{label} 연결 조회 실패 · v 재시도", "{label} link lookup failed · v to retry";)
                }
                ResolutionState::Linked => {
                    crate::tr!("{label} 연결 재확인 필요", "{label} link needs rechecking";)
                }
            }
        };
        lines.push(Line::default());
        crate::panel_style::wrap(
            lines,
            &state,
            width,
            if !s.links.discovery_unavailable
                && matches!(
                    s.links.resolution(kind),
                    ResolutionState::Error | ResolutionState::Ambiguous
                )
            {
                Role::Warning
            } else {
                Role::Muted
            },
        );
        if candidate_count > 0 {
            muted(
                lines,
                &crate::tr!("후보 {candidate_count}건 · l로 선택/열기", "Candidates: {candidate_count} · l to select/open";),
                width,
            );
        } else if s.links.discovery_unavailable {
            muted(
                lines,
                crate::tr!(
                    "l로 OWNER/REPO#N 또는 GitHub 링크 지정",
                    "l to set OWNER/REPO#N or a GitHub link"
                ),
                width,
            );
        } else if s.links.resolution(kind) == ResolutionState::None {
            muted(
                lines,
                crate::tr!(
                    "l로 번호 또는 GitHub 링크 지정",
                    "l to set a number or GitHub link"
                ),
                width,
            );
        }
    }
    if let Some(error) = s.links.error(kind)
        && Some(error) != s.links.discovery_error.as_deref()
        && Some(error) != s.links.primary(kind).and_then(|link| link.error.as_deref())
    {
        crate::panel_style::wrap(lines, error, width, Role::Warning);
    }
}
fn history(s: &Snapshot, completions: &[&Record]) -> Vec<Vec<String>> {
    let mut entries = Vec::new();
    for r in s
        .journal
        .records
        .values()
        .filter(|r| r.kind == Kind::Decision && !r.active())
    {
        for v in &r.versions {
            let relation = r
                .replaced_by
                .as_ref()
                .and_then(|id| s.journal.records.get(id))
                .map_or_else(
                    || {
                        if r.excluded() {
                            crate::tr!(
                                "오기록 제외 · 실제 결정 철회 아님",
                                "Excluded as incorrect · Decision not withdrawn"
                            )
                            .into()
                        } else if r.withdrawn {
                            crate::tr!("철회", "Withdrawn").into()
                        } else {
                            crate::tr!("변경", "Changed").into()
                        }
                    },
                    |next| {
                        crate::tr!("→ {} · 변경 확인 {}", "→ {} · Change confirmed {}";
                            next.text(),
                            s.times.display(next.latest().at_unix_utc)
                        )
                    },
                );
            entries.push(vec![
                crate::tr!("결정 변경 이력 · {}", "Decision change history · {}"; r.id),
                format!(
                    "{}  {}{} · {relation}",
                    s.times.display(v.at_unix_utc),
                    v.text,
                    v.condition.as_ref().map_or(
                        String::new(),
                        |c| crate::tr!(" · 조건: {c}", " · Condition: {c}";)
                    )
                ),
            ]);
        }
    }
    for r in completions.iter().filter(|r| !completed_today(r, s)) {
        entries.push(vec![
            crate::tr!("이전 완료 · {}", "Earlier completion · {}"; r.id),
            text_record(r, s),
        ]);
    }
    for old in &s.legacy {
        let mut lines = vec![
            crate::tr!("미연결 구버전 요약 r{} · 개별 확정 기록으로 이관하지 않음", "Unlinked legacy summary r{} · Not converted into confirmed records";
                old.revision
            ),
        ];
        if !old.summary.objective.is_empty() {
            lines.push(crate::tr!("목표: {}", "Objective: {}"; old.summary.objective));
        }
        for d in &old.summary.decisions {
            lines.push(crate::tr!("시각 미확인 · 결정 후보/기록 {:?}: {}", "Time unknown · Decision candidate/record {:?}: {}";
                d.status, d.text
            ));
        }
        for p in &old.summary.progress {
            lines.push(crate::tr!("시각 미확인 · 진행 기록 {:?}: {}", "Time unknown · Progress record {:?}: {}";
                p.status, p.text
            ));
        }
        for b in &old.summary.blockers {
            lines.push(crate::tr!("시각 미확인 · 막힘 기록: {}", "Time unknown · Blocker record: {}"; b.text()));
        }
        for a in &old.summary.next_actions {
            lines.push(crate::tr!("{} — {} · 조건: {}", "{} — {} · Condition: {}";
                a.owner,
                a.text,
                a.when.as_deref().unwrap_or(crate::tr!("미확인", "Unknown"))
            ));
        }
        entries.push(lines);
    }
    append_unlinked(s, &mut entries);
    entries
}
pub fn render(s: &Snapshot, width: usize, view: &View) -> Vec<Line> {
    let mut lines = render_header(s, width);
    lines.extend(render_records(s, width, view));
    lines
}

pub fn render_header(s: &Snapshot, width: usize) -> Vec<Line> {
    let width = width.saturating_sub(2).max(2);
    let mut lines = Vec::new();
    let repository = s
        .scope
        .repository
        .as_ref()
        .and_then(|r| std::path::Path::new(r).parent()?.file_name())
        .map_or_else(
            || crate::tr!("저장소 미확인", "Repository unknown").into(),
            |n| n.to_string_lossy().into_owned(),
        );
    crate::panel_style::wrap(
        &mut lines,
        &crate::tr!("{repository} / {} · 관찰 브랜치", "{repository} / {} · Observed branch";
            s.scope.branch.as_deref().unwrap_or(crate::tr!("브랜치 미확인", "Branch unknown"))
        ),
        width,
        Role::Title,
    );
    if let Some(tab) = &s.scope.tab_id {
        muted(
            &mut lines,
            &crate::tr!("탭 {tab}의 기록 · 다른 탭과 분리", "Records for tab {tab} · Separate from other tabs";),
            width,
        );
    }
    if let Some(error) = &s.error {
        crate::panel_style::wrap(&mut lines, error, width, Role::Warning);
    }
    if let Some(task) = &s.task {
        dialogue_links(&mut lines, task, width);
    }
    if s.task.as_ref().is_none_or(|task| {
        task.current.is_empty() && task.related_pr.is_empty() && !task.explicit_none
    }) {
        branch_links(&mut lines, s, width);
    }
    muted(
        &mut lines,
        crate::tr!(
            "l 연결 정정 · v 다시 확인 · o 열기",
            "l edit links · v recheck · o open"
        ),
        width,
    );
    lines
}

#[allow(
    clippy::too_many_lines,
    reason = "Keeps dialogue link ordering together; localized labels add presentation lines"
)]
fn dialogue_links(lines: &mut Vec<Line>, task: &task_links::TaskLinks, width: usize) {
    if !task.current_request.trim().is_empty() {
        heading(lines, crate::tr!("현재 요청", "Current request"), width);
        put(lines, &short_text(&task.current_request, width * 2), width);
    }
    heading(
        lines,
        crate::tr!("현재 대화의 작업", "Work in this conversation"),
        width,
    );
    if task.current.is_empty() && task.related_pr.is_empty() {
        muted(
            lines,
            if task.explicit_none {
                crate::tr!("연결 안 함 · 직접 정정", "No link · Manual correction")
            } else {
                crate::tr!("지정된 이슈·PR 없음", "No issue/PR selected")
            },
            width,
        );
    }
    for reference in task.current.iter().take(2) {
        task_reference(lines, reference, width, false);
    }
    let related = task
        .related_pr
        .iter()
        .filter(|reference| {
            !task
                .current
                .iter()
                .any(|current| current.id == reference.id)
        })
        .collect::<Vec<_>>();
    for reference in related.iter().take(1) {
        task_reference(lines, reference, width, true);
    }
    let remaining = task.current.len().saturating_sub(2) + related.len().saturating_sub(1);
    if remaining > 0 {
        muted(
            lines,
            &crate::tr!("외 {remaining}개 · l로 전체 참조 확인", "{remaining} more · l for all references";),
            width,
        );
    }
    if let Some(location) = &task.worktree {
        if location.verification == task_links::VerificationState::Verified {
            if let Some(context) = &location.context {
                muted(
                    lines,
                    &crate::tr!("인계한 작업 위치 · {}", "Reported work location · {}";
                        context.branch.as_deref().unwrap_or(crate::tr!("브랜치 미확인", "Branch unknown"))
                    ),
                    width,
                );
            }
        } else {
            muted(
                lines,
                crate::tr!(
                    "작업 위치 미확인 · l에서 인계 근거 확인",
                    "Work location unknown · l for handoff evidence"
                ),
                width,
            );
        }
    }
    if let Some(previous) = task.previous.last() {
        let ids = if previous.current.len() > 1 {
            crate::tr!("{}개", "{}"; previous.current.len())
        } else {
            previous
                .current
                .first()
                .map(task_reference_label)
                .unwrap_or_default()
        };
        if !ids.is_empty() || previous.explicit_none {
            muted(
                lines,
                &short_text(
                    &crate::tr!("이전 연결 {}{}", "Previous link {}{}";
                        if ids.is_empty() { crate::tr!("없음", "None") } else { &ids },
                        if task.can_undo() {
                            crate::tr!(" · l에서 되돌리기", " · l to undo")
                        } else {
                            ""
                        }
                    ),
                    width * 2,
                ),
                width,
            );
        }
    }
    if !task.candidates.is_empty() {
        muted(
            lines,
            &crate::tr!("대화 후보 {}개 · l로 근거 확인/정정", "Conversation candidates: {} · l for evidence/edit"; task.candidates.len()),
            width,
        );
    }
}

fn task_reference(
    lines: &mut Vec<Line>,
    reference: &task_links::TaskReference,
    width: usize,
    related: bool,
) {
    crate::panel_style::wrap(
        lines,
        &format!(
            "{}{} · {}",
            if related {
                crate::tr!("관련 ", "Related ")
            } else {
                ""
            },
            task_reference_label(reference),
            if !related && reference.verification == task_links::VerificationState::Checking {
                crate::tr!("지정됨 · 확인 중", "Selected · Checking")
            } else {
                task_reference_status(reference)
            }
        ),
        width,
        task_reference_role(reference),
    );
    put(
        lines,
        &short_text(
            reference
                .link
                .as_ref()
                .and_then(|link| link.title.as_deref())
                .unwrap_or(
                    if reference.verification == task_links::VerificationState::Checking {
                        crate::tr!("제목 확인 중", "Checking title")
                    } else {
                        crate::tr!("제목 미확인", "Title unknown")
                    },
                ),
            width * 2,
        ),
        width,
    );
    muted(lines, task_reference_source(reference.provenance), width);
    if let Some(error) = &reference.error {
        crate::panel_style::wrap(lines, &short_text(error, width * 2), width, Role::Warning);
    }
}

pub(crate) fn task_reference_label(reference: &task_links::TaskReference) -> String {
    let kind = match reference.kind {
        Some(work_links::Kind::Issue) => crate::tr!("이슈", "Issue"),
        Some(work_links::Kind::Pr) => "PR",
        None => crate::tr!("참조", "Reference"),
    };
    reference.repository.as_ref().map_or_else(
        || {
            crate::tr!("{kind} {} · 저장소 미확인", "{kind} {} · Repository unknown";
                if reference.raw.is_empty() {
                    format!("#{}", reference.number)
                } else {
                    short_text(&reference.raw, 160)
                }
            )
        },
        |repository| format!("{kind} {repository}#{}", reference.number),
    )
}

pub(crate) fn task_reference_status(reference: &task_links::TaskReference) -> &'static str {
    match reference.verification {
        task_links::VerificationState::Checking => crate::tr!("확인 중", "Checking"),
        task_links::VerificationState::Error => crate::tr!("확인 실패", "Check failed"),
        task_links::VerificationState::Verified => match reference
            .link
            .as_ref()
            .and_then(|link| link.state.as_deref())
        {
            Some("OPEN") => crate::tr!("열림", "Open"),
            Some("CLOSED") => crate::tr!("닫힘", "Closed"),
            Some("MERGED") => crate::tr!("병합됨", "Merged"),
            _ => crate::tr!("확인됨", "Checked"),
        },
    }
}

pub(crate) fn task_reference_source(provenance: task_links::Provenance) -> &'static str {
    match provenance {
        task_links::Provenance::UserPrompt => crate::tr!("사용자 지정", "User selected"),
        task_links::Provenance::AgentReport => crate::tr!("Codex 인계", "Codex handoff"),
        task_links::Provenance::Manual => crate::tr!("직접 정정", "Manual correction"),
        task_links::Provenance::AssistantMention => crate::tr!(
            "Codex 언급 · 작업 지정 아님",
            "Codex mention · Work not selected"
        ),
    }
}

fn task_reference_role(reference: &task_links::TaskReference) -> Role {
    match reference.verification {
        task_links::VerificationState::Checking | task_links::VerificationState::Error => {
            Role::Warning
        }
        task_links::VerificationState::Verified => Role::Heading,
    }
}

pub(crate) fn short_text(text: &str, cells: usize) -> String {
    use unicode_width::UnicodeWidthChar;
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut result = String::new();
    let mut used = 0;
    for character in text.chars().filter(|character| !character.is_control()) {
        let width = character.width().unwrap_or(0);
        if used + width > cells.saturating_sub(1) {
            result.push('…');
            break;
        }
        result.push(character);
        used += width;
    }
    result
}

fn branch_links(lines: &mut Vec<Line>, snapshot: &Snapshot, width: usize) {
    let kinds = [work_links::Kind::Issue, work_links::Kind::Pr];
    if kinds
        .iter()
        .any(|kind| snapshot.links.primary(*kind).is_some())
    {
        heading(
            lines,
            &crate::tr!("관찰 브랜치에서 확인한 참조 · {}", "References found on observed branch · {}";
                snapshot.scope.branch.as_deref().unwrap_or(crate::tr!("브랜치 미확인", "Branch unknown"))
            ),
            width,
        );
        for kind in kinds {
            if snapshot.links.primary(kind).is_some() {
                link_section(lines, snapshot, kind, width);
            }
        }
    } else {
        let count = kinds
            .iter()
            .map(|kind| snapshot.links.candidates(*kind).len())
            .sum::<usize>();
        if count > 0 {
            muted(
                lines,
                &crate::tr!("관찰 브랜치의 연결 후보 {count}개 · l로 확인", "Observed-branch link candidates: {count} · l to view";),
                width,
            );
        }
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "One ordered terminal summary preserves the saved branch journal and history"
)]
pub fn render_records(s: &Snapshot, width: usize, view: &View) -> Vec<Line> {
    let width = width.saturating_sub(2).max(2);
    let mut lines = Vec::new();
    crate::panel_style::wrap(
        &mut lines,
        &crate::tr!("저장된 브랜치 기록 · {}", "Saved branch records · {}";
            s.scope.branch.as_deref().unwrap_or(crate::tr!("브랜치 미확인", "Branch unknown"))
        ),
        width,
        Role::Title,
    );
    if let Some(reason) = &s.scope.unlinked_reason {
        put(
            &mut lines,
            &crate::tr!("미연결 기록 · {reason}", "Unlinked records · {reason}";),
            width,
        );
    }
    heading(
        &mut lines,
        crate::tr!("결정 — 현재 유효한 결정", "Decisions — Currently active"),
        width,
    );
    let mut decisions: Vec<_> = s
        .journal
        .records
        .values()
        .filter(|r| r.kind == Kind::Decision && r.active())
        .collect();
    decisions.sort_by_key(|r| {
        (
            r.latest().at_unix_utc.is_none(),
            std::cmp::Reverse(r.latest().at_unix_utc),
            std::cmp::Reverse(r.latest().order),
        )
    });
    if decisions.is_empty() {
        muted(
            &mut lines,
            crate::tr!(
                "기록 없음 · 확정 근거가 저장되지 않음",
                "No records · No confirmation evidence saved"
            ),
            width,
        );
    }
    for r in decisions {
        put(&mut lines, &text_record(r, s), width);
    }
    heading(
        &mut lines,
        crate::tr!("오늘 완료 — 전체", "Today’s completion reports — All"),
        width,
    );
    let mut completed: Vec<_> = s
        .journal
        .records
        .values()
        .filter(|r| r.kind == Kind::Completion && !r.excluded())
        .collect();
    completed.sort_by_key(|r| std::cmp::Reverse(r.latest().order));
    if !completed.iter().any(|r| completed_today(r, s)) {
        muted(
            &mut lines,
            crate::tr!(
                "오늘 완료 보고 없음 · 이전/시각 미확인 기록은 h",
                "No completion reports today · h for earlier/unknown times"
            ),
            width,
        );
    }
    for r in completed.iter().filter(|r| completed_today(r, s)) {
        put(&mut lines, &text_record(r, s), width);
    }
    render_states(s, width, &mut lines);
    let unresolved: Vec<_> = completed
        .iter()
        .filter(|r| {
            matches!(r.outcome, Outcome::Failed | Outcome::Rework)
                && !latest_follow(r, &s.journal).is_some_and(|f| {
                    f.outcome == Outcome::Passed && f.latest().order > r.latest().order
                })
        })
        .collect();
    if let Some(failure) = unresolved.first() {
        put(
            &mut lines,
            &crate::tr!("완료 후 후속 문제 {}건 · {} — {}", "Follow-up problems: {} · {} — {}";
                unresolved.len(),
                outcome(failure.outcome),
                failure.text()
            ),
            width,
        );
    }
    let excluded = s.journal.records.values().filter(|r| r.excluded()).count();
    if excluded > 0 {
        muted(
            &mut lines,
            &crate::tr!("오기록 제외 {excluded}건 · e 근거·정정에서 확인/되돌리기", "Incorrect records excluded: {excluded} · e for evidence/undo";),
            width,
        );
    }
    muted(
        &mut lines,
        crate::tr!(
            "e: 항목 선택 · 근거 보기 / 정정",
            "e: select a record · Evidence / correction"
        ),
        width,
    );
    render_history(s, width, view, &completed, &mut lines);
    render_freshness(s, width, &mut lines);
    lines
}

fn render_history(
    s: &Snapshot,
    width: usize,
    view: &View,
    completed: &[&Record],
    lines: &mut Vec<Line>,
) {
    let history = history(s, completed);
    let decision_count = s
        .journal
        .records
        .values()
        .filter(|r| r.kind == Kind::Decision && !r.active())
        .map(|r| r.versions.len())
        .sum::<usize>();
    heading(
        lines,
        &crate::tr!("{} 이전 기록: 결정 변경 이력 {decision_count}건 · 이전 완료 {}건", "{} Earlier records: decision changes {decision_count} · Earlier completions {}";
            if view.expanded { "▼" } else { "▶" },
            completed.iter().filter(|r| !completed_today(r, s)).count()
        ),
        width,
    );
    if !s.legacy.is_empty() {
        put(
            lines,
            &crate::tr!("미연결 구버전 요약 {}건 포함", "Unlinked legacy summaries included: {}"; s.legacy.len()),
            width,
        );
    }
    if !s.unlinked_records.is_empty() || !s.unlinked_states.is_empty() {
        put(
            lines,
            &crate::tr!("미연결 수집 항목 {}건 포함 · 현재 브랜치로 추정하지 않음", "Unlinked collected items: {} · Original scope preserved";
                s.unlinked_records.len() + s.unlinked_states.len()
            ),
            width,
        );
    }
    put(
        lines,
        if view.expanded {
            crate::tr!(
                "h: 이전 기록 접기 · [ / ]: 기록 페이지",
                "h: collapse history · [ / ]: history pages"
            )
        } else {
            crate::tr!("h: 이전 기록 펼치기", "h: expand history")
        },
        width,
    );
    if view.expanded {
        let pages = history.len().div_ceil(PAGE_SIZE).max(1);
        let page = view.page.min(pages - 1);
        put(
            lines,
            &crate::tr!("이전 기록 페이지 {}/{}", "History page {}/{}"; page + 1, pages),
            width,
        );
        for entry in history.iter().skip(page * PAGE_SIZE).take(PAGE_SIZE) {
            for line in entry {
                put(lines, line, width);
            }
            lines.push(Line::default());
        }
        heading(
            lines,
            crate::tr!("현재 상태 상세", "Current state details"),
            width,
        );
        for state in s.journal.states.values() {
            put(lines, &state_line(state, s), width);
        }
        heading(
            lines,
            crate::tr!("이슈·PR 연결 상세", "Issue/PR link details"),
            width,
        );
        for link in s.links.entries.values() {
            put(
                lines,
                &format!(
                    "{:?} {}#{} — {}",
                    link.kind,
                    link.repository,
                    link.number,
                    link.title
                        .as_deref()
                        .unwrap_or(crate::tr!("조회 전", "Not checked"))
                ),
                width,
            );
            put(lines, &link.url, width);
            if let Some(error) = &link.error {
                put(
                    lines,
                    &crate::tr!("조회 실패: {error} · 마지막 확인 {}", "Lookup failed: {error} · Last checked {}";
                        s.times.display(link.checked_at_unix_utc)
                    ),
                    width,
                );
            }
        }
    }
}

fn render_states(s: &Snapshot, width: usize, lines: &mut Vec<Line>) {
    for (kind, label) in [
        (StateKind::Progress, crate::tr!("진행", "Progress")),
        (StateKind::Blocker, crate::tr!("막힘", "Blockers")),
        (StateKind::Action, crate::tr!("다음 행동", "Next actions")),
    ] {
        if kind == StateKind::Blocker
            && s.journal
                .states
                .values()
                .any(|v| v.kind == kind && v.status == StateStatus::Active)
        {
            lines.push(Line::default());
            crate::panel_style::wrap(lines, label, width, Role::Warning);
        } else if kind == StateKind::Action {
            lines.push(Line::default());
            crate::panel_style::wrap(lines, label, width, Role::Action);
        } else {
            heading(lines, label, width);
        }
        let mut states: Vec<_> = s
            .journal
            .states
            .values()
            .filter(|v| v.kind == kind && v.status != StateStatus::Resolved)
            .collect();
        states.sort_by_key(|v| std::cmp::Reverse(v.order));
        if let Some(state) = states.first() {
            put(lines, &state_line(state, s), width);
            if states.len() > 1 {
                put(
                    lines,
                    &crate::tr!("추가 {}건 · h로 상세", "More: {} · h for details"; states.len() - 1),
                    width,
                );
            }
        } else {
            muted(
                lines,
                if kind == StateKind::Blocker {
                    crate::tr!(
                        "막힘 미확인 · 없음으로 단정하지 않음",
                        "Blockers unknown · Absence not confirmed"
                    )
                } else {
                    crate::tr!("현재 상태 미확인", "Current state unknown")
                },
                width,
            );
        }
    }
}

fn render_freshness(s: &Snapshot, width: usize, lines: &mut Vec<Line>) {
    let first = lines.len();
    put(
        lines,
        &crate::tr!("요약 반영 시각: {}", "Summary updated: {}";
            s.times.display(s.journal.updated_at_unix_utc)
        ),
        width,
    );
    heading(
        lines,
        crate::tr!(
            "항목별 시각: 대화 기준 확인 시각",
            "Record times: based on source conversation"
        ),
        width,
    );
    put(
        lines,
        &crate::tr!("대화 반영 시각: {}", "Conversation included through: {}";
            s.times.display(s.journal.through_unix_utc)
        ),
        width,
    );
    put(
        lines,
        &crate::tr!("미반영 기록: {}개 · 다른 브랜치 대기: {}개", "Pending records: {} · Other-branch pending: {}";
            s.pending, s.other_pending
        ),
        width,
    );
    let gh_at = s
        .links
        .entries
        .values()
        .filter_map(|l| l.checked_at_unix_utc)
        .chain(s.links.discovery_checked_at_unix_utc)
        .max();
    put(
        lines,
        &crate::tr!("GitHub 조회 시각: {}", "GitHub checked: {}";
            if gh_at.is_none() {
                crate::tr!("조회 전", "Not checked").into()
            } else {
                s.times.display(gh_at)
            }
        ),
        width,
    );
    if s.gap {
        put(
            lines,
            crate::tr!(
                "수집 공백 있음 · 마지막 확인 기록 · 전체 과거 복원 아님",
                "Collection gap · Last confirmed record · History incomplete"
            ),
            width,
        );
    }
    put(
        lines,
        crate::tr!(
            "다음 행동은 안내이며 실행·승인 권한이 아닙니다.",
            "Next actions are guidance, not execution or approval authority."
        ),
        width,
    );
    for line in &mut lines[first..] {
        line.role = Role::Muted;
    }
}

fn append_unlinked(s: &Snapshot, entries: &mut Vec<Vec<String>>) {
    for record in &s.unlinked_records {
        for version in &record.versions {
            entries.push(vec![
                crate::tr!("미연결 수집 기록 · {:?} {} · 현재 브랜치 기록 아님", "Unlinked collected record · {:?} {} · Outside current branch";
                    record.kind, record.id
                ),
                crate::tr!("{} {} · 조건: {} · 관계: {:?} {:?}", "{} {} · Condition: {} · Relation: {:?} {:?}";
                    s.times.display(version.at_unix_utc),
                    version.text,
                    version.condition.as_deref().unwrap_or(crate::tr!("없음/미기재", "None/not recorded")),
                    version.operation,
                    version.target
                ),
            ]);
        }
    }
    for state in &s.unlinked_states {
        entries.push(vec![
            crate::tr!(
                "미연결 현재 상태 · 현재 브랜치 기록 아님",
                "Unlinked current state · Outside current branch"
            )
            .into(),
            state_line(state, s),
        ]);
    }
}
