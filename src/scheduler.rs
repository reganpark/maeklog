//! One coordinator, at most three independent single-attempt space workers.
use crate::AppResult;
use crate::storage::{SpaceStore, try_lock_file, unix_now};
use crate::{config, worker};
use std::collections::HashSet;
use std::fs::File;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

pub const MAX_PARALLEL: usize = 3;

pub fn run() -> AppResult<()> {
    use std::os::unix::process::CommandExt;
    let root = config::state_root()?;
    let Some(_coordinator) = try_lock_file(&root.join("worker.lock"))? else {
        return Ok(());
    };
    let mut active: Vec<(String, Child)> = Vec::new();
    let mut failed = HashSet::new();
    let mut last_dispatched = String::new();
    loop {
        let mut i = 0;
        while i < active.len() {
            if let Some(status) = active[i].1.try_wait()? {
                let (key, _) = active.swap_remove(i);
                if !status.success() {
                    failed.insert(key);
                }
            } else {
                i += 1;
            }
        }
        let mut stores = SpaceStore::all(&root)?;
        // Rotate across rounds: a busy early space cannot continually take the free slot.
        let start = stores
            .iter()
            .position(|s| s.key > last_dispatched)
            .unwrap_or(0);
        stores.rotate_left(start);
        let mut pending = !active.is_empty();
        for store in stores {
            if failed.contains(&store.key) || active.iter().any(|(key, _)| key == &store.key) {
                continue;
            }
            // A space worker can outlive a crashed coordinator and still owns its lease.
            if try_lock_file(&store.path(".worker.lock"))?.is_none() {
                pending = true;
                continue;
            }
            let due = match next_attempt_at(&store) {
                Ok(due) => due,
                Err(error) => {
                    eprintln!("maeklog: cannot schedule {}: {error}", store.key);
                    failed.insert(store.key);
                    continue;
                }
            };
            let Some(due) = due else {
                continue;
            };
            pending = true;
            if due > unix_now() || active.len() >= MAX_PARALLEL || !slot_available(&root)? {
                continue;
            }
            let child = Command::new(std::env::current_exe()?)
                .args(["worker-space", &store.key])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .process_group(0)
                .spawn()?;
            last_dispatched.clone_from(&store.key);
            active.push((store.key, child));
        }
        if !pending {
            return Ok(());
        }
        // Retry waits do not occupy a space worker or a model slot.
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn slot_available(root: &Path) -> AppResult<bool> {
    for index in 0..MAX_PARALLEL {
        if try_lock_file(&root.join(format!("worker-slots/{index}.lock")))?.is_some() {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(crate) fn next_attempt_at(store: &SpaceStore) -> AppResult<Option<u64>> {
    let completed = worker::next_attempt_at(store)?;
    let requested = crate::live_summary::next_attempt_at(store)?;
    Ok(match (completed, requested) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(at), None) | (None, Some(at)) => Some(at),
        (None, None) => None,
    })
}

fn claim(store: &SpaceStore) -> AppResult<Option<(File, File)>> {
    let Some(space) = try_lock_file(&store.path(".worker.lock"))? else {
        return Ok(None);
    };
    for index in 0..MAX_PARALLEL {
        if let Some(slot) =
            try_lock_file(&store.state_root.join(format!("worker-slots/{index}.lock")))?
        {
            return Ok(Some((space, slot)));
        }
    }
    Ok(None)
}

pub fn run_space(key: Option<&str>) -> AppResult<()> {
    let key = key.ok_or("worker-space requires a registered space key")?;
    // Resolve only registered directories; never join an untrusted CLI path.
    let store = SpaceStore::all(&config::state_root()?)?
        .into_iter()
        .find(|store| store.key == key)
        .ok_or("space is not registered")?;
    let Some(_lease) = claim(&store)? else {
        return Ok(());
    };
    let ordinary_due = worker::next_attempt_at(&store)?.is_some_and(|at| at <= unix_now());
    let result = if ordinary_due {
        worker::attempt_space(&store).map(|_| ())
    } else {
        crate::live_summary::attempt_space(&store)
    };
    if let Err(error) = &result {
        let _ = store.set_status(
            "error",
            &crate::tr!("요약 작업 오류 · 기록 보존 · r 재시도 · {error}", "Summary job failed · Records retained · r to retry · {error}";),
        );
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn leases_enforce_three_spaces_and_one_attempt_per_space() {
        let root = std::env::temp_dir().join(format!("pm-scheduler-test-{}", std::process::id()));
        let stores: Vec<_> = (0..4)
            .map(|i| {
                SpaceStore::for_workspace(&root, "test", &format!("w{i}"), "/repo", None).unwrap()
            })
            .collect();
        let one = claim(&stores[0]).unwrap().unwrap();
        assert!(claim(&stores[0]).unwrap().is_none());
        let two = claim(&stores[1]).unwrap().unwrap();
        let three = claim(&stores[2]).unwrap().unwrap();
        assert!(claim(&stores[3]).unwrap().is_none());
        assert!(!slot_available(&root).unwrap());
        drop(two);
        let four = claim(&stores[3]).unwrap().unwrap();
        drop((one, three, four));
        assert!(slot_available(&root).unwrap());
        std::fs::remove_dir_all(root).unwrap();
    }
}
