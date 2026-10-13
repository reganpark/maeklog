//! Non-model contract tests: isolated Git repositories and deterministic evidence.
use crate::{branch_context as ctx, journal::*, model::*, storage::*};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
static NEXT: AtomicU64 = AtomicU64::new(0);
#[test]
fn requests_questions_and_conditional_approvals_have_distinct_admission() {
    use crate::decision_policy::Intent;
    let fixture = Fixture::new();
    for (seq, text, intent) in [
        (1, "시점별 버튼은 어디갔어?", Intent::Question),
        (2, "버튼을 복원해 줘", Intent::Request),
        (3, "이 방안도 검토 중이다", Intent::Uncertain),
        (4, "다른 방안을 제안한다", Intent::Proposal),
    ] {
        let event = fixture.event(seq, EventKind::UserPrompt, text, Some(seq * 100));
        let mut change = decision(&event, &format!("case-{seq}"));
        change.decision_basis.as_mut().unwrap().intent = intent;
        let summary = Summary {
            record_updates: vec![change.clone()],
            ..Default::default()
        };
        assert!(
            apply(&fixture.store, std::slice::from_ref(&event), &summary)
                .unwrap_err()
                .to_string()
                .contains("decision_basis_not_confirmed")
        );
        change.decision_basis = None;
        assert!(
            apply(
                &fixture.store,
                std::slice::from_ref(&event),
                &Summary {
                    record_updates: vec![change],
                    ..Default::default()
                }
            )
            .unwrap_err()
            .to_string()
            .contains("decision_basis_required")
        );
        if intent == Intent::Request || intent == Intent::Question {
            let mut action = state(&event, StateKind::Action, &format!("request-{seq}"));
            action.owner = "codex".into();
            apply_changes(&fixture, &event, vec![], vec![action]);
        }
    }
    assert!(fixture.journal().records.is_empty());
    assert_eq!(fixture.journal().states.len(), 2);
    let event = fixture.event(
        5,
        EventKind::UserPrompt,
        "검증 통과하면 반영해도 된다",
        Some(500),
    );
    let mut change = decision(&event, "conditional-approval");
    change.decision_basis.as_mut().unwrap().intent = Intent::ConditionalApproval;
    assert!(
        apply(
            &fixture.store,
            std::slice::from_ref(&event),
            &Summary {
                record_updates: vec![change.clone()],
                ..Default::default()
            }
        )
        .unwrap_err()
        .to_string()
        .contains("decision_basis_condition_required")
    );
    change.condition = Some("검증 통과 후".into());
    apply_changes(&fixture, &event, vec![change], vec![]);
    let journal = fixture.journal();
    let version = journal.records.values().next().unwrap().latest();
    assert_eq!(
        version.decision_basis.as_ref().unwrap().intent,
        Intent::ConditionalApproval
    );
    assert_eq!(version.at_unix_utc, Some(500));
    assert_eq!(version.condition.as_deref(), Some("검증 통과 후"));
}

#[test]
fn saved_legacy_decisions_are_not_reclassified_or_given_invented_basis() {
    let fixture = Fixture::new();
    let event = fixture.event(1, EventKind::UserPrompt, "기존 방안 확정", Some(100));
    apply_changes(&fixture, &event, vec![decision(&event, "legacy")], vec![]);
    let path = ctx::directory(&fixture.store.state_root, &fixture.scope()).join("journal.json");
    let mut old: serde_json::Value = read_json_or_default(&path).unwrap();
    for record in old["records"].as_object_mut().unwrap().values_mut() {
        for version in record["versions"].as_array_mut().unwrap() {
            version.as_object_mut().unwrap().remove("decision_basis");
        }
    }
    atomic_write_json(&path, &old).unwrap();
    let before = std::fs::read(&path).unwrap();
    let loaded = fixture.journal();
    assert!(
        loaded
            .records
            .values()
            .next()
            .unwrap()
            .latest()
            .decision_basis
            .is_none()
    );
    assert_eq!(std::fs::read(path).unwrap(), before);
}
#[test]
fn explicit_confirmation_is_not_rejected_because_support_contains_a_question() {
    let fixture = Fixture::new();
    let event = fixture.event(
        1,
        EventKind::UserPrompt,
        "버튼은 어디갔어? 시점 선택 버튼을 유지하기로 확정한다.",
        Some(100),
    );
    let mut change = decision(&event, "confirmed-view-controls");
    change.text = "시점 선택 버튼을 유지한다".into();
    change.evidence[0].quote = "시점 선택 버튼을 유지하기로 확정한다.".into();
    change.decision_basis.as_mut().unwrap().quote = change.evidence[0].quote.clone();
    change.evidence.push(Evidence {
        reference: "event:1".into(),
        quote: "버튼은 어디갔어?".into(),
    });
    assert!(
        apply(
            &fixture.store,
            &[event],
            &Summary {
                record_updates: vec![change],
                ..Default::default()
            }
        )
        .is_ok()
    );
}
#[test]
fn decisions_display_newest_first_with_unknown_times_last_without_rewriting_journal() {
    let fixture = Fixture::new();
    for (seq, text, at) in [
        (1, "오래된 결정", Some(100)),
        (2, "최근 결정", Some(200)),
        (3, "시각 없는 결정", None),
    ] {
        let event = fixture.event(seq, EventKind::UserPrompt, text, at);
        apply_changes(
            &fixture,
            &event,
            vec![decision(&event, &format!("decision-{seq}"))],
            vec![],
        );
    }
    let snapshot = crate::branch_panel::saved(&fixture.store.state_root, fixture.scope()).unwrap();
    let before = serde_json::to_string(&snapshot.journal).unwrap();
    for width in [30, 80] {
        let text =
            crate::branch_panel::render(&snapshot, width, &crate::branch_panel::View::default())
                .join("");
        assert!(text.find("최근 결정").unwrap() < text.find("오래된 결정").unwrap());
        assert!(text.find("오래된 결정").unwrap() < text.find("시각 없는 결정").unwrap());
    }
    assert_eq!(serde_json::to_string(&snapshot.journal).unwrap(), before);
}
#[test]
fn quote_failures_are_distinct_and_saved_input_can_replay_exact_validation() {
    let fixture = Fixture::new();
    let event = fixture.event(1, EventKind::AssistantFinal, "문구 수정\n완료", Some(100));
    for (quote, code) in [
        ("문구 수정 완료".into(), "quote_not_in_event"),
        (" ".into(), "empty_quote"),
        ("가".repeat(601), "quote_too_long"),
    ] {
        let evidence = vec![Evidence {
            reference: "event:1".into(),
            quote,
        }];
        let error =
            resolve_proof(&fixture.store, &evidence, std::slice::from_ref(&event)).unwrap_err();
        assert!(error.to_string().contains(code));
        assert!(!error.to_string().contains("문구"));
        let rejected = crate::reducer_diagnostics::RejectedResponse {
            stage: "journal_validation",
            cause: error,
            response: serde_json::to_vec(&evidence).unwrap(),
            model_events: vec![event.clone()],
        };
        crate::reducer_diagnostics::record(&fixture.store, &rejected, std::slice::from_ref(&event))
            .unwrap();
        let saved: serde_json::Value =
            read_json_or_default(&fixture.store.path(crate::reducer_diagnostics::FILE)).unwrap();
        let model: Vec<TurnEvent> = serde_json::from_value(saved["model_events"].clone()).unwrap();
        let quotes: Vec<Evidence> =
            serde_json::from_str(saved["response_utf8"].as_str().unwrap()).unwrap();
        assert_eq!(
            resolve_proof(&fixture.store, &quotes, &model)
                .unwrap_err()
                .to_string(),
            saved["detail"].as_str().unwrap()
        );
    }
    let good = vec![Evidence {
        reference: "event:1".into(),
        quote: event.content.clone(),
    }];
    assert!(resolve_proof(&fixture.store, &good, &[event]).is_ok());
}

