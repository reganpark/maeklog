//! Frozen reader data: every saved version, condition and evidence stays reachable.
use super::{
    Kind, Line, Outcome, Record, Role, Snapshot, StateKind, StateStatus, compact, completed_today,
    journal, latest_follow, outcome, state_owner,
};

#[derive(Clone, Debug)]
pub struct RecordSection {
    pub title: String,
    pub rows: Vec<Line>,
}

pub fn record_sections(s: &Snapshot) -> Vec<RecordSection> {
    let today = compact::completions(s);
    let decisions = compact::decisions(s);
    let mut sections = vec![
        records(
            crate::tr!("오늘 완료", "Today’s completion reports"),
            &today,
            s,
        ),
        records(crate::tr!("유효 결정", "Active decisions"), &decisions, s),
    ];
    let mut state_rows = Vec::new();
    let mut states: Vec<_> = s.journal.states.values().collect();
    states.sort_by_key(|state| std::cmp::Reverse(state.order));
    for state in &states {
        state_details(&mut state_rows, state, s);
    }
    sections.push(section(
        crate::tr!(
            "기록된 진행·막힘·다음 행동",
            "Recorded progress, blockers, next actions"
        ),
        states.len(),
        state_rows,
    ));
    let mut prior: Vec<_> = s
        .journal
        .records
        .values()
        .filter(|record| {
            !record.excluded()
                && match record.kind {
                    Kind::Decision => !record.active(),
                    Kind::Completion => !completed_today(record, s),
                }
        })
        .collect();
    prior.sort_by_key(|record| std::cmp::Reverse(record.latest().order));
    sections.push(records(
        crate::tr!(
            "이전 기록 · 시각 미확인 포함",
            "Earlier records · Includes unknown times"
        ),
        &prior,
        s,
    ));
    let excluded: Vec<_> = s
        .journal
        .records
        .values()
        .filter(|record| record.excluded())
        .collect();
    sections.push(records(
        crate::tr!(
            "사용자 정정으로 제외된 기록",
            "Records excluded by user correction"
        ),
        &excluded,
        s,
    ));
    sections.push(unlinked(s));
    sections.push(evidence(s));
    sections.push(diagnostics(s));
    sections
}

fn section(title: &str, count: usize, mut rows: Vec<Line>) -> RecordSection {
    if rows.is_empty() {
        row(
            &mut rows,
            crate::tr!(
                "이 범주의 저장 기록 없음",
                "No saved records in this category"
            ),
            Role::Muted,
        );
    }
    RecordSection {
        title: crate::tr!("{title} · {count}건", "{title} · {count}";),
        rows,
    }
}

fn records(title: &str, records: &[&Record], s: &Snapshot) -> RecordSection {
    let mut rows = Vec::new();
    for record in records {
        record_details(&mut rows, record, s);
    }
    section(title, records.len(), rows)
}

fn row(rows: &mut Vec<Line>, text: impl Into<String>, role: Role) {
    rows.push(Line {
        text: text.into(),
        role,
    });
}

fn record_details(rows: &mut Vec<Line>, r: &Record, s: &Snapshot) {
    row(
        rows,
        format!(
            "{} · {} · {}",
            if r.kind == Kind::Decision {
                crate::tr!("결정", "Decision")
            } else {
                crate::tr!("완료 보고", "Completion reported")
            },
            outcome(r.outcome),
            s.times.display(r.latest().at_unix_utc)
        ),
        if matches!(r.outcome, Outcome::Failed | Outcome::Rework) {
            Role::Warning
        } else {
            Role::Muted
        },
    );
    row(rows, r.text(), Role::Body);
    if let Some(condition) = r.condition() {
        row(
            rows,
            crate::tr!("조건: {condition}", "Condition: {condition}";),
            Role::Body,
        );
    }
    if r.excluded() {
        row(
            rows,
            crate::tr!("사용자 정정으로 제외됨", "Excluded by user correction"),
            Role::Muted,
        );
    }
    if r.withdrawn {
        row(
            rows,
            crate::tr!("철회된 결정", "Withdrawn decision"),
            Role::Muted,
        );
    }
    if let Some(id) = &r.replaced_by {
        let replacement = s.journal.records.get(id).map_or(id.as_str(), Record::text);
        row(
            rows,
            crate::tr!("대체된 결정: {replacement}", "Replaced by: {replacement}";),
            Role::Body,
        );
    }
    if let Some(id) = &r.follows {
        let original = s.journal.records.get(id).map_or(id.as_str(), Record::text);
        row(
            rows,
            crate::tr!("이전 보고의 후속: {original}", "Follows earlier report: {original}";),
            Role::Body,
        );
    }
    if let Some(next) = latest_follow(r, &s.journal) {
        row(
            rows,
            crate::tr!("후속 · {} · {}: {}", "Follow-up · {} · {}: {}";
                outcome(next.outcome),
                s.times.display(next.latest().at_unix_utc),
                next.text()
            ),
            Role::Body,
        );
        if let Some(condition) = next.condition() {
            row(
                rows,
                crate::tr!("후속 조건: {condition}", "Follow-up condition: {condition}";),
                Role::Body,
            );
        }
    }
    rows.push(Line::default());
}

