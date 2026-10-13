//! Read-only GitHub metadata cache. Fetches run on a background caller, never a renderer.
use crate::storage::{
    SpaceStore, atomic_write_json, lock_file, read_json_or_default, try_lock_file, unix_now,
};
use crate::{AppResult, branch_context as context};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

const CACHE_TTL: u64 = 60;
const HEAD_REFRESH_INTERVAL: u64 = 10;
const REFRESH_BUDGET: Duration = Duration::from_secs(30);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_CANDIDATES: usize = 100;
const MAX_METADATA_FETCHES: usize = 6;
const PR_LIST_FIELDS: &str = "number,url,title,state,headRefName,headRefOid,headRepository,headRepositoryOwner,isCrossRepository";
const PR_FIELDS: &str = "number,url,title,body,state,headRefName,headRefOid,headRepository,headRepositoryOwner,isCrossRepository,closingIssuesReferences";

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Issue,
    Pr,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Link {
    pub kind: Kind,
    pub repository: String,
    pub number: u64,
    pub url: String,
    pub title: Option<String>,
    pub purpose: Option<String>,
    pub body: Option<String>,
    pub checked_at_unix_utc: Option<u64>,
    #[serde(default)]
    pub association_checked_at_unix_utc: Option<u64>,
    pub error: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub head_ref_name: Option<String>,
    #[serde(default)]
    pub head_oid: Option<String>,
    #[serde(default)]
    pub head_repository: Option<String>,
    #[serde(default)]
    pub closing_issues: Vec<String>,
    #[serde(default)]
    pub closing_issues_error: Option<String>,
}
impl Link {
    pub fn id(&self) -> String {
        format!("{:?}:{}#{}", self.kind, self.repository, self.number)
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionState {
    #[default]
    Unchecked,
    Checking,
    Linked,
    None,
    Ambiguous,
    Error,
}

#[derive(Clone, Debug)]
pub enum Selection {
    Primary(String),
    None,
    Auto,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefreshOutcome {
    Refreshed,
    Skipped,
    Busy,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "Existing cache flags and explicit selection provenance remain backward compatible"
)]
pub struct Links {
    pub entries: BTreeMap<String, Link>,
    pub primary_issue: Option<String>,
    pub primary_pr: Option<String>,
    pub pr_not_created: bool,
    pub discovery_checked_at_unix_utc: Option<u64>,
    pub discovery_error: Option<String>,
    pub repository: Option<String>,
    pub issue_not_linked: bool,
    pub issue_state: ResolutionState,
    pub pr_state: ResolutionState,
    pub issue_candidates: Vec<String>,
    pub pr_candidates: Vec<String>,
    pub automatic_primary_issue: bool,
    pub automatic_primary_pr: bool,
    pub manual_issue: bool,
    pub manual_pr: bool,
    pub manual_entries: BTreeSet<String>,
    pub discovery_scope_fingerprint: Option<String>,
    pub discovery_head: Option<String>,
    pub refresh_started_at_unix_utc: Option<u64>,
    pub last_refresh_attempt_at_unix_utc: Option<u64>,
    pub next_refresh_at_unix_utc: Option<u64>,
    pub consecutive_failures: u32,
    pub selection_revision: u64,
    pub discovery_unavailable: bool,
    schema_version: u8,
    issues_for_pr: Option<String>,
}
impl Links {
    pub fn primary(&self, kind: Kind) -> Option<&Link> {
        self.primary_id(kind)
            .and_then(|id| self.entries.get(id))
            .filter(|link| link.kind == kind)
    }

    pub fn candidates(&self, kind: Kind) -> Vec<&Link> {
        let mut seen = BTreeSet::new();
        self.primary_id(kind)
            .into_iter()
            .chain(&self.manual_entries)
            .chain(self.candidate_ids(kind))
            .filter_map(|id| self.entries.get(id))
            .filter(|link| link.kind == kind)
            .filter(|link| seen.insert(link.id().to_ascii_lowercase()))
            .collect()
    }

    pub fn resolution(&self, kind: Kind) -> ResolutionState {
        if self.explicit_none(kind) {
            return ResolutionState::None;
        }
        let state = self.state(kind);
        if state == ResolutionState::Checking
            && self
                .refresh_started_at_unix_utc
                .is_some_and(|at| unix_now().saturating_sub(at) <= REFRESH_BUDGET.as_secs() + 10)
        {
            return ResolutionState::Checking;
        }
        if let Some(link) = self.primary(kind) {
            return if link.error.is_some() || state == ResolutionState::Error {
                ResolutionState::Error
            } else {
                ResolutionState::Linked
            };
        }
        if state == ResolutionState::Checking {
            return ResolutionState::Unchecked;
        }
        if state == ResolutionState::Unchecked
            && self.discovery_error.is_some()
            && !self.manual(kind)
        {
            return ResolutionState::Error;
        }
        state
    }

    pub fn is_manual(&self, kind: Kind) -> bool {
        self.manual(kind)
    }

    pub fn error(&self, kind: Kind) -> Option<&str> {
        if self.resolution(kind) != ResolutionState::Error {
            return None;
        }
        self.primary(kind)
            .and_then(|link| link.error.as_deref())
            .or_else(|| {
                (kind == Kind::Issue)
                    .then(|| self.primary(Kind::Pr))
                    .flatten()
                    .and_then(|pr| pr.closing_issues_error.as_deref().or(pr.error.as_deref()))
            })
            .or(self.discovery_error.as_deref())
    }

    fn primary_id(&self, kind: Kind) -> Option<&String> {
        match kind {
            Kind::Issue => self.primary_issue.as_ref(),
            Kind::Pr => self.primary_pr.as_ref(),
        }
    }
    fn candidate_ids(&self, kind: Kind) -> &[String] {
        match kind {
            Kind::Issue => &self.issue_candidates,
            Kind::Pr => &self.pr_candidates,
        }
    }
    fn state(&self, kind: Kind) -> ResolutionState {
        match kind {
            Kind::Issue => self.issue_state,
            Kind::Pr => self.pr_state,
        }
    }
    fn manual(&self, kind: Kind) -> bool {
        match kind {
            Kind::Issue => self.manual_issue,
            Kind::Pr => self.manual_pr,
        }
    }
    fn explicit_none(&self, kind: Kind) -> bool {
        match kind {
            Kind::Issue => self.issue_not_linked,
            Kind::Pr => self.pr_not_created,
        }
    }

    fn reconcile_scope(&mut self, scope: &context::Context) {
        // Old caches cannot distinguish user selections from automatic selections.
        // Preserve those choices; all new automatic associations carry provenance.
        if self.schema_version == 0 {
            self.manual_issue = self.primary_issue.is_some() || self.issue_not_linked;
            self.manual_pr = self.primary_pr.is_some() || self.pr_not_created;
            self.manual_entries.extend(self.entries.keys().cloned());
            self.schema_version = 1;
        }
        let fingerprint = scope_fingerprint(scope);
        if self.discovery_scope_fingerprint.as_ref() != Some(&fingerprint) {
            self.reset_discovery();
            self.discovery_scope_fingerprint = Some(fingerprint);
        }
    }

    fn reset_discovery(&mut self) {
        if self.automatic_primary_issue {
            self.primary_issue = None;
        }
        if self.automatic_primary_pr {
            self.primary_pr = None;
        }
        self.automatic_primary_issue = false;
        self.automatic_primary_pr = false;
        self.repository = None;
        self.issue_candidates.clear();
        self.pr_candidates.clear();
        self.issues_for_pr = None;
        self.issue_state = ResolutionState::Unchecked;
        self.pr_state = ResolutionState::Unchecked;
        self.discovery_checked_at_unix_utc = None;
        self.discovery_error = None;
        self.discovery_head = None;
        self.refresh_started_at_unix_utc = None;
        self.last_refresh_attempt_at_unix_utc = None;
        self.next_refresh_at_unix_utc = None;
        self.consecutive_failures = 0;
        self.discovery_unavailable = false;
    }
}

fn scope_fingerprint(scope: &context::Context) -> String {
    // Local HEAD changes do not change a branch's PR identity.
    context::hash(&format!(
        "{}\0{:?}\0{:?}\0{:?}",
        scope.key, scope.repository, scope.branch, scope.incarnation,
    ))
}

pub fn load(root: &Path, scope: &context::Context) -> AppResult<Links> {
    let mut links: Links =
        read_json_or_default(&context::directory(root, scope).join("links.json"))?;
    links.reconcile_scope(scope);
    Ok(links)
}

fn valid_repo(repo: &str) -> bool {
    let parts: Vec<_> = repo.split('/').collect();
    parts.len() == 2
        && parts.iter().all(|p| {
            !p.is_empty()
                && *p != "."
                && *p != ".."
                && p.bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
        })
}

pub fn parse(kind: Kind, value: &str) -> AppResult<Link> {
    let (repo, number) = if let Some(url) = value.strip_prefix("https://github.com/") {
        let parts: Vec<_> = url.trim_end_matches('/').split('/').collect();
        if parts.len() != 4
            || parts[2]
                != (if kind == Kind::Issue {
                    "issues"
                } else {
                    "pull"
                })
        {
            return Err("expected an exact GitHub issue/PR URL".into());
        }
        (
            format!("{}/{}", parts[0], parts[1]),
            parts[3].parse::<u64>()?,
        )
    } else {
        let (repo, n) = value
            .rsplit_once('#')
            .ok_or("use owner/repository#number, not a bare number")?;
        (repo.into(), n.parse::<u64>()?)
    };
    if !valid_repo(&repo) || number == 0 {
        return Err("invalid repository or issue/PR number".into());
    }
    let url = format!(
        "https://github.com/{}/{}/{}",
        repo,
        if kind == Kind::Issue {
            "issues"
        } else {
            "pull"
        },
        number
    );
    Ok(Link {
        kind,
        repository: repo,
        number,
        url,
        title: None,
        purpose: None,
        body: None,
        checked_at_unix_utc: None,
        association_checked_at_unix_utc: None,
        error: None,
        state: None,
        head_ref_name: None,
        head_oid: None,
        head_repository: None,
        closing_issues: Vec::new(),
        closing_issues_error: None,
    })
}

pub fn parse_scoped(kind: Kind, input: &str, repository: Option<&str>) -> AppResult<Link> {
    let input = input.trim();
    let number = input.strip_prefix('#').unwrap_or(input);
    if !number.is_empty() && number.bytes().all(|c| c.is_ascii_digit()) {
        let repository = repository
            .filter(|repo| valid_repo(repo))
            .ok_or(crate::tr!(
                "GitHub 저장소 미확인 · owner/repository#번호 또는 정확한 URL을 입력하세요",
                "GitHub repository unknown · Enter owner/repository#N or an exact URL"
            ))?;
        return parse(kind, &format!("{repository}#{number}"));
    }
    parse(kind, input)
}

pub fn remote_repository(remote: &str) -> Option<String> {
    let path = if let Some(path) = remote.strip_prefix("git@github.com:") {
        path
    } else if let Some(rest) = remote.strip_prefix("https://") {
        let (host, path) = rest.split_once('/')?;
        (host.rsplit('@').next()? == "github.com").then_some(path)?
    } else {
        remote.strip_prefix("ssh://git@github.com/")?
    };
    let path = path.trim_end_matches('/').trim_end_matches(".git");
    valid_repo(path).then(|| path.to_string())
}

/// Bounded local Git lookup. Do not call from pure rendering.
pub fn github_repository(scope: &context::Context) -> Option<String> {
    let result = crate::process::run_with_timeout(
        Command::new("git").current_dir(&scope.worktree).args([
            "config",
            "--get",
            "remote.origin.url",
        ]),
        None,
        Duration::from_secs(2),
        16384,
    )
    .ok()?;
    result
        .success
        .then(|| remote_repository(result.stdout.trim()))?
}

fn same_current_scope(scope: &context::Context) -> bool {
    let current = context::detect(&scope.worktree, &scope.key);
    current.repository == scope.repository
        && current.branch == scope.branch
        && current.incarnation == scope.incarnation
}

fn same_repository(left: Option<&str>, right: Option<&str>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => left.eq_ignore_ascii_case(right),
        (None, None) => true,
        _ => false,
    }
}

fn gh(args: &[&str], timeout: Duration) -> AppResult<serde_json::Value> {
    let executable = std::env::var_os("MAEKLOG_GH_BIN").unwrap_or_else(|| "gh".into());
    let result = crate::process::run_with_timeout(
        Command::new(executable)
            .args(args)
            .env("GH_PROMPT_DISABLED", "1")
            .env("GH_HOST", "github.com")
            .env("GH_PAGER", "cat")
            .env("GH_BROWSER", "false"),
        None,
        timeout,
        1024 * 1024,
    )?;
    if result.timed_out {
        return Err(crate::tr!(
            "GitHub 조회 시간 초과 · 마지막 성공 캐시 유지",
            "GitHub lookup timed out · Last successful cache retained"
        )
        .into());
    }
    if !result.success {
        return Err(crate::tr!(
            "GitHub 조회 실패 · 인증/네트워크/접근 권한을 확인하세요",
            "GitHub lookup failed · Check authentication, network, and access"
        )
        .into());
    }
    serde_json::from_str(&result.stdout).map_err(|_| {
        crate::tr!(
            "GitHub 응답 형식 오류 · 마지막 성공 캐시 유지",
            "Invalid GitHub response · Last successful cache retained"
        )
        .into()
    })
}

fn fetch(args: &[&str], deadline: Instant) -> AppResult<serde_json::Value> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(crate::tr!(
            "GitHub 조회 시간 제한 · 마지막 성공 캐시 유지",
            "GitHub lookup time limit reached · Last successful cache retained"
        )
        .into());
    }
    gh(args, remaining.min(COMMAND_TIMEOUT))
}

