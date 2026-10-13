//! Compact progress display uses original reports and structured lifecycle data.
use crate::live_summary::View;
use crate::live_turn::Snapshot;
use crate::panel_style::{Line, Role, wrap};
mod compact;
pub use compact::{collected_sections, preview};

fn put(lines: &mut Vec<Line>, text: &str, width: usize, role: Role) {
    wrap(
        lines,
        &crate::live_turn::clean(text, 500),
        width.max(8),
        role,
    );
}

pub fn render(
    live: Option<&Snapshot>,
    summary: Option<&View>,
    request: Option<&str>,
    width: usize,
) -> Vec<Line> {
    let mut lines = Vec::new();
    let Some(live) = live else {
        if let Some(request) = request {
            put(
                &mut lines,
                crate::tr!("현재 요청", "Current request"),
                width,
                Role::Heading,
            );
            put(&mut lines, request, width, Role::Body);
            put(
                &mut lines,
                crate::tr!(
                    "중간 활동 수집 대기 · 완료 답변 후 기존 요약에 반영",
                    "Waiting to collect activity · Included after the final response"
                ),
                width,
                Role::Muted,
            );
        }
        return lines;
    };
    put(
        &mut lines,
        if live.ended {
            match live.status.as_str() {
                "failed" => crate::tr!(
                    "응답 실패 · 완료 요약 여부 미확인",
                    "Response failed · Final summary unknown"
                ),
                "interrupted" => crate::tr!(
                    "응답 중단 · 최종 정리 대기",
                    "Response interrupted · Waiting for final summary"
                ),
                _ => crate::tr!(
                    "응답 종료 · 최종 정리 대기",
                    "Response ended · Waiting for final summary"
                ),
            }
        } else {
            crate::tr!("진행 중 · Codex 보고", "In progress · Codex reported")
        },
        width,
        Role::Heading,
    );
    let elapsed = crate::storage::unix_now().saturating_sub(live.started_at);
    put(
        &mut lines,
        &crate::tr!("요청 후 {}시간 {}분 · {}", "Since request: {}h {}m · {}";
            elapsed / 3_600,
            elapsed % 3_600 / 60,
            crate::local_time::LocalTimes::new([live.updated_at]).display(Some(live.updated_at))
        ),
        width,
        Role::Muted,
    );
    if !live.request.is_empty() {
        put(
            &mut lines,
            &crate::tr!("요청: {}", "Request: {}"; excerpt(&live.request, 240)),
            width,
            Role::Body,
        );
    }
    if let Some(report) = live.commentary.last() {
        put(
            &mut lines,
            &crate::tr!("최근 설명: {}", "Latest commentary: {}"; excerpt(&report.text, 320)),
            width,
            Role::Body,
        );
    }
    render_plan(&mut lines, live, width);
    render_activity(&mut lines, live, width);
    if live.truncated {
        put(
            &mut lines,
            crate::tr!(
                "최근 수집 기록 기준 · 앞선 기록 일부 생략",
                "Based on recent collected entries · Earlier entries may be omitted"
            ),
            width,
            Role::Muted,
        );
    }
    if !live.ended {
        put(
            &mut lines,
            crate::tr!(
                "s 여기까지 요약 · 필요할 때만 모델 호출",
                "s summarize so far · Calls the model on request"
            ),
            width,
            Role::Muted,
        );
    }
    render_summary(&mut lines, live, summary, width);
    lines
}

fn render_plan(lines: &mut Vec<Line>, live: &Snapshot, width: usize) {
    if !live.plan.is_empty() {
        put(
            lines,
            crate::tr!(
                "작업 계획 · Codex가 표시한 상태",
                "Work plan · Status shown by Codex"
            ),
            width,
            Role::Muted,
        );
        for step in live.plan.iter().take(8) {
            let status = match step.status.as_str() {
                "completed" => crate::tr!("완료 보고", "Completion reported"),
                "inProgress" => crate::tr!("진행", "Progress"),
                _ => crate::tr!("대기", "Waiting"),
            };
            put(
                lines,
                &format!("{status} · {}", step.step),
                width,
                Role::Body,
            );
        }
        if live.plan.len() > 8 {
            put(
                lines,
                crate::tr!(
                    "일부 계획 생략 · s로 여기까지 요약",
                    "Some plan entries omitted · s to summarize so far"
                ),
                width,
                Role::Muted,
            );
        }
    }
}

