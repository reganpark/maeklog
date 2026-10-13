//! Compact projections keep the journal intact and never interpret live work.
use super::{
    Kind, Line, Outcome, Record, Role, Snapshot, StateKind, StateStatus, completed_today, journal,
    latest_follow, muted, outcome, put, state_owner, task_links, task_reference_label,
    task_reference_status,
};

pub fn render_preview(s: &Snapshot, width: usize, queue: &[Line]) -> Vec<Line> {
    let width = width.saturating_sub(2).max(2);
    let mut lines = Vec::new();
    crate::panel_style::wrap(&mut lines, &scope_label(s), width, Role::Title);
    crate::panel_style::wrap(&mut lines, &"─".repeat(width.min(80)), width, Role::Body);
    attention(&mut lines, s, width);
    lines.extend_from_slice(queue);
    if let Some(reason) = &s.scope.unlinked_reason {
        muted(
            &mut lines,
            &crate::tr!("저장 범위 미확인 · {reason}", "Storage scope unknown · {reason}";),
            width,
        );
    }
    let decisions = decisions(s);
    let completed = completions(s);
    if !decisions.is_empty() {
        label(
            &mut lines,
            &crate::tr!("유효 결정 {}건", "Active decisions: {}"; decisions.len()),
            width,
        );
        for record in &decisions {
            record_preview(&mut lines, record, s, width, Role::Body);
        }
    }
    if !completed.is_empty() {
        label(
            &mut lines,
            &crate::tr!("오늘 완료 {}건", "Today’s completion reports: {}"; completed.len()),
            width,
        );
        for record in &completed {
            record_preview(&mut lines, record, s, width, Role::Body);
        }
    }
    let progress = states(s, StateKind::Progress);
    if !progress.is_empty() {
        label(
            &mut lines,
            &crate::tr!("기록된 진행 {}건", "Recorded progress: {}"; progress.len()),
            width,
        );
        for state in &progress {
            state_preview(&mut lines, state, s, width, Role::Body);
        }
    }
    let shown = completed.iter().map(|record| record.id.as_str()).collect();
    render_problems(&mut lines, s, width, &shown);
    let mut unknown = Vec::new();
    if decisions.is_empty() {
        unknown.push(crate::tr!("결정 기록 없음", "No saved decisions"));
    }
    if completed.is_empty() {
        unknown.push(crate::tr!(
            "오늘 완료 보고 없음",
            "No completion reports today"
        ));
    }
    if states(s, StateKind::Blocker).is_empty() {
        unknown.push(crate::tr!("막힘 미확인", "Blockers unknown"));
    }
    if states(s, StateKind::Action).is_empty() {
        unknown.push(crate::tr!("다음 행동 미확인", "Next actions unknown"));
    }
    if !unknown.is_empty() {
        muted(&mut lines, &unknown.join(" · "), width);
    }
    let excluded = s.journal.records.values().filter(|r| r.excluded()).count();
    if excluded > 0 {
        muted(
            &mut lines,
            &crate::tr!("정정으로 제외된 기록 {excluded}건 · h에서 전체 확인", "Excluded by correction: {excluded} · h for all";),
            width,
        );
    }
    lines
}

pub(super) fn scope_label(s: &Snapshot) -> String {
    format!(
        "{} · {}",
        if s.scope.tab_id.is_some() {
            crate::tr!("이 탭의 저장 기록", "This tab’s saved records")
        } else {
            crate::tr!("저장된 기록", "Saved records")
        },
        s.scope
            .branch
            .as_deref()
            .unwrap_or(crate::tr!("브랜치 미확인", "Branch unknown"))
    )
}

pub(super) fn decisions(s: &Snapshot) -> Vec<&Record> {
    let mut values: Vec<_> = s
        .journal
        .records
        .values()
        .filter(|r| r.kind == Kind::Decision && r.active())
        .collect();
    values.sort_by_key(|r| {
        (
            r.latest().at_unix_utc.is_none(),
            std::cmp::Reverse(r.latest().at_unix_utc),
            std::cmp::Reverse(r.latest().order),
        )
    });
    values
}