fn record_evidence(rows: &mut Vec<Line>, r: &Record, s: &Snapshot) {
    row(
        rows,
        crate::tr!("기록 {} · {}", "Record {} · {}"; r.id, r.text()),
        Role::Title,
    );
    for version in &r.versions {
        row(
            rows,
            crate::tr!("저장된 버전 · {:?} · order {} · {}", "Saved version · {:?} · order {} · {}";
                version.operation,
                version.order,
                s.times.display(version.at_unix_utc)
            ),
            Role::Muted,
        );
        row(rows, &version.text, Role::Body);
        if let Some(condition) = &version.condition {
            row(
                rows,
                crate::tr!("원래 조건: {condition}", "Original condition: {condition}";),
                Role::Body,
            );
        }
        if let Some(target) = &version.target {
            row(
                rows,
                crate::tr!("버전 대상: {target}", "Version target: {target}";),
                Role::Muted,
            );
        }
        proof_rows(rows, &version.proof, s);
    }
    for correction in &r.corrections {
        row(
            rows,
            crate::tr!("사용자 정정 #{} · {} · {}", "User correction #{} · {} · {}";
                correction.sequence,
                s.times.display(Some(correction.at_unix_utc)),
                correction.reason
            ),
            Role::Muted,
        );
        content_rows(
            rows,
            crate::tr!("원래 기록", "Original record"),
            &correction.original,
        );
        if let Some(before) = &correction.before {
            content_rows(rows, crate::tr!("정정 전", "Before correction"), before);
        }
        if let Some(after) = &correction.after {
            content_rows(rows, crate::tr!("정정 후", "After correction"), after);
        }
        row(
            rows,
            crate::tr!("정정 Space {} · 되돌린 정정 {:?}", "Correction Space {} · Undone correction {:?}";
                correction.space, correction.undo_of
            ),
            Role::Muted,
        );
    }
    rows.push(Line::default());
}

fn content_rows(rows: &mut Vec<Line>, label: &str, content: &crate::corrections::Content) {
    row(
        rows,
        crate::tr!("{label}: {} · 제외 {}", "{label}: {} · Excluded {}"; content.text, content.excluded),
        Role::Body,
    );
    if let Some(condition) = &content.condition {
        row(
            rows,
            crate::tr!("{label} 조건: {condition}", "{label} condition: {condition}";),
            Role::Body,
        );
    }
}

fn state_details(rows: &mut Vec<Line>, state: &journal::State, s: &Snapshot) {
    row(
        rows,
        format!(
            "{} · {} · {}",
            state_kind(state.kind),
            state_status(state.status),
            s.times.display(state.at_unix_utc)
        ),
        if state.kind == StateKind::Blocker && state.status == StateStatus::Active {
            Role::Warning
        } else {
            Role::Muted
        },
    );
    row(rows, &state.text, Role::Body);
    row(
        rows,
        crate::tr!("담당 {}{}", "Owner {}{}";
            state_owner(&state.owner),
            if state.uncertain_by_omission {
                crate::tr!(" · 후속 수집에서 현재 상태 미확인", " · Current state unknown in later collection")
            } else {
                ""
            }
        ),
        Role::Muted,
    );
    if let Some(condition) = &state.condition {
        row(
            rows,
            crate::tr!("조건: {condition}", "Condition: {condition}";),
            Role::Body,
        );
    }
    rows.push(Line::default());
}

