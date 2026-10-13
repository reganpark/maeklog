use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Summary {
    // Set by the local caller only; never accepted from model JSON or persisted.
    #[serde(skip)]
    pub correction_revision: Option<u64>,
    #[serde(default)]
    pub record_updates: Vec<crate::journal::Change>,
    #[serde(default)]
    pub state_updates: Vec<crate::journal::StateChange>,
    #[serde(default)]
    pub objective: String,
    #[serde(default)]
    pub objective_evidence: Vec<String>,
    #[serde(default)]
    pub discussion: Vec<String>,
    #[serde(default)]
    pub decisions: Vec<Decision>,
    #[serde(default)]
    pub progress: Vec<ProgressItem>,
    #[serde(default)]
    pub blockers: Vec<Blocker>,
    #[serde(default)]
    pub next_actions: Vec<NextAction>,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    pub id: String,
    pub text: String,
    pub status: DecisionStatus,
    #[serde(default)]
    pub supersedes: Option<String>,
    #[serde(default)]
    pub evidence: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionStatus {
    Active,
    Proposed,
    Superseded,
    Reversed,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProgressItem {
    pub text: String,
    pub status: ProgressStatus,
    #[serde(default)]
    pub evidence: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgressStatus {
    Done,
    Reported,
    InProgress,
    Blocked,
    Unverified,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RebuildRequest {
    pub session_id: String,
    pub requested_at_unix: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SessionReference {
    pub session_id: String,
    pub updated_at_unix: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NextAction {
    pub owner: String,
    pub text: String,
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(default)]
    pub step: Option<u8>,
    #[serde(default)]
    pub when: Option<String>,
}

// Old cached briefs used plain strings. They remain readable without guessing urgency.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum Blocker {
    Legacy(String),
    Classified(ClassifiedBlocker),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClassifiedBlocker {
    pub text: String,
    pub priority: BlockerPriority,
    pub evidence: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockerPriority {
    UserDecision,
    Current,
    Later,
    Unknown,
}

impl Blocker {
    pub fn text(&self) -> &str {
        match self {
            Self::Legacy(text) => text,
            Self::Classified(item) => &item.text,
        }
    }

    pub fn text_mut(&mut self) -> &mut String {
        match self {
            Self::Legacy(text) => text,
            Self::Classified(item) => &mut item.text,
        }
    }

    pub fn priority(&self) -> BlockerPriority {
        match self {
            Self::Legacy(_) => BlockerPriority::Unknown,
            Self::Classified(item) => item.priority,
        }
    }

    pub fn evidence(&self) -> &[String] {
        match self {
            Self::Legacy(_) => &[],
            Self::Classified(item) => &item.evidence,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RepoSnapshot {
    pub available: bool,
    #[serde(default)]
    pub root: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub head: Option<String>,
    #[serde(default)]
    pub remote: Option<String>,
    #[serde(default)]
    pub changed_files: Vec<String>,
    #[serde(default)]
    pub changed_file_count: usize,
    #[serde(default)]
    pub truncated: bool,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CurrentState {
    pub revision: u64,
    pub base_revision: Option<u64>,
    pub source_seq: u64,
    pub updated_at_unix: Option<u64>,
    #[serde(default)]
    pub timing: ContextTiming,
    #[serde(default)]
    pub repo: RepoSnapshot,
    #[serde(default)]
    pub summary: Summary,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ContextTiming {
    pub through_unix: Option<u64>,
    pub partial_history: bool,
    pub decisions: std::collections::BTreeMap<String, u64>,
    pub progress: std::collections::BTreeMap<String, u64>,
    pub blockers: std::collections::BTreeMap<String, u64>,
    pub next_actions: std::collections::BTreeMap<String, u64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    UserPrompt,
    AssistantFinal,
    Interrupted,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TurnEvent {
    pub seq: u64,
    pub kind: EventKind,
    pub session_id: String,
    #[serde(default)]
    pub turn_id: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    pub content: String,
    pub truncated: bool,
    pub recorded_at_unix: u64,
    pub cwd: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SpaceMeta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<String>,
    pub space_key: String,
    pub workspace_id: String,
    pub session_name: String,
    pub cwd: String,
    pub label: String,
    pub created_at_unix: u64,
    pub last_seen_at_unix: u64,
    pub archived: bool,
    #[serde(default)]
    pub archived_at_unix: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct UiState {
    pub visible: bool,
    pub panel_pane_id: Option<String>,
    pub updated_at_unix: Option<u64>,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            visible: true,
            panel_pane_id: None,
            updated_at_unix: None,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct WorkerStatus {
    pub state: String,
    pub message: String,
    pub updated_at_unix: Option<u64>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct CodexHookInput {
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub agent_type: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub session_id: String,
    #[serde(default)]
    pub cwd: String,
    #[serde(default)]
    pub hook_event_name: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub turn_id: Option<String>,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub last_assistant_message: Option<String>,
}