pub(super) fn completions(s: &Snapshot) -> Vec<&Record> {
    let mut values: Vec<_> = s
        .journal
        .records
        .values()
        .filter(|r| r.kind == Kind::Completion && !r.excluded() && completed_today(r, s))
        .collect();
    values.sort_by_key(|r| std::cmp::Reverse(r.latest().order));
    values
}

pub(super) fn states(s: &Snapshot, kind: StateKind) -> Vec<&journal::State> {
    let mut values: Vec<_> = s
        .journal
        .states
        .values()
        .filter(|v| v.kind == kind && v.status != StateStatus::Resolved)
        .collect();
    values.sort_by_key(|v| std::cmp::Reverse(v.order));
    values
}

fn label(lines: &mut Vec<Line>, text: &str, width: usize) {
    if lines
        .last()
        .is_some_and(|line| !line.text.trim().is_empty())
    {
        lines.push(Line::default());
    }
    crate::panel_style::wrap(lines, text, width, Role::Title);
}

fn attention(lines: &mut Vec<Line>, s: &Snapshot, width: usize) {
    if s.journal.through_unix_utc.is_some() {
        crate::panel_style::wrap(
            lines,
            &crate::tr!("저장 상태 · 마지막 반영 {}", "Saved status · Last included {}";
                s.times.compact(s.journal.through_unix_utc, s.today.as_deref())
            ),
            width,
            Role::Body,
        );
    }
    if let Some(queue) = &s.daily {
        let mut pending = Vec::new();
        if queue.today_turns > 0 {
            pending.push(if queue.today_turns == 1 {
                crate::tr!("오늘 1턴", "Today: 1 turn").into()
            } else {
                crate::tr!("오늘 {}턴", "Today: {} turns"; queue.today_turns)
            });
        }
        let held: usize = queue.manual.values().sum();
        if held > 0 {
            pending.push(if held == 1 {
                crate::tr!("이전·보류 1턴 · b 날짜 선택", "Earlier/held: 1 turn · b select date").into()
            } else {
                crate::tr!("이전·보류 {held}턴 · b 날짜 선택", "Earlier/held: {held} turns · b select date";)
            });
        }
        if !pending.is_empty() {
            crate::panel_style::wrap(
                lines,
                &crate::tr!("이 탭 미반영 완료 대화 · {}", "This tab’s pending completed turns · {}"; pending.join(" · ")),
                width,
                Role::Body,
            );
        }
    }
    if s.gap {
        if lines
            .last()
            .is_some_and(|line| !line.text.trim().is_empty())
        {
            lines.push(Line::default());
        }
        crate::panel_style::wrap(
            lines,
            crate::tr!(
                "과거 수집 공백 있음 · h 상세",
                "Past collection gap · h for details"
            ),
            width,
            Role::Warning,
        );
    }
    if let Some(error) = saved_error(s) {
        label(
            lines,
            crate::tr!(
                "저장 기록 읽기 실패 · h 상세",
                "Saved-record read failed · h for details"
            ),
            width,
        );
        crate::panel_style::wrap(lines, error, width, Role::Body);
    }
}

// Match only app-authored diagnostic prefixes, including already saved messages.
fn task_links_error(error: &str) -> bool {
    ["대화 연결 읽기 실패", "Conversation links read failed"]
        .into_iter()
        .any(|prefix| error.starts_with(prefix) || error.contains(&format!(" · {prefix}")))
}

fn saved_error(s: &Snapshot) -> Option<&str> {
    let error = s.error.as_deref()?;
    if ["대화 연결 읽기 실패", "Conversation links read failed"]
        .into_iter()
        .any(|prefix| error.starts_with(prefix))
    {
        return None;
    }
    Some(
        [
            " · 대화 연결 읽기 실패",
            " · Conversation links read failed",
        ]
        .into_iter()
        .find_map(|separator| error.split_once(separator))
        .map_or(error, |(saved, _)| saved),
    )
}

