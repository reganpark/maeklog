//! Per-space opt-out, independent of panel visibility. One short lock orders
//! hook writes, preference changes, and model commits; never held during inference.
use crate::AppResult;
use crate::storage::{SpaceStore, atomic_write_json, lock_file, read_json_or_default};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::File;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct Preference {
    pub enabled: bool,
    pub generation: u64,
    // Never import whole transcripts after an opt-out: they include excluded turns.
    pub history_blocked: bool,
    pub open_turns: BTreeMap<String, Option<String>>,
}

impl Default for Preference {
    fn default() -> Self {
        Self {
            enabled: true,
            generation: 0,
            history_blocked: false,
            open_turns: BTreeMap::new(),
        }
    }
}

pub fn load(store: &SpaceStore) -> AppResult<Preference> {
    read_json_or_default(&store.path("participation.json"))
}

pub fn lock(store: &SpaceStore) -> AppResult<File> {
    lock_file(&store.path(".participation.lock"), false)
}

pub fn save(store: &SpaceStore, value: &Preference) -> AppResult<()> {
    atomic_write_json(&store.path("participation.json"), value)
}

pub fn set_enabled(store: &SpaceStore, enabled: bool) -> AppResult<()> {
    let _guard = lock(store)?;
    let mut value = load(store)?;
    if value.enabled != enabled {
        value.enabled = enabled;
        value.generation = value
            .generation
            .checked_add(1)
            .ok_or("preference generation overflow")?;
        value.history_blocked = true;
        value.open_turns.clear();
        save(store, &value)?;
        crate::task_links::generation_changed(store, value.generation)?;
        if !enabled {
            crate::journal::mark_gap(
                store,
                crate::tr!(
                    "Space 수집 중지 · 마지막 확인 기록",
                    "Space collection stopped · Last confirmed record"
                ),
            )?;
        }
    }
    Ok(())
}

pub fn ensure_enabled(store: &SpaceStore) -> AppResult<()> {
    if !load(store)?.enabled {
        return Err(crate::tr!("이 탭의 Maeklog가 꺼져 있습니다. prefix + m으로 다시 켜면 최근 완료 대화 최대 3턴부터 정리합니다.", "Maeklog is off in this tab. Press prefix + m to resume with up to 3 recent completed turns.").into());
    }
    Ok(())
}

pub struct Lease<'a> {
    pub store: &'a SpaceStore,
    generation: u64,
    pub automatic_until: Option<u64>,
}

impl<'a> Lease<'a> {
    pub fn new(store: &'a SpaceStore) -> AppResult<Option<Self>> {
        let value = load(store)?;
        Ok(value.enabled.then_some(Self {
            store,
            generation: value.generation,
            automatic_until: None,
        }))
    }

    pub fn cancelled(&self) -> bool {
        load(self.store).map_or(true, |value| {
            !value.enabled
                || value.generation != self.generation
                || self
                    .automatic_until
                    .is_some_and(|end| crate::storage::unix_now() >= end)
        })
    }

    pub fn guard(&self) -> AppResult<File> {
        let guard = lock(self.store)?;
        if self.cancelled() {
            return Err(
                "Maeklog work cancelled by Space preference change; records retained".into(),
            );
        }
        Ok(guard)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn opt_out_persists_and_invalidates_inflight_work_even_after_reenable() {
        let root = std::env::temp_dir().join(format!("pm-optout-{}", std::process::id()));
        let store = SpaceStore::for_workspace(&root, "test", "w1", "/repo", None).unwrap();
        let old = Lease::new(&store).unwrap().unwrap();
        assert!(!load(&store).unwrap().history_blocked);
        set_enabled(&store, false).unwrap();
        assert!(old.guard().is_err());
        let reopened = SpaceStore::for_workspace(&root, "test", "w1", "/repo", None).unwrap();
        assert!(!load(&reopened).unwrap().enabled);
        set_enabled(&reopened, true).unwrap();
        assert!(old.cancelled());
        assert!(load(&reopened).unwrap().history_blocked);
        assert!(!Lease::new(&reopened).unwrap().unwrap().cancelled());
        std::fs::remove_dir_all(root).unwrap();
    }
}
