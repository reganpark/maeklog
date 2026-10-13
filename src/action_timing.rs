//! Quarantine scheduling disagreements without inventing action readiness.
use crate::AppResult;
use crate::model::{Summary, TurnEvent};
use crate::storage::{atomic_write_json, lock_file, read_json_or_default, unix_now};
use serde::{Deserialize, Serialize};
use std::path::Path;

const MAX_DIAGNOSTICS: usize = 50;

#[derive(Debug, Deserialize, Serialize)]
pub struct Conflict {
    pub action_index: usize,
    pub step: Option<u8>,
    pub when: Option<String>,
}

pub fn quarantine(summary: &mut Summary) -> AppResult<Vec<Conflict>> {
    let mut conflicts = Vec::new();
    for (index, action) in summary.next_actions.iter_mut().enumerate() {
        if action.step.is_some_and(|step| step > 5)
            || action
                .when
                .as_ref()
                .is_some_and(|value| value.chars().count() > 40)
        {
            return Err("action timing metadata exceeds schema bounds".into());
        }
        let consistent = match (action.step, action.when.as_deref()) {
            (None, None) | (Some(0), Some("지금" | "now")) => true,
            (Some(1..=5), Some(condition)) => {
                !condition.trim().is_empty() && !matches!(condition, "지금" | "now")
            }
            _ => false,
        };
        if !consistent {
            conflicts.push(Conflict {
                action_index: index,
                step: action.step,
                when: action.when.clone(),
            });
            action.step = None;
            action.when = None;
        }
    }
    Ok(conflicts)
}

#[derive(Debug, Deserialize, Serialize)]
struct Diagnostic {
    recorded_at_unix: u64,
    base_revision: u64,
    session_id: Option<String>,
    first_event_seq: Option<u64>,
    last_event_seq: Option<u64>,
    conflicts: Vec<Conflict>,
}

pub fn record(
    root: &Path,
    base_revision: u64,
    events: &[TurnEvent],
    conflicts: Vec<Conflict>,
) -> AppResult<()> {
    if conflicts.is_empty() {
        return Ok(());
    }
    let directory = crate::config::private_dir(root.join("runtime/diagnostics"))?;
    let _lock = lock_file(&directory.join(".action-timing.lock"), false)?;
    let path = directory.join("action-timing.json");
    let mut entries: Vec<Diagnostic> = read_json_or_default(&path)?;
    entries.push(Diagnostic {
        recorded_at_unix: unix_now(),
        base_revision,
        session_id: events.last().map(|event| event.session_id.clone()),
        first_event_seq: events.first().map(|event| event.seq),
        last_event_seq: events.last().map(|event| event.seq),
        conflicts,
    });
    if entries.len() > MAX_DIAGNOSTICS {
        entries.drain(..entries.len() - MAX_DIAGNOSTICS);
    }
    atomic_write_json(&path, &entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn summary() -> Summary {
        serde_json::from_value(
            serde_json::json!({"objective":"프로젝트 목표", "next_actions":[
                {"owner":"user","text":"계획 승인","evidence":["event:1"],"step":0,"when":"지금"},
                {"owner":"sol","text":"구현","evidence":["event:2"],"step":0,"when":"승인 후"},
                {"owner":"sol","text":"검증","evidence":["event:3"],"step":2,"when":"지금"},
                {"owner":"sol","text":"후속 작업","evidence":[],"step":null,"when":"확인 후"}
            ]}),
        )
        .unwrap()
    }
    #[test]
    fn only_conflicting_metadata_changes_and_readiness_is_never_promoted() {
        let mut summary = summary();
        let original = summary.clone();
        assert!(crate::summarizer::validate(&summary).is_err());
        let conflicts = quarantine(&mut summary).unwrap();
        assert_eq!(conflicts.len(), 3);
        assert_eq!(conflicts[0].step, Some(0));
        assert_eq!(conflicts[0].when.as_deref(), Some("승인 후"));
        assert_eq!(summary.objective, original.objective);
        assert_eq!(summary.next_actions[0], original.next_actions[0]);
        for (new, old) in summary.next_actions[1..]
            .iter()
            .zip(&original.next_actions[1..])
        {
            assert_eq!(
                (&new.text, &new.owner, &new.evidence),
                (&old.text, &old.owner, &old.evidence)
            );
            assert_eq!((new.step, new.when.as_deref()), (None, None));
        }
        assert!(crate::summarizer::validate(&summary).is_ok());
        assert!(quarantine(&mut summary).unwrap().is_empty());
    }
    #[test]
    fn korean_and_english_now_keep_the_same_scheduling_contract() {
        for now in ["지금", "now"] {
            let mut value = Summary::default();
            value.next_actions.push(crate::model::NextAction {
                owner: "sol".into(),
                text: "Keep original content".into(),
                evidence: vec![],
                step: Some(0),
                when: Some(now.into()),
            });
            let original = value.clone();
            assert!(quarantine(&mut value).unwrap().is_empty());
            assert_eq!(value, original);
            value.next_actions[0].step = Some(2);
            assert_eq!(quarantine(&mut value).unwrap().len(), 1);
            assert_eq!(value.next_actions[0].text, original.next_actions[0].text);
            assert_eq!(value.next_actions[0].step, None);
            assert_eq!(value.next_actions[0].when, None);
        }
    }

    #[test]
    fn unrelated_validation_and_structural_errors_still_fail() {
        let mut summary = summary();
        summary.next_actions[1].owner = "invalid".into();
        quarantine(&mut summary).unwrap();
        assert!(crate::summarizer::validate(&summary).is_err());
        summary.next_actions[0].step = Some(255);
        assert!(quarantine(&mut summary).is_err());
        summary.next_actions[0].step = Some(0);
        summary.next_actions[0].when = Some("가".repeat(41));
        assert!(quarantine(&mut summary).is_err());
    }
    #[test]
    fn diagnostics_are_private_bounded_and_omit_action_text() {
        use std::os::unix::fs::PermissionsExt;
        let root =
            std::env::temp_dir().join(format!("pm-action-diagnostic-{}", std::process::id()));
        for revision in 0..55 {
            record(&root, revision, &[], quarantine(&mut summary()).unwrap()).unwrap();
        }
        let path = root.join("runtime/diagnostics/action-timing.json");
        let bytes = std::fs::read(&path).unwrap();
        let entries: Vec<Diagnostic> = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(entries.len(), MAX_DIAGNOSTICS);
        assert_eq!(entries[0].base_revision, 5);
        assert!(!String::from_utf8(bytes).unwrap().contains("구현"));
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
