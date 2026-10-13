// Legacy bounded importer retained for compatibility tests, not automatically invoked.
#![cfg_attr(not(test), allow(dead_code))]
use crate::AppResult;
use serde::Serialize;
use serde_json::Value;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_TRANSCRIPT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_HISTORY_MESSAGES: usize = 64;
const FIRST_HISTORY_MESSAGES: usize = 12;
const MAX_MESSAGE_CHARS: usize = 8_000;
const MAX_HISTORY_CHARS: usize = 160_000;
const SPARSE_TRANSCRIPT_MESSAGES: usize = 8;
const MAX_PREDECESSOR_AGE_SECS: u64 = 30 * 24 * 60 * 60;
const MAX_PREDECESSOR_CANDIDATES: usize = 256;
const MAX_SESSION_META_BYTES: u64 = 128 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TranscriptExcerpt {
    pub truncated: bool,
    pub messages: Vec<TranscriptMessage>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TranscriptMessage {
    pub evidence: String,
    pub role: String,
    pub content: String,
    pub previous_session: bool,
    pub unfinished_turn: bool,
    pub recorded_at_unix: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RawMessage {
    role: String,
    content: String,
    previous_session: bool,
    unfinished_turn: bool,
    recorded_at_unix: Option<u64>,
}

#[derive(Debug)]
struct SessionMeta {
    session_id: String,
    cwd: PathBuf,
    started_at_secs: u64,
}

pub fn load_for_session(
    codex_home: &Path,
    session_id: &str,
) -> AppResult<Option<TranscriptExcerpt>> {
    if !valid_session_id(session_id) {
        return Err("invalid Codex session id".into());
    }
    let Some(path) = find_transcript(&codex_home.join("sessions"), session_id)? else {
        return Ok(None);
    };
    let sessions_root = codex_home.join("sessions");
    let metadata = fs::metadata(&path)?;
    let input_bytes = metadata.len();
    if input_bytes > MAX_TRANSCRIPT_BYTES {
        return Err(format!(
            "Codex transcript exceeds {} MiB bootstrap limit",
            MAX_TRANSCRIPT_BYTES / (1024 * 1024)
        )
        .into());
    }

    let mut raw = read_context_messages(&path)?;
    if raw.len() <= SPARSE_TRANSCRIPT_MESSAGES
        && let Some(current_meta) = read_session_meta(&path)?
        && let Some(previous_path) = find_predecessor(&sessions_root, &path, &current_meta)?
    {
        let previous_bytes = fs::metadata(&previous_path)?.len();
        if input_bytes.saturating_add(previous_bytes) <= MAX_TRANSCRIPT_BYTES {
            let mut previous = read_context_messages(&previous_path)?;
            for message in &mut previous {
                message.previous_session = true;
            }
            previous.append(&mut raw);
            raw = previous;
        }
    }
    if raw.is_empty() {
        return Ok(None);
    }

    let original_count = raw.len();
    let selected = if original_count > MAX_HISTORY_MESSAGES {
        let recent_count = MAX_HISTORY_MESSAGES - FIRST_HISTORY_MESSAGES;
        let mut selected = raw[..FIRST_HISTORY_MESSAGES].to_vec();
        selected.extend_from_slice(&raw[original_count - recent_count..]);
        selected
    } else {
        raw
    };
    let per_message_limit = (MAX_HISTORY_CHARS / selected.len()).min(MAX_MESSAGE_CHARS);
    let mut content_truncated = false;
    let messages = selected
        .into_iter()
        .enumerate()
        .map(|(index, message)| {
            let (content, was_truncated) = truncate_chars(&message.content, per_message_limit);
            content_truncated |= was_truncated;
            TranscriptMessage {
                evidence: format!("history:{}", index + 1),
                role: message.role,
                content,
                previous_session: message.previous_session,
                unfinished_turn: message.unfinished_turn,
                recorded_at_unix: message.recorded_at_unix,
            }
        })
        .collect();

    Ok(Some(TranscriptExcerpt {
        truncated: original_count > MAX_HISTORY_MESSAGES || content_truncated,
        messages,
    }))
}

/// Explicit reactivation imports at most three completed turns from this session,
/// never a predecessor or the old first+last-history bootstrap.
pub type RecentTurn = (String, String, Option<u64>, Option<u64>);
pub fn load_recent_turns(codex_home: &Path, session_id: &str) -> AppResult<Vec<RecentTurn>> {
    if !valid_session_id(session_id) {
        return Err("invalid Codex session id".into());
    }
    let Some(path) = find_transcript(&codex_home.join("sessions"), session_id)? else {
        return Ok(Vec::new());
    };
    let messages = read_messages(&path)?;
    let mut turns = Vec::new();
    let mut prompts = Vec::new();
    let mut prompt_time = None;
    for message in messages {
        if message.role == "user" {
            prompt_time = if prompts.is_empty() {
                message.recorded_at_unix
            } else {
                None
            };
            prompts.push(message.content);
        } else if message.role == "assistant" && !prompts.is_empty() {
            turns.push((
                prompts.join("\n"),
                message.content,
                message.recorded_at_unix,
                prompt_time,
            ));
            prompts.clear();
        }
    }
    if turns.len() > 3 {
        turns.drain(..turns.len() - 3);
    }
    Ok(turns)
}

fn read_context_messages(path: &Path) -> AppResult<Vec<RawMessage>> {
    let mut messages = read_messages(path)?;
    for message in messages
        .iter_mut()
        .rev()
        .take_while(|message| message.role == "user")
    {
        // A user's decision exists as soon as they state it, even while the
        // assistant is working. Keep that evidence without claiming completion.
        message.unfinished_turn = true;
    }
    Ok(messages)
}

fn find_transcript(root: &Path, session_id: &str) -> AppResult<Option<PathBuf>> {
    if !root.is_dir() {
        return Ok(None);
    }
    let expected_suffix = format!("-{session_id}.jsonl");
    let mut candidates = Vec::new();
    collect_candidates(root, &expected_suffix, 0, &mut candidates)?;
    candidates.sort_by_key(|(modified, _)| *modified);
    Ok(candidates.pop().map(|(_, path)| path))
}

fn collect_candidates(
    directory: &Path,
    expected_suffix: &str,
    depth: usize,
    candidates: &mut Vec<(SystemTime, PathBuf)>,
) -> AppResult<()> {
    if depth > 6 {
        return Ok(());
    }
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            continue;
        }
        let path = entry.path();
        if file_type.is_dir() {
            collect_candidates(&path, expected_suffix, depth + 1, candidates)?;
        } else if file_type.is_file()
            && entry
                .file_name()
                .to_string_lossy()
                .ends_with(expected_suffix)
        {
            let modified = entry.metadata()?.modified().unwrap_or(UNIX_EPOCH);
            candidates.push((modified, path));
        }
    }
    Ok(())
}

