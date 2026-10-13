//! Cache-only current-turn preview and read-only collected explanation.
use super::{Line, Role, Snapshot, View, wrap};
use crate::branch_panel::RecordSection;
use crate::local_time::LocalTimes;

#[allow(
    clippy::too_many_lines,
    reason = "Keeps current-turn preview ordering together; localized labels add presentation lines"
)]
pub fn preview(
    live: Option<&Snapshot>,
    summary: Option<&View>,
    times: &LocalTimes,
    today: Option<&str>,
    width: usize,
) -> Vec<Line> {
    let mut lines = vec![Line::default()];
    let Some(live) = live else {
        wrap(
            &mut lines,
            crate::tr!("진행 정보 없음", "No progress information"),
            width,
            Role::Muted,
        );
        return lines;
    };
    let response_role = if matches!(live.status.as_str(), "failed" | "interrupted") {
        Role::Warning
    } else {
        Role::Title
    };
    if let Some(report) = live
        .commentary
        .last()
        .filter(|report| !report.text.trim().is_empty())
    {
        wrap(
            &mut lines,
            &crate::tr!("진행 설명 · {} · {}", "Progress commentary · {} · {}";
                times.compact(Some(report.at_unix), today),
                response_label(live)
            ),
            width,
            response_role,
        );
        description(&mut lines, &report.text, width);
    } else {
        wrap(
            &mut lines,
            &crate::tr!("진행 설명 미수집 · {} · 상태 {}", "No commentary collected · {} · Status {}";
                response_label(live),
                times.compact(Some(live.updated_at), today)
            ),
            width,
            if response_role == Role::Warning {
                Role::Warning
            } else {
                Role::Body
            },
        );
    }
    if let Some(activity) = live
        .activities
        .iter()
        .rev()
        .find(|activity| activity.status == "failed")
    {
        wrap(
            &mut lines,
            &crate::tr!("현재 턴 도구 실패 · {} · {}", "Current-turn tool failed · {} · {}";
                times.compact(Some(activity.at_unix), today),
                activity.label
            ),
            width,
            Role::Warning,
        );
    }
    if live.truncated {
        wrap(
            &mut lines,
            crate::tr!(
                "현재 턴 수집 기록 일부 생략",
                "Some collected current-turn records omitted"
            ),
            width,
            Role::Warning,
        );
    }
    if let Some(view) = same_summary(live, summary) {
        match view.status.as_str() {
            "queued" | "running" => wrap(
                &mut lines,
                crate::tr!(
                    "현재 턴 잠정 요약 · 요청한 시점 정리 중",
                    "Current-turn draft summary · Preparing requested snapshot"
                ),
                width,
                Role::Muted,
            ),
            "failed" => wrap(
                &mut lines,
                &crate::tr!("현재 턴 잠정 요약 실패 · {}", "Current-turn draft summary failed · {}"; view.error),
                width,
                Role::Warning,
            ),
            "ready" if view.snapshot.revision != live.revision => wrap(
                &mut lines,
                crate::tr!(
                    "현재 턴 잠정 요약 이후 새 활동 있음 · 미반영",
                    "New activity after draft summary · Not yet included"
                ),
                width,
                Role::Warning,
            ),
            "ready" => wrap(
                &mut lines,
                crate::tr!(
                    "요청한 진행 요약 준비됨 · m 현재 진행 상세",
                    "Requested progress summary ready · m for live details"
                ),
                width,
                Role::Muted,
            ),
            _ => {}
        }
    }
    lines
}

fn response_label(live: &Snapshot) -> &'static str {
    if live.ended {
        lifecycle(live)
    } else {
        crate::tr!("응답 진행 중", "Response in progress")
    }
}

fn description(lines: &mut Vec<Line>, text: &str, width: usize) {
    let mut body = Vec::new();
    crate::panel::wrapped_bullet(&mut body, text, width, "  ");
    lines.extend(body.into_iter().map(|text| Line {
        text,
        role: Role::Body,
    }));
}

