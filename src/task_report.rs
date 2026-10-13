//! Root-turn relay. Sandboxed tools write only a private temporary queue; an
//! existing host hook/panel verifies provenance before touching Maeklog state.
use crate::AppResult;
use crate::branch_context;
use crate::client_routing::Route;
use crate::model::{EventKind, TurnEvent};
use crate::storage::{SpaceStore, atomic_write_json, read_json_optional, unix_now};
use crate::task_links::{self, Report, ReportContext};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const HOST_FILE: &str = "task-report-host.json";
const MAX_BYTES: u64 = 16 * 1024;
const MAX_PENDING: usize = 4;
const MAX_RECEIPTS: usize = 8;
const TOKEN_TTL: u64 = 24 * 60 * 60;
const MESSAGE_TTL: u64 = 10 * 60;
const CONTEXT_WAIT: Duration = Duration::from_secs(2);

#[derive(Clone, Deserialize, Serialize)]
struct Host {
    spool: PathBuf,
    route: Route,
    thread_id: String,
    turn_id: String,
    source_uid: String,
    source_seq: u64,
    generation: u64,
    issued_at: u64,
}

#[derive(Deserialize, Serialize)]
struct Owner {
    space_key: String,
}

#[derive(Clone, Deserialize, Serialize)]
struct PublicContext {
    source_uid: String,
    source_seq: u64,
    thread_id: String,
    turn_id: String,
    generation: u64,
    expected_revision: u64,
    repository: Option<String>,
    current: Vec<String>,
    issued_at: u64,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Queued {
    caller_thread: String,
    sent_at: u64,
    report: Report,
}

#[derive(Deserialize, Serialize)]
struct Ack {
    status: String,
    receipt: Option<Value>,
    error: Option<String>,
}

fn random_id() -> AppResult<String> {
    use std::fmt::Write as _;
    let mut bytes = [0_u8; 32];
    File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    let mut nonce = String::with_capacity(64);
    for byte in bytes {
        write!(&mut nonce, "{byte:02x}")?;
    }
    Ok(nonce)
}

fn valid_hex(value: &str, length: usize) -> bool {
    value.len() == length && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn spool_parent(root: &Path) -> PathBuf {
    let temporary = std::env::temp_dir();
    fs::canonicalize(&temporary)
        .unwrap_or(temporary)
        .join(format!(
            "maeklog-task-reports-{}",
            branch_context::hash(&root.to_string_lossy())
        ))
}

fn checked_spool(path: &Path) -> AppResult<()> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    let parent = path.parent().ok_or(crate::tr!(
        "작업 보고 큐 경로 오류",
        "Invalid work report queue path"
    ))?;
    let prefix = parent
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    if !path.is_absolute() || !valid_hex(name, 64) || !prefix.starts_with("maeklog-task-reports-") {
        return Err(crate::tr!("작업 보고 큐 경로 오류", "Invalid work report queue path").into());
    }
    for directory in [parent, path] {
        let meta = fs::symlink_metadata(directory)?;
        if !meta.file_type().is_dir() || fs::canonicalize(directory)? != directory {
            return Err(crate::tr!(
                "작업 보고 큐는 실제 전용 디렉터리여야 합니다",
                "Work report queue must be a real dedicated directory"
            )
            .into());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if meta.permissions().mode() & 0o777 != 0o700 {
                return Err(crate::tr!(
                    "작업 보고 큐 권한 오류",
                    "Invalid work report queue permissions"
                )
                .into());
            }
        }
    }
    Ok(())
}

fn read_bounded<T: serde::de::DeserializeOwned>(path: &Path) -> AppResult<T> {
    if !fs::symlink_metadata(path)?.file_type().is_file() {
        return Err(crate::tr!(
            "작업 보고 파일 형식 오류",
            "Invalid work report file format"
        )
        .into());
    }
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(crate::tr!(
            "작업 보고는 16 KiB 이내여야 합니다",
            "Work report must be at most 16 KiB"
        )
        .into());
    }
    Ok(serde_json::from_slice(&bytes)?)
}