/// Exact read-only lookup for a conversation reference. GitHub's issue endpoint
/// identifies PRs too, so a bare number does not require a kind selection.
pub fn lookup_reference(
    repository: &str,
    number: u64,
    claimed_kind: Option<Kind>,
    deadline: Instant,
) -> AppResult<Link> {
    lookup_reference_with(repository, number, claimed_kind, deadline, &mut fetch)
}

fn lookup_reference_with(
    repository: &str,
    number: u64,
    claimed_kind: Option<Kind>,
    deadline: Instant,
    request: &mut impl FnMut(&[&str], Instant) -> AppResult<serde_json::Value>,
) -> AppResult<Link> {
    let expected = parse(Kind::Issue, &format!("{repository}#{number}"))?;
    let endpoint = format!("repos/{repository}/issues/{number}");
    let response = request(&["api", "--method", "GET", &endpoint], deadline)?;
    let kind = if response
        .get("pull_request")
        .is_some_and(serde_json::Value::is_object)
    {
        Kind::Pr
    } else {
        Kind::Issue
    };
    if claimed_kind.is_some_and(|claimed| claimed != kind) {
        return Err(crate::tr!(
            "사용자가 지정한 종류와 GitHub 참조가 다릅니다",
            "Selected reference type differs from GitHub"
        )
        .into());
    }
    let url = response["html_url"]
        .as_str()
        .ok_or(crate::tr!(
            "GitHub 참조 URL 응답 누락",
            "GitHub reference URL missing from response"
        ))?
        .to_string();
    let link = parse(kind, &url)?;
    if response["number"].as_u64() != Some(number)
        || link.number != number
        || !link.repository.eq_ignore_ascii_case(&expected.repository)
    {
        return Err(crate::tr!(
            "GitHub 참조의 저장소·번호가 변경되었습니다 · 직접 확인 필요",
            "GitHub reference repository/number changed · Manual check needed"
        )
        .into());
    }
    if kind == Kind::Pr {
        let pr_url = response["pull_request"]["html_url"]
            .as_str()
            .ok_or(crate::tr!(
                "GitHub PR URL 응답 누락",
                "GitHub PR URL missing from response"
            ))?;
        let pr = parse(Kind::Pr, pr_url)?;
        if pr.number != number || !pr.repository.eq_ignore_ascii_case(repository) {
            return Err(crate::tr!(
                "GitHub PR 참조의 저장소·번호 불일치",
                "GitHub PR repository/number mismatch"
            )
            .into());
        }
        return fetch_link(&link, deadline, request).map_err(Into::into);
    }
    let mut value = response;
    value["url"] = serde_json::Value::String(url);
    if value["body"].is_null() {
        value["body"] = serde_json::Value::String(String::new());
    }
    value["state"] = serde_json::Value::String(
        value["state"]
            .as_str()
            .ok_or(crate::tr!(
                "GitHub 상태 응답 누락",
                "GitHub status missing from response"
            ))?
            .to_ascii_uppercase(),
    );
    metadata(link, &value, unix_now())
}