#[test]
fn diagnostic_payload_is_bounded_and_preserves_model_vs_raw_difference() {
    let fixture = Fixture::new();
    let raw = fixture.event(1, EventKind::AssistantFinal, "원문 전체", Some(100));
    let mut model = raw.clone();
    model.content = "원문".into();
    model.truncated = true;
    let mut rejected = crate::reducer_diagnostics::RejectedResponse {
        stage: "decode",
        cause: "synthetic failure".into(),
        response: b"{}".to_vec(),
        model_events: vec![model.clone()],
    };
    crate::reducer_diagnostics::record(&fixture.store, &rejected, std::slice::from_ref(&raw))
        .unwrap();
    let path = fixture.store.path(crate::reducer_diagnostics::FILE);
    let saved: serde_json::Value = read_json_or_default(&path).unwrap();
    assert_eq!(saved["model_events"][0]["content"], model.content);
    assert_eq!(saved["original_events"][0]["content"], raw.content);
    rejected.response = vec![b'x'; 2 * 1024 * 1024];
    crate::reducer_diagnostics::record(&fixture.store, &rejected, &[raw]).unwrap();
    let saved: serde_json::Value = read_json_or_default(&path).unwrap();
    assert_eq!(saved["payload_omitted"], true);
    assert!(saved["response_utf8"].is_null());
    assert_eq!(saved["model_events"], serde_json::json!([]));
    assert!(std::fs::metadata(path).unwrap().len() < 4096);
}
#[test]
fn panel_roles_distinguish_headings_unknowns_and_body_without_mutating_memory() {
    use crate::panel_style::Role;
    let fixture = Fixture::new();
    let snapshot = crate::branch_panel::saved(&fixture.store.state_root, fixture.scope()).unwrap();
    let before = serde_json::to_string(&snapshot.journal).unwrap();
    for width in [30, 80] {
        let lines =
            crate::branch_panel::render(&snapshot, width, &crate::branch_panel::View::default());
        assert_eq!(lines[0].role, Role::Title);
        assert_eq!(
            lines
                .iter()
                .find(|l| l.text == crate::tr!("막힘", "Blockers"))
                .unwrap()
                .role,
            Role::Heading
        );
        assert_eq!(
            lines
                .iter()
                .find(|l| l.text == crate::tr!("다음 행동", "Next actions"))
                .unwrap()
                .role,
            Role::Action
        );
        assert!(
            lines
                .iter()
                .filter(|l| l.starts_with(crate::tr!("기록 없음", "No records")))
                .all(|l| l.role == Role::Muted)
        );
        assert!(lines.iter().all(|l| !l.contains('\x1b')));
    }
    assert_eq!(serde_json::to_string(&snapshot.journal).unwrap(), before);
}
struct Fixture {
    root: PathBuf,
    repo: PathBuf,
    store: SpaceStore,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "pm-branch-contract-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(root.join("repo")).unwrap();
        let repo = root.join("repo");
        git(&repo, &["init", "-b", "feature/guide"]);
        git(
            &repo,
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "commit",
                "--allow-empty",
                "-m",
                "fixture",
            ],
        );
        let store = SpaceStore::for_workspace(
            &root.join("state"),
            "test",
            "one",
            repo.to_str().unwrap(),
            None,
        )
        .unwrap();
        Self { root, repo, store }
    }
    fn scope(&self) -> ctx::Context {
        ctx::detect(self.repo.to_str().unwrap(), &self.store.key)
    }
    fn journal(&self) -> Journal {
        load(&self.store.state_root, &self.scope()).unwrap()
    }
    fn event(&self, seq: u64, kind: EventKind, text: &str, at: Option<u64>) -> TurnEvent {
        let e = TurnEvent {
            seq,
            kind,
            session_id: "fixture-session".into(),
            turn_id: Some(format!("turn-{seq}")),
            model: None,
            content: text.into(),
            truncated: false,
            recorded_at_unix: at.unwrap_or(0),
            cwd: self.repo.to_string_lossy().into_owned(),
        };
        let _lock = lock_file(&self.store.path(".event.lock"), false).unwrap();
        assert!(ctx::capture(&self.store, &e).unwrap());
        e
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}
fn git(repo: &std::path::Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .current_dir(repo)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().into()
}
fn decision(e: &TurnEvent, key: &str) -> Change {
    Change {
        kind: Kind::Decision,
        operation: Operation::Add,
        key: key.into(),
        target: None,
        text: e.content.clone(),
        condition: None,
        stage: Stage::Decision,
        outcome: Outcome::Confirmed,
        evidence: vec![Evidence {
            reference: format!("event:{}", e.seq),
            quote: e.content.clone(),
        }],
        decision_basis: Some(crate::decision_policy::Basis {
            intent: crate::decision_policy::Intent::ExplicitChoice,
            reference: format!("event:{}", e.seq),
            quote: e.content.clone(),
        }),
    }
}
fn completion(
    e: &TurnEvent,
    key: &str,
    stage: Stage,
    outcome: Outcome,
    target: Option<String>,
) -> Change {
    Change {
        kind: Kind::Completion,
        operation: if target.is_some() {
            Operation::FollowUp
        } else {
            Operation::Add
        },
        key: key.into(),
        target,
        text: e.content.clone(),
        condition: None,
        stage,
        outcome,
        decision_basis: None,
        evidence: vec![Evidence {
            reference: format!("event:{}", e.seq),
            quote: e.content.clone(),
        }],
    }
}
fn state(e: &TurnEvent, kind: StateKind, key: &str) -> StateChange {
    StateChange {
        kind,
        key: key.into(),
        text: e.content.clone(),
        owner: "user".into(),
        condition: Some("사용자 승인 후".into()),
        status: StateStatus::Active,
        change: StateEdit::Changed,
        evidence: vec![Evidence {
            reference: format!("event:{}", e.seq),
            quote: e.content.clone(),
        }],
    }
}
fn apply_changes(fixture: &Fixture, e: &TurnEvent, records: Vec<Change>, states: Vec<StateChange>) {
    apply(
        &fixture.store,
        std::slice::from_ref(e),
        &Summary {
            record_updates: records,
            state_updates: states,
            ..Default::default()
        },
    )
    .unwrap();
}