fn find_predecessor(
    root: &Path,
    current_path: &Path,
    current_meta: &SessionMeta,
) -> AppResult<Option<PathBuf>> {
    let mut candidates = Vec::new();
    collect_candidates(root, ".jsonl", 0, &mut candidates)?;
    candidates.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));

    let mut best: Option<(u64, PathBuf)> = None;
    for (_, path) in candidates
        .into_iter()
        .filter(|(_, path)| path != current_path)
        .take(MAX_PREDECESSOR_CANDIDATES)
    {
        // Optional history must not prevent reading the requested live session.
        let Ok(Some(meta)) = read_session_meta(&path) else {
            continue;
        };
        if meta.session_id == current_meta.session_id
            || meta.cwd != current_meta.cwd
            || !recent_predecessor(current_meta.started_at_secs, meta.started_at_secs)
        {
            continue;
        }
        if best
            .as_ref()
            .is_none_or(|(timestamp, _)| meta.started_at_secs > *timestamp)
        {
            best = Some((meta.started_at_secs, path));
        }
    }
    Ok(best.map(|(_, path)| path))
}

fn recent_predecessor(current: u64, candidate: u64) -> bool {
    current
        .checked_sub(candidate)
        .is_some_and(|age| age > 0 && age <= MAX_PREDECESSOR_AGE_SECS)
}

