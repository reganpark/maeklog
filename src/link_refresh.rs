//! Background GitHub refresh owned by the interactive panel, never by rendering.
use crate::{AppResult, branch_context::Context, storage::SpaceStore, task_links, work_links};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const DISPATCH_INTERVAL: Duration = Duration::from_secs(10);

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TaskKey {
    revision: u64,
    generation: u64,
    thread_id: Option<String>,
}

impl TaskKey {
    fn of(task: &task_links::TaskLinks) -> Self {
        Self {
            revision: task.revision,
            generation: task.generation,
            thread_id: task.thread_id.clone(),
        }
    }

    pub fn matches(&self, task: &task_links::TaskLinks) -> bool {
        *self == Self::of(task)
    }
}

#[derive(Default)]
pub struct Refresh {
    active: Option<Job>,
    pending: Option<(Context, TaskKey, bool)>,
    dispatched: Option<(Context, TaskKey, Instant)>,
}

struct Job {
    scope: Context,
    task: TaskKey,
    explicit: bool,
    handle: JoinHandle<AppResult<work_links::RefreshOutcome>>,
}

impl Refresh {
    pub fn request(
        &mut self,
        store: &SpaceStore,
        scope: &Context,
        task: &task_links::TaskLinks,
        explicit: bool,
    ) -> AppResult<()> {
        let task = TaskKey::of(task);
        if let Some(active) = &self.active {
            if same_association(&active.scope, scope) {
                let same_pending = self
                    .pending
                    .as_ref()
                    .filter(|(queued, _, _)| same_association(queued, scope));
                let force = same_pending.is_some_and(|(_, _, force)| *force)
                    || explicit && (!active.explicit || active.task != task);
                if same_pending.is_some()
                    || force
                    || active.scope.head != scope.head
                    || active.task != task
                {
                    // HEAD can advance while a chooser is open. Keep the force
                    // request but attach it to the newest same-branch snapshot.
                    self.pending = Some((scope.clone(), task, force));
                } else {
                    self.pending = None;
                }
                return Ok(());
            }
            // A checkout during a fetch keeps only the newest requested scope.
            // The old result is never applied directly to the current screen.
            let explicit = explicit
                || self
                    .pending
                    .as_ref()
                    .is_some_and(|(queued, _, force)| same_association(queued, scope) && *force);
            self.pending = Some((scope.clone(), task, explicit));
            return Ok(());
        }
        if !explicit
            && self.dispatched.as_ref().is_some_and(|(old, revision, at)| {
                old == scope && *revision == task && at.elapsed() < DISPATCH_INTERVAL
            })
        {
            return Ok(());
        }
        self.start(store.clone(), scope.clone(), task, explicit)
    }

    /// A newly selected reference may not be in the running fetch's input.
    /// Queue one metadata refresh after it, preserving the newer local choice.
    pub fn after_change(
        &mut self,
        store: &SpaceStore,
        scope: &Context,
        task: &task_links::TaskLinks,
    ) -> AppResult<()> {
        self.dispatched = None;
        if self.active.is_some() {
            self.pending = Some((scope.clone(), TaskKey::of(task), true));
            Ok(())
        } else {
            self.request(store, scope, task, true)
        }
    }

    fn start(
        &mut self,
        store: SpaceStore,
        scope: Context,
        task: TaskKey,
        explicit: bool,
    ) -> AppResult<()> {
        let captured = scope.clone();
        let handle = std::thread::Builder::new()
            .name("pm-link-refresh".into())
            .spawn(move || {
                let activation_error = crate::client_routing::activate_attached_task(&store).err();
                let report_error = crate::task_report::drain(&store).err().or(activation_error);
                let task = task_links::load(&store)?;
                let signal = !task.current.is_empty()
                    || !task.related_pr.is_empty()
                    || !task.candidates.is_empty()
                    || task.explicit_none;
                let task = task_links::refresh(&store, explicit)?;
                let result = if signal {
                    task
                } else if explicit {
                    work_links::refresh_forced(&store.state_root, &captured)?
                } else {
                    work_links::refresh_if_due(&store.state_root, &captured)?
                };
                if let Some(error) = report_error {
                    return Err(crate::tr!("작업 인계 수신 실패 · {error}", "Work handoff failed · {error}";).into());
                }
                Ok(result)
            })?;
        self.dispatched = Some((scope.clone(), task.clone(), Instant::now()));
        self.active = Some(Job {
            scope,
            task,
            explicit,
            handle,
        });
        Ok(())
    }

    /// Poll without waiting. Expected GitHub failures live in the cache; report
    /// unexpected failures such as an unwritable cache even for automatic work.
    pub fn poll(
        &mut self,
        store: &SpaceStore,
    ) -> Option<(Context, TaskKey, AppResult<work_links::RefreshOutcome>)> {
        if !self
            .active
            .as_ref()
            .is_some_and(|job| job.handle.is_finished())
        {
            return None;
        }
        let job = self.active.take()?;
        let result = job.handle.join().unwrap_or_else(|_| {
            Err(crate::tr!(
                "연결 조회 작업이 중단되었습니다 · v로 재시도",
                "Link lookup interrupted · v to retry"
            )
            .into())
        });
        let outcome = (job.explicit || result.is_err()).then_some((job.scope, job.task, result));
        if let Some((scope, task, explicit)) = self.pending.take()
            && let Err(error) = self.start(store.clone(), scope.clone(), task.clone(), explicit)
        {
            return Some((scope, task, Err(error)));
        }
        outcome
    }
}