#[test]
fn confirmed_decisions_survive_omission_implementation_restart_and_checkout() {
    let fixture = Fixture::new();
    let e = fixture.event(
        1,
        EventKind::UserPrompt,
        "브랜치별로 저장하기로 한다.",
        Some(1000),
    );
    apply_changes(&fixture, &e, vec![decision(&e, "branch-storage")], vec![]);
    let done = fixture.event(
        2,
        EventKind::AssistantFinal,
        "브랜치별 저장 구현 완료",
        Some(1100),
    );
    apply_changes(
        &fixture,
        &done,
        vec![completion(
            &done,
            "implemented",
            Stage::Implementation,
            Outcome::Reported,
            None,
        )],
        vec![],
    );
    let journal = fixture.journal();
    assert_eq!(journal.records.len(), 2);
    assert!(
        journal
            .records
            .values()
            .find(|r| r.kind == Kind::Decision)
            .unwrap()
            .active()
    );
    let a = fixture.scope();
    git(&fixture.repo, &["checkout", "-b", "feature/other"]);
    assert!(fixture.journal().records.is_empty());
    assert_eq!(load(&fixture.store.state_root, &a).unwrap(), journal);
    git(&fixture.repo, &["checkout", "feature/guide"]);
    assert_eq!(fixture.journal(), journal);
    crate::participation::set_enabled(&fixture.store, false).unwrap();
    crate::restart::prepare(&fixture.store, None).unwrap();
    assert_eq!(fixture.journal().records, journal.records);
    assert!(!fixture.journal().gaps.is_empty());
    crate::participation::set_enabled(&fixture.store, true).unwrap();
    assert_eq!(fixture.journal().records, journal.records);
}

#[test]
fn proposals_and_fabricated_evidence_never_commit() {
    let fixture = Fixture::new();
    let e = fixture.event(
        1,
        EventKind::UserPrompt,
        "브랜치별로 저장할까요?",
        Some(100),
    );
    let mut proposed = decision(&e, "proposal");
    proposed.decision_basis.as_mut().unwrap().intent = crate::decision_policy::Intent::Proposal;
    assert!(
        apply(
            &fixture.store,
            std::slice::from_ref(&e),
            &Summary {
                record_updates: vec![proposed],
                ..Default::default()
            }
        )
        .is_err()
    );
    assert!(fixture.journal().records.is_empty());
    let mut change = decision(&e, "invented");
    change.evidence[0].quote = "확정한다".into();
    assert!(
        apply(
            &fixture.store,
            &[e],
            &Summary {
                record_updates: vec![change],
                ..Default::default()
            }
        )
        .is_err()
    );
    assert!(fixture.journal().records.is_empty());
}

#[test]
fn replacement_withdrawal_and_late_results_keep_relations_and_original_times() {
    let fixture = Fixture::new();
    let a = fixture.event(
        1,
        EventKind::UserPrompt,
        "최근 완료 2건 표시로 확정한다",
        Some(100),
    );
    apply_changes(&fixture, &a, vec![decision(&a, "two")], vec![]);
    let old = fixture
        .journal()
        .records
        .values()
        .next()
        .unwrap()
        .id
        .clone();
    let b = fixture.event(
        2,
        EventKind::UserPrompt,
        "앞선 결정을 최근 완료 3건으로 바꾼다",
        Some(200),
    );
    let c = fixture.event(
        3,
        EventKind::UserPrompt,
        "앞선 표시 건수 결정을 4건으로 바꾼다",
        Some(300),
    );
    let mut change = decision(&c, "four");
    change.operation = Operation::Replace;
    change.target = Some(old.clone());
    apply_changes(&fixture, &c, vec![change], vec![]);
    let newest = fixture
        .journal()
        .records
        .values()
        .find(|r| r.key == "four")
        .unwrap()
        .id
        .clone();
    let mut late = decision(&b, "three");
    late.operation = Operation::Replace;
    late.target = Some(old.clone());
    apply_changes(&fixture, &b, vec![late], vec![]);
    let journal = fixture.journal();
    assert_eq!(journal.records[&old].replaced_by.as_ref(), Some(&newest));
    assert_eq!(journal.records.values().filter(|r| r.active()).count(), 1);
    assert_eq!(journal.records[&old].latest().at_unix_utc, Some(100));
    let d = fixture.event(
        4,
        EventKind::UserPrompt,
        "4건 표시 결정을 철회한다",
        Some(400),
    );
    let mut withdraw = decision(&d, "withdraw-four");
    withdraw.operation = Operation::Withdraw;
    withdraw.target = Some(newest.clone());
    apply_changes(&fixture, &d, vec![withdraw], vec![]);
    assert!(fixture.journal().records[&newest].withdrawn);
    assert_eq!(fixture.journal().records[&newest].versions.len(), 2);
}

