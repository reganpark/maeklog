//! Explicit local inspection/deletion of saved scopes, including detached history.
use crate::storage::{atomic_write_json, read_json_optional};
use crate::{AppResult, branch_context as context, storage::SpaceStore};

pub fn scopes(root: &std::path::Path) -> AppResult<Vec<context::Context>> {
    let directory = root.join("branches");
    if !directory.exists() {
        return Ok(Vec::new());
    }
    let mut scopes = Vec::new();
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        if let Some(scope) =
            read_json_optional::<context::Context>(&entry.path().join("context.json"))?
        {
            if entry.file_name().to_string_lossy() != scope.key {
                return Err("scope directory identity mismatch".into());
            }
            scopes.push(scope);
        }
    }
    scopes.sort_by(|a, b| a.key.cmp(&b.key));
    Ok(scopes)
}

pub fn clear(root: &std::path::Path, scope: &context::Context) -> AppResult<()> {
    // Lock existing participation gates, then fail fast if a reducer is in flight.
    // Never disable a peer Space as a side effect of deletion.
    let stores = SpaceStore::all(root)?;
    let _guards = stores
        .iter()
        .map(crate::participation::lock)
        .collect::<AppResult<Vec<_>>>()?;
    for store in &stores {
        let contributes = context::sources(store)?
            .values()
            .any(|s| s.context.key == scope.key)
            || store.meta()?.is_some_and(|m| {
                context::detect_for_store(store, &m.cwd).is_ok_and(|c| c.key == scope.key)
            });
        if contributes && crate::participation::load(store)?.enabled {
            return Err(format!(
                "Space {} is enabled; turn off every contributing Space first",
                store.key
            )
            .into());
        }
        if store
            .unprocessed_events()?
            .iter()
            .any(|e| context::source(store, e).is_ok_and(|s| s.context.key == scope.key))
        {
            return Err(format!(
                "Space {} has pending input for this scope; clear its private records first",
                store.key
            )
            .into());
        }
    }
    let dir = context::directory(root, scope);
    let _reduce =
        crate::storage::try_lock_file(&dir.join(".reduce.lock"))?.ok_or("scope reducer is busy")?;
    crate::maintenance::clear_scope_files(root, scope)?;
    atomic_write_json(
        &dir.join("deletion.json"),
        &serde_json::json!({
            "scope":scope.key,"explicit_shared_deletion":true,"receipts_retained_for_deduplication":true
        }),
    )
}

pub fn cli(args: &[String]) -> AppResult<()> {
    let root = crate::config::state_root()?;
    let op = args.first().map_or("list", String::as_str);
    if op == "list" {
        println!("{}", serde_json::to_string_pretty(&scopes(&root)?)?);
        for store in SpaceStore::all(&root)? {
            println!(
                "legacy {}: {} snapshots",
                store.key,
                crate::legacy::read(&store)?.len()
            );
        }
        return Ok(());
    }
    let key = args.get(1).ok_or(
        "use memory <show|history|clear> <exact-scope-key>, or memory legacy <exact-space-key>",
    )?;
    if op == "legacy" {
        let store = SpaceStore::all(&root)?
            .into_iter()
            .find(|s| &s.key == key)
            .ok_or("unknown Space key")?;
        println!(
            "{}",
            serde_json::to_string_pretty(
                &serde_json::json!({"snapshots":crate::legacy::read(&store)?,"original_files":crate::legacy::raw(&store)?})
            )?
        );
        return Ok(());
    }
    let scope = scopes(&root)?
        .into_iter()
        .find(|s| &s.key == key)
        .ok_or("unknown exact scope key")?;
    match op {
        "show" => println!(
            "{}",
            serde_json::to_string_pretty(&crate::journal::load(&root, &scope)?)?
        ),
        "history" => {
            let snapshot = crate::branch_panel::saved(&root, scope)?;
            println!(
                "{}",
                crate::branch_panel::render(
                    &snapshot,
                    80,
                    &crate::branch_panel::View {
                        expanded: true,
                        page: args
                            .get(2)
                            .map(|v| v.parse::<usize>())
                            .transpose()?
                            .unwrap_or(1)
                            .saturating_sub(1)
                    }
                )
                .join("\n")
            );
        }
        "clear" => {
            if args.get(2).map(String::as_str) != Some("--confirm-shared")
                || args.get(3) != Some(key)
            {
                return Err("This deletes the shared branch journal and GitHub cache for ALL Spaces. Repeat the exact key: memory clear KEY --confirm-shared KEY. No built-in undo; back up first. All contributing Spaces must be off and their queues empty.".into());
            }
            clear(&root, &scope)?;
            println!(
                "Deleted shared journal and links for {key}; all Spaces are affected. Dedup tombstones retained. No built-in undo."
            );
        }
        _ => return Err("unknown memory operation".into()),
    }
    Ok(())
}