fn excerpt(body: &str, kind: Kind) -> Option<String> {
    let headings = if kind == Kind::Issue {
        ["목적", "문제", "결과", "purpose", "problem"]
    } else {
        ["범위", "변경", "scope", "changes", "무엇이"]
    };
    let lines: Vec<_> = body.lines().collect();
    let start = lines
        .iter()
        .position(|line| {
            line.starts_with('#') && headings.iter().any(|h| line.to_lowercase().contains(h))
        })
        .map_or(0, |i| i + 1);
    let text = lines[start..]
        .iter()
        .skip_while(|l| l.trim().is_empty())
        .take_while(|l| !l.starts_with('#'))
        .filter(|l| !l.trim().is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join(" ");
    (!text.is_empty()).then(|| text.chars().take(1000).collect())
}

fn head_repository(value: &serde_json::Value) -> Option<String> {
    let repository = &value["headRepository"];
    if let Some(name) = repository["nameWithOwner"]
        .as_str()
        .filter(|name| valid_repo(name))
    {
        let (owner, name_part) = name.split_once('/')?;
        if repository["name"]
            .as_str()
            .is_some_and(|actual| !actual.eq_ignore_ascii_case(name_part))
            || value["headRepositoryOwner"]["login"]
                .as_str()
                .is_some_and(|actual| !actual.eq_ignore_ascii_case(owner))
        {
            return None;
        }
        return Some(name.to_string());
    }
    let name = repository["name"].as_str()?;
    let owner = value["headRepositoryOwner"]["login"].as_str()?;
    let repo = format!("{owner}/{name}");
    valid_repo(&repo).then_some(repo)
}

fn validated_link(kind: Kind, value: &serde_json::Value) -> AppResult<Link> {
    let link = parse(
        kind,
        value["url"].as_str().ok_or(crate::tr!(
            "GitHub URL 응답 누락",
            "GitHub URL missing from response"
        ))?,
    )?;
    if value["number"].as_u64() != Some(link.number) {
        return Err(crate::tr!(
            "응답의 저장소·번호가 요청과 다름",
            "Response repository/number differs from request"
        )
        .into());
    }
    Ok(link)
}

fn closing_issues(value: &serde_json::Value) -> AppResult<Vec<Link>> {
    let issues = value["closingIssuesReferences"]
        .as_array()
        .ok_or(crate::tr!(
            "PR의 연결 이슈 응답 누락",
            "PR’s linked issues missing from response"
        ))?;
    let mut result = BTreeMap::new();
    for issue in issues {
        let link = parse(
            Kind::Issue,
            issue["url"]
                .as_str()
                .ok_or(crate::tr!("연결 이슈 URL 누락", "Linked issue URL missing"))?,
        )?;
        if issue.get("number").is_some() && issue["number"].as_u64() != Some(link.number) {
            return Err(crate::tr!(
                "연결 이슈의 URL·번호 불일치",
                "Linked issue URL/number mismatch"
            )
            .into());
        }
        if let Some(repository) = issue.get("repository") {
            let name = repository["nameWithOwner"]
                .as_str()
                .map(String::from)
                .or_else(|| {
                    Some(format!(
                        "{}/{}",
                        repository["owner"]["login"].as_str()?,
                        repository["name"].as_str()?
                    ))
                })
                .ok_or(crate::tr!(
                    "연결 이슈 저장소 응답 오류",
                    "Invalid linked issue repository response"
                ))?;
            if !name.eq_ignore_ascii_case(&link.repository) {
                return Err(crate::tr!(
                    "연결 이슈의 URL·저장소 불일치",
                    "Linked issue URL/repository mismatch"
                )
                .into());
            }
        }
        result.insert(link.id(), link);
    }
    Ok(result.into_values().collect())
}

fn metadata(mut link: Link, value: &serde_json::Value, now: u64) -> AppResult<Link> {
    let checked = validated_link(link.kind, value)?;
    if checked.number != link.number || !checked.repository.eq_ignore_ascii_case(&link.repository) {
        return Err(crate::tr!(
            "응답의 저장소·번호가 요청과 다름",
            "Response repository/number differs from request"
        )
        .into());
    }
    link.title = Some(
        value["title"]
            .as_str()
            .ok_or(crate::tr!(
                "GitHub 제목 응답 누락",
                "GitHub title missing from response"
            ))?
            .chars()
            .take(2000)
            .collect(),
    );
    link.body = Some(
        value["body"]
            .as_str()
            .ok_or(crate::tr!(
                "GitHub 본문 응답 누락",
                "GitHub body missing from response"
            ))?
            .chars()
            .take(32000)
            .collect(),
    );
    link.purpose = link.body.as_deref().and_then(|b| excerpt(b, link.kind));
    let state = value["state"].as_str().ok_or(crate::tr!(
        "GitHub 상태 응답 누락",
        "GitHub status missing from response"
    ))?;
    if !["OPEN", "CLOSED", "MERGED"].contains(&state)
        || link.kind == Kind::Issue && state == "MERGED"
    {
        return Err(crate::tr!(
            "GitHub 상태 응답 형식 오류",
            "Invalid GitHub status response"
        )
        .into());
    }
    link.state = Some(state.to_string());
    if link.kind == Kind::Pr {
        link.head_ref_name = value["headRefName"].as_str().map(String::from);
        link.head_oid = value["headRefOid"].as_str().map(String::from);
        link.head_repository = head_repository(value);
        match closing_issues(value) {
            Ok(issues) => {
                link.closing_issues = issues.iter().map(Link::id).collect();
                link.closing_issues_error = None;
            }
            Err(error) => link.closing_issues_error = Some(error.to_string()),
        }
    }
    link.checked_at_unix_utc = Some(now);
    link.error = None;
    Ok(link)
}

struct Discovery {
    prs: BTreeMap<String, Link>,
    open: Vec<String>,
}

fn discover(
    scope: &context::Context,
    repo: &str,
    value: &serde_json::Value,
    now: u64,
) -> AppResult<Discovery> {
    let candidates = value.as_array().ok_or(crate::tr!(
        "GitHub PR 목록 응답 형식 오류",
        "Invalid GitHub PR list response"
    ))?;
    if candidates.len() >= MAX_CANDIDATES {
        return Err(crate::tr!(
            "PR 후보 조회 한도 도달 · 정확한 저장소·번호로 연결을 지정하세요",
            "PR candidate lookup limit reached · Set an exact repository/number"
        )
        .into());
    }
    let mut result = Discovery {
        prs: BTreeMap::new(),
        open: Vec::new(),
    };
    for candidate in candidates.iter().take(MAX_CANDIDATES) {
        let mut link = validated_link(Kind::Pr, candidate)?;
        if !link.repository.eq_ignore_ascii_case(repo) {
            continue;
        }
        let branch = candidate["headRefName"].as_str().ok_or(crate::tr!(
            "PR 브랜치 응답 누락",
            "PR branch missing from response"
        ))?;
        if Some(branch) != scope.branch.as_deref() {
            continue;
        }
        if candidate["isCrossRepository"].as_bool() == Some(true) {
            continue;
        }
        let source = head_repository(candidate).ok_or(crate::tr!(
            "PR의 원본 저장소 응답 미확인",
            "PR source repository missing from response"
        ))?;
        if !source.eq_ignore_ascii_case(repo) {
            continue;
        }
        let state = candidate["state"].as_str().ok_or(crate::tr!(
            "PR 상태 응답 누락",
            "PR status missing from response"
        ))?;
        if !["OPEN", "CLOSED", "MERGED"].contains(&state) {
            return Err(crate::tr!(
                "GitHub PR 상태 응답 형식 오류",
                "Invalid GitHub PR status response"
            )
            .into());
        }
        let id = link.id();
        link.title = Some(
            candidate["title"]
                .as_str()
                .ok_or(crate::tr!(
                    "PR 제목 응답 누락",
                    "PR title missing from response"
                ))?
                .chars()
                .take(2000)
                .collect(),
        );
        link.state = Some(state.to_string());
        link.head_ref_name = Some(branch.to_string());
        link.head_oid = candidate["headRefOid"].as_str().map(String::from);
        link.head_repository = Some(source);
        link.association_checked_at_unix_utc = Some(now);
        if state == "OPEN" && !result.open.contains(&id) {
            result.open.push(id.clone());
        }
        result.prs.insert(id, link);
    }
    Ok(result)
}

fn apply_discovery(links: &mut Links, discovery: &Discovery) {
    links.pr_candidates = discovery.prs.keys().cloned().collect();
    for (id, association) in &discovery.prs {
        let link = links
            .entries
            .entry(id.clone())
            .or_insert_with(|| association.clone());
        link.title.clone_from(&association.title);
        link.state.clone_from(&association.state);
        link.head_ref_name.clone_from(&association.head_ref_name);
        link.head_oid.clone_from(&association.head_oid);
        link.head_repository
            .clone_from(&association.head_repository);
        link.association_checked_at_unix_utc = association.association_checked_at_unix_utc;
    }
    if !links.manual_pr {
        links.primary_pr = if discovery.open.len() == 1 {
            discovery.open.first().cloned()
        } else {
            None
        };
        links.automatic_primary_pr = links.primary_pr.is_some();
        links.pr_state = if discovery.open.len() > 1 {
            ResolutionState::Ambiguous
        } else if links.primary_pr.is_some() {
            ResolutionState::Linked
        } else {
            ResolutionState::None
        };
    }
}

fn apply_issues(links: &mut Links, pr_id: Option<&str>, issues: Option<&[Link]>) {
    links.issues_for_pr = pr_id.map(String::from);
    links.issue_candidates = issues.unwrap_or_default().iter().map(Link::id).collect();
    for issue in issues.unwrap_or_default() {
        links
            .entries
            .entry(issue.id())
            .or_insert_with(|| issue.clone());
    }
    if !links.manual_issue {
        links.primary_issue = if let Some([issue]) = issues {
            Some(issue.id())
        } else {
            None
        };
        links.automatic_primary_issue = links.primary_issue.is_some();
        links.issue_state = match issues {
            Some([_]) => ResolutionState::Linked,
            Some([]) => ResolutionState::None,
            Some(_) => ResolutionState::Ambiguous,
            None if links.pr_state == ResolutionState::None || links.pr_not_created => {
                ResolutionState::None
            }
            None => ResolutionState::Unchecked,
        };
    }
}

fn due(links: &Links, scope: &context::Context, now: u64) -> bool {
    if links
        .refresh_started_at_unix_utc
        .is_some_and(|at| now.saturating_sub(at) > REFRESH_BUDGET.as_secs() + 10)
    {
        return true;
    }
    if links.discovery_unavailable
        && links.next_refresh_at_unix_utc != Some(0)
        && links.discovery_head == scope.head
        && links.primary_pr.is_none()
        && links.primary_issue.is_none()
        && links.manual_entries.is_empty()
    {
        return false;
    }
    if links
        .next_refresh_at_unix_utc
        .is_none_or(|next| now >= next)
    {
        return true;
    }
    links.discovery_head != scope.head
        && links.consecutive_failures == 0
        && links
            .last_refresh_attempt_at_unix_utc
            .is_none_or(|at| now.saturating_sub(at) >= HEAD_REFRESH_INTERVAL)
}

/// This is a bounded synchronous worker operation; dispatch it outside the UI thread.
pub fn refresh_if_due(root: &Path, scope: &context::Context) -> AppResult<RefreshOutcome> {
    refresh_inner(root, scope, false)
}

pub fn refresh(root: &Path, scope: &context::Context) -> AppResult<()> {
    refresh_forced(root, scope).map(|_| ())
}

pub fn refresh_forced(root: &Path, scope: &context::Context) -> AppResult<RefreshOutcome> {
    refresh_inner(root, scope, true)
}

fn refresh_inner(root: &Path, scope: &context::Context, force: bool) -> AppResult<RefreshOutcome> {
    refresh_with(root, scope, force, fetch)
}

fn refresh_with(
    root: &Path,
    scope: &context::Context,
    force: bool,
    mut request: impl FnMut(&[&str], Instant) -> AppResult<serde_json::Value>,
) -> AppResult<RefreshOutcome> {
    let dir = context::directory(root, scope);
    let Some(_fetch_lock) = try_lock_file(&dir.join(".links-fetch.lock"))? else {
        return Ok(RefreshOutcome::Busy);
    };
    let mut initial = load(root, scope)?;
    let now = unix_now();
    // Origin can change without a commit. This bounded local lookup happens before
    // TTL/unavailable skips, outside the UI thread and without GitHub traffic.
    let repository = github_repository(scope);
    if !force
        && same_repository(initial.repository.as_deref(), repository.as_deref())
        && !due(&initial, scope, now)
    {
        return Ok(RefreshOutcome::Skipped);
    }
    if !same_current_scope(scope) {
        return Ok(RefreshOutcome::Skipped);
    }
    {
        let _lock = lock_file(&dir.join(".links.lock"), false)?;
        initial = load(root, scope)?;
        if !same_repository(initial.repository.as_deref(), repository.as_deref()) {
            initial.reset_discovery();
        }
        initial.repository.clone_from(&repository);
        initial.refresh_started_at_unix_utc = Some(now);
        initial.last_refresh_attempt_at_unix_utc = Some(now);
        if !initial.issue_not_linked {
            initial.issue_state = ResolutionState::Checking;
        }
        if !initial.pr_not_created {
            initial.pr_state = ResolutionState::Checking;
        }
        atomic_write_json(&dir.join("links.json"), &initial)?;
        if !dir.join("context.json").exists() {
            atomic_write_json(&dir.join("context.json"), scope)?;
        }
    }

    let deadline = Instant::now() + REFRESH_BUDGET;
    let batch = fetch_batch(
        &initial,
        scope,
        repository.as_deref(),
        deadline,
        &mut request,
    );

    // Git checkout/remote changes during a slow query must not attach its results to
    // the user's new context. HEAD-only changes still belong to this branch's PR.
    let current_repository = github_repository(scope);
    if !same_current_scope(scope)
        || !same_repository(current_repository.as_deref(), repository.as_deref())
    {
        let _lock = lock_file(&dir.join(".links.lock"), false)?;
        let mut current = load(root, scope)?;
        if !same_repository(current_repository.as_deref(), repository.as_deref()) {
            current.reset_discovery();
            current.repository = current_repository;
        }
        current.refresh_started_at_unix_utc = None;
        current.issue_state = ResolutionState::Unchecked;
        current.pr_state = ResolutionState::Unchecked;
        current.next_refresh_at_unix_utc = Some(0);
        atomic_write_json(&dir.join("links.json"), &current)?;
        return Ok(RefreshOutcome::Skipped);
    }

    let _lock = lock_file(&dir.join(".links.lock"), false)?;
    let mut current = load(root, scope)?;
    current.repository = repository;
    current.discovery_unavailable =
        current.repository.is_none() || scope.branch.is_none() || scope.unlinked_reason.is_some();
    current.refresh_started_at_unix_utc = None;
    current.discovery_head.clone_from(&scope.head);
    let completed = unix_now();
    apply_batch(&mut current, &initial, &batch, completed);
    atomic_write_json(&dir.join("links.json"), &current)?;
    Ok(RefreshOutcome::Refreshed)
}

type Updates = BTreeMap<String, Result<Link, String>>;

struct FetchBatch {
    discovery: AppResult<Discovery>,
    selected_pr_id: Option<String>,
    issues: Option<Vec<Link>>,
    issue_error: Option<String>,
    updates: Updates,
}

fn fetch_batch(
    initial: &Links,
    scope: &context::Context,
    repository: Option<&str>,
    deadline: Instant,
    request: &mut impl FnMut(&[&str], Instant) -> AppResult<serde_json::Value>,
) -> FetchBatch {
    let discovery = match (repository, scope.branch.as_deref()) {
        (Some(repo), Some(branch)) if scope.unlinked_reason.is_none() => request(
            &[
                "pr",
                "list",
                "--repo",
                repo,
                "--head",
                branch,
                "--state",
                "all",
                "--limit",
                "100",
                "--json",
                PR_LIST_FIELDS,
            ],
            deadline,
        )
        .and_then(|value| discover(scope, repo, &value, unix_now())),
        _ => Err(crate::tr!(
            "GitHub 저장소 또는 브랜치 미확인 · 명시적 연결 필요",
            "GitHub repository/branch unknown · Explicit link needed"
        )
        .into()),
    };
    let mut working = initial.clone();
    if let Ok(found) = &discovery {
        apply_discovery(&mut working, found);
    }

    let mut batch = fetch_selected(&working, scope, repository, discovery, deadline, request);
    let mut requests = batch.updates.len();
    apply_issues(
        &mut working,
        batch.selected_pr_id.as_deref(),
        batch.issues.as_deref(),
    );

    let mut targets: Vec<Link> = working.primary(Kind::Issue).cloned().into_iter().collect();
    let mut remaining: Vec<Link> = batch
        .issues
        .iter()
        .flatten()
        .map(|link| working.entries.get(&link.id()).unwrap_or(link))
        .cloned()
        .collect();
    remaining.extend(
        initial
            .manual_entries
            .iter()
            .filter_map(|id| initial.entries.get(id))
            .cloned(),
    );
    remaining.sort_by_key(|link| link.checked_at_unix_utc.unwrap_or(0));
    targets.extend(remaining);
    let mut seen: BTreeSet<String> = batch
        .updates
        .keys()
        .map(|id| id.to_ascii_lowercase())
        .collect();
    for link in targets {
        let id = link.id();
        if !seen.insert(id.to_ascii_lowercase()) || requests >= MAX_METADATA_FETCHES {
            continue;
        }
        requests += 1;
        batch
            .updates
            .insert(id, fetch_link(&link, deadline, request));
    }

    batch
}

fn fetch_selected(
    working: &Links,
    scope: &context::Context,
    repository: Option<&str>,
    discovery: AppResult<Discovery>,
    deadline: Instant,
    request: &mut impl FnMut(&[&str], Instant) -> AppResult<serde_json::Value>,
) -> FetchBatch {
    let mut batch = FetchBatch {
        discovery,
        selected_pr_id: working.primary(Kind::Pr).map(Link::id),
        issues: None,
        issue_error: None,
        updates: Updates::new(),
    };
    if let Some(pr) = working.primary(Kind::Pr).cloned() {
        // Body and closing references are fetched only for the selected PR.
        let refreshed = fetch_link(&pr, deadline, request);
        match refreshed {
            Ok(pr) => {
                let id = pr.id();
                if !working.manual_pr
                    && (!same_repository(pr.head_repository.as_deref(), repository)
                        || pr.head_ref_name != scope.branch)
                {
                    let error = crate::tr!(
                        "PR 세부 응답의 브랜치·원본 저장소 불일치",
                        "PR detail branch/source repository mismatch"
                    )
                    .to_string();
                    batch.discovery = Err(error.clone().into());
                    batch.issue_error = Some(error.clone());
                    batch.updates.insert(id, Err(error));
                } else {
                    if !working.manual_pr
                        && pr.state.as_deref() != Some("OPEN")
                        && let Ok(found) = &mut batch.discovery
                    {
                        found.open.retain(|open| open != &id);
                        found.prs.insert(id.clone(), pr.clone());
                    }
                    if let Some(error) = &pr.closing_issues_error {
                        batch.issue_error = Some(error.clone());
                    } else {
                        batch.issues = Some(
                            pr.closing_issues
                                .iter()
                                .filter_map(|id| reference_link(Kind::Issue, id).ok())
                                .collect(),
                        );
                    }
                    batch.updates.insert(id, Ok(pr));
                }
            }
            Err(error) => {
                batch.issue_error = Some(error.clone());
                if let Some(id) = &batch.selected_pr_id {
                    batch.updates.insert(id.clone(), Err(error));
                }
            }
        }
    }
    batch
}

fn apply_updates(current: &mut Links, updates: &Updates) {
    for (id, update) in updates {
        if let Some(link) = current.entries.get_mut(id) {
            match update {
                Ok(fetched) => *link = fetched.clone(),
                Err(error) => link.error = Some(error.clone()),
            }
        }
    }
}

fn apply_batch(current: &mut Links, initial: &Links, batch: &FetchBatch, completed: u64) {
    match &batch.discovery {
        Ok(found) => {
            apply_discovery(current, found);
            current.discovery_checked_at_unix_utc = Some(completed);
            current.discovery_error = None;
        }
        Err(error) => {
            current.discovery_error = Some(error.to_string());
            if !current.manual_pr {
                current.pr_state = ResolutionState::Error;
            }
            if !current.manual_issue {
                current.issue_state = ResolutionState::Error;
            }
        }
    }
    apply_updates(current, &batch.updates);
    let same_selection = current.primary_pr.as_deref() == batch.selected_pr_id.as_deref();
    if same_selection
        && batch.issue_error.is_none()
        && (batch.discovery.is_ok() || current.manual_pr)
    {
        apply_issues(
            current,
            batch.selected_pr_id.as_deref(),
            batch.issues.as_deref(),
        );
        // Issue metadata can be fetched before insertion into the persisted cache.
        apply_updates(current, &batch.updates);
    } else if !current.manual_issue {
        if same_selection || batch.discovery.is_err() {
            current.issue_state = ResolutionState::Error;
        } else {
            apply_issues(current, None, None);
        }
    }
    for kind in [Kind::Issue, Kind::Pr] {
        if current.manual(kind) && !current.explicit_none(kind) {
            let state = if current
                .primary(kind)
                .is_some_and(|link| link.error.is_some())
            {
                ResolutionState::Error
            } else {
                ResolutionState::Linked
            };
            match kind {
                Kind::Issue => current.issue_state = state,
                Kind::Pr => current.pr_state = state,
            }
        }
    }
    let failed = batch.discovery.is_err()
        || batch.issue_error.is_some()
        || batch.updates.values().any(Result::is_err);
    current.consecutive_failures = if failed {
        current.consecutive_failures.saturating_add(1)
    } else {
        0
    };
    let delay = if failed {
        (30_u64 << current.consecutive_failures.saturating_sub(1).min(4)).min(300)
    } else {
        CACHE_TTL
    };
    current.next_refresh_at_unix_utc = Some(
        if current.selection_revision == initial.selection_revision
            && (same_selection || batch.discovery.is_err())
        {
            completed.saturating_add(delay)
        } else {
            0
        },
    );
}

fn reference_link(kind: Kind, id: &str) -> AppResult<Link> {
    let prefix = match kind {
        Kind::Issue => "Issue:",
        Kind::Pr => "Pr:",
    };
    parse(kind, id.strip_prefix(prefix).unwrap_or(id))
}

fn fetch_link(
    link: &Link,
    deadline: Instant,
    request: &mut impl FnMut(&[&str], Instant) -> AppResult<serde_json::Value>,
) -> Result<Link, String> {
    let kind = if link.kind == Kind::Issue {
        "issue"
    } else {
        "pr"
    };
    let fields = if link.kind == Kind::Issue {
        "number,url,title,body,state"
    } else {
        PR_FIELDS
    };
    request(
        &[
            kind,
            "view",
            &link.number.to_string(),
            "--repo",
            &link.repository,
            "--json",
            fields,
        ],
        deadline,
    )
    .and_then(|value| metadata(link.clone(), &value, unix_now()))
    .map_err(|error| error.to_string())
}

/// Local-only selection update. A concurrent fetch merges this choice rather than
/// holding a lock over network access or overwriting it with its older snapshot.
pub fn set_selection(
    root: &Path,
    scope: &context::Context,
    kind: Kind,
    selection: Selection,
) -> AppResult<()> {
    if !same_current_scope(scope) {
        return Err(crate::tr!(
            "브랜치가 변경되었습니다 · 현재 연결 선택 화면을 다시 여세요",
            "Branch changed · Reopen the link picker"
        )
        .into());
    }
    let repository = github_repository(scope);
    let dir = context::directory(root, scope);
    let _lock = lock_file(&dir.join(".links.lock"), false)?;
    let mut links = load(root, scope)?;
    // A local selection must not relabel an old automatic association as belonging
    // to a newly added/changed origin. Preserve explicit choices and cached metadata.
    if !same_repository(links.repository.as_deref(), repository.as_deref()) {
        links.reset_discovery();
    }
    links.repository.clone_from(&repository);
    let (primary, explicit_none, manual, state) = match selection {
        Selection::Primary(value) => {
            let link = parse_scoped(kind, &value, repository.as_deref())?;
            let id = link.id();
            links.entries.entry(id.clone()).or_insert(link);
            links.manual_entries.insert(id.clone());
            (Some(id), false, true, ResolutionState::Linked)
        }
        Selection::None => (None, true, true, ResolutionState::None),
        Selection::Auto => (None, false, false, ResolutionState::Unchecked),
    };
    match kind {
        Kind::Issue => {
            links.primary_issue = primary;
            links.issue_not_linked = explicit_none;
            links.manual_issue = manual;
            links.automatic_primary_issue = false;
            links.issue_state = state;
        }
        Kind::Pr => {
            links.primary_pr = primary;
            links.pr_not_created = explicit_none;
            links.manual_pr = manual;
            links.automatic_primary_pr = false;
            links.pr_state = state;
            if !links.manual_issue {
                apply_issues(&mut links, None, None);
            }
        }
    }
    links.selection_revision = links.selection_revision.saturating_add(1);
    links.next_refresh_at_unix_utc = Some(0);
    atomic_write_json(&dir.join("context.json"), scope)?;
    atomic_write_json(&dir.join("links.json"), &links)
}

pub fn cli(args: &[String]) -> AppResult<()> {
    let root = crate::config::state_root()?;
    let store = SpaceStore::for_caller(&root)?;
    let meta = store.meta()?.ok_or("Space metadata missing")?;
    let scope = context::detect_for_store(&store, &meta.cwd)?;
    let op = args.first().map_or("show", String::as_str);
    match op {
        "refresh" => return refresh(&root, &scope),
        "refresh-if-due" => return refresh_if_due(&root, &scope).map(|_| ()),
        "show" => {
            println!("{}", serde_json::to_string_pretty(&load(&root, &scope)?)?);
            return Ok(());
        }
        _ => (),
    }
    let kind = match args.get(1).map(String::as_str) {
        Some("issue") => Kind::Issue,
        Some("pr") => Kind::Pr,
        _ => {
            return Err(
                "use links <add|primary|none|auto|remove> <issue|pr> [owner/repo#number]".into(),
            );
        }
    };
    match op {
        "primary" => {
            return set_selection(
                &root,
                &scope,
                kind,
                Selection::Primary(
                    args.get(2)
                        .ok_or("repository#number, number or URL required")?
                        .clone(),
                ),
            );
        }
        "none" => return set_selection(&root, &scope, kind, Selection::None),
        "auto" => return set_selection(&root, &scope, kind, Selection::Auto),
        "add" | "remove" => (),
        _ => return Err("unknown links operation".into()),
    }
    let link = parse_scoped(
        kind,
        args.get(2)
            .ok_or("repository#number, number or URL required")?,
        github_repository(&scope).as_deref(),
    )?;
    let id = link.id();
    let dir = context::directory(&root, &scope);
    let _lock = lock_file(&dir.join(".links.lock"), false)?;
    let mut links = load(&root, &scope)?;
    if op == "add" {
        links.entries.entry(id.clone()).or_insert(link);
        links.manual_entries.insert(id);
    } else {
        links.entries.remove(&id);
        links.manual_entries.remove(&id);
        links.issue_candidates.retain(|candidate| candidate != &id);
        links.pr_candidates.retain(|candidate| candidate != &id);
        if links.primary_issue.as_ref() == Some(&id) {
            links.primary_issue = None;
            links.manual_issue = false;
            links.automatic_primary_issue = false;
            links.issue_state = ResolutionState::Unchecked;
        }
        if links.primary_pr.as_ref() == Some(&id) {
            links.primary_pr = None;
            links.manual_pr = false;
            links.automatic_primary_pr = false;
            links.pr_state = ResolutionState::Unchecked;
            if !links.manual_issue {
                apply_issues(&mut links, None, None);
            }
        }
    }
    links.selection_revision = links.selection_revision.saturating_add(1);
    links.next_refresh_at_unix_utc = Some(0);
    atomic_write_json(&dir.join("context.json"), &scope)?;
    atomic_write_json(&dir.join("links.json"), &links)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    #[test]
    fn conversation_number_lookup_identifies_kind_and_validates_canonical_identity() {
        let deadline = Instant::now() + Duration::from_secs(5);
        let issue = lookup_reference_with("Owner/Repo", 142, None, deadline, &mut |args, _| {
            assert_eq!(args, ["api", "--method", "GET", "repos/Owner/Repo/issues/142"]);
            Ok(json!({"number":142,"html_url":"https://github.com/owner/repo/issues/142","title":"Issue","body":null,"state":"closed"}))
        }).unwrap();
        assert_eq!(issue.kind, Kind::Issue);
        assert_eq!(issue.state.as_deref(), Some("CLOSED"));
        assert_eq!(issue.body.as_deref(), Some(""));

        let mut count = 0;
        let pr = lookup_reference_with("owner/repo", 168, None, deadline, &mut |args, _| {
            count += 1;
            if count == 1 {
                assert_eq!(args[0], "api");
                Ok(json!({"number":168,"html_url":"https://github.com/owner/repo/pull/168","pull_request":{"html_url":"https://github.com/owner/repo/pull/168"}}))
            } else {
                assert_eq!(&args[..5], ["pr","view","168","--repo","owner/repo"]);
                Ok(json!({"number":168,"url":"https://github.com/owner/repo/pull/168","title":"PR","body":"","state":"MERGED","headRefName":"feature","headRefOid":"head","headRepository":{"nameWithOwner":"fork/repo"},"closingIssuesReferences":[]}))
            }
        }).unwrap();
        assert_eq!(count, 2);
        assert_eq!(pr.kind, Kind::Pr);
        assert_eq!(pr.state.as_deref(), Some("MERGED"));
        assert_eq!(pr.head_repository.as_deref(), Some("fork/repo"));

        for bad in [
            json!({"number":142,"html_url":"https://github.com/other/repo/issues/142"}),
            json!({"number":143,"html_url":"https://github.com/owner/repo/issues/142"}),
            json!({"number":142,"html_url":"https://evil.test/owner/repo/issues/142"}),
            json!({"number":142,"html_url":"https://github.com/owner/repo/pull/142","pull_request":{"html_url":"https://github.com/owner/repo/pull/142"}}),
        ] {
            assert!(
                lookup_reference_with(
                    "owner/repo",
                    142,
                    Some(Kind::Issue),
                    deadline,
                    &mut |_, _| Ok(bad.clone())
                )
                .is_err()
            );
        }
    }

    struct SelectionFixture {
        dir: std::path::PathBuf,
        root: std::path::PathBuf,
        scope: context::Context,
    }

    impl SelectionFixture {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("maeklog-selection-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let mut fixture = Self {
                root: dir.join("state"),
                dir,
                scope: context::Context::default(),
            };
            fixture.git(&["init", "--initial-branch", "feature"]);
            fixture.git(&[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "--allow-empty",
                "-m",
                "fixture",
            ]);
            fixture.scope = context::detect(fixture.dir.to_str().unwrap(), "test");
            fixture
        }

        fn git(&self, args: &[&str]) {
            let output = Command::new("git")
                .current_dir(&self.dir)
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        fn resolve(&self) -> Links {
            refresh_with(&self.root, &self.scope, false, |args, _| {
                Ok(if args[1] == "list" { json!([pr(10, "OPEN", 20)]) }
                    else if args[0] == "pr" { pr(10, "OPEN", 20) }
                    else { json!({"number":20,"url":"https://github.com/team/project/issues/20","title":"Issue 20","body":"purpose","state":"OPEN"}) })
            }).unwrap();
            load(&self.root, &self.scope).unwrap()
        }
    }

    impl Drop for SelectionFixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn scope() -> context::Context {
        context::Context {
            key: "test-branch".into(),
            repository: Some("/repository/.git".into()),
            branch: Some("feature".into()),
            head: Some("local-unpushed-commit".into()),
            incarnation: Some("generation-1".into()),
            ..Default::default()
        }
    }

    fn pr(number: u64, state: &str, issue: u64) -> Value {
        json!({
            "number": number,
            "url": format!("https://github.com/team/project/pull/{number}"),
            "title": format!("PR {number}"), "body": "## 변경 범위\n브랜치의 작업",
            "state": state, "headRefName": "feature", "headRefOid": "remote-pushed-commit",
            "headRepository": {"name": "project", "nameWithOwner": "team/project"},
            "headRepositoryOwner": {"login": "team"}, "isCrossRepository": false,
            "closingIssuesReferences": [{
                "number": issue, "url": format!("https://github.com/team/project/issues/{issue}"),
                "repository": {"name": "project", "owner": {"login": "team"}}
            }]
        })
    }

    fn cache() -> Links {
        let mut links = Links::default();
        links.reconcile_scope(&scope());
        links
    }

    #[test]
    fn unique_open_branch_pr_remains_connected_with_unpushed_local_commits() {
        let found = discover(&scope(), "team/project", &json!([pr(10, "OPEN", 20)]), 100).unwrap();
        let mut links = cache();
        apply_discovery(&mut links, &found);
        assert_eq!(links.resolution(Kind::Pr), ResolutionState::Linked);
        let linked = links.primary(Kind::Pr).unwrap();
        assert_eq!(linked.number, 10);
        assert_ne!(linked.head_oid, scope().head);
        let mut edited = scope();
        edited.head = Some("new-local-commit".into());
        links.reconcile_scope(&edited);
        assert_eq!(links.primary(Kind::Pr).unwrap().number, 10);
    }

    #[test]
    fn closed_history_is_selectable_but_does_not_become_an_automatic_primary() {
        let found = discover(
            &scope(),
            "team/project",
            &json!([pr(10, "MERGED", 20)]),
            100,
        )
        .unwrap();
        let mut links = cache();
        apply_discovery(&mut links, &found);
        assert_eq!(links.resolution(Kind::Pr), ResolutionState::None);
        assert!(links.primary_pr.is_none());
        assert_eq!(links.candidates(Kind::Pr).len(), 1);
        assert_eq!(
            links.candidates(Kind::Pr)[0].state.as_deref(),
            Some("MERGED")
        );
        links.primary_pr = Some("Pr:team/project#10".into());
        links.manual_pr = true;
        links.pr_state = ResolutionState::Linked;
        apply_discovery(&mut links, &found);
        assert_eq!(links.primary(Kind::Pr).unwrap().number, 10);
    }

    #[test]
    fn same_name_forks_and_other_branches_never_qualify() {
        let mut fork = pr(10, "OPEN", 20);
        fork["headRepository"]["nameWithOwner"] = json!("someone/project");
        fork["headRepositoryOwner"]["login"] = json!("someone");
        fork["isCrossRepository"] = json!(true);
        let mut other_branch = pr(11, "OPEN", 21);
        other_branch["headRefName"] = json!("unrelated");
        let mut wrong_repo = pr(12, "OPEN", 22);
        wrong_repo["url"] = json!("https://github.com/elsewhere/project/pull/12");
        let found = discover(
            &scope(),
            "team/project",
            &json!([fork, other_branch, wrong_repo, pr(13, "OPEN", 23)]),
            100,
        )
        .unwrap();
        assert_eq!(found.open, ["Pr:team/project#13"]);
        assert_eq!(found.prs.len(), 1);
    }

    #[test]
    fn missing_or_contradictory_source_identity_is_an_error_not_no_match() {
        let mut missing = pr(10, "OPEN", 20);
        missing["headRepository"] = Value::Null;
        assert!(discover(&scope(), "team/project", &json!([missing]), 100).is_err());
        let mut inconsistent = pr(10, "OPEN", 20);
        inconsistent["headRepositoryOwner"]["login"] = json!("someone");
        assert!(discover(&scope(), "team/project", &json!([inconsistent]), 100).is_err());
    }

    #[test]
    fn ambiguous_prs_do_not_union_their_closing_issue_references() {
        let found = discover(
            &scope(),
            "team/project",
            &json!([pr(10, "OPEN", 20), pr(11, "OPEN", 21)]),
            100,
        )
        .unwrap();
        let mut links = cache();
        apply_discovery(&mut links, &found);
        apply_issues(&mut links, None, None);
        assert_eq!(links.resolution(Kind::Pr), ResolutionState::Ambiguous);
        assert_eq!(links.resolution(Kind::Issue), ResolutionState::Unchecked);
        assert!(links.primary_issue.is_none());
        assert!(links.issue_candidates.is_empty());
        assert!(links.entries.values().all(|link| link.kind == Kind::Pr));
    }

    #[test]
    fn automatic_issues_come_only_from_the_selected_open_pr() {
        let selected = pr(11, "OPEN", 21);
        let found = discover(
            &scope(),
            "team/project",
            &json!([pr(10, "CLOSED", 20), selected.clone()]),
            100,
        )
        .unwrap();
        let mut links = cache();
        apply_discovery(&mut links, &found);
        apply_issues(
            &mut links,
            Some("Pr:team/project#11"),
            Some(&closing_issues(&selected).unwrap()),
        );
        assert_eq!(links.primary(Kind::Issue).unwrap().number, 21);
        assert!(!links.entries.contains_key("Issue:team/project#20"));
    }

    #[test]
    fn discovery_respects_manual_primary_and_explicit_none() {
        let mut links = cache();
        let chosen = parse(Kind::Issue, "other/project#90").unwrap();
        links.primary_issue = Some(chosen.id());
        links.entries.insert(chosen.id(), chosen);
        links.manual_issue = true;
        links.issue_state = ResolutionState::Linked;
        links.manual_pr = true;
        links.pr_not_created = true;
        let found = discover(&scope(), "team/project", &json!([pr(10, "OPEN", 20)]), 100).unwrap();
        apply_discovery(&mut links, &found);
        apply_issues(&mut links, None, None);
        assert_eq!(links.primary(Kind::Issue).unwrap().number, 90);
        assert_eq!(links.resolution(Kind::Pr), ResolutionState::None);
        assert!(links.primary_pr.is_none());
        assert!(links.is_manual(Kind::Issue));
    }

    #[test]
    fn generation_change_drops_automatic_primary_but_preserves_explicit_choice_and_cache() {
        let mut links = cache();
        let found = discover(&scope(), "team/project", &json!([pr(10, "OPEN", 20)]), 100).unwrap();
        apply_discovery(&mut links, &found);
        let chosen = parse(Kind::Issue, "team/project#99").unwrap();
        links.primary_issue = Some(chosen.id());
        links.entries.insert(chosen.id(), chosen);
        links.manual_issue = true;
        let mut recreated = scope();
        recreated.incarnation = Some("generation-2".into());
        links.reconcile_scope(&recreated);
        assert!(links.primary_pr.is_none());
        assert!(links.pr_candidates.is_empty());
        assert_eq!(
            links.entries["Pr:team/project#10"].association_checked_at_unix_utc,
            Some(100)
        );
        assert_eq!(links.primary(Kind::Issue).unwrap().number, 99);
    }

    #[test]
    fn due_checks_respect_ttl_head_floor_backoff_and_unavailable_source() {
        let mut links = cache();
        links.discovery_head.clone_from(&scope().head);
        links.last_refresh_attempt_at_unix_utc = Some(100);
        links.next_refresh_at_unix_utc = Some(160);
        assert!(!due(&links, &scope(), 159));
        assert!(due(&links, &scope(), 160));
        let mut edited = scope();
        edited.head = Some("new-local-commit".into());
        assert!(!due(&links, &edited, 109));
        assert!(due(&links, &edited, 110));
        links.consecutive_failures = 1;
        assert!(!due(&links, &edited, 110));
        links.discovery_unavailable = true;
        assert!(!due(&links, &scope(), 1000));
        links.next_refresh_at_unix_utc = Some(0);
        assert!(due(&links, &scope(), 1000));
        links.refresh_started_at_unix_utc = Some(100);
        links.next_refresh_at_unix_utc = Some(1000);
        assert!(due(&links, &scope(), 141));
    }

    #[test]
    fn invalid_closing_references_fail_issue_resolution_without_hiding_valid_pr() {
        let mut broken = pr(10, "OPEN", 20);
        broken["closingIssuesReferences"][0]["number"] = json!(999);
        let found = discover(&scope(), "team/project", &json!([broken.clone()]), 100).unwrap();
        let mut links = cache();
        apply_discovery(&mut links, &found);
        links.entries.insert(
            "Pr:team/project#10".into(),
            metadata(parse(Kind::Pr, "team/project#10").unwrap(), &broken, 100).unwrap(),
        );
        links.issue_state = ResolutionState::Error;
        assert_eq!(links.resolution(Kind::Pr), ResolutionState::Linked);
        assert_eq!(links.resolution(Kind::Issue), ResolutionState::Error);
        assert!(
            links
                .error(Kind::Issue)
                .unwrap()
                .contains(crate::tr!("번호 불일치", "number mismatch"))
        );
    }

    #[test]
    fn incomplete_metadata_and_wrong_exact_url_cannot_replace_successful_cache() {
        let link = metadata(
            parse(Kind::Pr, "team/project#10").unwrap(),
            &pr(10, "OPEN", 20),
            100,
        )
        .unwrap();
        let mut wrong = pr(10, "OPEN", 20);
        wrong["url"] = json!("https://github.com/another/project/pull/10");
        assert!(metadata(link.clone(), &wrong, 200).is_err());
        let mut missing = pr(10, "OPEN", 20);
        missing["body"] = Value::Null;
        assert!(metadata(link.clone(), &missing, 200).is_err());
        assert_eq!(link.checked_at_unix_utc, Some(100));
        assert_eq!(link.title.as_deref(), Some("PR 10"));
        assert!(
            metadata(
                parse(Kind::Pr, "Team/Project#10").unwrap(),
                &pr(10, "OPEN", 20),
                200
            )
            .is_ok()
        );
    }

    #[test]
    fn scoped_numbers_require_a_proven_repository_and_urls_remain_exact() {
        assert!(parse_scoped(Kind::Issue, "42", None).is_err());
        assert!(parse_scoped(Kind::Pr, "#42", Some("../repo")).is_err());
        assert_eq!(
            parse_scoped(Kind::Issue, "#42", Some("team/project"))
                .unwrap()
                .number,
            42
        );
        assert!(parse(Kind::Pr, "https://github.com/team/project/pull/42/files").is_err());
        assert!(parse(Kind::Pr, "https://github.com/team/project/issues/42").is_err());
        for remote in [
            "https://github.com/team/project.git",
            "git@github.com:team/project.git",
            "ssh://git@github.com/team/project.git",
            "https://token@github.com/team/project.git",
        ] {
            assert_eq!(remote_repository(remote).as_deref(), Some("team/project"));
        }
        assert!(remote_repository("https://github.com.example/team/project").is_none());
    }

    #[test]
    fn successful_manual_pr_issue_resolution_survives_unavailable_auto_source() {
        let mut links = cache();
        let selected = parse(Kind::Pr, "team/project#10").unwrap();
        links.primary_pr = Some(selected.id());
        links.entries.insert(selected.id(), selected);
        links.manual_pr = true;
        links.pr_state = ResolutionState::Linked;
        links.discovery_error = Some("GitHub 저장소 미확인".into());
        links.discovery_unavailable = true;
        apply_issues(
            &mut links,
            Some("Pr:team/project#10"),
            Some(&[parse(Kind::Issue, "team/project#20").unwrap()]),
        );
        assert_eq!(links.resolution(Kind::Issue), ResolutionState::Linked);
        apply_issues(&mut links, Some("Pr:team/project#10"), Some(&[]));
        assert_eq!(links.resolution(Kind::Issue), ResolutionState::None);
    }

    #[test]
    fn adding_or_changing_origin_refreshes_inside_ttl_with_unchanged_head() {
        let fixture =
            std::env::temp_dir().join(format!("maeklog-links-origin-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&fixture);
        std::fs::create_dir_all(&fixture).unwrap();
        let git = |args: &[&str]| {
            let output = Command::new("git")
                .current_dir(&fixture)
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["init", "--initial-branch", "feature"]);
        git(&[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "--allow-empty",
            "-m",
            "fixture",
        ]);
        let scope = context::detect(fixture.to_str().unwrap(), "test");
        let root = fixture.join("state");
        let first = refresh_with(&root, &scope, false, |_, _| {
            panic!("unavailable source must not invoke GitHub")
        })
        .unwrap();
        assert_eq!(first, RefreshOutcome::Refreshed);
        assert!(load(&root, &scope).unwrap().discovery_unavailable);
        git(&[
            "remote",
            "add",
            "origin",
            "https://github.com/team/project.git",
        ]);
        let mut queries = 0;
        let refreshed = refresh_with(&root, &scope, false, |args, _| {
            queries += 1;
            assert!(matches!(args[1], "list" | "view"));
            Ok(if args[1] == "list" { json!([pr(10, "OPEN", 20)]) }
                else if args[0] == "pr" { pr(10, "OPEN", 20) }
                else { json!({"number":20,"url":"https://github.com/team/project/issues/20","title":"Issue 20","body":"purpose","state":"OPEN"}) })
        }).unwrap();
        assert_eq!(refreshed, RefreshOutcome::Refreshed);
        assert_eq!(queries, 3);
        let links = load(&root, &scope).unwrap();
        assert!(!links.discovery_unavailable);
        assert_eq!(links.primary(Kind::Pr).unwrap().number, 10);
        assert_eq!(links.primary(Kind::Issue).unwrap().number, 20);
        assert_eq!(
            context::detect(fixture.to_str().unwrap(), "test").head,
            scope.head
        );
        git(&[
            "remote",
            "set-url",
            "origin",
            "https://github.com/other/project.git",
        ]);
        let changed = refresh_with(&root, &scope, false, |args, _| {
            assert_eq!(args[1], "list");
            Ok(json!([]))
        })
        .unwrap();
        assert_eq!(changed, RefreshOutcome::Refreshed);
        let links = load(&root, &scope).unwrap();
        assert_eq!(links.repository.as_deref(), Some("other/project"));
        assert!(links.primary_pr.is_none());
        assert!(links.primary_issue.is_none());
        assert!(links.entries.contains_key("Pr:team/project#10"));
        std::fs::remove_dir_all(fixture).unwrap();
    }

    #[test]
    fn manual_selection_after_origin_change_cannot_keep_or_restore_previous_automatic_pr() {
        for (name, next_origin, input) in [
            ("changed", Some("other/project"), "#77"),
            ("removed", None, "other/project#77"),
        ] {
            let fixture = SelectionFixture::new(name);
            fixture.git(&[
                "remote",
                "add",
                "origin",
                "https://github.com/team/project.git",
            ]);
            let cached = fixture.resolve();
            assert!(cached.automatic_primary_pr);
            if let Some(repo) = next_origin {
                fixture.git(&[
                    "remote",
                    "set-url",
                    "origin",
                    &format!("https://github.com/{repo}.git"),
                ]);
            } else {
                fixture.git(&["remote", "remove", "origin"]);
            }
            set_selection(
                &fixture.root,
                &fixture.scope,
                Kind::Issue,
                Selection::Primary(input.into()),
            )
            .unwrap();
            let selected = load(&fixture.root, &fixture.scope).unwrap();
            assert_eq!(selected.repository.as_deref(), next_origin);
            assert!(
                selected.primary_pr.is_none(),
                "old automatic PR survived manual selection after origin change"
            );
            assert!(!selected.automatic_primary_pr);
            assert!(selected.issue_candidates.is_empty());
            assert_eq!(
                selected.primary(Kind::Issue).unwrap().repository,
                "other/project"
            );
            assert_eq!(
                selected.entries["Pr:team/project#10"].body,
                cached.entries["Pr:team/project#10"].body
            );
            refresh_with(&fixture.root, &fixture.scope, false, |args, _| {
                assert!(
                    args[1] != "view" || args[0] != "pr",
                    "old automatic PR was fetched after source change"
                );
                Err("offline".into())
            })
            .unwrap();
            let failed = load(&fixture.root, &fixture.scope).unwrap();
            assert!(failed.primary_pr.is_none());
            assert_eq!(failed.primary(Kind::Issue).unwrap().number, 77);
            assert!(failed.manual_issue);
        }
    }

    #[test]
    fn manual_selection_after_adding_origin_preserves_explicit_pr_and_clears_derived_cache() {
        let fixture = SelectionFixture::new("added");
        set_selection(
            &fixture.root,
            &fixture.scope,
            Kind::Pr,
            Selection::Primary("team/project#10".into()),
        )
        .unwrap();
        let cached = fixture.resolve();
        assert_eq!(cached.repository, None);
        assert!(cached.automatic_primary_issue);
        assert!(!cached.issue_candidates.is_empty());
        fixture.git(&[
            "remote",
            "add",
            "origin",
            "https://github.com/other/project.git",
        ]);
        set_selection(
            &fixture.root,
            &fixture.scope,
            Kind::Issue,
            Selection::Primary("77".into()),
        )
        .unwrap();
        let selected = load(&fixture.root, &fixture.scope).unwrap();
        assert!(
            selected.issue_candidates.is_empty(),
            "old automatic discovery survived added origin"
        );
        assert!(selected.manual_pr);
        assert_eq!(
            selected.primary(Kind::Pr).unwrap().repository,
            "team/project"
        );
        assert_eq!(
            selected.primary(Kind::Issue).unwrap().repository,
            "other/project"
        );
        refresh_with(&fixture.root, &fixture.scope, false, |_, _| {
            Err("offline".into())
        })
        .unwrap();
        let failed = load(&fixture.root, &fixture.scope).unwrap();
        assert!(failed.manual_pr);
        assert!(failed.manual_issue);
        assert_eq!(
            failed.primary(Kind::Pr).unwrap().body,
            cached.primary(Kind::Pr).unwrap().body
        );
        assert_eq!(failed.primary(Kind::Issue).unwrap().number, 77);
    }

    #[test]
    fn none_and_auto_selections_reset_changed_origin_for_both_kinds() {
        for kind in [Kind::Issue, Kind::Pr] {
            for (name, selection) in [("none", Selection::None), ("auto", Selection::Auto)] {
                let fixture = SelectionFixture::new(&format!("{kind:?}-{name}"));
                fixture.git(&[
                    "remote",
                    "add",
                    "origin",
                    "https://github.com/team/project.git",
                ]);
                let cached = fixture.resolve();
                assert!(cached.automatic_primary_pr && cached.automatic_primary_issue);
                fixture.git(&[
                    "remote",
                    "set-url",
                    "origin",
                    "https://github.com/other/project.git",
                ]);
                set_selection(&fixture.root, &fixture.scope, kind, selection).unwrap();
                let selected = load(&fixture.root, &fixture.scope).unwrap();
                assert_eq!(selected.repository.as_deref(), Some("other/project"));
                assert!(selected.primary_pr.is_none() && selected.primary_issue.is_none());
                assert!(!selected.automatic_primary_pr && !selected.automatic_primary_issue);
                assert!(selected.pr_candidates.is_empty() && selected.issue_candidates.is_empty());
                assert_eq!(selected.explicit_none(kind), name == "none");
                refresh_with(&fixture.root, &fixture.scope, false, |_, _| {
                    Err("offline".into())
                })
                .unwrap();
                let failed = load(&fixture.root, &fixture.scope).unwrap();
                assert!(failed.primary_pr.is_none() && failed.primary_issue.is_none());
                assert_eq!(failed.explicit_none(kind), name == "none");
            }
        }
    }
}