fn read_session_meta(path: &Path) -> AppResult<Option<SessionMeta>> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file.take(MAX_SESSION_META_BYTES));
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Ok(None);
    }
    let Ok(value) = serde_json::from_str::<Value>(&line) else {
        return Ok(None);
    };
    if value.get("type").and_then(Value::as_str) != Some("session_meta") {
        return Ok(None);
    }
    if value
        .pointer("/payload/thread_source")
        .and_then(Value::as_str)
        != Some("user")
        || !matches!(
            value.pointer("/payload/source").and_then(Value::as_str),
            Some("cli" | "vscode")
        )
    {
        return Ok(None);
    }
    let Some(session_id) = value
        .pointer("/payload/session_id")
        .or_else(|| value.pointer("/payload/id"))
        .and_then(Value::as_str)
        .filter(|id| valid_session_id(id))
    else {
        return Ok(None);
    };
    if !path.file_name().is_some_and(|name| {
        name.to_string_lossy()
            .ends_with(&format!("-{session_id}.jsonl"))
    }) {
        return Ok(None);
    }
    let Some(cwd) = value.pointer("/payload/cwd").and_then(Value::as_str) else {
        return Ok(None);
    };
    let Some(started_at_secs) = value
        .pointer("/payload/timestamp")
        .and_then(Value::as_str)
        .and_then(utc_timestamp_seconds)
    else {
        return Ok(None);
    };
    Ok(Some(SessionMeta {
        session_id: session_id.to_string(),
        cwd: PathBuf::from(cwd),
        started_at_secs,
    }))
}