fn unresolved(record: &Record, s: &Snapshot) -> bool {
    record.kind == Kind::Completion
        && !record.excluded()
        && matches!(record.outcome, Outcome::Failed | Outcome::Rework)
        && !latest_follow(record, &s.journal).is_some_and(|next| {
            next.outcome == Outcome::Passed && next.latest().order > record.latest().order
        })
}

pub(super) fn record_preview(
    lines: &mut Vec<Line>,
    r: &Record,
    s: &Snapshot,
    width: usize,
    role: Role,
) {
    let at = s.times.compact(r.latest().at_unix_utc, s.today.as_deref());
    let tag = if r.kind == Kind::Completion {
        match r.outcome {
            Outcome::Failed => crate::tr!("[실패 보고]", "[Failure reported]"),
            Outcome::Rework => crate::tr!("[재작업 필요]", "[Rework needed]"),
            Outcome::Passed => crate::tr!("[후속 통과 보고]", "[Follow-up passed: reported]"),
            _ => crate::tr!("[보고]", "[Reported]"),
        }
    } else {
        crate::tr!("[결정]", "[Decision]")
    };
    let correction = if crate::corrections::current(r).is_some() {
        crate::tr!(" [사용자 정정]", " [User correction]")
    } else {
        ""
    };
    bullet(
        lines,
        &format!("{at} {tag}{correction} {}", r.text()),
        width,
        if matches!(r.outcome, Outcome::Failed | Outcome::Rework) {
            Role::Warning
        } else {
            role
        },
    );
    if let Some(condition) = r.condition() {
        hanging(
            lines,
            &crate::tr!("조건: {condition}", "Condition: {condition}";),
            width,
            role,
        );
    }
    if let Some(follow) = latest_follow(r, &s.journal) {
        hanging(
            lines,
            &crate::tr!("후속 관계 · {}: {}", "Follow-up · {}: {}"; outcome(follow.outcome), follow.text()),
            width,
            if matches!(follow.outcome, Outcome::Failed | Outcome::Rework) {
                Role::Warning
            } else {
                Role::Body
            },
        );
        if let Some(condition) = follow.condition() {
            hanging(
                lines,
                &crate::tr!("후속 조건: {condition}", "Follow-up condition: {condition}";),
                width,
                Role::Body,
            );
        }
    }
    lines.push(Line::default());
}

fn bullet(lines: &mut Vec<Line>, text: &str, width: usize, role: Role) {
    item(lines, text, width, "• ", role);
}
fn hanging(lines: &mut Vec<Line>, text: &str, width: usize, role: Role) {
    item(lines, text, width, "  ", role);
}
fn item(lines: &mut Vec<Line>, text: &str, width: usize, prefix: &str, role: Role) {
    let mut plain = Vec::new();
    crate::panel::wrapped_bullet(&mut plain, text, width, prefix);
    lines.extend(plain.into_iter().map(|text| Line { text, role }));
}

pub(super) fn state_preview(
    lines: &mut Vec<Line>,
    state: &journal::State,
    s: &Snapshot,
    width: usize,
    role: Role,
) {
    let at = s.times.compact(state.at_unix_utc, s.today.as_deref());
    bullet(
        lines,
        &format!(
            "{at} [{}{}] {}",
            state_owner(&state.owner),
            if state.status == StateStatus::Unconfirmed {
                crate::tr!(" · 현재 상태 미확인", " · Current state unknown")
            } else {
                ""
            },
            state.text
        ),
        width,
        role,
    );
    if let Some(condition) = &state.condition {
        hanging(
            lines,
            &crate::tr!("조건: {condition}", "Condition: {condition}";),
            width,
            Role::Body,
        );
    }
    lines.push(Line::default());
}

