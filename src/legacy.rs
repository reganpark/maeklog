//! Legacy snapshots remain readable as snapshots, never promoted to confirmed facts.
use crate::AppResult;
use crate::storage::{SpaceStore, atomic_write, atomic_write_json, lock_file};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Snapshot {
    pub revision: u64,
    pub summary: crate::model::Summary,
}
fn dir(store: &SpaceStore) -> PathBuf {
    store.state_root.join("legacy-snapshots").join(&store.key)
}
pub fn preserve(store: &SpaceStore) -> AppResult<()> {
    let directory = dir(store);
    let _lock = lock_file(&directory.join(".migration.lock"), false)?;
    if directory.join("index.json").exists() {
        return Ok(());
    }
    for name in ["current.json", "timeline.jsonl"] {
        let source = store.path(name);
        if source.is_file() {
            atomic_write(&directory.join(name), &std::fs::read(source)?)?;
        }
    }
    atomic_write_json(
        &directory.join("index.json"),
        &serde_json::json!({"format":"legacy-summary-snapshots","source_space":store.key,"branch_assignment":"unlinked","individual_records_inferred":false}),
    )
}
pub fn read(store: &SpaceStore) -> AppResult<Vec<Snapshot>> {
    let directory = if dir(store).join("index.json").exists() {
        dir(store)
    } else {
        store.directory()
    };
    let mut values = Vec::new();
    for name in ["timeline.jsonl", "current.json"] {
        let path = directory.join(name);
        if !path.is_file() {
            continue;
        }
        let text = std::fs::read_to_string(path)?;
        if name.ends_with("jsonl") {
            for line in text.lines() {
                if let Ok(value) = serde_json::from_str::<Snapshot>(line) {
                    values.push(value);
                }
            }
        } else if let Ok(value) = serde_json::from_str::<Snapshot>(&text) {
            values.push(value);
        }
    }
    values.retain(|s| s.revision > 0);
    values.sort_by_key(|s| s.revision);
    values.dedup_by_key(|s| s.revision);
    values.reverse();
    Ok(values)
}
#[cfg(test)]
pub fn record_snapshot(store: &SpaceStore, state: &crate::model::CurrentState) -> AppResult<()> {
    // v3 deltas are separately preserved; keep imported/old-schema prose as an
    // explicitly unlinked snapshot, not an inferred decision/completion record.
    if !state.summary.record_updates.is_empty() || !state.summary.state_updates.is_empty() {
        return Ok(());
    }
    let directory = dir(store);
    let _lock = lock_file(&directory.join(".migration.lock"), false)?;
    crate::storage::append_json_line(&directory.join("timeline.jsonl"), state)
}
pub fn clear(store: &SpaceStore) -> AppResult<()> {
    for name in ["index.json", "current.json", "timeline.jsonl"] {
        let path = dir(store).join(name);
        if path.is_file() {
            std::fs::remove_file(path)?;
        }
    }
    Ok(())
}

pub fn raw(store: &SpaceStore) -> AppResult<std::collections::BTreeMap<String, String>> {
    let directory = if dir(store).join("index.json").exists() {
        dir(store)
    } else {
        store.directory()
    };
    let mut raw = std::collections::BTreeMap::new();
    for name in ["index.json", "current.json", "timeline.jsonl"] {
        if directory.join(name).is_file() {
            raw.insert(name.into(), std::fs::read_to_string(directory.join(name))?);
        }
    }
    Ok(raw)
}