#[test]
fn retries_paraphrases_stages_and_unknown_times_are_distinct() {
    let fixture = Fixture::new();
    let a = fixture.event(1, EventKind::UserPrompt, "승인 후 반영하기로 한다", None);
    let mut d = decision(&a, "approval");
    d.condition = Some("승인 후".into());
    apply_changes(&fixture, &a, vec![d.clone()], vec![]);
    let before = fixture.journal();
    apply_changes(&fixture, &a, vec![d.clone()], vec![]);
    assert_eq!(fixture.journal(), before);
    let b = fixture.event(
        2,
        EventKind::UserPrompt,
        "이전 결정대로 승인을 받고 반영한다",
        Some(500),
    );
    d.operation = Operation::Restate;
    d.text = b.content.clone();
    d.evidence = decision(&b, "approval").evidence;
    d.decision_basis = decision(&b, "approval").decision_basis;
    apply_changes(&fixture, &b, vec![d], vec![]);
    assert_eq!(fixture.journal().records.len(), 1);
    assert_eq!(
        fixture
            .journal()
            .records
            .values()
            .next()
            .unwrap()
            .latest()
            .at_unix_utc,
        None
    );
    let c = fixture.event(
        3,
        EventKind::AssistantFinal,
        "구현 완료, 검증 통과, 병합 완료, 배포 완료",
        Some(600),
    );
    let changes = [
        Stage::Implementation,
        Stage::Verification,
        Stage::Merge,
        Stage::Deployment,
    ]
    .into_iter()
    .map(|s| completion(&c, "work", s, Outcome::Reported, None))
    .collect();
    apply_changes(&fixture, &c, changes, vec![]);
    assert_eq!(fixture.journal().records.len(), 5);
    let original = fixture.journal();
    let mut alternate = completion(
        &c,
        "wording-only",
        Stage::Implementation,
        Outcome::Reported,
        None,
    );
    alternate.text = "구현을 끝냈음".into();
    alternate.evidence[0].quote = "구현 완료".into();
    apply_changes(&fixture, &c, vec![alternate], vec![]);
    assert_eq!(fixture.journal().records, original.records);
    assert_eq!(
        fixture.journal(),
        original,
        "same-event retries cannot add a paraphrase under a different key/quote"
    );
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "One end-to-end failure/revalidation scenario keeps its evidence chain together"
)]
fn completion_failure_is_visible_folded_and_pass_follows_the_chain() {
    let fixture = Fixture::new();
    let a = fixture.event(1, EventKind::AssistantFinal, "문구 수정 완료", Some(100));
    apply_changes(
        &fixture,
        &a,
        vec![completion(
            &a,
            "implementation",
            Stage::Implementation,
            Outcome::Reported,
            None,
        )],
        vec![],
    );
    let root = fixture
        .journal()
        .records
        .values()
        .next()
        .unwrap()
        .id
        .clone();
    let b = fixture.event(
        2,
        EventKind::AssistantFinal,
        "검증에서 누락 발견, 재작업 필요",
        Some(200),
    );
    apply_changes(
        &fixture,
        &b,
        vec![completion(
            &b,
            "failure",
            Stage::Verification,
            Outcome::Failed,
            Some(root.clone()),
        )],
        vec![state(&b, StateKind::Progress, "main")],
    );
    let failure = fixture
        .journal()
        .records
        .values()
        .find(|r| r.key == "failure")
        .unwrap()
        .id
        .clone();
    for n in 3..8 {
        let e = fixture.event(
            n,
            EventKind::AssistantFinal,
            &format!("별도 작업 {n} 완료"),
            Some(n * 100),
        );
        apply_changes(
            &fixture,
            &e,
            vec![completion(
                &e,
                &format!("other-{n}"),
                Stage::Other,
                Outcome::Reported,
                None,
            )],
            vec![],
        );
    }
    let snapshot = crate::branch_panel::saved(&fixture.store.state_root, fixture.scope()).unwrap();
    let default = crate::branch_panel::render(&snapshot, 52, &crate::branch_panel::View::default())
        .join("\n");
    let normalize = |value: &str| value.split_whitespace().collect::<String>();
    assert!(normalize(&default).contains(&normalize(crate::tr!(
        "후속 검증 실패",
        "Follow-up verification failed"
    ))));
    // This phrase is original Korean evidence, not the Outcome::Rework label.
    assert!(normalize(&default).contains(&normalize(&b.content)));
    let c = fixture.event(
        8,
        EventKind::AssistantFinal,
        "누락 보완 후 재검증 통과",
        Some(800),
    );
    apply_changes(
        &fixture,
        &c,
        vec![completion(
            &c,
            "rechecked",
            Stage::Verification,
            Outcome::Passed,
            Some(failure),
        )],
        vec![],
    );
    let snapshot = crate::branch_panel::saved(&fixture.store.state_root, fixture.scope()).unwrap();
    let expanded = crate::branch_panel::render(
        &snapshot,
        100,
        &crate::branch_panel::View {
            expanded: true,
            page: 0,
        },
    )
    .join("\n");
    assert!(normalize(&expanded).contains(&normalize(crate::tr!(
        "문구 수정 완료 [보고] → 후속 통과 보고",
        "문구 수정 완료 [Reported] → Follow-up passed: reported"
    ))));
    assert!(!expanded.contains(crate::tr!("완료 후 후속 문제", "Follow-up problems")));
    assert_eq!(
        fixture.journal().records[&root].latest().at_unix_utc,
        Some(100)
    );
}

#[test]
fn current_state_omission_is_not_resolution_rephrasing_is_not_a_new_time() {
    let fixture = Fixture::new();
    let a = fixture.event(1, EventKind::AssistantFinal, "사용자 승인 필요", Some(100));
    apply_changes(
        &fixture,
        &a,
        vec![],
        vec![state(&a, StateKind::Blocker, "approval")],
    );
    let b = fixture.event(
        2,
        EventKind::AssistantFinal,
        "계속 사용자 승인 대기 중",
        Some(200),
    );
    let mut repeated = state(&b, StateKind::Blocker, "approval");
    repeated.change = StateEdit::Same;
    apply_changes(&fixture, &b, vec![], vec![repeated]);
    assert_eq!(
        fixture
            .journal()
            .states
            .values()
            .next()
            .unwrap()
            .at_unix_utc,
        Some(100)
    );
    let c = fixture.event(3, EventKind::AssistantFinal, "다른 작업 보고", Some(300));
    apply_changes(&fixture, &c, vec![], vec![]);
    assert_eq!(
        fixture.journal().states.values().next().unwrap().status,
        StateStatus::Unconfirmed
    );
    let repeated_event = fixture.event(
        4,
        EventKind::AssistantFinal,
        "아직 사용자 승인이 필요하다",
        Some(350),
    );
    let mut reconfirm = state(&repeated_event, StateKind::Blocker, "approval");
    reconfirm.change = StateEdit::Same;
    apply_changes(&fixture, &repeated_event, vec![], vec![reconfirm]);
    let repeated_journal = fixture.journal();
    let repeated_state = repeated_journal.states.values().next().unwrap();
    assert_eq!(repeated_state.at_unix_utc, Some(100));
    assert_eq!(repeated_state.proof[0].source.at_unix_utc, Some(100));
    let d = fixture.event(5, EventKind::UserPrompt, "승인한다", Some(400));
    let mut resolved = state(&d, StateKind::Blocker, "approval");
    resolved.status = StateStatus::Resolved;
    apply_changes(&fixture, &d, vec![], vec![resolved]);
    apply_changes(
        &fixture,
        &a,
        vec![],
        vec![state(&a, StateKind::Blocker, "approval")],
    );
    let journal = fixture.journal();
    assert_eq!(
        journal.states.values().next().unwrap().status,
        StateStatus::Resolved
    );
    assert_eq!(
        journal.states.values().next().unwrap().at_unix_utc,
        Some(400)
    );
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "One isolated worktree/concurrent-Space scenario shares a single Git lifetime"
)]
fn capture_context_worktrees_and_concurrent_spaces_do_not_mix_or_lose_records() {
    let fixture = Fixture::new();
    let a = fixture.event(
        1,
        EventKind::UserPrompt,
        "현재 브랜치에서 작업한다",
        Some(100),
    );
    let original = fixture.scope();
    git(&fixture.repo, &["checkout", "-b", "other"]);
    let mut final_event = a.clone();
    final_event.seq = 2;
    final_event.kind = EventKind::AssistantFinal;
    final_event.content = "완료 보고".into();
    assert!(ctx::capture(&fixture.store, &final_event).unwrap());
    assert_eq!(
        ctx::source(&fixture.store, &final_event)
            .unwrap()
            .context
            .key,
        original.key
    );
    let b = fixture.event(3, EventKind::UserPrompt, "다른 브랜치 작업", Some(300));
    assert_ne!(
        ctx::source(&fixture.store, &b).unwrap().context.key,
        original.key
    );
    let late_peer = SpaceStore::for_workspace(
        &fixture.store.state_root,
        "test",
        "late-peer",
        fixture.repo.to_str().unwrap(),
        None,
    )
    .unwrap();
    assert!(
        !ctx::capture(&late_peer, &a).unwrap(),
        "late duplicate must not be reattributed after checkout"
    );
    assert!(late_peer.events().unwrap().is_empty());
    assert_eq!(
        ctx::same_context_batch(
            &fixture.store,
            vec![a.clone(), final_event, b.clone()],
            false
        )
        .unwrap()
        .len(),
        2
    );
    assert!(
        ctx::model_base(&fixture.store, &[b])
            .unwrap()
            .summary
            .decisions
            .is_empty()
    );
    let worktree = fixture.root.join("worktree");
    git(
        &fixture.repo,
        &[
            "worktree",
            "add",
            worktree.to_str().unwrap(),
            "feature/guide",
        ],
    );
    let peer = SpaceStore::for_workspace(
        &fixture.store.state_root,
        "test",
        "two",
        worktree.to_str().unwrap(),
        None,
    )
    .unwrap();
    assert_eq!(
        ctx::detect(worktree.to_str().unwrap(), &peer.key).key,
        original.key
    );
    let mut duplicate = a.clone();
    duplicate.cwd = worktree.to_string_lossy().into_owned();
    assert!(!ctx::capture(&peer, &duplicate).unwrap());
    assert!(peer.events().unwrap().is_empty());
    let mut unique = duplicate;
    unique.seq = 2;
    unique.turn_id = Some("peer-turn".into());
    unique.content = "두 번째 결정을 확정한다".into();
    assert!(ctx::capture(&peer, &unique).unwrap());
    let left = fixture.store.clone();
    let right = peer.clone();
    std::thread::scope(|s| {
        s.spawn(|| {
            apply(
                &left,
                std::slice::from_ref(&a),
                &Summary {
                    record_updates: vec![decision(&a, "first")],
                    ..Default::default()
                },
            )
            .unwrap();
        });
        s.spawn(|| {
            apply(
                &right,
                std::slice::from_ref(&unique),
                &Summary {
                    record_updates: vec![decision(&unique, "second")],
                    ..Default::default()
                },
            )
            .unwrap();
        });
    });
    assert_eq!(
        load(&fixture.store.state_root, &original)
            .unwrap()
            .records
            .len(),
        2
    );
    crate::participation::set_enabled(&peer, false).unwrap();
    assert!(crate::participation::load(&fixture.store).unwrap().enabled);
    assert!(!crate::participation::load(&peer).unwrap().enabled);
}