fn render_problems(
    lines: &mut Vec<Line>,
    s: &Snapshot,
    width: usize,
    shown: &std::collections::BTreeSet<&str>,
) {
    let failures: Vec<_> = s
        .journal
        .records
        .values()
        .filter(|record| unresolved(record, s) && !shown.contains(record.id.as_str()))
        .collect();
    if !failures.is_empty() {
        label(
            lines,
            &crate::tr!("저장된 후속 실패·재작업 {}건", "Saved follow-up failures/rework: {}"; failures.len()),
            width,
        );
        for record in failures {
            record_preview(lines, record, s, width, Role::Warning);
        }
    }
    let blockers = states(s, StateKind::Blocker);
    if !blockers.is_empty() {
        label(
            lines,
            &crate::tr!("기록된 막힘 {}건", "Recorded blockers: {}"; blockers.len()),
            width,
        );
        for state in blockers {
            state_preview(lines, state, s, width, Role::Body);
        }
    }
    let actions = states(s, StateKind::Action);
    if !actions.is_empty() {
        label(
            lines,
            &crate::tr!("다음 행동 {}건", "Next actions: {}"; actions.len()),
            width,
        );
        for state in actions {
            state_preview(lines, state, s, width, Role::Body);
        }
    }
}

/// Only the dialogue overlay may identify current work. Branch discovery is auxiliary.
pub fn render_current_references(s: &Snapshot, width: usize) -> Vec<Line> {
    let mut lines = Vec::new();
    let width = width.saturating_sub(2).max(2);
    let Some(task) = &s.task else {
        if let Some(error) = s.error.as_ref().filter(|error| task_links_error(error)) {
            crate::panel_style::wrap(&mut lines, error, width, Role::Warning);
        } else {
            muted(
                &mut lines,
                crate::tr!("대화 참조 정보 미확인", "Conversation references unknown"),
                width,
            );
        }
        return lines;
    };
    let unlinked = task.current.is_empty() && task.related_pr.is_empty();
    if unlinked {
        let mut info = crate::tr!("연결 정보 · {}", "Links · {}";
            if task.explicit_none {
                crate::tr!("참조 연결 안 함 · 직접 정정", "No reference link · Manual correction")
            } else {
                crate::tr!("이슈·PR 미연결", "No issue/PR linked")
            }
        );
        if !task.candidates.is_empty() {
            info.push_str(&crate::tr!(" · 참고 후보 {}개 · l로 보기", " · Reference candidates: {} · l to view"; task.candidates.len()));
        }
        muted(&mut lines, &info, width);
    }
    for reference in &task.current {
        current_reference(&mut lines, reference, false, width);
    }
    let related: Vec<_> = task
        .related_pr
        .iter()
        .filter(|reference| {
            !task
                .current
                .iter()
                .any(|current| current.id == reference.id)
        })
        .collect();
    for reference in related {
        current_reference(&mut lines, reference, true, width);
    }
    if let Some(location) = &task.worktree {
        if location.verification == task_links::VerificationState::Verified {
            if let Some(context) = &location.context {
                muted(
                    &mut lines,
                    &crate::tr!("인계한 작업 위치 · {}", "Reported work location · {}";
                        context.branch.as_deref().unwrap_or(crate::tr!("브랜치 미확인", "Branch unknown"))
                    ),
                    width,
                );
            }
        } else {
            muted(
                &mut lines,
                crate::tr!("인계한 작업 위치 미확인", "Reported work location unknown"),
                width,
            );
        }
    }
    if let Some(previous) = task.previous.last() {
        let name = match previous.current.as_slice() {
            [] if previous.explicit_none => crate::tr!("연결 없음", "No link").into(),
            [] => String::new(),
            [reference] => task_reference_label(reference),
            refs => crate::tr!("참조 {}개", "References: {}"; refs.len()),
        };
        if !name.is_empty() {
            muted(
                &mut lines,
                &crate::tr!("이전 연결 · {name}", "Previous link · {name}";),
                width,
            );
        }
    }
    if !unlinked && !task.candidates.is_empty() {
        muted(
            &mut lines,
            &crate::tr!("연결 정보 · 참고 후보 {}개 · l로 보기", "Links · Reference candidates: {} · l to view";
                task.candidates.len()
            ),
            width,
        );
    }
    lines
}