// Codex rollout metadata uses UTC RFC3339 timestamps. Unknown formats are skipped.
pub(crate) fn utc_timestamp_seconds(value: &str) -> Option<u64> {
    let (date, time) = value.strip_suffix('Z')?.split_once('T')?;
    let date: Vec<u64> = date
        .split('-')
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    let time = time.split_once('.').map_or(time, |(whole, _)| whole);
    let time: Vec<u64> = time
        .split(':')
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    let [year, month, day] = date.as_slice() else {
        return None;
    };
    let [hour, minute, second] = time.as_slice() else {
        return None;
    };
    if !(1970..=9999).contains(year)
        || !(1..=12).contains(month)
        || *hour > 23
        || *minute > 59
        || *second > 59
    {
        return None;
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let month_days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let month_index = usize::try_from(month - 1).ok()?;
    if *day == 0 || *day > month_days[month_index] {
        return None;
    }
    let leaps_through = |year: u64| year / 4 - year / 100 + year / 400;
    let days = (year - 1970) * 365 + leaps_through(year - 1) - leaps_through(1969)
        + month_days[..month_index].iter().sum::<u64>()
        + day
        - 1;
    Some(((days * 24 + hour) * 60 + minute) * 60 + second)
}

fn read_messages(path: &Path) -> AppResult<Vec<RawMessage>> {
    let file = File::open(path)?;
    let snapshot_bytes = file.metadata()?.len();
    if snapshot_bytes > MAX_TRANSCRIPT_BYTES {
        return Err("Codex transcript exceeds the history input limit".into());
    }
    let mut messages: Vec<RawMessage> = Vec::new();
    for line in BufReader::new(file.take(snapshot_bytes)).lines() {
        let line = line?;
        if !line.contains("\"type\":\"response_item\"") || !line.contains("\"type\":\"message\"") {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(message) = extract_message(&value) else {
            continue;
        };
        if messages.last().is_some_and(|previous| previous == &message) {
            continue;
        }
        messages.push(message);
    }
    Ok(messages)
}

fn extract_message(value: &Value) -> Option<RawMessage> {
    if value.get("type")?.as_str()? != "response_item"
        || value.pointer("/payload/type")?.as_str()? != "message"
    {
        return None;
    }
    let role = value.pointer("/payload/role")?.as_str()?;
    let expected_part = match role {
        "user" => "input_text",
        "assistant" if value.pointer("/payload/phase")?.as_str()? == "final_answer" => {
            "output_text"
        }
        _ => return None,
    };
    let content = value
        .pointer("/payload/content")?
        .as_array()?
        .iter()
        .filter(|part| part.get("type").and_then(Value::as_str) == Some(expected_part))
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();
    if content.is_empty() || (role == "user" && is_ambient_user_message(&content)) {
        return None;
    }
    Some(RawMessage {
        role: role.to_string(),
        content,
        previous_session: false,
        unfinished_turn: false,
        recorded_at_unix: value
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(utc_timestamp_seconds),
    })
}

fn is_ambient_user_message(content: &str) -> bool {
    let value = content.trim();
    value.starts_with("<environment_context>") || value.starts_with("<permissions instructions>")
}

fn valid_session_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
}

fn truncate_chars(value: &str, maximum: usize) -> (String, bool) {
    let mut characters = value.chars();
    let prefix: String = characters.by_ref().take(maximum).collect();
    if characters.next().is_some() {
        (format!("{prefix}…"), true)
    } else {
        (prefix, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_ID: AtomicU64 = AtomicU64::new(1);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "maeklog-transcript-test-{}-{}",
                std::process::id(),
                TEST_ID.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(path.join("sessions/2026/09/04")).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn message(role: &str, phase: Option<&str>, part_type: &str, text: &str) -> Value {
        json!({
            "type": "response_item",
            "payload": {
                "type": "message",
                "role": role,
                "phase": phase,
                "content": [{"type": part_type, "text": text}]
            }
        })
    }

    fn session_meta(session: &str, timestamp: &str, cwd: &str) -> Value {
        json!({
            "type": "session_meta",
            "payload": {
                "session_id": session,
                "timestamp": timestamp,
                "cwd": cwd,
                "source": "cli",
                "thread_source": "user"
            }
        })
    }

    #[test]
    fn imports_only_user_and_final_assistant_messages() {
        let directory = TestDirectory::new();
        let session = "01-test-session";
        let path = directory
            .0
            .join(format!("sessions/2026/09/04/rollout-now-{session}.jsonl"));
        let mut file = File::create(path).unwrap();
        for value in [
            message("developer", None, "input_text", "hidden"),
            message("user", None, "input_text", "프로젝트 목표"),
            message("assistant", Some("commentary"), "output_text", "working"),
            message(
                "assistant",
                Some("final_answer"),
                "output_text",
                "결정 완료",
            ),
            message("user", None, "input_text", "아직 답하지 않은 요청"),
        ] {
            writeln!(file, "{}", serde_json::to_string(&value).unwrap()).unwrap();
        }

        let excerpt = load_for_session(&directory.0, session).unwrap().unwrap();
        assert_eq!(excerpt.messages.len(), 3);
        assert_eq!(excerpt.messages[0].role, "user");
        assert_eq!(excerpt.messages[0].content, "프로젝트 목표");
        assert_eq!(excerpt.messages[1].role, "assistant");
        assert_eq!(excerpt.messages[1].content, "결정 완료");
        assert!(!excerpt.messages[1].unfinished_turn);
        assert_eq!(excerpt.messages[2].content, "아직 답하지 않은 요청");
        assert!(excerpt.messages[2].unfinished_turn);
    }

    #[test]
    fn rejects_path_like_session_ids() {
        let directory = TestDirectory::new();
        assert!(load_for_session(&directory.0, "../../secret").is_err());
    }

    #[test]
    fn recent_restart_reads_only_last_three_completed_turns_without_predecessors() {
        let directory = TestDirectory::new();
        let session = "recent-test";
        let mut file = File::create(
            directory
                .0
                .join(format!("sessions/2026/09/04/rollout-{session}.jsonl")),
        )
        .unwrap();
        for i in 1..=5 {
            writeln!(
                file,
                "{}",
                message("user", None, "input_text", &format!("request {i}"))
            )
            .unwrap();
            writeln!(
                file,
                "{}",
                message(
                    "assistant",
                    Some("commentary"),
                    "output_text",
                    "private progress"
                )
            )
            .unwrap();
            writeln!(
                file,
                "{}",
                message(
                    "assistant",
                    Some("final_answer"),
                    "output_text",
                    &format!("answer {i}")
                )
            )
            .unwrap();
        }
        writeln!(
            file,
            "{}",
            message("user", None, "input_text", "unfinished request")
        )
        .unwrap();
        let turns = load_recent_turns(&directory.0, session).unwrap();
        assert_eq!(turns.len(), 3);
        assert_eq!(turns[0].0, "request 3");
        assert_eq!(turns[2].1, "answer 5");
        assert!(!format!("{turns:?}").contains("private progress"));
        assert!(!format!("{turns:?}").contains("unfinished request"));
        assert!(load_recent_turns(&directory.0, "../secret").is_err());
        assert!(
            load_recent_turns(&directory.0, "missing")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn extracts_message_time_from_record_without_using_file_mtime() {
        let mut value = message("user", None, "input_text", "승인합니다");
        value["timestamp"] = json!("2026-09-06T00:40:00.100Z");
        assert_eq!(
            extract_message(&value).unwrap().recorded_at_unix,
            utc_timestamp_seconds("2026-09-06T00:40:00Z")
        );
        value["timestamp"] = json!("invalid");
        assert_eq!(extract_message(&value).unwrap().recorded_at_unix, None);
    }

    #[test]
    fn sparse_new_session_inherits_the_previous_session_in_the_same_cwd() {
        let directory = TestDirectory::new();
        let session_directory = directory.0.join("sessions/2026/09/04");
        let previous_session = "01-previous-session";
        let current_session = "02-current-session";
        let previous_path = session_directory.join(format!(
            "rollout-2026-09-04T10-00-00-{previous_session}.jsonl"
        ));
        let current_path = session_directory.join(format!(
            "rollout-2026-09-04T11-00-00-{current_session}.jsonl"
        ));

        let mut previous = File::create(previous_path).unwrap();
        for value in [
            session_meta(previous_session, "2026-09-04T10:00:00Z", "/repo/one"),
            message("user", None, "input_text", "장기 목표"),
            message(
                "assistant",
                Some("final_answer"),
                "output_text",
                "기존 결정",
            ),
        ] {
            writeln!(previous, "{}", serde_json::to_string(&value).unwrap()).unwrap();
        }
        let mut current = File::create(current_path).unwrap();
        for value in [
            session_meta(current_session, "2026-09-04T11:00:00Z", "/repo/one"),
            message("user", None, "input_text", "계속 진행"),
            message(
                "assistant",
                Some("final_answer"),
                "output_text",
                "새 진행 상황",
            ),
        ] {
            writeln!(current, "{}", serde_json::to_string(&value).unwrap()).unwrap();
        }

        let excerpt = load_for_session(&directory.0, current_session)
            .unwrap()
            .unwrap();
        assert_eq!(excerpt.messages.len(), 4);
        assert_eq!(excerpt.messages[0].content, "장기 목표");
        assert_eq!(excerpt.messages[3].content, "새 진행 상황");
        assert!(excerpt.messages[0].previous_session);
        assert!(!excerpt.messages[3].previous_session);
    }

    #[test]
    fn predecessor_ignores_other_folders_subagents_duplicates_and_old_sessions() {
        let directory = TestDirectory::new();
        let session_directory = directory.0.join("sessions/2026/09/04");
        let mut subagent = session_meta("helper", "2026-09-04T10:59:00Z", "/repo/one");
        subagent["payload"]["thread_source"] = json!("subagent");
        subagent["payload"]["source"] = json!({"subagent": {}});
        let candidates = [
            session_meta("valid", "2026-09-04T10:00:00Z", "/repo/one"),
            session_meta("other", "2026-09-04T10:58:00Z", "/repo/two"),
            session_meta("future", "2026-09-04T12:00:00Z", "/repo/one"),
            session_meta("old", "2026-07-01T10:00:00Z", "/repo/one"),
            session_meta("current", "2026-09-04T10:59:59Z", "/repo/one"),
            subagent,
        ];
        for meta in candidates {
            let id = meta["payload"]["session_id"].as_str().unwrap();
            let mut file =
                File::create(session_directory.join(format!("rollout-{id}.jsonl"))).unwrap();
            writeln!(file, "{meta}").unwrap();
        }
        let meta = SessionMeta {
            session_id: "current".into(),
            cwd: PathBuf::from("/repo/one"),
            started_at_secs: utc_timestamp_seconds("2026-09-04T11:00:00Z").unwrap(),
        };
        let previous = find_predecessor(
            &directory.0.join("sessions"),
            &session_directory.join("active-current.jsonl"),
            &meta,
        )
        .unwrap()
        .unwrap();
        assert_eq!(previous.file_name().unwrap(), "rollout-valid.jsonl");
    }

    #[test]
    fn predecessor_age_uses_session_time_not_recent_file_modifications() {
        assert_eq!(utc_timestamp_seconds("1970-01-01T00:00:00.000Z"), Some(0));
        assert_eq!(
            utc_timestamp_seconds("2000-03-01T00:00:00Z"),
            Some(951_868_800)
        );
        assert_eq!(
            utc_timestamp_seconds("2026-09-04T00:00:00Z"),
            Some(1_788_480_000)
        );
        assert!(utc_timestamp_seconds("2026-02-29T00:00:00Z").is_none());
        assert!(utc_timestamp_seconds("2026-09-04T00:00:00+09:00").is_none());
        assert!(recent_predecessor(MAX_PREDECESSOR_AGE_SECS + 1, 1));
        assert!(!recent_predecessor(MAX_PREDECESSOR_AGE_SECS + 2, 1));
        assert!(!recent_predecessor(1, 1));
        assert!(!recent_predecessor(1, 2));
    }
}