#[test]
fn ambiguous_branch_lifetimes_and_legacy_are_preserved_without_guessing() {
    let fixture = Fixture::new();
    let original = fixture.scope();
    git(&fixture.repo, &["branch", "-m", "renamed"]);
    assert_ne!(fixture.scope().key, original.key);
    git(&fixture.repo, &["checkout", "-b", "temporary"]);
    git(&fixture.repo, &["branch", "-D", "renamed"]);
    git(&fixture.repo, &["checkout", "-b", "feature/guide"]);
    assert_ne!(fixture.scope().key, original.key);
    git(&fixture.repo, &["checkout", "--detach"]);
    assert!(fixture.scope().unlinked_reason.is_some());
    let old = CurrentState {
        revision: 5,
        summary: Summary {
            objective: "旧記録".into(),
            ..Default::default()
        },
        ..Default::default()
    };
    atomic_write_json(&fixture.store.path("current.json"), &old).unwrap();
    crate::legacy::preserve(&fixture.store).unwrap();
    atomic_write_json(
        &fixture.store.path("current.json"),
        &CurrentState::default(),
    )
    .unwrap();
    assert_eq!(
        crate::legacy::read(&fixture.store).unwrap()[0]
            .summary
            .objective,
        "旧記録"
    );
    assert!(
        load(&fixture.store.state_root, &original)
            .unwrap()
            .records
            .is_empty()
    );
}

#[test]
fn all_today_completions_are_visible_and_other_dates_remain_in_history() {
    let fixture = Fixture::new();
    let at = crate::transcript::utc_timestamp_seconds("2026-09-28T03:00:00Z").unwrap();
    for n in 1..=9 {
        let timestamp = match n {
            7 => Some(at - 86400),
            8 => None,
            9 => Some(at + 86400),
            _ => Some(at + n),
        };
        let event = fixture.event(
            n,
            EventKind::AssistantFinal,
            &format!("completion-item-{n}"),
            timestamp,
        );
        apply_changes(
            &fixture,
            &event,
            vec![completion(
                &event,
                &format!("completion-{n}"),
                Stage::Other,
                Outcome::Reported,
                None,
            )],
            vec![],
        );
    }
    let mut snapshot =
        crate::branch_panel::saved(&fixture.store.state_root, fixture.scope()).unwrap();
    snapshot.today = crate::daily::day_at(at);
    let render = |s: &crate::branch_panel::Snapshot, expanded| {
        crate::branch_panel::render(s, 200, &crate::branch_panel::View { expanded, page: 0 })
            .join("\n")
    };
    let today = render(&snapshot, false);
    assert!(today.contains(crate::tr!(
        "오늘 완료 — 전체",
        "Today’s completion reports — All"
    )));
    for n in 1..=6 {
        assert!(today.contains(&format!("completion-item-{n}")));
    }
    for n in 7..=9 {
        assert!(!today.contains(&format!("completion-item-{n}")));
    }
    assert!(today.contains(crate::tr!("이전 완료 3건", "Earlier completions 3")));
    let expanded = render(&snapshot, true);
    for n in 1..=9 {
        assert_eq!(expanded.matches(&format!("completion-item-{n}")).count(), 1);
    }
    snapshot.today = crate::daily::day_at(at + 86400);
    let tomorrow = render(&snapshot, false);
    assert!(tomorrow.contains("completion-item-9"));
    assert!(!tomorrow.contains("completion-item-6"));
    assert!(tomorrow.contains(crate::tr!("이전 완료 8건", "Earlier completions 8")));
    snapshot.today = None;
    assert!(
        render(&snapshot, false).contains(crate::tr!("이전 완료 9건", "Earlier completions 9"))
    );
}

#[test]
fn folded_history_is_pure_paginated_and_korean_conditions_are_not_truncated() {
    let fixture = Fixture::new();
    for n in 1..32 {
        let e = fixture.event(
            n,
            EventKind::AssistantFinal,
            &format!("한글 완료 보고 {n}"),
            if n == 1 { None } else { Some(n * 100) },
        );
        apply_changes(
            &fixture,
            &e,
            vec![completion(
                &e,
                &format!("completion-{n}"),
                Stage::Other,
                Outcome::Reported,
                None,
            )],
            vec![],
        );
    }
    let snapshot = crate::branch_panel::saved(&fixture.store.state_root, fixture.scope()).unwrap();
    let before = std::fs::read(
        ctx::directory(&fixture.store.state_root, &fixture.scope()).join("journal.json"),
    )
    .unwrap();
    let folded = crate::branch_panel::render(&snapshot, 52, &crate::branch_panel::View::default())
        .join("\n");
    let normalize = |value: &str| value.split_whitespace().collect::<String>();
    assert!(normalize(&folded).contains(&normalize(crate::tr!(
        "이전 완료 31건",
        "Earlier completions 31"
    ))));
    assert!(!folded.contains("한글 완료 보고 1\n"));
    assert_eq!(crate::branch_panel::max_page(&snapshot), 1);
    let wide = crate::branch_panel::render(
        &snapshot,
        80,
        &crate::branch_panel::View {
            expanded: true,
            page: 1,
        },
    )
    .join("\n");
    assert!(wide.contains(crate::tr!("시각 미확인", "Time unknown")));
    for width in [12, 18, 24, 40, 52] {
        let narrow = crate::branch_panel::render(
            &snapshot,
            width,
            &crate::branch_panel::View {
                expanded: true,
                page: 1,
            },
        );
        assert!(
            narrow
                .iter()
                .all(|line| unicode_width::UnicodeWidthStr::width(line.text.as_str()) <= width)
        );
        assert!(
            narrow
                .join("")
                .split_whitespace()
                .collect::<String>()
                .contains("한글완료보고1")
        );
    }
    assert_eq!(
        std::fs::read(
            ctx::directory(&fixture.store.state_root, &fixture.scope()).join("journal.json")
        )
        .unwrap(),
        before
    );
}