fn lifecycle(live: &Snapshot) -> &'static str {
    if live.ended {
        match live.status.as_str() {
            "failed" => crate::tr!(
                "응답 실패 · 작업 완료 여부 미확인",
                "Response failed · Work completion unknown"
            ),
            "interrupted" => crate::tr!(
                "응답 중단 · 작업 완료 여부 미확인",
                "Response interrupted · Work completion unknown"
            ),
            _ => crate::tr!(
                "응답 종료 · 작업 완료를 뜻하지 않음",
                "Response ended · Work completion not confirmed"
            ),
        }
    } else {
        crate::tr!("응답 진행 중 · Codex", "Response in progress · Codex")
    }
}

fn same_summary<'a>(live: &Snapshot, summary: Option<&'a View>) -> Option<&'a View> {
    summary.filter(|view| {
        view.snapshot.thread_id == live.thread_id
            && view.snapshot.turn_id == live.turn_id
            && view.snapshot.generation == live.generation
            && view.snapshot.context_key == live.context_key
    })
}

pub fn collected_sections(
    live: Option<&Snapshot>,
    summary: Option<&View>,
    request: Option<&str>,
    times: &LocalTimes,
) -> Vec<RecordSection> {
    let Some(live) = live else {
        let mut rows = vec![line(
            crate::tr!(
                "현재 주 대화의 수집된 진행 정보 없음",
                "No collected progress for the primary conversation"
            ),
            Role::Muted,
        )];
        if let Some(request) = request {
            rows.push(line(
                crate::tr!("최근 요청: {request}", "Latest request: {request}";),
                Role::Body,
            ));
        }
        return vec![RecordSection {
            title: crate::tr!(
                "현재 진행 · 수집된 설명",
                "Live progress · Collected commentary"
            )
            .into(),
            rows,
        }];
    };
    let mut reports = Vec::new();
    for report in &live.commentary {
        reports.push(line(
            crate::tr!("공개 설명 · {}", "Public commentary · {}"; times.display(Some(report.at_unix))),
            Role::Muted,
        ));
        reports.push(line(&report.text, Role::Body));
        reports.push(Line::default());
    }
    if reports.is_empty() {
        reports.push(line(
            crate::tr!(
                "공개 진행 설명 미수집",
                "No public progress commentary collected"
            ),
            Role::Muted,
        ));
    }
    let mut sections = vec![RecordSection {
        title: crate::tr!("공개 진행 설명 · 수집된 {}건 전체", "Public progress commentary · All {} collected entries"; live.commentary.len()),
        rows: reports,
    }];
    sections.push(activity_section(live, times));
    if let Some(view) = same_summary(live, summary) {
        sections.push(summary_section(view, live, times));
    }
    let mut source = vec![
        line(lifecycle(live), Role::Muted),
        line(
            crate::tr!("현재 최근 요청: {}", "Current latest request: {}"; request.unwrap_or(&live.request)),
            Role::Body,
        ),
        line(
            crate::tr!("요청 시각 {} · 활동 수집 {}", "Requested {} · Activity collected {}";
                times.display(Some(live.started_at)),
                times.display(Some(live.updated_at))
            ),
            Role::Muted,
        ),
        line(
            crate::tr!("주 대화 {} · 턴 {} · 수집 revision {}", "Primary conversation {} · Turn {} · Collected revision {}";
                live.thread_id, live.turn_id, live.revision
            ),
            Role::Muted,
        ),
        line(
            crate::tr!(
                "수집된 캐시 전체를 표시합니다 · 대화 원문 전체나 도구 출력은 아님",
                "All collected cache entries · Full transcript/tool output not collected"
            ),
            Role::Muted,
        ),
    ];
    if request.is_some_and(|request| request != live.request) && !live.request.is_empty() {
        source.push(line(
            crate::tr!("수집된 턴 요청: {}", "Collected turn request: {}"; live.request),
            Role::Body,
        ));
    }
    if live.truncated {
        source.push(line(
            crate::tr!(
                "수집 기록 일부 생략 · 앞선 설명이나 긴 내용이 모두 보존된 것은 아님",
                "Some collected entries omitted · Earlier or long commentary may be incomplete"
            ),
            Role::Warning,
        ));
    }
    sections.push(RecordSection {
        title: crate::tr!("수집 범위·최근 요청", "Collection scope and latest request").into(),
        rows: source,
    });
    sections
}

