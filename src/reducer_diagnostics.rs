//! Private, bounded evidence for post-response failures; never model input or a journal.
use crate::{
    AppResult,
    model::{TurnEvent, WorkerStatus},
    storage::{SpaceStore, atomic_write_json},
};
use serde::Serialize;
use std::{error::Error, fmt};

pub const FILE: &str = "reducer-failure.json";
const MAX_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug)]
pub struct RejectedResponse {
    pub stage: &'static str,
    pub cause: Box<dyn Error + Send + Sync>,
    pub response: Vec<u8>,
    pub model_events: Vec<TurnEvent>,
}
impl fmt::Display for RejectedResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.cause.fmt(f)
    }
}
impl Error for RejectedResponse {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.cause.as_ref())
    }
}

#[derive(Serialize)]
struct Failure<'a> {
    schema: &'static str,
    stage: &'a str,
    detail: String,
    payload_omitted: bool,
    model_events: &'a [TurnEvent],
    original_events: &'a [TurnEvent],
    response_utf8: Option<&'a str>,
    response_bytes: usize,
}

pub fn record(
    store: &SpaceStore,
    error: &RejectedResponse,
    original: &[TurnEvent],
) -> AppResult<()> {
    // Retain only the latest rejected selected batch. No full transcript, model
    // reasoning, stderr or unrelated Space data. Never truncate and call it replayable.
    let response = std::str::from_utf8(&error.response).ok();
    let mut failure = Failure {
        schema: "pm-reducer-failure/v1",
        stage: error.stage,
        detail: error.to_string().chars().take(500).collect(),
        payload_omitted: false,
        model_events: &error.model_events,
        original_events: original,
        response_utf8: response,
        response_bytes: error.response.len(),
    };
    if response.is_none() || serde_json::to_vec_pretty(&failure)?.len() > MAX_BYTES {
        failure.payload_omitted = true;
        failure.model_events = &[];
        failure.original_events = &[];
        failure.response_utf8 = None;
    }
    atomic_write_json(&store.path(FILE), &failure)
}

/// Presentation of the currently selected batch's status, not a pipeline-health
/// claim. Callers must pass `daily::status`, since raw status.json can describe a
/// newer successful batch while an older selected batch still has an exhausted
/// retry budget. Technical detail stays verbatim in WorkerStatus.message.
pub fn failure_copy(status: &WorkerStatus) -> Option<String> {
    failure_copy_for_language(status, crate::i18n::current())
}

fn failure_copy_for_language(
    status: &WorkerStatus,
    language: crate::i18n::Language,
) -> Option<String> {
    if !matches!(status.state.as_str(), "error" | "retry_wait") || status.message.trim().is_empty()
    {
        return None;
    }
    let detail = diagnostic_message(&status.message);
    let (ko, en) = if detail.starts_with("decision_stage_required:") {
        (
            "요약 응답의 결정 기록에 잘못된 단계 값이 있어 저장하지 못했습니다.",
            "The summary response used an invalid stage for a decision record and was not saved.",
        )
    } else if detail.starts_with("decision_outcome_required:") {
        (
            "요약 응답의 결정 확정 상태가 형식 규칙에 맞지 않아 저장하지 못했습니다.",
            "The summary response used an invalid outcome for a decision record and was not saved.",
        )
    } else if detail.starts_with("decision_user_evidence_required:") {
        (
            "요약 응답의 결정 근거가 사용자 발언에 연결되지 않아 저장하지 못했습니다.",
            "The summary response did not anchor its decision to a user statement and was not saved.",
        )
    } else if detail.starts_with("decision_basis_not_confirmed:") {
        (
            "요약 응답이 요청·질문·제안을 확정된 결정으로 분류해 저장하지 못했습니다.",
            "The summary response classified a request, question or proposal as a confirmed decision and was not saved.",
        )
    } else if detail.starts_with("decision_basis_condition_required:") {
        (
            "조건부 승인의 전제조건이 요약 응답에서 빠져 저장하지 못했습니다.",
            "The summary response omitted a conditional approval's prerequisite and was not saved.",
        )
    } else if detail.starts_with("decision_basis_anchor_mismatch:")
        || detail == "decision_basis_anchor_missing"
    {
        (
            "요약 응답의 결정 근거가 선택된 대화 원문과 맞지 않아 저장하지 못했습니다.",
            "The summary response's decision anchor did not match the selected source conversation and was not saved.",
        )
    } else if detail.starts_with("decision_basis_required:") {
        (
            "요약 응답에 결정의 확정 근거가 없어 저장하지 못했습니다.",
            "The summary response omitted the decision's confirmation basis and was not saved.",
        )
    } else if detail == "decision needs confirmed user evidence" {
        // Older releases combined stage, outcome and source guards into one
        // message. It cannot identify which condition failed retrospectively.
        (
            "요약 응답의 결정 기록이 검증 조건을 충족하지 못했습니다.",
            "The summary response's decision record did not meet its validation conditions.",
        )
    } else if [
        "journal evidence quote_not_in_event at event:",
        "journal evidence empty_quote at event:",
        "journal evidence quote_too_long at event:",
        "evidence candidate event not in batch",
        "unknown evidence candidate_id",
        "mixed evidence candidate and quote",
        "obsolete evidence range:",
    ]
    .iter()
    .any(|code| detail.starts_with(code))
    {
        (
            "요약 응답의 근거가 선택된 대화 원문과 맞지 않아 저장하지 못했습니다.",
            "The summary response's evidence did not match the selected source conversation and was not saved.",
        )
    } else {
        return None;
    };
    Some(match language {
        crate::i18n::Language::Ko => format!("{ko} 원문과 이전 기록은 보존됩니다."),
        crate::i18n::Language::En => {
            format!("{en} Original conversations and previous records are retained.")
        }
    })
}

