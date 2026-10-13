use super::*;

#[test]
fn request_is_visible_without_a_model_and_progress_reflows_in_narrow_panes() {
    for width in [12, 24, 52] {
        let lines = render(None, None, Some("수전 모델 제작과 렌더 검토"), width);
        let joined = lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<String>();
        let normalize = |value: &str| value.split_whitespace().collect::<String>();
        assert!(
            normalize(&joined).contains(&normalize(crate::tr!("현재 요청", "Current request")))
        );
        assert!(
            lines
                .iter()
                .all(|line| unicode_width::UnicodeWidthStr::width(line.text.as_str()) <= width)
        );
        assert!(lines.iter().all(|line| {
            !line
                .text
                .contains(crate::tr!("현재 상태 미확인", "Current state unknown"))
        }));
    }
}

#[test]
fn actual_plan_activity_and_stale_summary_stay_distinct_and_fit_small_panes() {
    let live: Snapshot = serde_json::from_value(serde_json::json!({
        "generation":0,"thread_id":"thread","turn_id":"turn","context_key":"scope","revision":2,
        "started_at":1,"updated_at":2,"ended":false,"status":"inProgress","request":"렌더를 확인하세요",
        "commentary":[{"id":"r1","text":"치수를 대조한 다음 렌더를 확인하고 있습니다","at_unix":2}],
        "plan":[{"step":"치수 대조","status":"completed"},{"step":"렌더 확인","status":"inProgress"}],
        "activities":[{"id":"t1","label":"Blender · render","status":"inProgress","at_unix":2}],"truncated":false
    })).unwrap();
    let view = View {
        snapshot: Snapshot {
            revision: 1,
            ..live.clone()
        },
        status: "ready".into(),
        requested_at: 2,
        error: String::new(),
        execution: None,
        request_id: String::new(),
        result: Some(crate::live_summary::Summary {
            current: crate::live_summary::Statement {
                text: "렌더 확인 중".into(),
                evidence: vec!["report:r1".into()],
            },
            progress: vec![],
            remaining: vec![],
        }),
    };
    for width in [12, 28, 52] {
        let lines = render(Some(&live), Some(&view), None, width);
        assert!(
            lines
                .iter()
                .all(|line| unicode_width::UnicodeWidthStr::width(line.text.as_str()) <= width)
        );
        let text = lines.iter().map(|l| l.text.as_str()).collect::<String>();
        assert!(
            text.contains(crate::tr!("Codex 보고", "Codex reported"))
                && text.contains(crate::tr!("완료 보고", "Completion reported"))
                && text.contains(crate::tr!("잠정", "Draft"))
                && text.contains(crate::tr!("새 활동 있음", "New activity"))
        );
        assert!(!text.contains("report:r1"));
    }
}