fn activity_section(live: &Snapshot, times: &LocalTimes) -> RecordSection {
    let mut activity = Vec::new();
    for step in &live.plan {
        activity.push(line(
            crate::tr!("계획 · {}: {}", "Plan · {}: {}";
                match step.status.as_str() {
                    "completed" => crate::tr!("완료 보고", "Completion reported"),
                    "inProgress" => crate::tr!("진행 보고", "Progress reported"),
                    _ => crate::tr!("대기", "Waiting"),
                },
                step.step
            ),
            Role::Body,
        ));
    }
    for item in &live.activities {
        activity.push(line(
            format!(
                "{} · {}: {}",
                times.display(Some(item.at_unix)),
                match item.status.as_str() {
                    "failed" => crate::tr!("도구 실패", "Tool failed"),
                    "completed" => crate::tr!(
                        "도구 종료 · 작업 완료 미확인",
                        "Tool ended · Work completion unknown"
                    ),
                    "inProgress" => crate::tr!("실행 중", "Running"),
                    _ => crate::tr!("결과 미수집", "Result not collected"),
                },
                item.label
            ),
            if item.status == "failed" {
                Role::Warning
            } else {
                Role::Body
            },
        ));
    }
    if activity.is_empty() {
        activity.push(line(
            crate::tr!("수집된 계획·활동 없음", "No collected plans or activity"),
            Role::Muted,
        ));
    }
    RecordSection {
        title: crate::tr!("수집된 계획·도구 활동", "Collected plans and tool activity").into(),
        rows: activity,
    }
}

fn summary_section(view: &View, live: &Snapshot, times: &LocalTimes) -> RecordSection {
    let mut rows = vec![line(
        crate::tr!("요청한 시점 {} · {}", "Requested snapshot {} · {}";
            times.display(Some(view.snapshot.updated_at)),
            match view.status.as_str() {
                "queued" => crate::tr!("요청 대기", "Request queued"),
                "running" => crate::tr!("정리 중", "Preparing summary"),
                "ready" => crate::tr!("잠정 요약", "Draft summary"),
                "failed" => crate::tr!("요약 실패", "Summary failed"),
                _ => crate::tr!("상태 미확인", "Status unknown"),
            }
        ),
        Role::Muted,
    )];
    if let Some(result) = &view.result {
        if !result.current.text.is_empty() {
            rows.push(line(
                crate::tr!("현재: {}", "Current: {}"; result.current.text),
                Role::Body,
            ));
        }
        for item in &result.progress {
            rows.push(line(
                crate::tr!("진척 보고: {}", "Progress reported: {}"; item.text),
                Role::Body,
            ));
        }
        for item in &result.remaining {
            rows.push(line(
                crate::tr!("남은 확인: {}", "Remaining checks: {}"; item.text),
                Role::Body,
            ));
        }
    }
    if !view.error.is_empty() {
        rows.push(line(&view.error, Role::Warning));
    }
    if view.snapshot.revision != live.revision {
        rows.push(line(
            crate::tr!(
                "이 요약 이후 새 활동 있음 · 아직 미반영",
                "New activity since this summary · Not yet included"
            ),
            Role::Warning,
        ));
    }
    if view.snapshot.truncated {
        rows.push(line(
            crate::tr!(
                "수집된 일부 기록만 정리한 요약",
                "Summary of some collected entries"
            ),
            Role::Warning,
        ));
    }
    RecordSection {
        title: crate::tr!(
            "여기까지 요약 · 저장 원장과 별도",
            "Summary so far · Separate from saved journal"
        )
        .into(),
        rows,
    }
}