#[test]
fn private_delete_preserves_shared_records_and_explicit_shared_delete_requires_off_empty_spaces() {
    let fixture = Fixture::new();
    let e = fixture.event(
        1,
        EventKind::UserPrompt,
        "공유 기록으로 보존한다",
        Some(100),
    );
    apply_changes(&fixture, &e, vec![decision(&e, "shared")], vec![]);
    let scope = fixture.scope();
    assert!(crate::memory::clear(&fixture.store.state_root, &scope).is_err());
    crate::participation::set_enabled(&fixture.store, false).unwrap();
    assert!(
        crate::memory::clear(&fixture.store.state_root, &scope)
            .unwrap_err()
            .to_string()
            .contains("pending input")
    );
    crate::maintenance::clear_records(&fixture.store).unwrap();
    assert_eq!(fixture.journal().records.len(), 1);
    assert!(!crate::participation::load(&fixture.store).unwrap().enabled);
    let peer = SpaceStore::for_workspace(
        &fixture.store.state_root,
        "test",
        "delete-peer",
        fixture.repo.to_str().unwrap(),
        None,
    )
    .unwrap();
    assert!(
        crate::memory::clear(&fixture.store.state_root, &scope)
            .unwrap_err()
            .to_string()
            .contains("is enabled")
    );
    crate::participation::set_enabled(&peer, false).unwrap();
    crate::memory::clear(&fixture.store.state_root, &scope).unwrap();
    assert!(fixture.journal().records.is_empty());
    apply_changes(&fixture, &e, vec![decision(&e, "shared")], vec![]);
    assert!(
        fixture.journal().records.is_empty(),
        "deleted source replay must remain tombstoned"
    );
}

#[test]
fn preview_never_registers_durable_records_until_cumulative_commit() {
    let fixture = Fixture::new();
    let a = fixture.event(
        1,
        EventKind::UserPrompt,
        "조건부로 구현하기로 한다",
        Some(100),
    );
    let b = fixture.event(2, EventKind::AssistantFinal, "구현을 준비한다", Some(200));
    let summary = Summary {
        record_updates: vec![decision(&a, "conditional")],
        ..Default::default()
    };
    crate::latest::commit(
        &fixture.store,
        &crate::latest::LatestContext {
            first_event_seq: 1,
            cumulative_base_revision: 0,
            state: CurrentState {
                revision: 1,
                source_seq: 2,
                summary: summary.clone(),
                ..Default::default()
            },
        },
    )
    .unwrap();
    assert!(fixture.journal().records.is_empty());
    fixture
        .store
        .commit_summary(
            0,
            b.seq,
            summary,
            RepoSnapshot::default(),
            ContextTiming::default(),
        )
        .unwrap();
    assert_eq!(fixture.journal().records.len(), 1);
    assert!(fixture.store.events().unwrap().is_empty());
    assert!(
        crate::latest::visible(&fixture.store, &fixture.store.current().unwrap())
            .unwrap()
            .is_none()
    );
}

