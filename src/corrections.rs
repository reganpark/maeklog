//! User amendments are an append-only overlay, never rewritten source evidence.
use crate::{
    AppResult, branch_context as context,
    journal::{self, Record},
    storage,
};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Content {
    pub text: String,
    pub condition: Option<String>,
    pub excluded: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Correction {
    pub sequence: u64,
    pub at_unix_utc: u64,
    pub actor: String,
    pub space: String,
    pub reason: String,
    pub original: Content,
    pub before: Option<Content>,
    pub after: Option<Content>,
    pub undo_of: Option<u64>,
}
#[derive(Clone, Debug)]
pub enum Edit {
    Amend {
        text: String,
        condition: Option<String>,
    },
    Exclude,
    Undo,
}

pub fn current(record: &Record) -> Option<&Content> {
    record.corrections.last().and_then(|c| c.after.as_ref())
}
pub fn content(record: &Record) -> Content {
    current(record).cloned().unwrap_or_else(|| Content {
        text: record.latest().text.clone(),
        condition: record.latest().condition.clone(),
        excluded: false,
    })
}
pub fn undo_target(record: &Record) -> Option<&Correction> {
    let mut stack = Vec::new();
    for entry in &record.corrections {
        if entry.undo_of.is_some() {
            stack.pop();
        } else {
            stack.push(entry);
        }
    }
    stack.pop()
}
fn valid_text(text: &str, limit: usize, empty: bool) -> bool {
    (empty || !text.trim().is_empty())
        && text.chars().count() <= limit
        && !text.chars().any(char::is_control)
}
pub fn prepare(record: &Record, edit: &Edit, reason: &str, space: &str) -> AppResult<Correction> {
    if !valid_text(reason, 600, false) {
        return Err(crate::tr!(
            "정정 사유를 1~600자로 입력하세요. 제어 문자는 사용할 수 없습니다.",
            "Enter a correction reason of 1–600 characters, without control characters."
        )
        .into());
    }
    let before = current(record).cloned();
    let mut after = content(record);
    let (after, undo_of) = match edit {
        Edit::Amend { text, condition } => {
            if after.excluded {
                return Err(crate::tr!(
                    "오기록 제외를 먼저 되돌린 뒤 내용을 정정하세요.",
                    "Undo the incorrect-record exclusion before correcting its content."
                )
                .into());
            }
            if !valid_text(text, 1200, false)
                || condition
                    .as_ref()
                    .is_some_and(|v| !valid_text(v, 600, false))
            {
                return Err(
                    crate::tr!("내용은 1~1200자, 조건은 1~600자로 입력하세요. 조건 없음은 빈칸으로 둡니다.", "Enter 1–1200 characters for content and 1–600 for a condition. Leave the condition blank for none.")
                        .into(),
                );
            }
            after.text = text.trim().into();
            after.condition = condition.as_ref().map(|v| v.trim().into());
            if after == content(record) {
                return Err(crate::tr!("바뀐 내용이 없습니다.", "Nothing changed.").into());
            }
            (Some(after), None)
        }
        Edit::Exclude => {
            if after.excluded {
                return Err(crate::tr!(
                    "이미 오기록으로 제외한 항목입니다.",
                    "This record is already excluded as incorrect."
                )
                .into());
            }
            after.excluded = true;
            (Some(after), None)
        }
        Edit::Undo => {
            let target = undo_target(record).ok_or(crate::tr!(
                "되돌릴 사용자 정정이 없습니다.",
                "No user correction to undo."
            ))?;
            (target.before.clone(), Some(target.sequence))
        }
    };
    Ok(Correction {
        sequence: u64::try_from(record.corrections.len())? + 1,
        at_unix_utc: storage::unix_now(),
        actor: "user".into(),
        space: space.into(),
        reason: reason.trim().into(),
        original: Content {
            text: record.latest().text.clone(),
            condition: record.latest().condition.clone(),
            excluded: false,
        },
        before,
        after,
        undo_of,
    })
}
/// Optimistic revision check under the same lock used by model commits and deletion.
/// A preview cannot silently overwrite another Space's edit or a new model result.
pub fn save(
    root: &Path,
    scope: &context::Context,
    id: &str,
    revision: u64,
    edit: &Edit,
    reason: &str,
    space: &str,
) -> AppResult<journal::Journal> {
    let dir = context::directory(root, scope);
    let _guard = storage::lock_file(&dir.join(".journal.lock"), false)?;
    let mut journal = journal::load(root, scope)?;
    if journal.revision != revision {
        return Err(crate::tr!("다른 Space 또는 새 요약이 기록을 변경했습니다. 목록을 다시 열어 최신 근거를 확인한 뒤 정정하세요.", "Another Space or summary changed this record. Reopen the list and review the latest evidence before correcting it.").into());
    }
    let record = journal.records.get_mut(id).ok_or(crate::tr!(
        "항목이 없어졌습니다. 목록을 다시 여세요.",
        "Record no longer exists. Reopen the list."
    ))?;
    let correction = prepare(record, edit, reason, space)?;
    record.corrections.push(correction);
    journal.revision += 1;
    journal.correction_revision += 1;
    journal.updated_at_unix_utc = Some(storage::unix_now());
    storage::atomic_write_json(&dir.join("journal.json"), &journal)?;
    Ok(journal)
}

#[derive(Debug)]
pub struct StaleCorrections;
impl std::fmt::Display for StaleCorrections {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(crate::tr!(
            "사용자 정정 후 이전 요약 결과 보류 · 최신 정정 기준으로 다시 처리",
            "Earlier summary held after user correction · Reprocessing with the latest correction"
        ))
    }
}
impl std::error::Error for StaleCorrections {}
pub fn check_revision(journal: &journal::Journal, expected: Option<u64>) -> AppResult<()> {
    if expected.is_some_and(|v| v != journal.correction_revision) {
        return Err(Box::new(StaleCorrections));
    }
    Ok(())
}