fn state_kind(kind: StateKind) -> &'static str {
    match kind {
        StateKind::Progress => crate::tr!("기록된 진행", "Recorded progress"),
        StateKind::Blocker => crate::tr!("막힘", "Blockers"),
        StateKind::Action => crate::tr!("다음 행동", "Next actions"),
    }
}
fn state_status(status: StateStatus) -> &'static str {
    match status {
        StateStatus::Active => crate::tr!("유효", "Active"),
        StateStatus::Resolved => crate::tr!("해결 근거 확인", "Resolution evidence found"),
        StateStatus::Unconfirmed => crate::tr!("현재 상태 미확인", "Current state unknown"),
    }
}

fn evidence(s: &Snapshot) -> RecordSection {
    let mut rows = Vec::new();
    for record in s.journal.records.values().chain(&s.unlinked_records) {
        record_evidence(&mut rows, record, s);
    }
    for state in s.journal.states.values().chain(&s.unlinked_states) {
        row(
            &mut rows,
            crate::tr!("상태 {} · {}", "State {} · {}"; state.key, state.text),
            Role::Title,
        );
        proof_rows(&mut rows, &state.proof, s);
    }
    if rows.is_empty() {
        row(
            &mut rows,
            crate::tr!("저장된 변경·근거 없음", "No saved changes or evidence"),
            Role::Muted,
        );
    }
    RecordSection {
        title: crate::tr!(
            "변경·근거 상세 · 저장된 버전과 발췌",
            "Changes and evidence · Saved versions and excerpts"
        )
        .into(),
        rows,
    }
}

fn proof_rows(rows: &mut Vec<Line>, proof: &[journal::Proof], s: &Snapshot) {
    for proof in proof {
        row(
            rows,
            crate::tr!("저장된 근거 발췌 · {:?} · {}", "Saved evidence excerpt · {:?} · {}";
                proof.source.kind,
                s.times.display(proof.source.at_unix_utc)
            ),
            Role::Muted,
        );
        row(rows, &proof.quote, Role::Body);
        row(
            rows,
            crate::tr!("Space {} · 세션 {} · 턴 {} · 원래 브랜치 {}", "Space {} · Session {} · Turn {} · Original branch {}";
                proof.source.space,
                proof.source.session,
                proof.source.turn.as_deref().unwrap_or(crate::tr!("미확인", "Unknown")),
                proof.source.context.branch.as_deref().unwrap_or(crate::tr!("미확인", "Unknown"))
            ),
            Role::Muted,
        );
    }
}

fn unlinked(s: &Snapshot) -> RecordSection {
    let mut rows = Vec::new();
    let count = s.legacy.len() + s.unlinked_records.len() + s.unlinked_states.len();
    for legacy in &s.legacy {
        row(
            &mut rows,
            crate::tr!("미연결 구버전 요약 r{} · 현재 탭의 확정 기록으로 추정하지 않음", "Unlinked legacy summary r{} · Not treated as this tab’s confirmed records";
                legacy.revision
            ),
            Role::Title,
        );
        row(&mut rows, &legacy.summary.objective, Role::Body);
        for decision in &legacy.summary.decisions {
            row(
                &mut rows,
                crate::tr!("결정 후보 · {}: {}", "Decision candidate · {}: {}";
                    match decision.status {
                        crate::model::DecisionStatus::Active => crate::tr!("유효 보고", "Active: reported"),
                        crate::model::DecisionStatus::Proposed => crate::tr!("제안", "Proposed"),
                        crate::model::DecisionStatus::Superseded => crate::tr!("대체됨", "Superseded"),
                        crate::model::DecisionStatus::Reversed => crate::tr!("철회 보고", "Withdrawn: reported"),
                    },
                    decision.text
                ),
                Role::Body,
            );
        }
        for progress in &legacy.summary.progress {
            row(
                &mut rows,
                crate::tr!("진행 · {}: {}", "Progress · {}: {}";
                    match progress.status {
                        crate::model::ProgressStatus::Done => crate::tr!("완료 보고", "Completion reported"),
                        crate::model::ProgressStatus::Reported => crate::tr!("보고", "Reported"),
                        crate::model::ProgressStatus::InProgress => crate::tr!("진행 보고", "Progress reported"),
                        crate::model::ProgressStatus::Blocked => crate::tr!("막힘", "Blockers"),
                        crate::model::ProgressStatus::Unverified => crate::tr!("미확인", "Unknown"),
                    },
                    progress.text
                ),
                Role::Body,
            );
        }
        for blocker in &legacy.summary.blockers {
            row(
                &mut rows,
                crate::tr!("막힘: {}", "Blocker: {}"; blocker.text()),
                Role::Body,
            );
        }
        for action in &legacy.summary.next_actions {
            row(
                &mut rows,
                crate::tr!("다음 행동: {} · 담당 {} · 조건 {}", "Next action: {} · Owner {} · Condition {}";
                    action.text,
                    action.owner,
                    action.when.as_deref().unwrap_or(crate::tr!("미확인", "Unknown"))
                ),
                Role::Body,
            );
        }
    }
    for record in &s.unlinked_records {
        record_details(&mut rows, record, s);
    }
    for state in &s.unlinked_states {
        state_details(&mut rows, state, s);
    }
    section(
        crate::tr!(
            "미연결·구버전 기록 · 현재 범위로 재귀속하지 않음",
            "Unlinked and legacy records · Original scope preserved"
        ),
        count,
        rows,
    )
}

