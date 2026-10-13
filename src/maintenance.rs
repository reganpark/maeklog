//! Explicit record deletion, retaining only Space identity and the off preference.
use crate::storage::{SpaceStore, atomic_write_json, lock_file, read_json_or_default};
use crate::{AppResult, participation};
use std::collections::HashSet;

pub fn clear_records(store: &SpaceStore) -> AppResult<usize> {
    participation::set_enabled(store, false)?;
    let _worker = lock_file(&store.path(".worker.lock"), false)?;
    let _participation = participation::lock(store)?;
    let _commit = lock_file(&store.path(".commit.lock"), false)?;
    let _events = lock_file(&store.path(".event.lock"), false)?;
    crate::task_report::clear_locked(store)?;
    crate::legacy::clear(store)?;
    // Only this Space's unlinked bucket is private. Shared branch journals and
    // their event receipts deliberately survive the existing Space command.
    let scope = crate::branch_context::Context {
        key: format!("unlinked-{}", crate::branch_context::hash(&store.key)),
        ..Default::default()
    };
    clear_scope_files(&store.state_root, &scope)?;
    let connection: crate::connection::Connection =
        read_json_or_default(&store.path("connection.json"))?;
    let mut sessions: HashSet<String> = connection.prior_sessions.into_iter().collect();
    sessions.insert(connection.session_id);
    if let Some(session) = store.latest_session_id()? {
        sessions.insert(session);
    }
    sessions.extend(store.events()?.into_iter().map(|e| e.session_id));
    sessions.remove("");
    let diagnostics = store.state_root.join("runtime/diagnostics");
    let path = diagnostics.join("action-timing.json");
    if path.is_file() {
        let _diagnostic = lock_file(&diagnostics.join(".action-timing.lock"), false)?;
        let mut entries: Vec<serde_json::Value> = read_json_or_default(&path)?;
        entries.retain(|v| {
            !v.get("session_id")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|s| sessions.contains(s))
        });
        atomic_write_json(&path, &entries)?;
    }
    let mut count = 0;
    for entry in std::fs::read_dir(store.directory())? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        // Never replace lock inodes or remove the opt-out tombstone.
        if name.starts_with('.')
            || ["meta.json", "ui.json", "participation.json"].contains(&name.as_ref())
        {
            continue;
        }
        if !entry.file_type()?.is_file() {
            return Err("unexpected non-file in Maeklog record directory".into());
        }
        std::fs::remove_file(entry.path())?;
        count += 1;
    }
    let mut preference = participation::load(store)?;
    preference.open_turns.clear();
    participation::save(store, &preference)?;
    Ok(count)
}

pub fn clear_scope_files(
    root: &std::path::Path,
    scope: &crate::branch_context::Context,
) -> AppResult<()> {
    let dir = crate::branch_context::directory(root, scope);
    let _capture = lock_file(&dir.join(".capture.lock"), false)?;
    let _journal = lock_file(&dir.join(".journal.lock"), false)?;
    let _links = lock_file(&dir.join(".links.lock"), false)?;
    // Receipts are retained as dedup tombstones: deleting visible records must
    // not let another Space replay the very same collected event into them.
    let old = crate::journal::load(root, scope)?;
    if dir.join("journal.json").is_file() {
        atomic_write_json(
            &dir.join("journal.json"),
            &crate::journal::Journal {
                revision: old.revision + 1,
                correction_revision: old.correction_revision + 1,
                through_order: old.through_order,
                applied: old.applied,
                processed_events: old.processed_events,
                ..Default::default()
            },
        )?;
    }
    for name in ["links.json"] {
        match std::fs::remove_file(dir.join(name)) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deletion_is_scoped_and_preserves_off_tombstone_and_peer_diagnostics() {
        let root = std::env::temp_dir().join(format!("pm-clear-test-{}", std::process::id()));
        let target = SpaceStore::for_workspace(&root, "test", "target", "/target", None).unwrap();
        let peer = SpaceStore::for_workspace(&root, "test", "peer", "/peer", None).unwrap();
        target.remember_session("target-session").unwrap();
        let current = crate::model::CurrentState::default();
        atomic_write_json(&target.path("current.json"), &current).unwrap();
        atomic_write_json(&peer.path("current.json"), &current).unwrap();
        crate::task_links::activate(&target, "target-thread").unwrap();
        crate::task_links::set_manual(
            &target,
            crate::task_links::load(&target).unwrap().revision,
            "owner/repo#142",
        )
        .unwrap();
        crate::task_links::activate(&peer, "peer-thread").unwrap();
        crate::task_links::set_manual(
            &peer,
            crate::task_links::load(&peer).unwrap().revision,
            "owner/repo#156",
        )
        .unwrap();
        let peer_task = std::fs::read(peer.path("task-links.json")).unwrap();
        let target_meta = std::fs::read(target.path("meta.json")).unwrap();
        atomic_write_json(
            &root.join("runtime/diagnostics/action-timing.json"),
            &serde_json::json!([
                {"session_id":"target-session"},{"session_id":"peer-session"}
            ]),
        )
        .unwrap();
        let lock = lock_file(&target.path(".event.lock"), false).unwrap();
        drop(lock);
        // Ordinary opt-out retains the selected conversation work. Explicit deletion
        // removes that private task state alongside the existing collected records.
        participation::set_enabled(&target, false).unwrap();
        assert_eq!(
            crate::task_links::load(&target).unwrap().current[0].id,
            "owner/repo#142"
        );
        let private_records = ["current.json", "session.json", "task-links.json"];
        for name in private_records {
            assert!(target.path(name).is_file(), "fixture must include {name}");
        }
        assert_eq!(clear_records(&target).unwrap(), private_records.len());
        for name in private_records {
            assert!(
                !target.path(name).exists(),
                "private {name} must be deleted"
            );
        }
        assert!(target.path(".event.lock").exists());
        assert_eq!(
            std::fs::read(target.path("meta.json")).unwrap(),
            target_meta
        );
        assert!(crate::task_links::load(&target).unwrap().current.is_empty());
        assert_eq!(crate::task_links::active_thread(&target).unwrap(), None);
        assert!(!participation::load(&target).unwrap().enabled);
        assert!(peer.path("current.json").exists());
        assert_eq!(
            std::fs::read(peer.path("task-links.json")).unwrap(),
            peer_task
        );
        assert_eq!(
            crate::task_links::load(&peer).unwrap().current[0].id,
            "owner/repo#156"
        );
        assert!(participation::load(&peer).unwrap().enabled);
        let diagnostics: Vec<serde_json::Value> =
            read_json_or_default(&root.join("runtime/diagnostics/action-timing.json")).unwrap();
        assert_eq!(
            diagnostics,
            vec![serde_json::json!({"session_id":"peer-session"})]
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