// Only unwrap messages authored by this app. An external stderr passage that
// merely mentions a diagnostic code must stay an unknown processing failure.
fn diagnostic_message(message: &str) -> &str {
    const WRAPPERS: [&str; 4] = [
        "요약 실패 · 원문 보존 · ",
        "Summary failed · Original retained · ",
        "동일한 검증 오류가 반복되어 자동 재시도를 중단했습니다 · 원문 보존 · 오늘 r / 이전 날짜 b · ",
        "Repeated validation error · Automatic retry stopped · Original retained · Today: r / Earlier dates: b · ",
    ];
    WRAPPERS
        .iter()
        .find_map(|prefix| message.strip_prefix(prefix))
        .unwrap_or(message)
        .trim()
}

#[cfg(test)]
mod presentation_tests {
    use super::*;
    use crate::i18n::Language;

    fn status(state: &str, message: &str) -> WorkerStatus {
        WorkerStatus {
            state: state.into(),
            message: message.into(),
            updated_at_unix: Some(123),
        }
    }

    #[test]
    fn known_failure_copy_names_the_response_cause_in_both_languages() {
        let value = status(
            "error",
            "요약 실패 · 원문 보존 · decision_stage_required: kind=decision requires stage=decision",
        );
        assert!(
            failure_copy_for_language(&value, Language::Ko)
                .unwrap()
                .contains("잘못된 단계")
        );
        assert!(
            failure_copy_for_language(&value, Language::En)
                .unwrap()
                .contains("invalid stage")
        );
        assert!(
            failure_copy_for_language(&value, Language::En)
                .unwrap()
                .contains("previous records are retained")
        );
        assert!(value.message.contains("decision_stage_required:"));
        let value = status(
            "retry_wait",
            "decision_outcome_required: kind=decision requires outcome=confirmed",
        );
        assert!(
            failure_copy_for_language(&value, Language::En)
                .unwrap()
                .contains("invalid outcome")
        );
        let value = status(
            "error",
            "decision_user_evidence_required: first evidence must cite a user event",
        );
        assert!(
            failure_copy_for_language(&value, Language::En)
                .unwrap()
                .contains("anchor its decision")
        );
    }

    #[test]
    fn legacy_generic_and_unknown_errors_do_not_invent_a_specific_cause() {
        let value = status(
            "error",
            "Repeated validation error · Automatic retry stopped · Original retained · Today: r / Earlier dates: b · decision needs confirmed user evidence",
        );
        for language in [Language::Ko, Language::En] {
            let copy = failure_copy_for_language(&value, language).unwrap();
            assert!(!copy.contains("stage") && !copy.contains("단계"));
            assert!(!copy.contains("approval") && !copy.contains("승인"));
        }
        let value = status("error", "file write failed");
        assert!(failure_copy_for_language(&value, Language::En).is_none());
    }

    #[test]
    fn external_stderr_mentions_do_not_become_validated_response_failures() {
        for message in [
            "provider stderr quoted decision_stage_required: as an example",
            "Summary failed · Original retained · provider stderr quoted decision_stage_required: as an example",
            "codex exited with 1: decision needs confirmed user evidence",
        ] {
            let value = status("error", message);
            assert!(failure_copy_for_language(&value, Language::En).is_none());
        }
        for wrapper in [
            "",
            "요약 실패 · 원문 보존 · ",
            "Summary failed · Original retained · ",
            "동일한 검증 오류가 반복되어 자동 재시도를 중단했습니다 · 원문 보존 · 오늘 r / 이전 날짜 b · ",
            "Repeated validation error · Automatic retry stopped · Original retained · Today: r / Earlier dates: b · ",
        ] {
            let value = status(
                "error",
                &format!("{wrapper}decision_stage_required: kind=decision requires stage=decision"),
            );
            assert!(
                failure_copy_for_language(&value, Language::En)
                    .unwrap()
                    .contains("invalid stage")
            );
        }
    }

    #[test]
    fn idle_and_active_states_do_not_surface_old_failure_details() {
        for state in ["idle", "queued", "summarizing", "ready", ""] {
            let value = status(state, "decision_stage_required: old diagnostic");
            assert!(failure_copy_for_language(&value, Language::En).is_none());
        }
        assert!(failure_copy_for_language(&status("error", ""), Language::En).is_none());
    }
}
