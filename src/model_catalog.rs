//! Bounded, read-only model discovery through a separate installed Codex app-server.
//! A returned catalog can be bundled/cached; it is not an account entitlement check.
use crate::{AppResult, config, model_settings};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::io::{Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_STDOUT_BYTES: usize = 2 * 1024 * 1024;
const MAX_LINE_BYTES: usize = 512 * 1024;
const MAX_STDERR_BYTES: usize = 64 * 1024;
const MAX_MODELS: usize = 256;
const MAX_PAGES: u64 = 16;
const MAX_EFFORTS: usize = 16;

#[derive(Clone, Debug)]
pub struct Catalog {
    pub models: Vec<Model>,
}

impl Catalog {
    pub fn find(&self, model: &str) -> Option<&Model> {
        self.models.iter().find(|entry| entry.model == model)
    }
}

#[derive(Clone, Debug)]
#[allow(
    clippy::struct_field_names,
    reason = "Matches the official model/list protocol fields"
)]
pub struct Model {
    pub id: String,
    pub model: String,
    pub display_name: String,
    pub supported_reasoning_efforts: Vec<ReasoningEffort>,
    pub default_reasoning_effort: Option<String>,
    pub is_default: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningEffort {
    pub reasoning_effort: String,
    pub description: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelPage {
    data: Vec<RawModel>,
    next_cursor: Option<String>,
}

// Deliberately discard all unknown config fields, including authentication/header data.
#[derive(Deserialize)]
struct CatalogConfigResponse {
    config: CatalogConfig,
}

#[derive(Deserialize)]
struct CatalogConfig {
    model_provider: Option<String>,
    profile: Option<serde::de::IgnoredAny>,
    model_catalog_json: Option<serde::de::IgnoredAny>,
    openai_base_url: Option<serde::de::IgnoredAny>,
    chatgpt_base_url: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawModel {
    id: String,
    model: String,
    display_name: String,
    supported_reasoning_efforts: Vec<ReasoningEffort>,
    default_reasoning_effort: Option<String>,
    hidden: bool,
    is_default: bool,
    input_modalities: Option<Vec<String>>,
}

pub fn discover() -> AppResult<Catalog> {
    discover_cancellable(|| false)
}

/// Run in a UI worker. Cancellation is polled every 40 ms and kills only this child group.
pub fn discover_cancellable(cancelled: impl Fn() -> bool) -> AppResult<Catalog> {
    if cancelled() {
        return Err(cancelled_error());
    }
    let mut command = Command::new(config::codex_bin());
    command
        .args([
            "app-server",
            "--listen",
            "stdio://",
            "--config",
            "features.hooks=false",
            "--config",
            "model_provider=\"openai\"",
        ])
        .current_dir(std::env::temp_dir())
        .env("MAEKLOG_INTERNAL", "1");
    discover_command(&mut command, DISCOVERY_TIMEOUT, &cancelled)
}

fn discover_command(
    command: &mut Command,
    timeout: Duration,
    cancelled: &impl Fn() -> bool,
) -> AppResult<Catalog> {
    let deadline = Instant::now() + timeout;
    let mut server = RpcProcess::spawn(command)?;
    server.send(&json!({"id":1,"method":"initialize","params":{
        "clientInfo":{"name":"maeklog","title":"Maeklog model catalog","version":env!("CARGO_PKG_VERSION")},
        "capabilities":{"experimentalApi":false,"explicitGatewayOauth":true}
    }}))?;
    server.response(1, deadline, cancelled)?;
    server.send(&json!({"method":"initialized","params":{}}))?;
    server.send(&json!({"id":2,"method":"config/read","params":{"includeLayers":false,"cwd":std::env::temp_dir()}}))?;
    validate_catalog_config(server.response(2, deadline, cancelled)?)?;
    let mut models = Vec::new();
    let mut model_ids = HashSet::new();
    let mut entry_ids = HashSet::new();
    let mut cursors = HashSet::new();
    let mut cursor = None;
    for page_number in 0..MAX_PAGES {
        let id = page_number + 3;
        server.send(&json!({"id":id,"method":"model/list","params":{
            "limit":32,"includeHidden":false,"cursor":cursor
        }}))?;
        let value = server.response(id, deadline, cancelled)?;
        let page: ModelPage = serde_json::from_value(value).map_err(|_| malformed_error())?;
        if page.data.len() > MAX_MODELS || models.len() + page.data.len() > MAX_MODELS {
            return Err(limit_error());
        }
        for raw in page.data {
            if raw.hidden
                || raw
                    .input_modalities
                    .as_ref()
                    .is_some_and(|inputs| !inputs.iter().any(|input| input == "text"))
            {
                continue;
            }
            let model = normalize(raw)?;
            if !model_ids.insert(model.model.clone()) || !entry_ids.insert(model.id.clone()) {
                return Err(malformed_error());
            }
            models.push(model);
        }
        match page.next_cursor {
            None => {
                return Ok(Catalog { models });
            }
            Some(next)
                if !next.is_empty()
                    && next.len() <= 1024
                    && !next.chars().any(char::is_control)
                    && cursors.insert(next.clone()) =>
            {
                cursor = Some(next);
            }
            Some(_) => return Err(malformed_error()),
        }
    }
    Err(limit_error())
}

fn validate_catalog_config(response: Value) -> AppResult<()> {
    let safe: CatalogConfigResponse =
        serde_json::from_value(response).map_err(|_| malformed_error())?;
    if safe
        .config
        .model_provider
        .as_deref()
        .is_some_and(|provider| provider != "openai")
        || safe.config.profile.is_some()
        || safe.config.model_catalog_json.is_some()
        || safe.config.openai_base_url.is_some()
        || safe.config.chatgpt_base_url.as_deref().is_some_and(|url| {
            // Same public default as Codex core Config construction; config/read may emit it.
            !matches!(
                url,
                "https://chatgpt.com/backend-api/" | "https://chatgpt.com/backend-api"
            )
        })
    {
        return Err(crate::tr!(
            "Codex 모델 목록 설정이 격리된 요약 모델 설정과 다릅니다 · 사용자 지정 카탈로그 또는 제공자 설정을 확인하세요 · 현재 설정은 유지됩니다",
            "Codex catalog configuration differs from isolated summary settings; check custom catalog or provider configuration; current settings are retained"
        ).into());
    }
    Ok(())
}

fn normalize(raw: RawModel) -> AppResult<Model> {
    if !model_settings::valid_token(&raw.model, 128)
        || !model_settings::valid_token(&raw.id, 128)
        || raw.supported_reasoning_efforts.len() > MAX_EFFORTS
    {
        return Err(malformed_error());
    }
    let mut efforts = raw.supported_reasoning_efforts;
    let mut seen = HashSet::new();
    for effort in &mut efforts {
        if !model_settings::valid_token(&effort.reasoning_effort, 32)
            || !seen.insert(effort.reasoning_effort.clone())
        {
            return Err(malformed_error());
        }
        effort.description = clean_metadata(&effort.description, 280);
    }
    let default = if efforts.is_empty() {
        None
    } else {
        let default = raw.default_reasoning_effort.ok_or_else(malformed_error)?;
        if !seen.contains(&default) {
            return Err(malformed_error());
        }
        Some(default)
    };
    let display_name = clean_metadata(&raw.display_name, 120);
    Ok(Model {
        id: raw.id,
        display_name: if display_name.is_empty() {
            raw.model.clone()
        } else {
            display_name
        },
        model: raw.model,
        supported_reasoning_efforts: efforts,
        default_reasoning_effort: default,
        is_default: raw.is_default,
    })
}

/// Strip terminal escape sequences, controls and directional/invisible formatting.
pub fn clean_metadata(value: &str, limit: usize) -> String {
    let mut clean = String::new();
    let mut chars = value.chars().peekable();
    while let Some(character) = chars.next() {
        if character == '\u{1b}' {
            match chars.next() {
                Some('[') => {
                    for c in chars.by_ref() {
                        if ('@'..='~').contains(&c) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    while let Some(c) = chars.next() {
                        if c == '\u{7}' || (c == '\u{1b}' && chars.peek() == Some(&'\\')) {
                            if c == '\u{1b}' {
                                chars.next();
                            }
                            break;
                        }
                    }
                }
                _ => {}
            }
            continue;
        }
        if character.is_whitespace() {
            clean.push(' ');
        } else if !character.is_control()
            && !matches!(character, '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{206f}' | '\u{feff}')
        {
            clean.push(character);
        }
    }
    clean
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(limit)
        .collect()
}

fn malformed_error() -> Box<dyn std::error::Error + Send + Sync> {
    crate::tr!(
        "Codex 모델 목록 응답이 유효하지 않습니다 · 현재 설정은 유지됩니다",
        "Malformed Codex model catalog response; current settings are retained"
    )
    .into()
}
fn limit_error() -> Box<dyn std::error::Error + Send + Sync> {
    crate::tr!(
        "Codex 모델 목록의 크기 또는 페이지 제한을 초과했습니다 · 현재 설정은 유지됩니다",
        "Codex model catalog exceeded size or page limits; current settings are retained"
    )
    .into()
}
fn cancelled_error() -> Box<dyn std::error::Error + Send + Sync> {
    crate::tr!(
        "모델 목록 조회를 취소했습니다",
        "Model catalog discovery cancelled"
    )
    .into()
}

struct RpcProcess {
    child: Child,
    requests: Option<SyncSender<Vec<u8>>>,
    responses: Option<Receiver<AppResult<Value>>>,
    writer: Option<JoinHandle<()>>,
    stdout: Option<JoinHandle<()>>,
    stderr: Option<JoinHandle<()>>,
    overflow: Arc<AtomicBool>,
}

impl RpcProcess {
    fn spawn(command: &mut Command) -> AppResult<Self> {
        use std::os::unix::process::CommandExt;
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        let mut child = command.spawn().map_err(|error| {
            crate::tr!(
                format!("Codex 모델 목록 프로세스를 시작할 수 없습니다: {error}"),
                format!("Unable to start Codex model catalog process: {error}")
            )
        })?;
        let mut stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        let mut stderr = child.stderr.take().expect("piped stderr");
        let (request_tx, request_rx) = mpsc::sync_channel::<Vec<u8>>(8);
        let (response_tx, response_rx) = mpsc::sync_channel(8);
        let overflow = Arc::new(AtomicBool::new(false));
        let stdout_limit = Arc::clone(&overflow);
        let stderr_limit = Arc::clone(&overflow);
        Ok(Self {
            child,
            requests: Some(request_tx),
            responses: Some(response_rx),
            writer: Some(thread::spawn(move || {
                while let Ok(payload) = request_rx.recv() {
                    if stdin
                        .write_all(&payload)
                        .and_then(|()| stdin.flush())
                        .is_err()
                    {
                        break;
                    }
                }
            })),
            stdout: Some(thread::spawn(move || {
                read_messages(stdout, &response_tx, &stdout_limit);
            })),
            stderr: Some(thread::spawn(move || {
                let mut buffer = [0_u8; 8192];
                let mut size = 0;
                while let Ok(read) = stderr.read(&mut buffer) {
                    if read == 0 {
                        break;
                    }
                    size += read;
                    if size > MAX_STDERR_BYTES {
                        stderr_limit.store(true, Ordering::Relaxed);
                        break;
                    }
                }
            })),
            overflow,
        })
    }

    fn send(&self, value: &Value) -> AppResult<()> {
        let mut bytes = serde_json::to_vec(value)?;
        bytes.push(b'\n');
        self.requests
            .as_ref()
            .expect("request sender")
            .try_send(bytes)
            .map_err(|_| {
                crate::tr!(
                    "Codex 모델 목록 요청을 보낼 수 없습니다",
                    "Unable to send Codex model catalog request"
                )
                .into()
            })
    }

    fn response(
        &mut self,
        id: u64,
        deadline: Instant,
        cancelled: &impl Fn() -> bool,
    ) -> AppResult<Value> {
        loop {
            if cancelled() {
                return Err(cancelled_error());
            }
            if self.overflow.load(Ordering::Relaxed) {
                return Err(limit_error());
            }
            if Instant::now() >= deadline {
                return Err(crate::tr!(
                    "Codex 모델 목록 조회 시간 초과 · 현재 설정은 유지됩니다",
                    "Codex model catalog discovery timed out; current settings are retained"
                )
                .into());
            }
            match self
                .responses
                .as_ref()
                .expect("response receiver")
                .recv_timeout(Duration::from_millis(40))
            {
                Ok(message) => {
                    let message = message?;
                    if !message.is_object() {
                        return Err(malformed_error());
                    }
                    if message.get("id").is_none() {
                        continue;
                    } // Notifications are bounded by the stdout budget.
                    if message.get("id").and_then(Value::as_u64) != Some(id) {
                        return Err(malformed_error());
                    }
                    if let Some(error) = message.get("error") {
                        let code = error.get("code").and_then(Value::as_i64);
                        return Err(crate::tr!(
                            format!("Codex 모델 목록 조회 실패 (코드 {code:?}) · 현재 설정은 유지됩니다"),
                            format!("Codex model catalog failed (code {code:?}); current settings are retained")
                        ).into());
                    }
                    return message.get("result").cloned().ok_or_else(malformed_error);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(crate::tr!(
                        "Codex 모델 목록 프로세스가 응답 전에 종료되었습니다 · 현재 설정은 유지됩니다",
                        "Codex model catalog process exited before responding; current settings are retained"
                    ).into());
                }
            }
        }
    }
}

fn read_messages(
    mut stdout: impl Read,
    sender: &SyncSender<AppResult<Value>>,
    overflow: &AtomicBool,
) {
    let mut buffer = [0_u8; 8192];
    let mut line = Vec::new();
    let mut total = 0;
    while let Ok(read) = stdout.read(&mut buffer) {
        if read == 0 {
            break;
        }
        total += read;
        if total > MAX_STDOUT_BYTES {
            overflow.store(true, Ordering::Relaxed);
            return;
        }
        for byte in &buffer[..read] {
            if *byte == b'\n' {
                if !line.is_empty() {
                    let message = serde_json::from_slice(&line).map_err(|_| malformed_error());
                    let failed = message.is_err();
                    if sender.send(message).is_err() || failed {
                        return;
                    }
                    line.clear();
                }
            } else {
                line.push(*byte);
                if line.len() > MAX_LINE_BYTES {
                    overflow.store(true, Ordering::Relaxed);
                    return;
                }
            }
        }
    }
    if !line.is_empty() {
        let _ = sender.send(serde_json::from_slice(&line).map_err(|_| malformed_error()));
    }
}

impl Drop for RpcProcess {
    fn drop(&mut self) {
        // Separate process group: never target the user's existing pane or app-server.
        self.requests.take();
        self.responses.take();
        let _ = Command::new("/bin/kill")
            .arg("-KILL")
            .arg(format!("-{}", self.child.id()))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = self.child.kill();
        let _ = self.child.wait();
        for handle in [&mut self.writer, &mut self.stdout, &mut self.stderr] {
            if let Some(handle) = handle.take() {
                let _ = handle.join();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(efforts: &Value, default: &Value) -> RawModel {
        serde_json::from_value(json!({"id":"model-id","model":"model-id","displayName":"\u{1b}[31mShown\u{1b}[0m\n\u{202e}name","supportedReasoningEfforts":efforts,"defaultReasoningEffort":default,"hidden":false,"isDefault":true})).unwrap()
    }

    #[test]
    fn custom_catalog_or_provider_is_rejected_without_exposing_config_values() {
        assert!(
            validate_catalog_config(
                json!({"config":{"model_provider":"openai","unknown_secret":"never-print"}})
            )
            .is_ok()
        );
        for field in [
            "profile",
            "model_catalog_json",
            "openai_base_url",
            "chatgpt_base_url",
        ] {
            let error = validate_catalog_config(json!({"config":{field:"never-print"}}))
                .unwrap_err()
                .to_string();
            assert!(!error.contains("never-print"));
        }
        assert!(validate_catalog_config(json!({"config":{"model_provider":"alternate"}})).is_err());
    }

    #[test]
    fn serialized_public_chatgpt_default_matches_isolated_exec() {
        for url in [
            None,
            Some("https://chatgpt.com/backend-api/"),
            Some("https://chatgpt.com/backend-api"),
        ] {
            assert!(
                validate_catalog_config(
                    json!({"config":{"model_provider":"openai", "chatgpt_base_url":url}})
                )
                .is_ok()
            );
        }
        for url in [
            "https://private.invalid/SECRET",
            "https://chatgpt.com/backend-api/?SECRET",
            "https://chatgpt.com/backend-api/SECRET",
            "",
        ] {
            let error = validate_catalog_config(json!({"config":{"chatgpt_base_url":url}}))
                .unwrap_err()
                .to_string();
            if !url.is_empty() {
                assert!(!error.contains(url));
            }
            assert!(!error.contains("SECRET"));
        }
    }

    #[test]
    fn metadata_is_sanitized_and_nonreasoning_models_have_no_override() {
        let model = normalize(raw(&json!([]), &json!("medium"))).unwrap();
        assert_eq!(model.display_name, "Shown name");
        assert_eq!(model.default_reasoning_effort, None);
        assert!(
            normalize(raw(
                &json!([{"reasoningEffort":"high","description":"ok"}]),
                &json!("medium")
            ))
            .is_err()
        );
        assert!(
            normalize(raw(
                &json!([{"reasoningEffort":"x\";bad","description":"ok"}]),
                &json!("x\";bad")
            ))
            .is_err()
        );
    }

    #[test]
    fn timeout_and_cancellation_stop_descendants_with_open_pipes() {
        let mut command = Command::new("sh");
        command.args(["-c", "sh -c 'trap \"\" TERM; sleep 30' & wait"]);
        let started = Instant::now();
        assert!(discover_command(&mut command, Duration::from_millis(100), &|| false).is_err());
        assert!(started.elapsed() < Duration::from_secs(2));
        let mut command = Command::new("sh");
        command.args(["-c", "sh -c 'trap \"\" TERM; sleep 30' & wait"]);
        let started = Instant::now();
        assert!(
            discover_command(&mut command, Duration::from_secs(5), &|| started.elapsed()
                > Duration::from_millis(100))
            .is_err()
        );
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn oversized_lines_fail_without_unbounded_allocation() {
        let mut command = Command::new("python3");
        command.args([
            "-c",
            "import sys,time; sys.stdout.write('x'*600000); sys.stdout.flush(); time.sleep(10)",
        ]);
        assert!(discover_command(&mut command, Duration::from_secs(2), &|| false).is_err());
    }
}