fn diagnostics(s: &Snapshot) -> RecordSection {
    let mut rows = Vec::new();
    row(
        &mut rows,
        crate::tr!("열람 범위 · {}", "Viewing scope · {}"; compact::scope_label(s)),
        Role::Body,
    );
    row(
        &mut rows,
        crate::tr!("탭 {:?} · scope {} · 저장소 {:?}", "Tab {:?} · Scope {} · Repository {:?}";
            s.scope.tab_id, s.scope.key, s.scope.repository
        ),
        Role::Muted,
    );
    row(
        &mut rows,
        crate::tr!("대화 반영 {} · 원장 갱신 {} · 미반영 {} · 다른 브랜치 대기 {}", "Conversation included {} · Journal updated {} · Pending {} · Other-branch pending {}";
            s.times.display(s.journal.through_unix_utc),
            s.times.display(s.journal.updated_at_unix_utc),
            s.pending,
            s.other_pending
        ),
        Role::Muted,
    );
    if let Some(queue) = &s.daily {
        row(
            &mut rows,
            crate::tr!("요약 큐 · 날짜 {} · 오늘 대기 {}턴 · 이전 날짜 {}개 · 열린 이벤트 {}", "Summary queue · Date {} · Today’s pending turns {} · Earlier dates {} · Open events {}";
                queue.today,
                queue.today_turns,
                queue.manual.len(),
                queue.open_events
            ),
            Role::Muted,
        );
    }
    for gap in &s.journal.gaps {
        row(
            &mut rows,
            crate::tr!("과거 수집 공백 · Space {} · generation {} · {}", "Past collection gap · Space {} · Generation {} · {}";
                gap.space, gap.generation, gap.reason
            ),
            Role::Warning,
        );
    }
    if s.gap {
        row(
            &mut rows,
            crate::tr!(
                "과거 수집 공백은 미반영 대화와 다릅니다 · r은 수집된 오늘 완료 대화를 반영합니다",
                "Past collection gap · r includes today’s collected completed conversations"
            ),
            Role::Warning,
        );
    }
    if s.history_limited {
        row(
            &mut rows,
            crate::tr!(
                "수집 범위 · 수집된 대화 기준이며 전체 과거를 복원한 기록은 아닙니다",
                "Collection scope · Based on collected conversations; earlier history may be incomplete"
            ),
            Role::Body,
        );
    }
    if let Some(error) = &s.error {
        row(&mut rows, error, Role::Warning);
    }
    for link in s.links.entries.values() {
        row(
            &mut rows,
            crate::tr!("브랜치 참조 {:?} {}#{} · {} · 마지막 확인 {}", "Branch reference {:?} {}#{} · {} · Last checked {}";
                link.kind,
                link.repository,
                link.number,
                link.title.as_deref().unwrap_or(crate::tr!("제목 미확인", "Title unknown")),
                s.times.display(link.checked_at_unix_utc)
            ),
            Role::Body,
        );
        row(&mut rows, &link.url, Role::Body);
        if let Some(error) = &link.error {
            row(&mut rows, error, Role::Warning);
        }
    }
    RecordSection {
        title: crate::tr!(
            "범위·시각·수집 상세",
            "Scope, times, and collection details"
        )
        .into(),
        rows,
    }
}