#[test]
fn primary_evidence_is_the_only_item_clock_and_unlinked_imports_remain_accessible() {
    let fixture = Fixture::new();
    let user = fixture.event(1, EventKind::UserPrompt, "승인 후 반영한다", None);
    let assistant = fixture.event(
        2,
        EventKind::AssistantFinal,
        "승인 후 반영 조건을 확인했다",
        Some(900),
    );
    let mut update = decision(&user, "conditional");
    update.evidence.push(Evidence {
        reference: "event:2".into(),
        quote: assistant.content.clone(),
    });
    apply(
        &fixture.store,
        &[user, assistant],
        &Summary {
            record_updates: vec![update],
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        fixture
            .journal()
            .records
            .values()
            .next()
            .unwrap()
            .latest()
            .at_unix_utc,
        None
    );
    let imported = TurnEvent {
        seq: 99,
        kind: EventKind::AssistantFinal,
        content: "귀속 미확인 완료 보고".into(),
        session_id: "legacy-session".into(),
        turn_id: None,
        model: None,
        cwd: fixture.repo.to_string_lossy().into_owned(),
        truncated: false,
        recorded_at_unix: 0,
    };
    apply_changes(
        &fixture,
        &imported,
        vec![completion(
            &imported,
            "unlinked",
            Stage::Other,
            Outcome::Reported,
            None,
        )],
        vec![],
    );
    let snapshot = crate::branch_panel::load(&fixture.store).unwrap();
    assert_eq!(snapshot.unlinked_records.len(), 1);
    let expanded = crate::branch_panel::render(
        &snapshot,
        80,
        &crate::branch_panel::View {
            expanded: true,
            page: 0,
        },
    )
    .join("\n");
    let normalize = |value: &str| value.split_whitespace().collect::<String>();
    assert!(normalize(&expanded).contains(&normalize(crate::tr!(
        "현재 브랜치 기록 아님",
        "Outside current branch"
    ))));
    assert!(expanded.contains("귀속 미확인 완료 보고"));
}

#[test]
fn newest_first_completion_can_link_a_later_arriving_older_predecessor() {
    let fixture = Fixture::new();
    let implementation = fixture.event(1, EventKind::AssistantFinal, "구현 완료", Some(100));
    let failure = fixture.event(
        2,
        EventKind::AssistantFinal,
        "검증 실패, 재작업 필요",
        Some(200),
    );
    apply_changes(
        &fixture,
        &failure,
        vec![completion(
            &failure,
            "failure",
            Stage::Verification,
            Outcome::Failed,
            None,
        )],
        vec![],
    );
    let failure_id = fixture
        .journal()
        .records
        .values()
        .next()
        .unwrap()
        .id
        .clone();
    apply_changes(
        &fixture,
        &implementation,
        vec![completion(
            &implementation,
            "implementation",
            Stage::Implementation,
            Outcome::Reported,
            Some(failure_id.clone()),
        )],
        vec![],
    );
    let journal = fixture.journal();
    let older = journal
        .records
        .values()
        .find(|r| r.key == "implementation")
        .unwrap();
    assert_eq!(
        journal.records[&failure_id].follows.as_ref(),
        Some(&older.id)
    );
    assert_eq!(older.follows, None);
    let snapshot = crate::branch_panel::saved(&fixture.store.state_root, fixture.scope()).unwrap();
    assert!(
        crate::branch_panel::render(
            &snapshot,
            100,
            &crate::branch_panel::View {
                expanded: true,
                page: 0
            }
        )
        .join("\n")
        .contains(crate::tr!(
            "구현 완료 [보고] → 후속 검증 실패",
            "구현 완료 [Reported] → Follow-up verification failed"
        ))
    );
}

#[test]
fn user_corrections_preserve_proof_and_undo_as_history_across_spaces() {
    use crate::corrections::{self, Edit};
    let f = Fixture::new();
    let event = f.event(1, EventKind::UserPrompt, "검증 후 적용", Some(100));
    apply_changes(&f, &event, vec![decision(&event, "apply")], vec![]);
    let initial = f.journal();
    let id = initial.records.keys().next().unwrap();
    let original = &initial.records[id];
    let corrected = corrections::save(
        &f.store.state_root,
        &f.scope(),
        id,
        initial.revision,
        &Edit::Amend {
            text: "검증을 통과한 경우에만 적용".into(),
            condition: Some("검증 통과".into()),
        },
        "누락된 조건 복원",
        "peer-space",
    )
    .unwrap();
    assert_eq!(corrected.records[id].versions, original.versions);
    assert_eq!(corrected.processed_events, initial.processed_events);
    assert_eq!(corrected.through_unix_utc, initial.through_unix_utc);
    assert_eq!(corrected.records[id].latest().at_unix_utc, Some(100));
    let loaded = f.journal();
    assert_eq!(loaded.records[id].condition(), Some("검증 통과"));
    assert_eq!(loaded.records[id].corrections[0].space, "peer-space");
    let catalog: serde_json::Value =
        serde_json::from_str(&catalog(&f.store, std::slice::from_ref(&event)).unwrap()).unwrap();
    assert_eq!(
        catalog["recent_record_catalog"][0]["condition"],
        "검증 통과"
    );
    assert_eq!(catalog["recent_record_catalog"][0]["user_corrected"], true);
    let excluded = corrections::save(
        &f.store.state_root,
        &f.scope(),
        id,
        loaded.revision,
        &Edit::Exclude,
        "질문을 승인으로 오인",
        &f.store.key,
    )
    .unwrap();
    assert!(!excluded.records[id].active());
    assert!(!excluded.records[id].withdrawn);
    let restored = corrections::save(
        &f.store.state_root,
        &f.scope(),
        id,
        excluded.revision,
        &Edit::Undo,
        "제외 취소",
        &f.store.key,
    )
    .unwrap();
    assert!(restored.records[id].active());
    assert_eq!(restored.records[id].condition(), Some("검증 통과"));
    assert_eq!(restored.records[id].corrections[2].undo_of, Some(2));
    let undone = corrections::save(
        &f.store.state_root,
        &f.scope(),
        id,
        restored.revision,
        &Edit::Undo,
        "첫 정정도 되돌림",
        &f.store.key,
    )
    .unwrap();
    assert_eq!(undone.records[id].text(), original.text());
    assert_eq!(undone.records[id].corrections.len(), 4);
    assert_eq!(undone.records[id].corrections[3].undo_of, Some(1));
    assert!(
        corrections::save(
            &f.store.state_root,
            &f.scope(),
            id,
            undone.revision,
            &Edit::Undo,
            "더 없음",
            &f.store.key
        )
        .is_err()
    );
}

#[test]
fn correction_conflicts_and_old_model_results_cannot_overwrite_user() {
    use crate::corrections::{self, Edit};
    let f = Fixture::new();
    let first = f.event(1, EventKind::UserPrompt, "기존 확정", Some(100));
    apply_changes(&f, &first, vec![decision(&first, "original")], vec![]);
    let before = f.journal();
    let id = before.records.keys().next().unwrap();
    let edited = corrections::save(
        &f.store.state_root,
        &f.scope(),
        id,
        before.revision,
        &Edit::Amend {
            text: "정정한 내용".into(),
            condition: None,
        },
        "요약 오류",
        &f.store.key,
    )
    .unwrap();
    assert!(
        corrections::save(
            &f.store.state_root,
            &f.scope(),
            id,
            before.revision,
            &Edit::Exclude,
            "다른 Space의 오래된 화면",
            "peer"
        )
        .is_err()
    );
    let next = f.event(2, EventKind::UserPrompt, "새 결정", Some(200));
    let summary = Summary {
        correction_revision: Some(before.correction_revision),
        record_updates: vec![decision(&next, "new")],
        ..Default::default()
    };
    assert!(
        apply(&f.store, std::slice::from_ref(&next), &summary)
            .unwrap_err()
            .downcast_ref::<corrections::StaleCorrections>()
            .is_some()
    );
    assert_eq!(f.journal(), edited);
    // Same source, a fresh key and a different substring cannot revive its old interpretation.
    apply_changes(
        &f,
        &first,
        vec![decision(&first, "different-model-key")],
        vec![],
    );
    assert_eq!(f.journal().records.len(), 1);
    assert_eq!(f.journal().records[id].text(), "정정한 내용");
    // Truly new evidence is allowed after the caller observes the updated correction revision.
    let summary = Summary {
        correction_revision: Some(edited.correction_revision),
        record_updates: vec![decision(&next, "new")],
        ..Default::default()
    };
    apply(&f.store, std::slice::from_ref(&next), &summary).unwrap();
    assert_eq!(f.journal().records.len(), 2);
    assert_eq!(
        f.journal().records[id].corrections,
        edited.records[id].corrections
    );
}

#[test]
fn excluded_completion_is_not_shown_as_a_success_or_follow_up() {
    use crate::corrections::{self, Edit};
    let f = Fixture::new();
    let event = f.event(1, EventKind::AssistantFinal, "배포 성공 보고", Some(100));
    let change = Change {
        kind: Kind::Completion,
        operation: Operation::Add,
        key: "deploy".into(),
        target: None,
        text: event.content.clone(),
        condition: None,
        stage: Stage::Deployment,
        outcome: Outcome::Reported,
        evidence: vec![Evidence {
            reference: "event:1".into(),
            quote: event.content.clone(),
        }],
        decision_basis: None,
    };
    apply_changes(&f, &event, vec![change], vec![]);
    let before = f.journal();
    let id = before.records.keys().next().unwrap();
    corrections::save(
        &f.store.state_root,
        &f.scope(),
        id,
        before.revision,
        &Edit::Exclude,
        "다른 환경의 배포였다",
        &f.store.key,
    )
    .unwrap();
    let snapshot = crate::branch_panel::saved(&f.store.state_root, f.scope()).unwrap();
    let display = crate::branch_panel::render(&snapshot, 80, &crate::branch_panel::View::default())
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!display.contains("배포 성공 보고"));
    assert!(display.contains(crate::tr!(
        "오기록 제외 1건",
        "Incorrect records excluded: 1"
    )));
    assert_eq!(
        snapshot.journal.records[id].versions,
        before.records[id].versions
    );
}