fn same_association(a: &Context, b: &Context) -> bool {
    a.key == b.key
        && a.repository == b.repository
        && a.branch == b.branch
        && a.incarnation == b.incarnation
        && a.worktree == b.worktree
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope(key: &str) -> Context {
        Context {
            key: key.into(),
            ..Context::default()
        }
    }

    fn store() -> SpaceStore {
        SpaceStore {
            state_root: "/unused".into(),
            key: "unused".into(),
        }
    }

    fn task(revision: u64) -> task_links::TaskLinks {
        let mut task = task_links::TaskLinks::default();
        task.revision = revision;
        task
    }

    fn active(scope: Context) -> Refresh {
        Refresh {
            active: Some(Job {
                scope,
                task: TaskKey::default(),
                explicit: false,
                handle: std::thread::spawn(|| Ok(work_links::RefreshOutcome::Skipped)),
            }),
            ..Refresh::default()
        }
    }

    #[test]
    fn repeated_requests_coalesce_and_latest_checkout_replaces_pending_scope() {
        let a = scope("a");
        let b = scope("b");
        let mut refresh = active(a.clone());
        refresh.request(&store(), &a, &task(0), true).unwrap();
        refresh.request(&store(), &a, &task(0), true).unwrap();
        assert_eq!(refresh.pending, Some((a.clone(), TaskKey::default(), true)));
        refresh.request(&store(), &b, &task(0), true).unwrap();
        refresh.request(&store(), &b, &task(0), false).unwrap();
        assert_eq!(refresh.pending, Some((b, TaskKey::default(), true)));
        refresh.request(&store(), &a, &task(0), false).unwrap();
        assert!(refresh.pending.is_none());
    }

    #[test]
    fn manual_selection_queues_one_follow_up_and_timer_keeps_it() {
        let a = scope("a");
        let mut refresh = active(a.clone());
        refresh.after_change(&store(), &a, &task(0)).unwrap();
        refresh.after_change(&store(), &a, &task(0)).unwrap();
        refresh.request(&store(), &a, &task(0), false).unwrap();
        assert_eq!(refresh.pending, Some((a, TaskKey::default(), true)));
    }

    #[test]
    fn same_branch_head_advance_preserves_forced_selection_refresh_with_newest_head() {
        let mut old = scope("a");
        old.head = Some("old-head".into());
        let mut newest = old.clone();
        newest.head = Some("new-head".into());
        let mut refresh = active(newest.clone());
        refresh.after_change(&store(), &old, &task(0)).unwrap();
        refresh.request(&store(), &newest, &task(0), false).unwrap();
        refresh.request(&store(), &newest, &task(0), false).unwrap();
        assert_eq!(refresh.pending, Some((newest, TaskKey::default(), true)));
    }

    #[test]
    fn auto_dispatch_is_throttled_without_starting_a_worker() {
        let a = scope("a");
        let mut refresh = Refresh {
            dispatched: Some((a.clone(), TaskKey::default(), Instant::now())),
            ..Refresh::default()
        };
        refresh.request(&store(), &a, &task(0), false).unwrap();
        assert!(refresh.active.is_none());
    }

    #[test]
    fn unexpected_automatic_failure_is_reported_without_starting_a_fetch() {
        let a = scope("a");
        let handle = std::thread::spawn(|| Err("cache not writable".into()));
        while !handle.is_finished() {
            std::thread::yield_now();
        }
        let mut refresh = Refresh {
            active: Some(Job {
                scope: a.clone(),
                task: TaskKey::default(),
                explicit: false,
                handle,
            }),
            ..Refresh::default()
        };
        let (reported, _, result) = refresh.poll(&store()).unwrap();
        assert_eq!(reported, a);
        assert_eq!(result.unwrap_err().to_string(), "cache not writable");
    }

    #[test]
    fn newer_dialogue_target_keeps_one_forced_follow_up_even_during_same_branch_fetch() {
        let a = scope("a");
        let mut refresh = active(a.clone());
        refresh.after_change(&store(), &a, &task(1)).unwrap();
        refresh.request(&store(), &a, &task(2), false).unwrap();
        refresh.request(&store(), &a, &task(2), false).unwrap();
        assert_eq!(refresh.pending, Some((a, TaskKey::of(&task(2)), true)));
    }

    #[test]
    fn explicit_refresh_for_new_target_is_not_covered_by_an_old_explicit_fetch() {
        let a = scope("a");
        let mut refresh = active(a.clone());
        refresh.active.as_mut().unwrap().explicit = true;
        refresh.request(&store(), &a, &task(1), true).unwrap();
        assert_eq!(refresh.pending, Some((a, TaskKey::of(&task(1)), true)));
    }

    #[test]
    fn thread_and_generation_changes_are_distinct_even_for_the_same_branch_and_revision() {
        let a = scope("a");
        let mut refresh = active(a.clone());
        let mut next = task(0);
        next.thread_id = Some("new-thread".into());
        next.generation = 2;
        refresh.request(&store(), &a, &next, false).unwrap();
        assert_eq!(refresh.pending, Some((a, TaskKey::of(&next), false)));
        assert!(!TaskKey::default().matches(&next));
    }
}