fn public_context(context: &ReportContext, issued_at: u64) -> AppResult<PublicContext> {
    Ok(PublicContext {
        source_uid: context.source_uid.clone(),
        source_seq: context.source_seq,
        thread_id: context.thread_id.clone(),
        turn_id: context.turn_id.clone().ok_or(crate::tr!(
            "작업 보고 turn 근거 없음",
            "No turn evidence for work report"
        ))?,
        generation: context.generation,
        expected_revision: context.expected_revision,
        repository: context.repository.clone(),
        current: context
            .current
            .iter()
            .map(|reference| reference.id.clone())
            .collect(),
        issued_at,
    })
}

fn queue_files(spool: &Path, prefix: &str) -> AppResult<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(spool)? {
        let path = entry?.path();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if let Some(id) = name
            .strip_prefix(prefix)
            .and_then(|name| name.strip_suffix(".json"))
            && valid_hex(id, 16)
        {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

fn retired(spool: &Path) -> AppResult<Option<String>> {
    if !spool.join("retired.json").exists() {
        return Ok(None);
    }
    let value: Value = read_bounded(&spool.join("retired.json"))?;
    Ok(Some(
        value["reason"]
            .as_str()
            .unwrap_or(crate::tr!("보고 경로 만료", "Report path expired"))
            .into(),
    ))
}

fn write_ack(spool: &Path, id: &str, ack: &Ack) -> AppResult<()> {
    atomic_write_json(&spool.join(format!("receipt-{id}.json")), ack)?;
    let mut receipts = queue_files(spool, "receipt-")?;
    receipts.sort_by_key(|path| fs::metadata(path).and_then(|m| m.modified()).ok());
    for path in receipts
        .iter()
        .take(receipts.len().saturating_sub(MAX_RECEIPTS))
    {
        fs::remove_file(path)?;
    }
    Ok(())
}

fn remove_if_present(path: &Path) -> AppResult<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn retire(host: &Host, reason: &str) -> AppResult<()> {
    checked_spool(&host.spool)?;
    let _queue = crate::storage::lock_file(&host.spool.join(".queue.lock"), false)?;
    atomic_write_json(
        &host.spool.join("retired.json"),
        &json!({"reason":reason,"retired_at":unix_now()}),
    )?;
    for path in queue_files(&host.spool, "request-")?
        .iter()
        .take(MAX_PENDING)
    {
        let id = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        let id = id.trim_start_matches("request-").trim_end_matches(".json");
        write_ack(
            &host.spool,
            id,
            &Ack {
                status: "rejected".into(),
                receipt: None,
                error: Some(reason.into()),
            },
        )?;
        remove_if_present(path)?;
    }
    Ok(())
}

/// Called after source capture while the hook owns the participation gate. No
/// GitHub/model work, global instructions or shell execution occurs here.
pub fn producer_context(
    store: &SpaceStore,
    route: &Route,
    event: &TurnEvent,
) -> AppResult<Option<String>> {
    if event.kind != EventKind::UserPrompt || event.truncated {
        return Ok(None);
    }
    let Some(context) = task_links::context_for_report_event(store, event)? else {
        return Ok(None);
    };
    crate::client_routing::validate_primary_route(&store.state_root, route, &context.thread_id)?;
    let old: Option<Host> = read_json_optional(&store.path(HOST_FILE))?;
    let host = if let Some(old) = old.filter(|old| {
        old.source_uid == context.source_uid
            && old.generation == context.generation
            && old.route == *route
            && unix_now().saturating_sub(old.issued_at) < TOKEN_TTL
            && checked_spool(&old.spool).is_ok()
            && retired(&old.spool).ok().flatten().is_none()
    }) {
        old
    } else {
        if let Some(old) = read_json_optional::<Host>(&store.path(HOST_FILE))? {
            let _ = retire(
                &old,
                crate::tr!(
                    "현재 사용자 요청이 바뀌었습니다",
                    "Current user request changed"
                ),
            );
        }
        let parent = crate::config::private_dir(spool_parent(&store.state_root))?;
        let spool = crate::config::private_dir(parent.join(random_id()?))?;
        Host {
            spool,
            route: route.clone(),
            thread_id: context.thread_id.clone(),
            turn_id: context.turn_id.clone().ok_or(crate::tr!(
                "작업 보고 turn 근거 없음",
                "No turn evidence for work report"
            ))?,
            source_uid: context.source_uid.clone(),
            source_seq: context.source_seq,
            generation: context.generation,
            issued_at: unix_now(),
        }
    };
    checked_spool(&host.spool)?;
    atomic_write_json(
        &host.spool.join("owner.json"),
        &Owner {
            space_key: store.key.clone(),
        },
    )?;
    atomic_write_json(
        &host.spool.join("context.json"),
        &public_context(&context, host.issued_at)?,
    )?;
    atomic_write_json(&store.path(HOST_FILE), &host)?;
    cleanup(&store.state_root, &host.spool)?;
    let executable = std::env::current_exe()?;
    let relay = json!({"spool":host.spool,"executable":executable,"source_uid":context.source_uid,
        "expected_revision":context.expected_revision,"thread_id":context.thread_id});
    Ok(Some(format!(
        "MAEKLOG_TASK_REPORT_CONTEXT_V1 {relay}\n\
         For this primary Codex turn only, relay known issue/PR work references to local Maeklog when selected, changed, resumed, or supplemented by a related PR.\n\
         Read current source/revision with the executable's `task context SPOOL`; send `task report SPOOL` JSON on stdin: {{source_uid,expected_revision,operation,references,quote,worktree,created}}.\n\
         operations: set/replace/resume/clear/related/reference; references are exact GitHub URLs, OWNER/REPO#N, or a number scoped by context.repository. quote is an exact clause from the current USER request (max600chars); worktree is optional. created is reported provenance, never proof of success.\n\
         Use set/replace/clear only for a direct user work choice; comparisons, quoted examples, past mentions and assistant guesses are reference candidates. Do not invent issue/PR relationships or completion. Relay child results yourself only after checking their relationship; children must not report or receive this routing context.\n\
         Report returns queued immediately. Before a later report in this turn, read task context for the latest receipt/revision; if pending or rejected, continue the coding task without retry loops or permission prompts. No report means no change. Do not call an extra model, run GitHub mutations, or edit global configuration for this relay."
    )))
}

fn validate_host(store: &SpaceStore, host: &Host) -> AppResult<ReportContext> {
    checked_spool(&host.spool)?;
    if host.spool.parent() != Some(spool_parent(&store.state_root).as_path())
        || unix_now().saturating_sub(host.issued_at) > TOKEN_TTL
    {
        return Err(crate::tr!("작업 보고 경로가 만료됐습니다", "Work report path expired").into());
    }
    crate::client_routing::validate_primary_route(&store.state_root, &host.route, &host.thread_id)?;
    let context = task_links::context_for_report(store)?.ok_or(crate::tr!(
        "현재 사용자 요청 근거 없음",
        "No current user-request evidence"
    ))?;
    if context.source_uid != host.source_uid
        || context.source_seq != host.source_seq
        || context.thread_id != host.thread_id
        || context.turn_id.as_deref() != Some(&host.turn_id)
        || context.generation != host.generation
        || task_links::active_thread(store)?.as_deref() != Some(&host.thread_id)
    {
        return Err(crate::tr!(
            "이전 대화 또는 요청의 작업 보고입니다",
            "Work report belongs to an earlier conversation or request"
        )
        .into());
    }
    let snapshot: crate::live_turn::Snapshot =
        read_json_optional(&store.path("live-turn.json"))?
            .ok_or(crate::tr!("현재 활성 turn 없음", "No active turn"))?;
    if snapshot.ended
        || snapshot.thread_id != host.thread_id
        || snapshot.turn_id != host.turn_id
        || snapshot.generation != host.generation
        || snapshot.source_prompt_seq != Some(host.source_seq)
    {
        return Err(crate::tr!(
            "작업 보고 turn이 끝났거나 바뀌었습니다",
            "Work report turn ended or changed"
        )
        .into());
    }
    Ok(context)
}

/// Existing host hooks/background refresh call this; render/history never do.
pub fn drain(store: &SpaceStore) -> AppResult<()> {
    let Some(_lock) = crate::storage::try_lock_file(&store.path(".task-report-drain.lock"))? else {
        return Ok(());
    };
    let Some(host): Option<Host> = read_json_optional(&store.path(HOST_FILE))? else {
        return Ok(());
    };
    if !host.spool.exists() {
        // Temporary queues disappear across reboot; the next prompt mints a new one.
        remove_if_present(&store.path(HOST_FILE))?;
        return Ok(());
    }
    checked_spool(&host.spool)?;
    let pending = queue_files(&host.spool, "request-")?;
    if pending.is_empty() {
        return Ok(());
    }
    // Hooks already hold this gate when retiring a previous source. Every host
    // path takes participation -> queue, while sandbox senders take only queue.
    let _gate = crate::participation::lock(store)?;
    let context = validate_host(store, &host);
    if let Err(error) = context {
        retire(&host, &error.to_string())?;
        return Ok(());
    }
    let _queue = crate::storage::lock_file(&host.spool.join(".queue.lock"), false)?;
    for path in queue_files(&host.spool, "request-")?
        .iter()
        .take(MAX_PENDING)
    {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        let id = name
            .trim_start_matches("request-")
            .trim_end_matches(".json");
        let accepted = (|| -> AppResult<Value> {
            let request: Queued = read_bounded(path)?;
            if request.caller_thread != host.thread_id
                || unix_now().saturating_sub(request.sent_at) > MESSAGE_TTL
            {
                return Err(crate::tr!(
                    "다른 에이전트 또는 만료된 작업 보고입니다",
                    "Work report belongs to another agent or has expired"
                )
                .into());
            }
            validate_host(store, &host)?;
            Ok(serde_json::to_value(task_links::accept_report_locked(
                store,
                &request.report,
            )?)?)
        })();
        let ack = match accepted {
            Ok(receipt) => Ack {
                status: "processed".into(),
                receipt: Some(receipt),
                error: None,
            },
            Err(error) => Ack {
                status: "rejected".into(),
                receipt: None,
                error: Some(error.to_string()),
            },
        };
        write_ack(&host.spool, id, &ack)?;
        remove_if_present(path)?;
    }
    if let Ok(context) = validate_host(store, &host) {
        atomic_write_json(
            &host.spool.join("context.json"),
            &public_context(&context, host.issued_at)?,
        )?;
    }
    Ok(())
}

fn cleanup(root: &Path, current: &Path) -> AppResult<()> {
    for entry in fs::read_dir(spool_parent(root))?.take(128) {
        let path = entry?.path();
        if path == current || checked_spool(&path).is_err() {
            continue;
        }
        if let Ok(context) = read_bounded::<PublicContext>(&path.join("context.json"))
            && unix_now().saturating_sub(context.issued_at) > TOKEN_TTL
        {
            fs::remove_dir_all(path)?;
        }
    }
    Ok(())
}

/// Explicit private deletion only. Caller holds the participation gate; ordinary
/// opt-out retains references. Maintenance removes the private host file itself.
pub fn clear_locked(store: &SpaceStore) -> AppResult<()> {
    let parent = spool_parent(&store.state_root);
    if !parent.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(parent)? {
        let path = entry?.path();
        if checked_spool(&path).is_err() {
            continue;
        }
        let Ok(owner) = read_bounded::<Owner>(&path.join("owner.json")) else {
            continue;
        };
        if owner.space_key != store.key {
            continue;
        }
        // The gate prevents this store minting another source. Senders only own
        // this queue lock; a racing sender finishes before its data is removed.
        let _queue = crate::storage::lock_file(&path.join(".queue.lock"), false)?;
        checked_spool(&path)?;
        fs::remove_dir_all(path)?;
    }
    Ok(())
}

fn cli_context(spool: &Path, caller_thread: &str) -> AppResult<PublicContext> {
    checked_spool(spool)?;
    if let Some(reason) = retired(spool)? {
        return Err(reason.into());
    }
    let context: PublicContext = read_bounded(&spool.join("context.json"))?;
    if caller_thread != context.thread_id {
        return Err(crate::tr!(
            "작업 보고는 현재 주 에이전트의 실행 thread에서만 가능합니다",
            "Work reports must come from the current primary agent’s running thread"
        )
        .into());
    }
    if unix_now().saturating_sub(context.issued_at) > TOKEN_TTL {
        return Err(crate::tr!("작업 보고 경로 만료", "Work report path expired").into());
    }
    Ok(context)
}

fn context_output(context: &PublicContext, pending: bool, receipts: &[Value]) -> AppResult<Value> {
    let mut output = serde_json::to_value(context)?;
    output["pending"] = json!(pending);
    output["receipts"] = json!(receipts);
    Ok(output)
}

fn context_snapshot(spool: &Path, initial: &PublicContext, wait: Duration) -> AppResult<Value> {
    let started = Instant::now();
    loop {
        if let Some(_queue) = crate::storage::try_lock_file(&spool.join(".queue.lock"))? {
            // Host drain keeps this lease through receipt creation, request
            // removal and the final revision commit. Never infer completion
            // from an empty directory while that commit is still in flight.
            let context = cli_context(spool, &initial.thread_id)?;
            let pending = !queue_files(spool, "request-")?.is_empty();
            if !pending || started.elapsed() >= wait {
                let receipts = queue_files(spool, "receipt-")?
                    .iter()
                    .filter_map(|path| read_bounded::<Value>(path).ok())
                    .collect::<Vec<_>>();
                return context_output(&context, pending, &receipts);
            }
        } else if started.elapsed() >= wait {
            // Preserve the bounded CLI contract if a host is busy; the caller
            // must not send another report using this unconfirmed revision.
            return context_output(initial, true, &[]);
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// Sandboxed CLI deliberately avoids `state_root()`, Herdr, Git and the network.
pub fn cli(arguments: &[String]) -> AppResult<()> {
    let [action, spool] = arguments else {
        return Err("usage: maeklog task <context|report> SPOOL (report JSON on stdin)".into());
    };
    let spool = Path::new(spool);
    let caller_thread = std::env::var("CODEX_THREAD_ID").unwrap_or_default();
    let initial = cli_context(spool, &caller_thread)?;
    match action.as_str() {
        "context" => {
            let output = context_snapshot(spool, &initial, CONTEXT_WAIT)?;
            println!("{output}");
        }
        "report" => {
            let mut bytes = Vec::new();
            std::io::stdin()
                .take(MAX_BYTES + 1)
                .read_to_end(&mut bytes)?;
            if bytes.len() as u64 > MAX_BYTES {
                return Err(crate::tr!(
                    "작업 보고는 16 KiB 이내여야 합니다",
                    "Work report must be at most 16 KiB"
                )
                .into());
            }
            let report: Report = serde_json::from_slice(&bytes)?;
            if report.source_uid != initial.source_uid {
                return Err(crate::tr!(
                    "이전 사용자 요청의 보고입니다",
                    "Report belongs to an earlier user request"
                )
                .into());
            }
            let queued = Queued {
                caller_thread: initial.thread_id,
                sent_at: unix_now(),
                report,
            };
            let payload = serde_json::to_string(&queued)?;
            if payload.len() as u64 > MAX_BYTES {
                return Err(crate::tr!(
                    "작업 보고는 16 KiB 이내여야 합니다",
                    "Work report must be at most 16 KiB"
                )
                .into());
            }
            let id = branch_context::hash(&serde_json::to_string(&queued.report)?);
            let _lock = crate::storage::lock_file(&spool.join(".queue.lock"), false)?;
            let _ = cli_context(spool, &caller_thread)?;
            let receipt = spool.join(format!("receipt-{id}.json"));
            if receipt.exists() {
                println!(
                    "{}",
                    serde_json::to_string(&read_bounded::<Ack>(&receipt)?)?
                );
            } else {
                let path = spool.join(format!("request-{id}.json"));
                if !path.exists() && queue_files(spool, "request-")?.len() >= MAX_PENDING {
                    return Err(crate::tr!(
                        "이미 작업 보고가 대기 중입니다 · 현재 작업을 계속하세요",
                        "A work report is already queued · Continue the current work"
                    )
                    .into());
                }
                if !path.exists() {
                    atomic_write_json(&path, &queued)?;
                }
                println!(
                    "{}",
                    json!({"status":"queued","ticket":id,"source_uid":queued.report.source_uid})
                );
            }
        }
        _ => return Err("unknown task relay action".into()),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Fixture {
        store: SpaceStore,
        host: Host,
    }

    impl Fixture {
        fn new() -> Self {
            static SERIAL: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "pm-report-test-{}-{}",
                std::process::id(),
                SERIAL.fetch_add(1, Ordering::Relaxed)
            ));
            let store = SpaceStore::for_workspace(
                &root,
                "report-test",
                "workspace",
                "/missing-report-repo",
                None,
            )
            .unwrap();
            let parent = crate::config::private_dir(spool_parent(&root)).unwrap();
            let spool = crate::config::private_dir(parent.join(random_id().unwrap())).unwrap();
            let host = Host {
                spool,
                route: Route {
                    session_name: "report-test".into(),
                    socket: "/missing-report-socket".into(),
                    herdr_bin: "/must-not-call-herdr".into(),
                    terminal_id: "terminal".into(),
                    pid: 1,
                    workspace_id: "workspace".into(),
                    tab_id: "tab".into(),
                },
                thread_id: "thread".into(),
                turn_id: "turn".into(),
                source_uid: "source".into(),
                source_seq: 1,
                generation: 0,
                issued_at: unix_now(),
            };
            atomic_write_json(
                &host.spool.join("owner.json"),
                &Owner {
                    space_key: store.key.clone(),
                },
            )
            .unwrap();
            atomic_write_json(&store.path(HOST_FILE), &host).unwrap();
            Self { store, host }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(spool_parent(&self.store.state_root));
            let _ = fs::remove_dir_all(&self.store.state_root);
        }
    }

    fn snapshot_context() -> PublicContext {
        PublicContext {
            source_uid: "source".into(),
            source_seq: 1,
            thread_id: "thread".into(),
            turn_id: "turn".into(),
            generation: 0,
            expected_revision: 2,
            repository: None,
            current: Vec::new(),
            issued_at: unix_now(),
        }
    }

    #[test]
    fn context_wait_is_bounded_and_never_claims_empty_queue_before_host_revision_commit() {
        let fixture = Fixture::new();
        let spool = &fixture.host.spool;
        let context = snapshot_context();
        atomic_write_json(&spool.join("context.json"), &context).unwrap();
        let queue = crate::storage::lock_file(&spool.join(".queue.lock"), false).unwrap();
        // Reproduce drain's previously observable intermediate state: request
        // already removed, receipt committed, context still the old revision.
        write_ack(
            spool,
            "0123456789abcdef",
            &Ack {
                status: "processed".into(),
                receipt: Some(json!({"revision":3})),
                error: None,
            },
        )
        .unwrap();
        let started = Instant::now();
        let busy = context_snapshot(spool, &context, Duration::from_millis(50)).unwrap();
        assert_eq!(busy["pending"], true);
        assert_eq!(busy["receipts"], json!([]));
        assert!(started.elapsed() < Duration::from_millis(500));
        let mut committed = context.clone();
        committed.expected_revision = 3;
        atomic_write_json(&spool.join("context.json"), &committed).unwrap();
        drop(queue);
        let done = context_snapshot(spool, &context, Duration::from_millis(50)).unwrap();
        assert_eq!(done["pending"], false);
        assert_eq!(done["expected_revision"], 3);
        assert_eq!(done["receipts"][0]["receipt"]["revision"], 3);
    }

    #[test]
    fn context_wait_reads_committed_revision_when_host_finishes_within_deadline() {
        let fixture = Fixture::new();
        let spool = fixture.host.spool.clone();
        let context = snapshot_context();
        atomic_write_json(&spool.join("context.json"), &context).unwrap();
        let queue = crate::storage::lock_file(&spool.join(".queue.lock"), false).unwrap();
        let reader_spool = spool.clone();
        let reader_context = context.clone();
        let reader = std::thread::spawn(move || {
            context_snapshot(&reader_spool, &reader_context, Duration::from_secs(1)).unwrap()
        });
        std::thread::sleep(Duration::from_millis(50));
        let mut committed = context;
        committed.expected_revision = 3;
        atomic_write_json(&spool.join("context.json"), &committed).unwrap();
        drop(queue);
        let done = reader.join().unwrap();
        assert_eq!(done["pending"], false);
        assert_eq!(done["expected_revision"], 3);
    }

    #[test]
    fn empty_queue_never_takes_participation_gate_or_queries_herdr() {
        let fixture = Fixture::new();
        // Opening this as a gate file would fail. The nonexistent Herdr path also
        // ensures no route probe is hidden in a no-report background tick.
        fs::create_dir(fixture.store.path(".participation.lock")).unwrap();
        drain(&fixture.store).unwrap();
        assert!(fixture.store.path(HOST_FILE).exists());
        assert!(!fixture.host.spool.join("retired.json").exists());
    }

    #[test]
    fn reboot_lost_temporary_spool_is_a_noop_and_clears_stale_host_record() {
        let fixture = Fixture::new();
        fs::remove_dir_all(&fixture.host.spool).unwrap();
        drain(&fixture.store).unwrap();
        assert!(!fixture.store.path(HOST_FILE).exists());
        drain(&fixture.store).unwrap();
    }

    #[test]
    fn retiring_a_source_terminates_pending_requests_with_rejected_receipts() {
        let fixture = Fixture::new();
        let id = "0123456789abcdef";
        let request = fixture.host.spool.join(format!("request-{id}.json"));
        atomic_write_json(&request, &json!({"fixture":"pending"})).unwrap();
        let _gate = crate::participation::lock(&fixture.store).unwrap();
        retire(&fixture.host, "새 사용자 요청").unwrap();
        assert!(!request.exists());
        let ack: Ack =
            read_bounded(&fixture.host.spool.join(format!("receipt-{id}.json"))).unwrap();
        assert_eq!(ack.status, "rejected");
        assert_eq!(ack.error.as_deref(), Some("새 사용자 요청"));
        assert!(retired(&fixture.host.spool).unwrap().is_some());
    }

    #[test]
    fn explicit_clear_removes_current_and_retired_owned_quotes_but_preserves_peer_spool() {
        let fixture = Fixture::new();
        let parent = spool_parent(&fixture.store.state_root);
        let old = crate::config::private_dir(parent.join(random_id().unwrap())).unwrap();
        let peer = crate::config::private_dir(parent.join(random_id().unwrap())).unwrap();
        for path in [&fixture.host.spool, &old, &peer] {
            atomic_write_json(
                &path.join("owner.json"),
                &Owner {
                    space_key: if *path == peer {
                        "different-peer-space".into()
                    } else {
                        fixture.store.key.clone()
                    },
                },
            )
            .unwrap();
            atomic_write_json(
                &path.join("request-0123456789abcdef.json"),
                &json!({"quote":"private user clause"}),
            )
            .unwrap();
        }
        atomic_write_json(&old.join("retired.json"), &json!({"reason":"new prompt"})).unwrap();
        let peer_before = fs::read(peer.join("request-0123456789abcdef.json")).unwrap();
        let _gate = crate::participation::lock(&fixture.store).unwrap();
        clear_locked(&fixture.store).unwrap();
        assert!(!fixture.host.spool.exists());
        assert!(!old.exists());
        assert_eq!(
            fs::read(peer.join("request-0123456789abcdef.json")).unwrap(),
            peer_before
        );
        clear_locked(&fixture.store).unwrap();
    }
}