fn current_reference(
    lines: &mut Vec<Line>,
    reference: &task_links::TaskReference,
    related: bool,
    width: usize,
) {
    let status = if !related && reference.verification == task_links::VerificationState::Checking {
        crate::tr!("지정됨 · 확인 중", "Selected · Checking")
    } else {
        task_reference_status(reference)
    };
    crate::panel_style::wrap(
        lines,
        &format!(
            "{}{} · {status}",
            if related {
                crate::tr!("관련 ", "Related ")
            } else {
                ""
            },
            task_reference_label(reference)
        ),
        width,
        match reference.verification {
            task_links::VerificationState::Verified => Role::Heading,
            task_links::VerificationState::Checking => Role::Muted,
            task_links::VerificationState::Error => Role::Warning,
        },
    );
    if let Some(title) = reference
        .link
        .as_ref()
        .and_then(|link| link.title.as_deref())
    {
        put(lines, title, width);
    }
    if let Some(error) = &reference.error {
        crate::panel_style::wrap(lines, error, width, Role::Warning);
    }
    lines.push(Line::default());
}

#[cfg(test)]
mod tests {
    use super::super::{RecordSection, record_sections};
    use super::*;
    use crate::{branch_context as context, local_time::LocalTimes, work_links};
    use std::collections::BTreeMap;
    fn record(number: u64, kind: Kind, outcome: Outcome, at: Option<u64>) -> Record {
        Record {
            corrections: vec![],
            id: format!("record{number}"),
            key: format!("key{number}"),
            kind,
            stage: journal::Stage::Deployment,
            outcome,
            versions: vec![journal::Version {
                operation: journal::Operation::Add,
                text: format!("원문{number}"),
                condition: Some(format!("조건{number} · 사용자 승인 후에만 적용")),
                at_unix_utc: at,
                order: number,
                target: None,
                proof: vec![],
                decision_basis: None,
            }],
            replaced_by: None,
            withdrawn: false,
            follows: None,
            relation_order: number,
        }
    }
    fn snapshot(records: Vec<Record>) -> Snapshot {
        let at = 1_792_000_000;
        let times = LocalTimes::new([at]);
        Snapshot {
            today: times.day(Some(at)),
            daily: None,
            scope: context::Context {
                tab_id: Some("tab".into()),
                branch: Some("main".into()),
                ..Default::default()
            },
            journal: journal::Journal {
                records: records.into_iter().map(|r| (r.id.clone(), r)).collect(),
                ..Default::default()
            },
            links: work_links::Links::default(),
            task: Some(task_links::TaskLinks::default()),
            legacy: vec![],
            unlinked_records: vec![],
            unlinked_states: vec![],
            times,
            pending: 0,
            other_pending: 0,
            gap: false,
            history_limited: false,
            error: None,
        }
    }
    fn text(lines: &[Line]) -> String {
        lines.iter().map(|line| line.text.as_str()).collect()
    }
    #[test]
    fn full_reading_keeps_all_records_conditions_and_failure_once() {
        let mut records: Vec<_> = (1..=5)
            .map(|number| {
                record(
                    number,
                    Kind::Completion,
                    Outcome::Reported,
                    Some(1_792_000_000),
                )
            })
            .collect();
        records.push(record(0, Kind::Completion, Outcome::Failed, None));
        let snapshot = snapshot(records);
        let lines = render_preview(&snapshot, 80, &[]);
        let text = text(&lines);
        assert!(
            text.contains(crate::tr!("오늘 완료 5건", "Today’s completion reports: 5"))
                && !text.contains(crate::tr!("추가", "More:"))
        );
        assert!(text.contains("원문5") && text.contains("조건5 · 사용자 승인 후에만 적용"));
        assert!(text.contains("원문1") && text.contains("조건1"));
        assert_eq!(text.matches("원문0").count(), 1);
        assert!(lines.iter().any(|line| line.text.contains(crate::tr!(
            "저장된 후속 실패·재작업 1건",
            "Saved follow-up failures/rework: 1"
        )) && line.role == Role::Title));
        assert_eq!(snapshot.journal.records.len(), 6);
        let full = record_sections(&snapshot);
        assert!(text_full(&full[0]).contains("원문1") && text_full(&full[0]).contains("조건1"));
        assert!(full[0].rows.iter().all(|line| !line.text.contains("order")
            && !line.text.contains("Deployment")
            && !line.text.contains("Reported")));
    }
    fn text_full(section: &RecordSection) -> String {
        text(&section.rows)
    }
    #[test]
    fn visible_failure_is_not_duplicated_and_owner_and_action_count_stay_clear() {
        let mut snapshot = snapshot(vec![record(
            1,
            Kind::Completion,
            Outcome::Failed,
            Some(1_792_000_000),
        )]);
        snapshot.journal.states = (1..=4)
            .map(|n| {
                (
                    format!("a{n}"),
                    journal::State {
                        key: format!("a{n}"),
                        kind: StateKind::Action,
                        text: format!("행동{n}"),
                        owner: "codex".into(),
                        condition: Some("승인한 경우에만".into()),
                        status: StateStatus::Unconfirmed,
                        at_unix_utc: None,
                        order: n,
                        proof: vec![],
                        uncertain_by_omission: false,
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        let lines = render_preview(&snapshot, 80, &[]);
        let text = text(&lines);
        assert_eq!(text.matches("원문1").count(), 1);
        assert!(
            text.contains(crate::tr!("실패 보고", "Failure reported"))
                && text.contains("Codex")
                && text.contains(crate::tr!("현재 상태 미확인", "Current state unknown"))
        );
        assert!(text.contains("행동4") && text.contains("행동1"));
        assert!(
            text.contains(crate::tr!("다음 행동 4건", "Next actions: 4"))
                && !text.contains(crate::tr!("추가", "More:"))
        );
        assert_eq!(text.matches("승인한 경우에만").count(), 4);
        assert!(!lines.iter().any(|line| line.role == Role::Heading));
    }
    #[test]
    fn branch_cache_never_becomes_dialogue_current_and_legacy_scope_is_neutral() {
        let mut snapshot = snapshot(vec![]);
        let link = work_links::parse(work_links::Kind::Pr, "example/project#7").unwrap();
        snapshot.links.primary_pr = Some(link.id());
        snapshot.links.entries.insert(link.id(), link);
        assert!(!text(&render_current_references(&snapshot, 80)).contains("example/project#7"));
        assert!(
            text(&render_current_references(&snapshot, 80))
                .contains(crate::tr!("미연결", "No issue/PR linked"))
        );
        snapshot.scope.tab_id = None;
        let text = text(&render_preview(&snapshot, 80, &[]));
        assert!(text.starts_with(crate::tr!("저장된 기록 · main", "Saved records · main")));
        assert!(!text.contains(crate::tr!("이 탭의 저장 기록", "This tab’s saved records")));
    }
    #[test]
    fn unlinked_candidates_share_one_connection_group_without_becoming_current() {
        let mut snapshot = snapshot(vec![]);
        let candidate: task_links::TaskReference = serde_json::from_value(serde_json::json!({
            "id":"example/project#7", "repository":"example/project", "number":7,
            "kind":"issue", "raw":"example/project#7", "verification":"checking",
            "provenance":"assistant_mention", "source":{}
        }))
        .unwrap();
        snapshot.task.as_mut().unwrap().candidates.push(candidate);
        let lines = render_current_references(&snapshot, 100);
        assert_eq!(lines.len(), 1);
        assert_eq!(
            lines[0].text,
            crate::tr!(
                "연결 정보 · 이슈·PR 미연결 · 참고 후보 1개 · l로 보기",
                "Links · No issue/PR linked · Reference candidates: 1 · l to view"
            )
        );
        assert_eq!(lines[0].role, Role::Muted);
        assert!(!lines[0].text.contains(crate::tr!("지정됨", "Selected")));

        snapshot.task.as_mut().unwrap().explicit_none = true;
        let lines = render_current_references(&snapshot, 100);
        assert_eq!(lines.len(), 1);
        assert_eq!(
            lines[0].text,
            crate::tr!(
                "연결 정보 · 참조 연결 안 함 · 직접 정정 · 참고 후보 1개 · l로 보기",
                "Links · No reference link · Manual correction · Reference candidates: 1 · l to view"
            )
        );
        let narrow = render_current_references(&snapshot, 30);
        assert_eq!(
            text(&narrow).split_whitespace().collect::<String>(),
            text(&lines).split_whitespace().collect::<String>()
        );
        assert!(narrow.iter().all(|line| line.role == Role::Muted));
        assert!(snapshot.task.as_ref().unwrap().current.is_empty());
    }
    #[test]
    fn long_items_wrap_with_time_body_and_hanging_indent_and_scope_status_is_once() {
        let mut snapshot = snapshot(vec![record(
            1,
            Kind::Completion,
            Outcome::Reported,
            Some(1_792_000_000),
        )]);
        let body = format!(
            "{} 원문 끝 · 아직 적용하지 못했습니다",
            "공통 평면과 공간 계획을 확인했습니다. ".repeat(15)
        );
        snapshot
            .journal
            .records
            .get_mut("record1")
            .unwrap()
            .versions[0]
            .text = body.clone();
        snapshot.journal.through_unix_utc = Some(1_792_000_000);
        snapshot.pending = 1;
        snapshot.gap = true;
        let lines = render_preview(&snapshot, 30, &[]);
        let rendered = text(&lines);
        let normalize = |value: &str| {
            value
                .chars()
                .filter(|character| !character.is_whitespace())
                .collect::<String>()
        };
        assert!(normalize(&rendered).contains(&normalize(&body)));
        assert!(
            !normalize(&rendered)
                .contains(&normalize(crate::tr!("미반영 이벤트", "Pending events")))
        );
        assert!(
            record_sections(&snapshot)
                .last()
                .unwrap()
                .rows
                .iter()
                .any(|line| { line.text.contains(crate::tr!("미반영 1", "Pending 1")) })
        );
        assert_eq!(
            normalize(&rendered)
                .matches(&normalize(crate::tr!(
                    "수집 공백 있음",
                    "Past collection gap"
                )))
                .count(),
            1
        );
        assert_eq!(
            normalize(&rendered)
                .matches(&normalize(crate::tr!("마지막 반영", "Last included")))
                .count(),
            1
        );
        let item = lines
            .iter()
            .position(|line| line.text.starts_with("• "))
            .unwrap();
        assert!(
            lines[item]
                .text
                .contains(crate::tr!("[보고]", "[Reported]"))
                && lines[item].text.contains("공통")
        );
        assert!(lines[item + 1].text.starts_with("  "));
        assert!(
            lines
                .iter()
                .all(|line| unicode_width::UnicodeWidthStr::width(line.text.as_str()) <= 28)
        );
        assert!(!rendered.contains(crate::tr!("추가", "More:")) && !rendered.contains('…'));
    }
    #[test]
    fn all_current_and_related_titles_are_readable_without_hidden_targets() {
        let mut snapshot = snapshot(vec![]);
        let refs:Vec<task_links::TaskReference>=(1..=4).map(|n| serde_json::from_value(serde_json::json!({"id":format!("example/project#{n}"),"repository":"example/project","number":n,"kind":"issue","raw":format!("example/project#{n}"),"verification":"verified","provenance":"user_prompt","source":{},"link":{"kind":"issue","repository":"example/project","number":n,"url":format!("https://github.com/example/project/issues/{n}"),"title":format!("{} 제목{n}마지막까지", "긴 한국어 작업 이름을 읽습니다. ".repeat(20))}})).unwrap()).collect();
        snapshot.task.as_mut().unwrap().current = refs;
        let lines = render_current_references(&snapshot, 30);
        let text = text(&lines);
        for n in 1..=4 {
            assert!(text.contains(&format!("제목{n}마지막까지")));
        }
        assert!(!text.contains(crate::tr!("외 ", "more")) && !text.contains('…'));
    }
    #[test]
    fn freshness_pending_and_recorded_blockers_keep_separate_meanings_at_all_widths() {
        let at = 1_792_000_000;
        let mut snapshot = snapshot(vec![]);
        let body = format!(
            "{} 아직 공개할 수 없음 · 이후 해결 여부는 이 원문에 보고되지 않았음",
            "개인 경로와 기존 식별자를 확인해야 합니다. ".repeat(4)
        );
        snapshot.journal.through_unix_utc = Some(at);
        snapshot.pending = 8;
        snapshot.other_pending = 1;
        snapshot.daily = Some(crate::daily::Queue {
            today: "2026-10-13".into(),
            today_turns: 1,
            manual: [("2026-10-12".into(), 1)].into(),
            open_events: 1,
            requested: None,
        });
        snapshot.journal.states.insert(
            "old-blocker".into(),
            journal::State {
                key: "old-blocker".into(),
                kind: StateKind::Blocker,
                text: body.clone(),
                owner: "unknown".into(),
                condition: Some("원문 조건 · 확인 전에는 해결로 바꾸지 않음".into()),
                status: StateStatus::Active,
                at_unix_utc: Some(at),
                order: 1,
                proof: vec![],
                uncertain_by_omission: false,
            },
        );
        let original = serde_json::to_value(&snapshot.journal).unwrap();
        let normalize = |value: &str| value.split_whitespace().collect::<String>();
        for width in [30, 50, 80] {
            let lines = render_preview(&snapshot, width, &[]);
            let rendered = normalize(&text(&lines));
            assert!(rendered.contains(&normalize(&body)));
            assert!(rendered.contains(&normalize("원문 조건 · 확인 전에는 해결로 바꾸지 않음")));
            assert!(rendered.contains(&normalize(
                &snapshot.times.compact(Some(at), snapshot.today.as_deref())
            )));
            assert!(rendered.contains(&normalize(crate::tr!(
                "기록된 막힘 1건",
                "Recorded blockers: 1"
            ))));
            assert_eq!(
                rendered
                    .matches(&normalize(crate::tr!("마지막 반영", "Last included")))
                    .count(),
                1
            );
            assert_eq!(
                rendered
                    .matches(&normalize(crate::tr!(
                        "이 탭 미반영 완료 대화",
                        "This tab’s pending completed turns"
                    )))
                    .count(),
                1
            );
            assert!(rendered.contains(&normalize(crate::tr!("오늘 1턴", "Today: 1 turn"))));
            assert!(rendered.contains(&normalize(crate::tr!(
                "이전·보류 1턴",
                "Earlier/held: 1 turn"
            ))));
            assert!(!rendered.contains(&normalize(crate::tr!("미반영 이벤트", "Pending events"))));
            assert!(
                lines
                    .iter()
                    .all(|line| line.role != Role::Warning && line.role != Role::Error)
            );
            assert!(
                lines.iter().all(
                    |line| unicode_width::UnicodeWidthStr::width(line.text.as_str()) <= width - 2
                )
            );
            assert!(
                !lines
                    .windows(2)
                    .any(|pair| pair.iter().all(|line| line.text.trim().is_empty()))
            );
        }
        assert_eq!(serde_json::to_value(&snapshot.journal).unwrap(), original);
        assert_eq!(
            snapshot.journal.states["old-blocker"].status,
            StateStatus::Active
        );
    }

    #[test]
    fn recorded_unconfirmed_state_keeps_its_status_and_full_body() {
        let mut snapshot = snapshot(vec![]);
        snapshot.journal.states.insert(
            "unknown-blocker".into(),
            journal::State {
                key: "unknown-blocker".into(),
                kind: StateKind::Blocker,
                text: "확인되지 않은 막힘 원문 · 완료 여부를 추정하지 않음".into(),
                owner: "user".into(),
                condition: None,
                status: StateStatus::Unconfirmed,
                at_unix_utc: None,
                order: 1,
                proof: vec![],
                uncertain_by_omission: true,
            },
        );
        let normalize = |value: &str| value.split_whitespace().collect::<String>();
        let rendered = normalize(&text(&render_preview(&snapshot, 80, &[])));
        assert!(rendered.contains(&normalize(crate::tr!(
            "현재 상태 미확인",
            "Current state unknown"
        ))));
        assert!(rendered.contains(&normalize(
            "확인되지 않은 막힘 원문 · 완료 여부를 추정하지 않음"
        )));
        assert_eq!(
            snapshot.journal.states["unknown-blocker"].status,
            StateStatus::Unconfirmed
        );
    }
}