fn render_activity(lines: &mut Vec<Line>, live: &Snapshot, width: usize) {
    let running: Vec<_> = live
        .activities
        .iter()
        .filter(|a| a.status == "inProgress")
        .collect();
    if !live.ended && !running.is_empty() {
        for activity in running.iter().rev().take(3) {
            put(
                lines,
                &crate::tr!("실행 중: {}", "Running: {}"; activity.label),
                width,
                Role::Body,
            );
        }
    } else if let Some(activity) = live.activities.last() {
        put(
            lines,
            &crate::tr!("최근 활동: {} · {}", "Recent activity: {} · {}";
                activity.label,
                match activity.status.as_str() {
                    "failed" => crate::tr!("실패 보고", "Failure reported"),
                    "completed" => crate::tr!("도구 종료", "Tool ended"),
                    _ => crate::tr!("종료 결과 미수집", "Final result not collected"),
                }
            ),
            width,
            Role::Muted,
        );
    } else if live.commentary.is_empty() && live.plan.is_empty() {
        put(
            lines,
            crate::tr!(
                "요청만 수집됨 · 중간 활동 수집 대기",
                "Request collected · Waiting for activity"
            ),
            width,
            Role::Muted,
        );
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "Keeps summary lifecycle presentation together; localized labels add presentation lines"
)]
fn render_summary(lines: &mut Vec<Line>, live: &Snapshot, summary: Option<&View>, width: usize) {
    if let Some(view) = summary.filter(|view| {
        view.snapshot.turn_id == live.turn_id
            && view.snapshot.generation == live.generation
            && view.snapshot.thread_id == live.thread_id
            && view.snapshot.context_key == live.context_key
    }) {
        put(
            lines,
            crate::tr!("여기까지 요약 · 잠정", "Summary so far · Draft"),
            width,
            Role::Heading,
        );
        put(
            lines,
            &crate::tr!("{}까지 수집 기록", "Entries collected through {}";
                crate::local_time::LocalTimes::new([view.snapshot.updated_at])
                    .display(Some(view.snapshot.updated_at))
            ),
            width,
            Role::Muted,
        );
        match view.status.as_str() {
            "queued" => put(
                lines,
                crate::tr!(
                    "요청 대기 · 기존 완료 요약 우선",
                    "Request queued · Existing final summary first"
                ),
                width,
                Role::Muted,
            ),
            "running" => put(
                lines,
                crate::tr!(
                    "선택한 시점의 기록 정리 중",
                    "Summarizing the selected snapshot"
                ),
                width,
                Role::Muted,
            ),
            "failed" => put(
                lines,
                &crate::tr!("{} · s 다시 요청", "{} · s to request again"; view.error),
                width,
                Role::Warning,
            ),
            "ready" => {
                if let Some(result) = &view.result {
                    if result.current.text.is_empty()
                        && result.progress.is_empty()
                        && result.remaining.is_empty()
                    {
                        put(
                            lines,
                            crate::tr!(
                                "이 기록으로는 진행 상황을 정리하기 어렵습니다",
                                "Insufficient records to summarize progress"
                            ),
                            width,
                            Role::Muted,
                        );
                    }
                    if !result.current.text.is_empty() {
                        put(
                            lines,
                            &crate::tr!("현재: {}", "Current: {}"; result.current.text),
                            width,
                            Role::Body,
                        );
                    }
                    for item in &result.progress {
                        put(
                            lines,
                            &crate::tr!("진척: {}", "Progress: {}"; item.text),
                            width,
                            Role::Body,
                        );
                    }
                    for item in &result.remaining {
                        put(
                            lines,
                            &crate::tr!("남은 확인: {}", "Remaining checks: {}"; item.text),
                            width,
                            Role::Body,
                        );
                    }
                }
            }
            _ => {}
        }
        if view.snapshot.revision != live.revision {
            put(
                lines,
                crate::tr!(
                    "요청 후 새 활동 있음 · 이 요약에는 아직 미반영",
                    "New activity since request · Not yet included in this summary"
                ),
                width,
                Role::Muted,
            );
        }
        if view.snapshot.truncated {
            put(
                lines,
                crate::tr!(
                    "최근 기록 일부를 정리한 요약입니다",
                    "Summary of some recent entries"
                ),
                width,
                Role::Muted,
            );
        }
    }
}

fn excerpt(text: &str, limit: usize) -> String {
    let mut value = crate::live_turn::clean(text, limit);
    if text.chars().count() > limit {
        value.push('…');
    }
    value
}

#[cfg(test)]
#[path = "live_panel_tests.rs"]
mod tests;
