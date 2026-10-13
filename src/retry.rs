//! Persistent per-cursor retry budgets survive new hooks and process restarts.
use crate::AppResult;
use crate::storage::{SpaceStore, atomic_write_json, lock_file, read_json_or_default};
use serde::{Deserialize, Serialize};

pub const MAX_ATTEMPTS: u8 = 3;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct RetryState {
    pub key: String,
    pub attempts: u8,
    pub next_at: u64,
    #[serde(default)]
    pub validation_error: Option<String>,
    #[serde(default)]
    pub repeated_validation: u8,
}

impl RetryState {
    pub fn for_key(self, key: &str) -> Self {
        if self.key == key {
            self
        } else {
            Self {
                key: key.into(),
                ..Self::default()
            }
        }
    }

    pub fn exhausted(&self) -> bool {
        self.attempts >= MAX_ATTEMPTS || self.repeated_validation >= 2
    }

    pub fn begin(&mut self, now: u64) {
        self.attempts += 1;
        // A crashed worker cannot create an immediate unbounded retry loop.
        self.next_at = now + 360;
    }

    pub fn failed(&mut self, now: u64) {
        self.next_at = now + if self.attempts <= 1 { 60 } else { 180 };
    }
    pub fn validation_failed(&mut self, message: &str) {
        let message: String = message.chars().take(500).collect();
        self.repeated_validation = if self.validation_error.as_ref() == Some(&message) {
            self.repeated_validation.saturating_add(1)
        } else {
            1
        };
        self.validation_error = Some(message);
    }
    pub fn stop_message(&self) -> String {
        if self.repeated_validation >= 2 {
            crate::tr!("동일한 검증 오류가 반복되어 자동 재시도를 중단했습니다 · 원문 보존 · 오늘 r / 이전 날짜 b · {}", "Repeated validation error · Automatic retry stopped · Original retained · Today: r / Earlier dates: b · {}";
                self.validation_error.as_deref().unwrap_or(crate::tr!("상세 미확인", "Details unknown"))
            )
        } else {
            crate::tr!("이 대화는 3회 실패했습니다 · 오늘은 r, 이전/보류 날짜는 b로 명시적 재시도", "This conversation failed 3 times · Retry explicitly: r for today, b for earlier/held dates").into()
        }
    }
}

pub fn load(store: &SpaceStore, key: &str) -> AppResult<RetryState> {
    if key.starts_with("daily:") {
        let all: std::collections::BTreeMap<String, RetryState> =
            read_json_or_default(&store.path("daily-retries.json"))?;
        if let Some(value) = all.get(key) {
            return Ok(value.clone());
        }
    }
    Ok(read_json_or_default::<RetryState>(&store.path(filename(key)))?.for_key(key))
}

fn filename(key: &str) -> &'static str {
    if key.starts_with("latest:") {
        "latest-retry.json"
    } else {
        "retry.json"
    }
}

pub fn save(store: &SpaceStore, state: &RetryState) -> AppResult<()> {
    let _lock = lock_file(&store.path(".retry.lock"), false)?;
    if state.key.starts_with("daily:") {
        let path = store.path("daily-retries.json");
        let mut all: std::collections::BTreeMap<String, RetryState> = read_json_or_default(&path)?;
        all.insert(state.key.clone(), state.clone());
        atomic_write_json(&path, &all)?;
    }
    atomic_write_json(&store.path(filename(&state.key)), state)
}

pub fn reset(store: &SpaceStore) -> AppResult<()> {
    atomic_write_json(&store.path("daily-retries.json"), &serde_json::json!({}))?;
    save(store, &RetryState::default())?;
    save(
        store,
        &RetryState {
            key: "latest:reset".into(),
            ..RetryState::default()
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_validation_stops_within_budget_and_is_key_scoped() {
        let mut budget = RetryState::default().for_key("daily:today:2");
        budget.begin(0);
        budget.validation_failed("decision_basis_required");
        assert!(!budget.exhausted());
        budget.begin(1);
        budget.validation_failed("decision_basis_required");
        assert!(budget.exhausted());
        assert_eq!(budget.attempts, 2);
        let saved = serde_json::to_string(&budget).unwrap();
        let restored: RetryState = serde_json::from_str(&saved).unwrap();
        assert!(restored.exhausted());
        assert!(!restored.for_key("daily:today:4").exhausted());
        let legacy: RetryState =
            serde_json::from_str(r#"{"key":"daily:today:2","attempts":1,"next_at":0}"#).unwrap();
        assert!(legacy.validation_error.is_none());
        assert_eq!(legacy.repeated_validation, 0);
    }
    #[test]
    fn caps_attempts_and_resets_only_for_a_new_cursor() {
        let mut retry = RetryState::default().for_key("events:10");
        retry.begin(0);
        retry.failed(180);
        assert_eq!(retry.next_at, 240);
        retry.begin(240);
        retry.failed(420);
        assert_eq!(retry.next_at, 600);
        retry.begin(600);
        retry.failed(780);
        let loaded: RetryState =
            serde_json::from_str(&serde_json::to_string(&retry).unwrap()).unwrap();
        assert!(loaded.clone().for_key("events:10").exhausted());
        assert!(!loaded.for_key("events:12").exhausted());
    }
}