fn line(text: impl Into<String>, role: Role) -> Line {
    Line {
        text: text.into(),
        role,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn live(text: &str) -> Snapshot {
        serde_json::from_value(serde_json::json!({"generation":0,"thread_id":"thread","turn_id":"turn","context_key":"scope","revision":2,"started_at":1,"updated_at":2,"ended":false,"status":"inProgress","request":"그래서 적용된 거야?","commentary":[{"id":"r1","text":text,"at_unix":2}],"plan":[],"activities":[],"truncated":false})).unwrap()
    }
    fn normalize(text: &str) -> String {
        text.chars()
            .filter(|character| !character.is_whitespace())
            .collect()
    }
    #[test]
    fn full_main_explanation_and_collected_detail_keep_late_negation() {
        let report = format!(
            "{} 아직 적용하지 못했습니다. 사용자 승인 후에만 적용합니다.",
            "확인한 내용을 대조하고 있습니다. ".repeat(30)
        );
        let snapshot = live(&report);
        let lines = preview(Some(&snapshot), None, &LocalTimes::default(), None, 28);
        let text = lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<String>();
        assert!(normalize(&text).contains(&normalize(&report)));
        assert!(
            !text.contains(crate::tr!("발췌", "excerpt"))
                && !text.contains(crate::tr!("일부 표시", "partial display"))
        );
        assert!(
            lines
                .iter()
                .all(|line| unicode_width::UnicodeWidthStr::width(line.text.as_str()) <= 28)
        );
        let sections = collected_sections(Some(&snapshot), None, None, &LocalTimes::default());
        assert!(sections[0].rows.iter().any(|line| line.text == report));
        assert!(
            sections[0]
                .rows
                .iter()
                .any(|line| line.text.contains("못했습니다") && line.text.contains("승인 후에만"))
        );
        assert!(!lines.iter().any(|line| line.role == Role::Heading));
    }
    #[test]
    fn no_report_is_not_work_finished_and_old_summary_is_not_current() {
        let lines = preview(None, None, &LocalTimes::default(), None, 80);
        assert_eq!(
            lines[1].text,
            crate::tr!("진행 정보 없음", "No progress information")
        );
        let mut snapshot = live("원문 공개 진행");
        snapshot.ended = true;
        let lines = preview(Some(&snapshot), None, &LocalTimes::default(), None, 80);
        assert!(normalize(&lines.join("\n")).contains(&normalize(crate::tr!(
            "작업 완료를 뜻하지 않음",
            "Work completion not confirmed"
        ))));
        snapshot.truncated = true;
        assert!(
            collected_sections(Some(&snapshot), None, None, &LocalTimes::default())
                .last()
                .unwrap()
                .rows
                .iter()
                .any(|line| line.text.contains(crate::tr!(
                    "수집 기록 일부 생략",
                    "Some collected entries omitted"
                )))
        );
    }
    #[test]
    fn explanation_has_a_gap_one_status_header_and_inset_paragraphs() {
        let report = "첫 문단의 현재 공개 진행 설명을 모두 읽습니다.\n\n둘째 문단은 아직 적용 전이며 승인 후에만 진행합니다.";
        let mut snapshot = live(report);
        snapshot.updated_at = 99;
        let times = LocalTimes::default();
        let lines = preview(Some(&snapshot), None, &times, None, 160);
        assert!(lines[0].text.is_empty());
        assert_eq!(
            lines[1].text,
            crate::tr!("진행 설명 · {} · 응답 진행 중", "Progress commentary · {} · Response in progress";
                times.compact(Some(2), None)
            )
        );
        assert_eq!(lines[1].role, Role::Title);
        let body = &lines[2..];
        assert_eq!(body.len(), 3);
        assert_eq!(body[1].text, "  ");
        assert!(body.iter().all(|line| line.role == Role::Body));
        assert!(body.iter().all(|line| line.text.starts_with("  ")));
        let rendered = lines.join("\n");
        assert_eq!(
            rendered
                .matches(crate::tr!("응답 진행 중", "Response in progress"))
                .count(),
            1
        );
        assert!(!rendered.contains("epoch 99") && !rendered.contains("Codex"));
        assert!(normalize(&rendered).contains(&normalize(report)));
    }
    #[test]
    fn response_end_and_failure_stay_in_the_header_without_claiming_work_completion() {
        for (status, label, role) in [
            (
                "failed",
                crate::tr!(
                    "응답 실패 · 작업 완료 여부 미확인",
                    "Response failed · Work completion unknown"
                ),
                Role::Warning,
            ),
            (
                "interrupted",
                crate::tr!(
                    "응답 중단 · 작업 완료 여부 미확인",
                    "Response interrupted · Work completion unknown"
                ),
                Role::Warning,
            ),
            (
                "completed",
                crate::tr!(
                    "응답 종료 · 작업 완료를 뜻하지 않음",
                    "Response ended · Work completion not confirmed"
                ),
                Role::Title,
            ),
        ] {
            let mut snapshot = live("실제 공개 설명의 마지막 문장까지 그대로 보존합니다.");
            snapshot.status = status.into();
            snapshot.ended = true;
            let lines = preview(Some(&snapshot), None, &LocalTimes::default(), None, 180);
            assert!(lines[1].text.ends_with(label));
            assert_eq!(lines[1].role, role);
            assert_eq!(lines.len(), 3);
            assert_eq!(lines[2].role, Role::Body);
            assert_eq!(lines.join("\n").matches(label).count(), 1);
            assert_eq!(
                lines[2].text,
                "  실제 공개 설명의 마지막 문장까지 그대로 보존합니다."
            );
        }
    }
    #[test]
    fn requested_ready_summary_has_a_detail_path_without_reusing_stale_turns() {
        let snapshot = live("수집된 진행 설명");
        let mut view = View {
            snapshot: snapshot.clone(),
            status: "ready".into(),
            result: None,
            error: String::new(),
            requested_at: 2,
            execution: None,
            request_id: String::new(),
        };
        assert!(
            preview(
                Some(&snapshot),
                Some(&view),
                &LocalTimes::default(),
                None,
                80
            )
            .join("\n")
            .contains(crate::tr!(
                "요청한 진행 요약 준비됨",
                "Requested progress summary ready"
            ))
        );
        view.snapshot.turn_id = "older-turn".into();
        assert!(
            !preview(
                Some(&snapshot),
                Some(&view),
                &LocalTimes::default(),
                None,
                80
            )
            .join("\n")
            .contains(crate::tr!("요약 준비됨", "summary ready"))
        );
    }
    #[test]
    fn collected_details_show_the_whole_current_request_even_when_cached_turn_request_differs() {
        let snapshot = live("현재 공개 설명");
        let current = format!(
            "{} 요청 마지막 · 승인 전에는 변경하지 마세요",
            "긴 현재 사용자 요청을 이어 읽습니다. ".repeat(20)
        );
        let sections = collected_sections(
            Some(&snapshot),
            None,
            Some(&current),
            &LocalTimes::default(),
        );
        let rows = &sections.last().unwrap().rows;
        assert!(rows.iter().any(|line| line.text
            == crate::tr!("현재 최근 요청: {current}", "Current latest request: {current}";)));
        assert!(rows.iter().any(|line| {
            line.text
                .contains(crate::tr!("수집된 턴 요청:", "Collected turn request:"))
        }));
    }
}