#[test]
fn record_editor_requires_review_then_save_and_preserves_cancellation() {
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    let f = Fixture::new();
    let event = f.event(1, EventKind::UserPrompt, "기존 기록", Some(100));
    apply_changes(&f, &event, vec![decision(&event, "edit")], vec![]);
    let before = f.journal();
    let mut editor = crate::record_editor::Editor::open(f.scope(), before.clone());
    let key = |code| Event::Key(KeyEvent::new(code, KeyModifiers::NONE));
    assert!(!editor.handle(&f.store, &key(KeyCode::Enter)));
    let detail = editor
        .frame(30, 42)
        .0
        .iter()
        .map(ToString::to_string)
        .collect::<String>();
    assert!(detail.contains("fixture-session"));
    assert!(detail.contains("기존 기록"));
    assert!(detail.contains(crate::tr!("발언 시각", "statement times")));
    editor.handle(&f.store, &key(KeyCode::Char('e')));
    editor.handle(
        &f.store,
        &Event::Key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL)),
    );
    editor.handle(&f.store, &Event::Paste("한글 정정\n내용".into()));
    editor.handle(&f.store, &key(KeyCode::Tab));
    editor.handle(&f.store, &Event::Paste("검증 후".into()));
    editor.handle(&f.store, &key(KeyCode::Tab));
    editor.handle(&f.store, &Event::Paste("조건이 빠졌음".into()));
    editor.handle(&f.store, &key(KeyCode::Enter));
    let confirmation = editor
        .frame(80, 42)
        .0
        .iter()
        .map(ToString::to_string)
        .collect::<String>();
    assert!(confirmation.contains(crate::tr!("모든 Space", "all Spaces")));
    assert!(confirmation.contains(crate::tr!("변경 전", "Before")));
    assert!(confirmation.contains(crate::tr!("변경 후", "After")));
    assert_eq!(f.journal(), before);
    editor.handle(&f.store, &key(KeyCode::Esc)); // confirmation -> form, not save
    editor.handle(&f.store, &key(KeyCode::Enter));
    editor.handle(&f.store, &key(KeyCode::Enter));
    let changed = f.journal();
    let record = changed.records.values().next().unwrap();
    assert_eq!(record.text(), "한글 정정 내용");
    assert_eq!(record.condition(), Some("검증 후"));
    assert_eq!(record.corrections.len(), 1);
    assert_eq!(record.latest().text, "기존 기록");
    editor.handle(&f.store, &key(KeyCode::Char('x')));
    editor.handle(&f.store, &Event::Paste("취소할 제외".into()));
    editor.handle(&f.store, &key(KeyCode::Enter));
    editor.handle(&f.store, &key(KeyCode::Esc));
    editor.handle(&f.store, &key(KeyCode::Esc));
    assert_eq!(f.journal(), changed);
}

#[test]
fn same_branch_tabs_have_independent_memory_states_corrections_and_opt_out() {
    use crate::corrections::{self, Edit};
    let f = Fixture::new();
    let a = SpaceStore::for_scope(
        &f.store.state_root,
        "test",
        "one",
        Some("one:t1"),
        f.repo.to_str().unwrap(),
        None,
    )
    .unwrap();
    let b = SpaceStore::for_scope(
        &f.store.state_root,
        "test",
        "one",
        Some("one:t2"),
        f.repo.to_str().unwrap(),
        None,
    )
    .unwrap();
    assert_ne!(a.key, b.key);
    assert!(f.store.legacy_with_tabs().unwrap());
    assert!(!a.legacy_with_tabs().unwrap());
    for (store, name) in [(&a, "탭 A 승인"), (&b, "탭 B 승인")] {
        let event = store
            .append_event(NewTurnEvent {
                kind: EventKind::UserPrompt,
                session_id: store.key.clone(),
                turn_id: Some("turn-1".into()),
                model: None,
                content: name.into(),
                cwd: f.repo.to_str().unwrap().into(),
            })
            .unwrap();
        apply(
            store,
            std::slice::from_ref(&event),
            &Summary {
                record_updates: vec![decision(&event, "same-semantic-key")],
                state_updates: vec![state(&event, StateKind::Action, "same-action-key")],
                ..Default::default()
            },
        )
        .unwrap();
    }
    let ca = ctx::detect_for_store(&a, f.repo.to_str().unwrap()).unwrap();
    let cb = ctx::detect_for_store(&b, f.repo.to_str().unwrap()).unwrap();
    assert_ne!(ca.key, cb.key);
    let ja = load(&a.state_root, &ca).unwrap();
    let jb = load(&b.state_root, &cb).unwrap();
    let id = ja.records.keys().next().unwrap();
    assert_eq!(ja.records[id].text(), "탭 A 승인");
    assert_eq!(jb.records[id].text(), "탭 B 승인");
    corrections::save(
        &a.state_root,
        &ca,
        id,
        ja.revision,
        &Edit::Exclude,
        "A에서 잘못 기록",
        &a.key,
    )
    .unwrap();
    assert!(load(&a.state_root, &ca).unwrap().records[id].excluded());
    assert_eq!(load(&b.state_root, &cb).unwrap(), jb);
    crate::participation::set_enabled(&a, false).unwrap();
    assert!(crate::participation::load(&b).unwrap().enabled);
    let reloaded = SpaceStore::for_scope(
        &f.store.state_root,
        "test",
        "one",
        Some("one:t1"),
        f.repo.to_str().unwrap(),
        None,
    )
    .unwrap();
    assert!(!crate::participation::load(&reloaded).unwrap().enabled);
    let sources = ctx::sources(&a).unwrap();
    assert!(
        sources
            .values()
            .all(|s| s.context.tab_id.as_deref() == Some("one:t1"))
    );
}

#[test]
fn tab_creation_preserves_legacy_archive_and_inherits_explicit_opt_out() {
    let f = Fixture::new();
    let old = f.event(1, EventKind::UserPrompt, "예전 Space의 결정", Some(100));
    apply_changes(&f, &old, vec![decision(&old, "legacy-decision")], vec![]);
    crate::participation::set_enabled(&f.store, false).unwrap();
    let bytes = std::fs::read(ctx::directory(&f.store.state_root, &f.scope()).join("journal.json"))
        .unwrap();
    let tab = SpaceStore::for_scope(
        &f.store.state_root,
        "test",
        "one",
        Some("one:t1"),
        f.repo.to_str().unwrap(),
        None,
    )
    .unwrap();
    assert!(!crate::participation::load(&tab).unwrap().enabled);
    assert!(!tab.ui().unwrap().visible);
    assert_eq!(
        std::fs::read(ctx::directory(&f.store.state_root, &f.scope()).join("journal.json"))
            .unwrap(),
        bytes
    );
    assert!(ctx::sources(&tab).unwrap().is_empty());
    assert!(
        load(
            &tab.state_root,
            &ctx::detect_for_store(&tab, f.repo.to_str().unwrap()).unwrap()
        )
        .unwrap()
        .records
        .is_empty()
    );
    assert_eq!(crate::worker::next_attempt_at(&f.store).unwrap(), None);
}
