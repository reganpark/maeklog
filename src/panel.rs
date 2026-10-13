use crate::AppResult;
use crate::config;
use crate::herdr;
use crate::model::{
    CurrentState, DecisionStatus, EventKind, ProgressStatus, RepoSnapshot, WorkerStatus,
};
use crate::panel_style::{self, Line, Role};
use crate::repo_facts;
use crate::storage::{SpaceStore, lock_file, try_lock_file, unix_now};
use crate::worker;
use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event, KeyCode, KeyEventKind, MouseButton, MouseEventKind,
};
use crossterm::style::{
    Attribute, Print, ResetColor, SetAttribute, SetBackgroundColor, SetForegroundColor,
};
use crossterm::terminal::{
    self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode,
    enable_raw_mode,
};
use crossterm::{execute, queue};
use std::io::{IsTerminal, Write};
use std::time::{Duration, Instant};
use unicode_width::UnicodeWidthChar;
mod reader;

const REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const MAX_VISIBLE_DECISIONS: usize = 6;
const MAX_VISIBLE_PROGRESS: usize = 6;
const MAX_VISIBLE_BLOCKERS: usize = 3;
const MAX_VISIBLE_ACTIONS: usize = 4;

#[derive(Clone, Debug)]
struct PanelSnapshot {
    branch: Option<crate::branch_panel::Snapshot>,
    connection_notice: Option<String>,
    label: String,
    agent_status: String,
    agent_title: Option<String>,
    current_request: Option<String>,
    live: Option<crate::live_turn::Snapshot>,
    catch_up: Option<crate::live_summary::View>,
    pending: Pending,
    worker: WorkerStatus,
    current: CurrentState,
    live_repo: RepoSnapshot,
    captured_at: u64,
    display_times: crate::local_time::LocalTimes,
}

#[derive(Clone, Debug, Default)]
struct Pending {
    total: usize,
    completed: usize,
    today: usize,
    rebuild: bool,
    worker_alive: bool,
    preview: Option<Preview>,
    latest_work: bool,
    newer_events: usize,
}

#[derive(Clone, Debug)]
struct Preview {
    cumulative_through: Option<u64>,
}

impl Pending {
    fn from_events(events: &[crate::model::TurnEvent], now: u64) -> Self {
        Self {
            total: events.len(),
            completed: events
                .iter()
                .rposition(|event| {
                    matches!(
                        event.kind,
                        EventKind::AssistantFinal | EventKind::Interrupted
                    )
                })
                .map_or(0, |index| index + 1),
            today: events
                .iter()
                .filter(|event| {
                    event.recorded_at_unix.saturating_add(32_400) / 86_400
                        == now.saturating_add(32_400) / 86_400
                })
                .count(),
            ..Self::default()
        }
    }
}

pub fn run(render_once: bool) -> AppResult<()> {
    run_view(render_once, false)
}

pub fn run_view(render_once: bool, expanded: bool) -> AppResult<()> {
    let state_root = config::state_root()?;
    let store = SpaceStore::for_caller(&state_root)?;
    crate::participation::ensure_enabled(&store)?;

    if render_once || !std::io::stdout().is_terminal() {
        let snapshot = load_snapshot(&store)?;
        for line in render_view(
            &snapshot,
            52,
            &crate::branch_panel::View { expanded, page: 0 },
        ) {
            println!("{line}");
        }
        return Ok(());
    }

    let mut ui = store.ui()?;
    ui.visible = true;
    if let Ok(pane_id) = std::env::var("HERDR_PANE_ID") {
        ui.panel_pane_id = Some(pane_id);
    }
    store.set_ui(ui)?;

    // Restore terminal/mouse state before the action closes this very pane.
    match interactive(&store)? {
        Some(crate::panel_menu::Action::Disable) => crate::actions::run(Some("disable")),
        Some(crate::panel_menu::Action::Hide) => crate::actions::run(Some("hide")),
        Some(
            crate::panel_menu::Action::Records
            | crate::panel_menu::Action::Progress
            | crate::panel_menu::Action::Evidence
            | crate::panel_menu::Action::OpenLinks
            | crate::panel_menu::Action::Retry
            | crate::panel_menu::Action::History
            | crate::panel_menu::Action::CatchUp
            | crate::panel_menu::Action::Links
            | crate::panel_menu::Action::RefreshLinks
            | crate::panel_menu::Action::Model
            | crate::panel_menu::Action::Language,
        )
        | None => Ok(()),
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "One event loop owns menu, selection and history lifetimes; gesture routing must precede shortcut actions"
)]
fn interactive(store: &SpaceStore) -> AppResult<Option<crate::panel_menu::Action>> {
    let _guard = TerminalGuard::enter()?;
    let mut scroll = 0_usize;
    let now = Instant::now();
    let mut last_refresh = now.checked_sub(REFRESH_INTERVAL).unwrap_or(now);
    let mut last_recovery = now.checked_sub(Duration::from_secs(15)).unwrap_or(now);
    let mut snapshot = load_snapshot(store)?;
    let mut action_notice: Option<(String, Role)> = None;
    let mut menu: Option<crate::panel_menu::Menu> = None;
    let mut view = crate::branch_panel::View::default();
    let mut picker = crate::day_picker::Picker::default();
    let mut selection: Option<crate::panel_selection::Selection> = None;
    let mut editor: Option<crate::record_editor::Editor> = None;
    let mut link_picker: Option<crate::link_picker::Picker> = None;
    let mut reader: Option<reader::Reader> = None;
    let mut reader_source: Option<(PanelSnapshot, bool)> = None;
    let mut model_picker: Option<crate::model_picker::Picker> = None;
    let mut language_picker: Option<crate::language_picker::Picker> = None;
    let mut ui_language = crate::i18n::current();
    let mut last_language_poll = force_refresh();
    let mut language_saved = false;
    let mut link_refresh = crate::link_refresh::Refresh::default();

    loop {
        if last_language_poll.elapsed() >= REFRESH_INTERVAL {
            // Translation reads stay in memory; only this bounded poll reads settings.
            let _ = crate::i18n::refresh_cached();
            if let Some(active) = &mut language_picker {
                active.poll_saved();
            }
            last_language_poll = Instant::now();
        }
        if ui_language != crate::i18n::current() {
            ui_language = crate::i18n::current();
            let had_selection = selection.take().is_some();
            action_notice = had_selection.then(|| {
                (
                    crate::tr!(
                        "언어 변경으로 화면 선택을 해제했습니다",
                        "Language changed; screen selection cleared"
                    )
                    .into(),
                    Role::Muted,
                )
            });
            snapshot = load_snapshot(store)?;
            if let (Some(active), Some((captured, progress))) = (&mut reader, &reader_source) {
                active.language_changed(open_reader(captured, *progress, active.return_scroll));
            }
            if let Some(active) = &mut model_picker {
                active.language_changed();
            }
            if let Some(active) = &mut link_picker {
                active.language_changed();
            }
            if let Some(active) = &mut editor {
                active.language_changed();
            }
        }
        if language_saved {
            action_notice = Some((
                crate::tr!(
                    "언어 설정 저장됨 · 모든 Maeklog 탭에 적용 · 기존 기록 유지",
                    "Language settings saved · All Maeklog tabs · Existing records retained"
                )
                .into(),
                Role::Muted,
            ));
            language_saved = false;
        }
        if !crate::participation::load(store)?.enabled {
            return Ok(None);
        }
        if model_picker.is_none()
            && language_picker.is_none()
            && last_recovery.elapsed() >= Duration::from_secs(15)
        {
            let _ = worker::resume_pending(store);
            last_recovery = Instant::now();
        }
        if model_picker.is_none()
            && language_picker.is_none()
            && selection.is_none()
            && editor.is_none()
            && last_refresh.elapsed() >= REFRESH_INTERVAL
        {
            snapshot = load_snapshot(store)?;
            if let Some(branch) = &snapshot.branch {
                if let Err(error) = link_refresh.request(
                    store,
                    &branch.scope,
                    branch
                        .task
                        .as_ref()
                        .unwrap_or(&crate::task_links::TaskLinks::default()),
                    false,
                ) {
                    action_notice = Some((error.to_string(), Role::Warning));
                }
                if let Some(active) = &mut link_picker {
                    active.update(&branch.scope, &branch.links, branch.task.as_ref());
                }
            }
            last_refresh = Instant::now();
        }
        if let Some((scope, task, result)) = link_refresh.poll(store) {
            let relevant = snapshot.branch.as_ref().is_some_and(|branch| {
                task.matches(
                    branch
                        .task
                        .as_ref()
                        .unwrap_or(&crate::task_links::TaskLinks::default()),
                ) && (branch.scope.key == scope.key
                    || branch.task.as_ref().is_some_and(|task| {
                        !task.current.is_empty()
                            || !task.related_pr.is_empty()
                            || !task.candidates.is_empty()
                            || task.explicit_none
                    }))
            });
            if let Some(active) = &mut link_picker {
                active.refresh_finished(&task, &result);
            }
            if relevant {
                action_notice = Some(match result {
                    Ok(crate::work_links::RefreshOutcome::Refreshed) => (
                        if snapshot
                            .branch
                            .as_ref()
                            .and_then(|branch| branch.task.as_ref())
                            .is_some_and(|task| {
                                !task.current.is_empty()
                                    || !task.related_pr.is_empty()
                                    || !task.candidates.is_empty()
                            })
                        {
                            crate::tr!(
                                "현재 대화의 연결을 다시 확인했습니다",
                                "Current conversation links checked again"
                            )
                        } else {
                            crate::tr!(
                                "관찰 브랜치의 연결을 다시 확인했습니다",
                                "Observed branch links checked again"
                            )
                        }
                        .into(),
                        Role::Muted,
                    ),
                    Ok(crate::work_links::RefreshOutcome::Busy) => (
                        crate::tr!(
                            "다른 패널에서 연결 확인 중…",
                            "Another panel is checking links…"
                        )
                        .into(),
                        Role::Muted,
                    ),
                    Ok(crate::work_links::RefreshOutcome::Skipped) => (
                        crate::tr!(
                            "최근 연결 정보를 사용합니다",
                            "Using recent link information"
                        )
                        .into(),
                        Role::Muted,
                    ),
                    Err(error) => (
                        crate::tr!(
                            format!("작업 연결 확인 실패 · {error}"),
                            format!("Task link check failed · {error}")
                        ),
                        Role::Warning,
                    ),
                });
            } else if action_notice.as_ref().is_some_and(|(notice, _)| {
                notice == crate::tr!("작업 연결 확인 중…", "Checking task links…")
            }) {
                action_notice = None;
            }
            last_refresh = force_refresh();
        }
        if let Some(active) = &mut link_picker {
            active.poll_open();
        }
        if let Some(active) = &mut model_picker {
            active.poll();
        }
        let (width, height) = terminal::size()?;
        if let Some(branch) = &snapshot.branch {
            view.page = view.page.min(crate::branch_panel::max_page(branch));
        }
        let mut lines = Vec::new();
        lines.extend(picker.lines(usize::from(width)));
        if let Some((notice, role)) = &action_notice {
            panel_style::wrap(
                &mut lines,
                notice,
                usize::from(width).saturating_sub(2).max(10),
                *role,
            );
        }
        lines.extend(render_main(
            &snapshot,
            usize::from(width),
            usize::from(height),
        ));
        if let Some(selected) = &selection {
            lines.clone_from(&selected.lines);
            scroll = selected.scroll;
        }
        let body_height = usize::from(height).saturating_sub(1);
        scroll = scroll.min(lines.len().saturating_sub(body_height));
        if let Some(active) = &mut language_picker {
            let (frame, footer) = active.frame(usize::from(width), usize::from(height));
            draw(&frame, 0, width, height, None, Some(&footer))?;
        } else if let Some(active) = &mut model_picker {
            let (frame, footer) = active.frame(usize::from(width), usize::from(height));
            draw(&frame, 0, width, height, None, Some(&footer))?;
        } else if let Some(active) = &mut reader {
            let frame = active.frame(usize::from(width), usize::from(height));
            draw(
                &frame,
                0,
                width,
                height,
                active.selection(),
                Some(active.footer(usize::from(width))),
            )?;
        } else if let Some(active) = &link_picker {
            let (review, review_scroll) = active.frame(usize::from(width), usize::from(height));
            draw(
                &review,
                review_scroll,
                width,
                height,
                None,
                Some(active.footer()),
            )?;
        } else if let Some(editor) = &mut editor {
            let (review, review_scroll) = editor.frame(usize::from(width), usize::from(height));
            draw(
                &review,
                review_scroll,
                width,
                height,
                None,
                Some(editor.footer()),
            )?;
        } else {
            let footer = if menu.is_some() {
                crate::panel_menu::footer(usize::from(width))
            } else if picker.open {
                picker.footer(usize::from(width))
            } else {
                main_footer(lines.len(), scroll, usize::from(width), usize::from(height))
            };
            draw(
                &lines,
                scroll,
                width,
                height,
                selection.as_ref(),
                selection.is_none().then_some(footer.as_str()),
            )?;
        }
        if let Some(menu) = &menu {
            draw_menu(menu, height)?;
        }

        if !event::poll(Duration::from_millis(200))? {
            continue;
        }
        let event = event::read()?;
        if let Some(active) = &mut language_picker {
            let outcome = active.handle(&event);
            if outcome.close {
                language_saved = outcome.saved;
                language_picker = None;
                // Closing a preference view must not act as a work request.
                last_refresh = Instant::now();
                last_recovery = Instant::now();
            }
            continue;
        }
        if let Some(active) = &mut model_picker {
            let outcome = active.handle(&event);
            if outcome.close {
                if outcome.saved {
                    action_notice = Some((crate::tr!("요약 모델 설정을 저장했습니다 · 모든 Maeklog 탭의 다음 요청부터 적용", "Summary model settings saved · applies to all Maeklog tabs from the next request").into(), Role::Muted));
                }
                model_picker = None;
            }
            continue;
        }
        if let Some(active) = &mut reader {
            let close = active.handle(&event, usize::from(width), usize::from(height));
            if let Some(sequence) = active.take_copy() {
                let mut output = std::io::stdout();
                write!(output, "{sequence}")?;
                output.flush()?;
            }
            if close {
                scroll = active.return_scroll;
                reader = None;
                reader_source = None;
            }
            continue;
        }
        if let Some(active) = &mut link_picker {
            let outcome = active.handle(store, &event);
            if outcome.refresh {
                let refresh = if outcome.changed {
                    link_refresh.after_change(store, &active.scope, &active.task)
                } else {
                    link_refresh.request(store, &active.scope, &active.task, true)
                };
                action_notice = Some(match refresh {
                    Ok(()) => (
                        crate::tr!("작업 연결 확인 중…", "Checking task links…").into(),
                        Role::Muted,
                    ),
                    Err(error) => (error.to_string(), Role::Warning),
                });
            }
            if outcome.close {
                link_picker = None;
            }
            if outcome.changed || outcome.close || outcome.refresh {
                last_refresh = force_refresh();
            }
            continue;
        }
        if let Some(active) = &mut editor {
            if active.handle(store, &event) {
                editor = None;
                last_refresh = force_refresh();
            }
            continue;
        }
        // The date chooser owns input until it closes. A menu-opening gesture
        // must not escape it and expose unrelated summary or mutation actions.
        if picker.open {
            if let Some(result) = picker.handle(store, &event) {
                action_notice = result.err().map(|e| (e.to_string(), Role::Warning));
            }
            continue;
        }
        if selection.is_some() {
            match &event {
                Event::Key(key) if key.kind == KeyEventKind::Press && key.code == KeyCode::Esc => {
                    selection = None;
                    continue;
                }
                Event::Key(_) | Event::Resize(..) => selection = None,
                Event::Mouse(mouse)
                    if matches!(
                        mouse.kind,
                        MouseEventKind::Down(MouseButton::Right)
                            | MouseEventKind::ScrollUp
                            | MouseEventKind::ScrollDown
                    ) =>
                {
                    selection = None;
                }
                _ => {}
            }
        }
        let (handled, action) = crate::panel_menu::route(&mut menu, &event, width, height);
        if let Some(action) = action {
            if action == crate::panel_menu::Action::Language {
                language_picker = Some(crate::language_picker::Picker::open());
                selection = None;
                continue;
            }
            if action == crate::panel_menu::Action::Model {
                model_picker = Some(crate::model_picker::Picker::open());
                selection = None;
                continue;
            }
            if matches!(
                action,
                crate::panel_menu::Action::Records | crate::panel_menu::Action::Progress
            ) {
                let progress = action == crate::panel_menu::Action::Progress;
                reader = Some(open_reader(&snapshot, progress, scroll));
                reader_source = Some((snapshot.clone(), progress));
                selection = None;
                continue;
            }
            if action == crate::panel_menu::Action::Evidence {
                if let Some(branch) = &snapshot.branch {
                    editor = Some(crate::record_editor::Editor::open(
                        branch.scope.clone(),
                        branch.journal.clone(),
                    ));
                }
                selection = None;
                continue;
            }
            if action == crate::panel_menu::Action::Retry {
                action_notice = Some(request_record_retry(store));
                last_refresh = force_refresh();
                continue;
            }
            if action == crate::panel_menu::Action::OpenLinks {
                match open_link_picker(store, &snapshot) {
                    Ok(picker) => {
                        link_picker = picker;
                        if let Some(active) = &mut link_picker {
                            active.open_current(store);
                        }
                    }
                    Err(error) => action_notice = Some((error.to_string(), Role::Warning)),
                }
                selection = None;
                continue;
            }
            if action == crate::panel_menu::Action::History {
                picker.show(store)?;
                scroll = 0;
                continue;
            }
            if action == crate::panel_menu::Action::CatchUp {
                action_notice = Some(match crate::live_summary::request(store) {
                    Ok(notice) => (notice, Role::Muted),
                    Err(error) => (error.to_string(), Role::Warning),
                });
                last_refresh = force_refresh();
                scroll = 0;
                continue;
            }
            if action == crate::panel_menu::Action::Links {
                match open_link_picker(store, &snapshot) {
                    Ok(picker) => link_picker = picker,
                    Err(error) => action_notice = Some((error.to_string(), Role::Warning)),
                }
                selection = None;
                continue;
            }
            if action == crate::panel_menu::Action::RefreshLinks {
                action_notice = request_link_refresh(&mut link_refresh, store, &snapshot);
                last_refresh = force_refresh();
                continue;
            }
            return Ok(Some(action));
        }
        if handled {
            continue;
        }
        match event {
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::Down(MouseButton::Left) if mouse.row < height.saturating_sub(1) => {
                    selection = Some(crate::panel_selection::Selection::new(
                        lines.clone(),
                        scroll,
                        width,
                        height,
                        mouse.column,
                        mouse.row,
                    ));
                }
                MouseEventKind::Drag(MouseButton::Left) => {
                    if let Some(selected) = &mut selection {
                        selected.update(mouse.column, mouse.row);
                    }
                }
                MouseEventKind::Up(MouseButton::Left) => {
                    if let Some(selected) = &mut selection
                        && selected.dragging
                    {
                        selected.update(mouse.column, mouse.row);
                        selected.dragging = false;
                        let text = selected.text();
                        if let Some(sequence) = crate::panel_selection::clipboard_sequence(&text) {
                            let mut output = std::io::stdout();
                            write!(output, "{sequence}")?;
                            output.flush()?;
                        } else if text.is_empty() {
                            selection = None;
                        }
                    }
                }
                MouseEventKind::ScrollDown => scroll = scroll.saturating_add(3),
                MouseEventKind::ScrollUp => scroll = scroll.saturating_sub(3),
                _ => {}
            },
            Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::Char('h') => {
                    reader = Some(open_reader(&snapshot, false, scroll));
                    reader_source = Some((snapshot.clone(), false));
                    selection = None;
                }
                KeyCode::Char('q') | KeyCode::Esc => {
                    let mut ui = store.ui()?;
                    ui.visible = false;
                    store.set_ui(ui)?;
                    return Ok(None);
                }
                KeyCode::Char('e') => {
                    if let Some(branch) = &snapshot.branch {
                        editor = Some(crate::record_editor::Editor::open(
                            branch.scope.clone(),
                            branch.journal.clone(),
                        ));
                        selection = None;
                    }
                }
                KeyCode::Char('r') => {
                    action_notice = Some(request_record_retry(store));
                    last_refresh = force_refresh();
                }
                KeyCode::Char('v') if key.modifiers.is_empty() => {
                    action_notice = request_link_refresh(&mut link_refresh, store, &snapshot);
                    last_refresh = force_refresh();
                }
                KeyCode::Char('l' | 'o') => {
                    match open_link_picker(store, &snapshot) {
                        Ok(picker) => link_picker = picker,
                        Err(error) => action_notice = Some((error.to_string(), Role::Warning)),
                    }
                    if key.code == KeyCode::Char('o')
                        && let Some(active) = &mut link_picker
                    {
                        active.open_current(store);
                    }
                    selection = None;
                }
                KeyCode::Char('b') => {
                    picker.show(store)?;
                    scroll = 0;
                }
                KeyCode::Char('s') => {
                    action_notice = Some(match crate::live_summary::request(store) {
                        Ok(notice) => (notice, Role::Muted),
                        Err(error) => (error.to_string(), Role::Warning),
                    });
                    last_refresh = force_refresh();
                    scroll = 0;
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    scroll = scroll.saturating_add(1);
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    scroll = scroll.saturating_sub(1);
                }
                KeyCode::PageDown | KeyCode::Char(' ') => {
                    scroll = scroll.saturating_add(body_height.max(1));
                }
                KeyCode::PageUp => scroll = scroll.saturating_sub(body_height.max(1)),
                KeyCode::Home => scroll = 0,
                KeyCode::End => scroll = lines.len().saturating_sub(body_height),
                _ => {}
            },
            _ => {}
        }
    }
}

fn open_link_picker(
    store: &SpaceStore,
    snapshot: &PanelSnapshot,
) -> AppResult<Option<crate::link_picker::Picker>> {
    if snapshot.branch.is_none() {
        return Ok(None);
    }
    crate::client_routing::activate_attached_task(store)?;
    let branch = crate::branch_panel::load(store)?;
    Ok(Some(crate::link_picker::Picker::open(
        branch.scope,
        branch.links,
        branch.task.unwrap_or_default(),
    )))
}

fn request_link_refresh(
    refresh: &mut crate::link_refresh::Refresh,
    store: &SpaceStore,
    snapshot: &PanelSnapshot,
) -> Option<(String, Role)> {
    let branch = snapshot.branch.as_ref()?;
    Some(
        match refresh.request(
            store,
            &branch.scope,
            branch
                .task
                .as_ref()
                .unwrap_or(&crate::task_links::TaskLinks::default()),
            true,
        ) {
            Ok(()) => (
                crate::tr!("작업 연결 확인 중…", "Checking task links…").into(),
                Role::Muted,
            ),
            Err(error) => (error.to_string(), Role::Warning),
        },
    )
}

fn force_refresh() -> Instant {
    let now = Instant::now();
    now.checked_sub(REFRESH_INTERVAL).unwrap_or(now)
}

fn load_snapshot(store: &SpaceStore) -> AppResult<PanelSnapshot> {
    let meta = store.meta()?.ok_or("space metadata is missing")?;
    let agent = herdr::agents()
        .unwrap_or_default()
        .into_iter()
        .filter(|agent| {
            agent.workspace_id == meta.workspace_id
                && agent.tab_id == meta.tab_id
                && agent.agent == "codex"
        })
        .max_by_key(|agent| status_priority(&agent.agent_status));
    // Read summary and queue together so a concurrent commit cannot look falsely current.
    let (mut current, pending, worker, rebuild, latest) = {
        let _lock = lock_file(&store.path(".event.lock"), false)?;
        let current = store.current()?;
        let scope = crate::branch_context::detect_for_store(store, &meta.cwd)?;
        let sources = crate::branch_context::sources(store)?;
        let latest = crate::latest::visible(store, &current)?.filter(|v| {
            if store.path("selected-current.json").exists() {
                return false;
            }
            sources
                .get(&v.first_event_seq)
                .is_some_and(|s| s.context.key == scope.key)
                || scope.unlinked_reason.is_some() && !sources.contains_key(&v.first_event_seq)
        });
        (
            current,
            store.unprocessed_events()?,
            crate::daily::status(store)?,
            !crate::participation::load(store)?.history_blocked
                && store.rebuild_request()?.is_some(),
            latest,
        )
    };
    let mut freshness = Pending::from_events(&pending, unix_now());
    freshness.rebuild = rebuild;
    freshness.worker_alive = try_lock_file(&store.path(".worker.lock"))?.is_none();
    if let Some(latest) = latest {
        freshness.preview = Some(Preview {
            cumulative_through: current.timing.through_unix,
        });
        current = latest.state;
    }
    freshness.newer_events = pending
        .iter()
        .filter(|e| e.seq > current.source_seq)
        .count();
    let request_event = pending
        .iter()
        .rev()
        .take_while(|event| {
            !matches!(
                event.kind,
                EventKind::AssistantFinal | EventKind::Interrupted
            )
        })
        .find(|event| {
            event.seq > current.source_seq && matches!(event.kind, EventKind::UserPrompt)
        });
    let current_request = if let Some(event) = request_event
        && crate::live_turn::thread_current(store, &event.session_id, 0)?
    {
        Some(first_nonempty_line(&event.content))
    } else {
        None
    };

    let live = crate::live_turn::load(store)?;
    let catch_up = crate::live_summary::load(store)?;
    let captured_at = unix_now();
    let display_times = snapshot_times(live.as_ref(), catch_up.as_ref(), &worker, captured_at);

    Ok(PanelSnapshot {
        branch: Some(crate::branch_panel::load(store)?),
        connection_notice: crate::connection::notice(
            store,
            agent
                .as_ref()
                .and_then(|a| a.agent_session.as_ref())
                .map(|s| s.value.as_str()),
        )?,
        label: meta.label,
        agent_status: agent
            .as_ref()
            .map_or_else(|| "unknown".to_string(), |agent| agent.agent_status.clone()),
        agent_title: agent.and_then(|agent| agent.terminal_title_stripped),
        current_request,
        live,
        catch_up,
        captured_at,
        display_times,
        pending: freshness,
        worker,
        current,
        live_repo: repo_facts::collect(&meta.cwd),
    })
}

fn snapshot_times(
    live: Option<&crate::live_turn::Snapshot>,
    catch_up: Option<&crate::live_summary::View>,
    worker: &WorkerStatus,
    captured_at: u64,
) -> crate::local_time::LocalTimes {
    crate::local_time::LocalTimes::new(
        [captured_at]
            .into_iter()
            .chain(live.iter().flat_map(|live| {
                [live.started_at, live.updated_at]
                    .into_iter()
                    .chain(live.commentary.iter().map(|report| report.at_unix))
                    .chain(live.activities.iter().map(|activity| activity.at_unix))
            }))
            .chain(worker.updated_at_unix)
            .chain(
                catch_up
                    .iter()
                    .flat_map(|view| [view.requested_at, view.snapshot.updated_at]),
            ),
    )
}

#[cfg(test)]
fn render_lines(snapshot: &PanelSnapshot, width: usize) -> Vec<Line> {
    render_view(snapshot, width, &crate::branch_panel::View::default())
}

fn current_live(snapshot: &PanelSnapshot) -> Option<&crate::live_turn::Snapshot> {
    let live = snapshot.live.as_ref()?;
    let task = snapshot
        .branch
        .as_ref()
        .and_then(|branch| branch.task.as_ref());
    if let Some(task) = task {
        if let Some(thread) = &task.thread_id {
            if thread != &live.thread_id || task.generation != live.generation {
                return None;
            }
            if task.latest_source_seq > 0
                && live
                    .source_prompt_seq
                    .is_none_or(|seq| seq < task.latest_source_seq)
            {
                return None;
            }
        } else if live.ended {
            return None;
        }
    }
    Some(live)
}

fn current_request(snapshot: &PanelSnapshot) -> Option<&str> {
    snapshot
        .branch
        .as_ref()
        .and_then(|branch| branch.task.as_ref())
        .map(|task| task.current_request.as_str())
        .filter(|request| !request.trim().is_empty())
        .or(snapshot.current_request.as_deref())
        .or_else(|| {
            current_live(snapshot)
                .map(|live| live.request.as_str())
                .filter(|request| !request.trim().is_empty())
        })
}

fn request_record_retry(store: &SpaceStore) -> (String, Role) {
    match worker::request_refresh(store) {
        Ok(worker::RefreshOutcome::Requested { today_turns }) => (
            crate::tr!(
                format!("이 탭의 오늘 미반영 완료 대화 {today_turns}턴 · 기록 반영을 요청했습니다"),
                format!(
                    "Requested record reflection for {today_turns} pending completed turns from today in this tab"
                )
            ),
            Role::Body,
        ),
        Ok(worker::RefreshOutcome::NoTodayWork {
            held_turns,
            open_events,
        }) => {
            let mut message = crate::tr!(
                "이 탭에서 새로 반영할 오늘 완료 대화가 없습니다",
                "No new completed conversation from today to reflect in this tab"
            )
            .to_string();
            if open_events > 0 {
                message.push_str(crate::tr!(
                    " · 완료 응답이 아직 수집되지 않은 기록은 대기합니다",
                    " · Records awaiting a completed response stay pending"
                ));
            }
            if held_turns > 0 {
                use std::fmt::Write as _;
                let _ = write!(
                    message,
                    "{}",
                    crate::tr!(
                        format!(" · 이전/보류 {held_turns}턴은 b에서 선택"),
                        format!(" · Select {held_turns} earlier/held turns with b")
                    )
                );
            }
            (message, Role::Body)
        }
        Err(error) => (
            crate::tr!(
                format!("기록 반영 요청 실패 · {error}"),
                format!("Record reflection request failed · {error}")
            ),
            Role::Warning,
        ),
    }
}

fn render_main(snapshot: &PanelSnapshot, width: usize, _height: usize) -> Vec<Line> {
    let Some(branch) = &snapshot.branch else {
        return render_view(snapshot, width, &crate::branch_panel::View::default());
    };
    let inner = width.saturating_sub(2).max(2);
    let mut lines = Vec::new();
    panel_style::wrap(&mut lines, &observed_location(snapshot), inner, Role::Muted);
    panel_style::wrap(
        &mut lines,
        crate::tr!("현재 대화의 작업", "Current conversation task"),
        inner,
        Role::Title,
    );
    panel_style::wrap(&mut lines, &rule(inner), inner, Role::Body);
    lines.extend(crate::branch_panel::render_current_references(
        branch, width,
    ));
    lines.extend(crate::live_panel::preview(
        current_live(snapshot),
        snapshot.catch_up.as_ref(),
        &snapshot.display_times,
        branch.today.as_deref(),
        inner,
    ));
    let public_description = current_live(snapshot).is_some_and(|live| {
        live.commentary
            .last()
            .is_some_and(|report| !report.text.trim().is_empty())
    });
    if !public_description && let Some(request) = current_request(snapshot) {
        lines.push(Line::default());
        panel_style::wrap(
            &mut lines,
            crate::tr!("최근 요청", "Latest request"),
            inner,
            Role::Title,
        );
        panel_style::wrap(&mut lines, request, inner, Role::Body);
    }
    if let Some(notice) = &snapshot.connection_notice {
        panel_style::wrap(&mut lines, notice, inner, Role::Warning);
    }
    lines.push(Line::default());
    let queue = record_queue_preview(snapshot, inner);
    lines.extend(crate::branch_panel::render_preview(branch, width, &queue));
    render_annotations(snapshot, inner, &mut lines);
    lines
}

fn observed_location(snapshot: &PanelSnapshot) -> String {
    let scope = snapshot.branch.as_ref().map(|branch| &branch.scope);
    let repo = scope
        .and_then(|scope| scope.repository.as_deref())
        .and_then(|path| std::path::Path::new(path).parent()?.file_name())
        .map_or_else(
            || snapshot.label.clone(),
            |name| name.to_string_lossy().into_owned(),
        );
    crate::tr!(
        format!(
            "{repo} · {} · 관찰 위치",
            scope
                .and_then(|scope| scope.branch.as_deref())
                .unwrap_or(crate::tr!("브랜치 미확인", "Branch unconfirmed"))
        ),
        format!(
            "{repo} · {} · observed location",
            scope
                .and_then(|scope| scope.branch.as_deref())
                .unwrap_or(crate::tr!("브랜치 미확인", "Branch unconfirmed"))
        )
    )
}

fn record_queue_preview(snapshot: &PanelSnapshot, width: usize) -> Vec<Line> {
    let mut lines = Vec::new();
    let queue = snapshot
        .branch
        .as_ref()
        .and_then(|branch| branch.daily.as_ref());
    let state = snapshot.worker.state.as_str();
    // load_snapshot uses daily::status: an error is an exhausted current retry budget.
    let failure = matches!(state, "error" | "retry_wait");
    let status = match state {
        "error" => Some(crate::tr!(
            "요약 처리 실패 · 자동 재시도 중지",
            "Summary processing failed · Automatic retry stopped"
        )),
        "retry_wait" => Some(crate::tr!(
            "요약 실패 · 자동 재시도 대기",
            "Summary failed · Automatic retry pending"
        )),
        "summarizing" if snapshot.pending.worker_alive => {
            Some(crate::tr!("기록 요약 중", "Summarizing records"))
        }
        "summarizing" => Some(crate::tr!(
            "요약 처리 상태 미확인 · 프로세스 없음",
            "Summary processing unconfirmed · No process"
        )),
        _ if queue.is_some_and(|queue| queue.today_turns > 0 || queue.requested.is_some()) => {
            Some(crate::tr!("기록 요약 대기", "Record summary pending"))
        }
        _ => None,
    };
    if let Some(status) = status {
        lines.push(Line::default());
        panel_style::wrap(
            &mut lines,
            status,
            width,
            match state {
                "error" => Role::Error,
                "retry_wait" => Role::Warning,
                _ => Role::Title,
            },
        );
    }
    if failure {
        if let Some(explanation) = crate::reducer_diagnostics::failure_copy(&snapshot.worker) {
            panel_style::wrap(&mut lines, &explanation, width, Role::Body);
        }
        let recovery = if queue.is_some_and(|queue| queue.requested.is_some()) {
            crate::tr!(
                "b 날짜 재선택 · h 기록의 요약·처리 상세",
                "b reselect date · h records: summary and processing details"
            )
        } else {
            crate::tr!(
                "r 오늘 재시도 · h 기록의 요약·처리 상세",
                "r retry today · h records: summary and processing details"
            )
        };
        panel_style::wrap(&mut lines, recovery, width, Role::Body);
    }
    if let Some(queue) = queue
        && let Some(day) = &queue.requested
    {
        panel_style::wrap(
            &mut lines,
            &crate::tr!("선택한 날짜 · {day}", "Selected date · {day}";),
            width,
            Role::Body,
        );
    }
    lines
}

fn record_worker_section(snapshot: &PanelSnapshot) -> crate::branch_panel::RecordSection {
    let mut rows = record_queue_preview(snapshot, usize::MAX);
    if let Some(queue) = snapshot
        .branch
        .as_ref()
        .and_then(|branch| branch.daily.as_ref())
    {
        rows.push(Line {
            text: crate::tr!(
                "요약 대상 · 오늘 완료 {}턴 · 이전·보류 완료 {}턴 · 열린 이벤트 {}개",
                "Summary queue · Completed turns today: {} · Earlier/held completed turns: {} · Open events: {}";
                queue.today_turns, queue.manual.values().sum::<usize>(), queue.open_events
            ),
            role: Role::Body,
        });
    }
    let status = match snapshot.worker.state.as_str() {
        "error" => crate::tr!("실패", "Failed"),
        "retry_wait" => crate::tr!("실패 후 재시도 대기", "Retry pending after failure"),
        "summarizing" => crate::tr!("요약 중으로 보고됨", "Reported as summarizing"),
        "idle" | "" => crate::tr!("실행 중인 요약 없음", "No summary running"),
        _ => crate::tr!("상태 미확인", "Status unconfirmed"),
    };
    rows.push(Line {
        text: crate::tr!(
            "처리 상태: {status} · 상태 기록 시각 {}",
            "Processing status: {status} · Status recorded at {}";
            snapshot.display_times.display(snapshot.worker.updated_at_unix)
        ),
        role: Role::Muted,
    });
    if !snapshot.worker.message.is_empty() {
        rows.push(Line::default());
        rows.push(Line {
            text: crate::tr!("처리 진단 원문", "Processing diagnostic source").into(),
            role: Role::Title,
        });
        rows.push(Line {
            text: snapshot.worker.message.clone(),
            role: Role::Body,
        });
    }
    let process = if snapshot.pending.worker_alive {
        crate::tr!("실행 중", "Running")
    } else {
        crate::tr!("없음", "None")
    };
    rows.push(Line {
        text: crate::tr!(
            "미처리 이벤트 {}개 · 완료 대화에서 처리할 이벤트 {}개 · 요약 프로세스 {process}",
            "{} pending events · {} eligible completed-turn events · summary process {process}";
            snapshot.pending.total, snapshot.pending.completed
        ),
        role: Role::Muted,
    });
    crate::branch_panel::RecordSection {
        title: crate::tr!(
            "기록 요약·처리 상세",
            "Record summary and processing details"
        )
        .into(),
        rows,
    }
}

fn render_annotations(snapshot: &PanelSnapshot, width: usize, lines: &mut Vec<Line>) {
    if snapshot.pending.preview.is_none() {
        return;
    }
    panel_style::wrap(
        lines,
        crate::tr!(
            "저장 범위 · 잠정 미리보기 · 누적 원장과 별도",
            "Saved scope · provisional preview · separate from cumulative ledger"
        ),
        width,
        Role::Warning,
    );
    for update in &snapshot.current.summary.record_updates {
        panel_style::wrap(lines, &update.text, width, Role::Body);
        if let Some(condition) = &update.condition {
            panel_style::wrap(
                lines,
                &crate::tr!(
                    format!("조건: {condition}"),
                    format!("Condition: {condition}")
                ),
                width,
                Role::Body,
            );
        }
    }
    for update in &snapshot.current.summary.state_updates {
        panel_style::wrap(
            lines,
            &format!("{} — {}", owner_label(&update.owner), update.text),
            width,
            Role::Body,
        );
        if let Some(condition) = &update.condition {
            panel_style::wrap(
                lines,
                &crate::tr!(
                    format!("조건: {condition}"),
                    format!("Condition: {condition}")
                ),
                width,
                Role::Body,
            );
        }
    }
    if snapshot.current.summary.record_updates.is_empty()
        && snapshot.current.summary.state_updates.is_empty()
    {
        panel_style::wrap(
            lines,
            &snapshot.current.summary.objective,
            width,
            Role::Body,
        );
    }
}

fn open_reader(snapshot: &PanelSnapshot, progress: bool, scroll: usize) -> reader::Reader {
    let branch = snapshot.branch.as_ref();
    let scope = if progress {
        current_live(snapshot).map_or_else(
            || {
                crate::tr!(
                    "현재 대화 · 진행 정보 미수집",
                    "Current conversation · progress not collected"
                )
                .into()
            },
            |live| {
                crate::tr!(
                    format!("현재 주 대화 {} · 턴 {}", live.thread_id, live.turn_id),
                    format!(
                        "Current primary conversation {} · turn {}",
                        live.thread_id, live.turn_id
                    )
                )
            },
        )
    } else {
        branch.map_or_else(
            || crate::tr!("저장 범위 미확인", "Saved scope unconfirmed").into(),
            |branch| {
                format!(
                    "{} · {}",
                    if branch.scope.tab_id.is_some() {
                        crate::tr!("이 탭의 저장 기록", "Saved records for this tab")
                    } else {
                        crate::tr!("저장된 기록", "Saved records")
                    },
                    branch
                        .scope
                        .branch
                        .as_deref()
                        .unwrap_or(crate::tr!("브랜치 미확인", "Branch unconfirmed"))
                )
            },
        )
    };
    let mut sections = if progress {
        crate::live_panel::collected_sections(
            current_live(snapshot),
            snapshot.catch_up.as_ref(),
            current_request(snapshot),
            &snapshot.display_times,
        )
    } else {
        branch.map_or_else(Vec::new, crate::branch_panel::record_sections)
    };
    if !progress {
        sections.push(record_worker_section(snapshot));
    }
    reader::Reader::new(
        if progress {
            crate::tr!(
                "현재 진행 상세 · 읽기 전용",
                "Current progress details · read only"
            )
        } else {
            crate::tr!("전체 기록 · 읽기 전용", "All records · read only")
        },
        scope,
        snapshot.display_times.display(Some(snapshot.captured_at)),
        sections,
        scroll,
    )
}

#[allow(
    clippy::too_many_lines,
    reason = "Lazy source translations preserve complete original text while expanding the existing renderer"
)]
fn render_view(
    snapshot: &PanelSnapshot,
    width: usize,
    view: &crate::branch_panel::View,
) -> Vec<Line> {
    if let Some(branch) = &snapshot.branch {
        if !view.expanded {
            return render_main(snapshot, width, 40);
        }
        let mut lines = render_branch_content(snapshot, branch, width, view);
        let mut annotations = Vec::new();
        wrapped_bullet(
            &mut annotations,
            &format!("[Codex · {}] · {}", snapshot.agent_status, snapshot.label),
            width.saturating_sub(2).max(2),
            "",
        );
        if snapshot.pending.preview.is_some() {
            wrapped_bullet(
                &mut annotations,
                crate::tr!(
                    "잠정 최신 미리보기 있음 · 확정 누적 원장과 별도",
                    "Provisional latest preview · separate from confirmed cumulative ledger"
                ),
                width.saturating_sub(2).max(2),
                "",
            );
            for update in &snapshot.current.summary.record_updates {
                wrapped_bullet(
                    &mut annotations,
                    &crate::tr!(
                        format!(
                            "[잠정 · 누적 전] {}{}",
                            update.text,
                            update
                                .condition
                                .as_ref()
                                .map_or(String::new(), |c| crate::tr!(
                                    format!(" · 조건: {c}"),
                                    format!(" · Condition: {c}")
                                ))
                        ),
                        format!(
                            "[Provisional · before accumulation] {}{}",
                            update.text,
                            update
                                .condition
                                .as_ref()
                                .map_or(String::new(), |c| crate::tr!(
                                    format!(" · 조건: {c}"),
                                    format!(" · Condition: {c}")
                                ))
                        )
                    ),
                    width.saturating_sub(2).max(2),
                    "",
                );
            }
            for update in &snapshot.current.summary.state_updates {
                wrapped_bullet(
                    &mut annotations,
                    &crate::tr!(
                        format!(
                            "[잠정 · {:?}] {} — {}{}",
                            update.kind,
                            update.owner,
                            update.text,
                            update
                                .condition
                                .as_ref()
                                .map_or(String::new(), |c| crate::tr!(
                                    format!(" · 조건: {c}"),
                                    format!(" · Condition: {c}")
                                ))
                        ),
                        format!(
                            "[Provisional · {:?}] {} — {}{}",
                            update.kind,
                            update.owner,
                            update.text,
                            update
                                .condition
                                .as_ref()
                                .map_or(String::new(), |c| crate::tr!(
                                    format!(" · 조건: {c}"),
                                    format!(" · Condition: {c}")
                                ))
                        )
                    ),
                    width.saturating_sub(2).max(2),
                    "",
                );
            }
            if snapshot.current.summary.record_updates.is_empty()
                && snapshot.current.summary.state_updates.is_empty()
            {
                wrapped_bullet(
                    &mut annotations,
                    &crate::tr!(
                        format!(
                            "[잠정 · 구버전 요약] {}",
                            snapshot.current.summary.objective
                        ),
                        format!(
                            "[Provisional · legacy summary] {}",
                            snapshot.current.summary.objective
                        )
                    ),
                    width.saturating_sub(2).max(2),
                    "",
                );
            }
        }
        if let Some(notice) = &snapshot.connection_notice {
            wrapped_bullet(&mut annotations, notice, width.saturating_sub(2).max(2), "");
        }
        lines.extend(annotations.into_iter().map(|text| Line {
            text,
            role: Role::Muted,
        }));
        return lines;
    }
    let inner = width.saturating_sub(2).max(10);
    let mut lines = Vec::new();
    render_header(snapshot, inner, &mut lines);
    render_live_facts(snapshot, inner, &mut lines);
    render_verified_summary(snapshot, inner, &mut lines);
    lines
        .into_iter()
        .map(|text| Line {
            text,
            role: Role::Body,
        })
        .collect()
}

fn render_branch_content(
    snapshot: &PanelSnapshot,
    branch: &crate::branch_panel::Snapshot,
    width: usize,
    view: &crate::branch_panel::View,
) -> Vec<Line> {
    let mut lines = branch.daily.as_ref().map_or_else(Vec::new, |queue| {
        crate::day_picker::queue_lines(queue, &snapshot.worker, width)
    });
    lines.extend(crate::branch_panel::render_header(branch, width));
    let request_shown = branch
        .task
        .as_ref()
        .is_some_and(|task| !task.current_request.trim().is_empty());
    let mut live = snapshot.live.clone();
    if request_shown && let Some(live) = &mut live {
        live.request.clear();
    }
    let progress = crate::live_panel::render(
        live.as_ref(),
        snapshot.catch_up.as_ref(),
        snapshot
            .current_request
            .as_deref()
            .filter(|_| !request_shown),
        width,
    );
    if !progress.is_empty() {
        lines.push(Line::default());
        lines.extend(progress);
    }
    lines.push(Line::default());
    lines.extend(crate::branch_panel::render_records(branch, width, view));
    lines
}

fn render_header(snapshot: &PanelSnapshot, inner: usize, lines: &mut Vec<String>) {
    lines.push(fit(&format!("Maeklog · {}", snapshot.label), inner));
    lines.push(rule(inner));
    if let Some(at) = snapshot.current.timing.through_unix {
        wrapped_bullet(
            lines,
            &crate::tr!(
                format!(
                    "{}{} · 한국시간",
                    crate::timing::format_kst(at, unix_now()),
                    if snapshot.pending.preview.is_some() {
                        crate::tr!(" 최신 작업 반영", " latest work reflected")
                    } else {
                        crate::tr!("까지 대화 반영", " conversation reflected through")
                    }
                ),
                format!(
                    "{}{} · KST",
                    crate::timing::format_kst(at, unix_now()),
                    if snapshot.pending.preview.is_some() {
                        crate::tr!(" 최신 작업 반영", " latest work reflected")
                    } else {
                        crate::tr!("까지 대화 반영", " conversation reflected through")
                    }
                )
            ),
            inner,
            "",
        );
        if snapshot.current.timing.partial_history {
            wrapped_bullet(
                lines,
                crate::tr!(
                    "일부 이전 대화는 생략됨",
                    "Some earlier conversation omitted"
                ),
                inner,
                "",
            );
        }
    } else if snapshot.current.revision > 0 {
        wrapped_bullet(
            lines,
            crate::tr!(
                "대화 반영 시각 미기록",
                "Conversation reflection time not recorded"
            ),
            inner,
            "",
        );
        if let Some(at) = snapshot.current.updated_at_unix {
            wrapped_bullet(
                lines,
                &crate::tr!(
                    format!(
                        "요약 생성 {} · 한국시간",
                        crate::timing::format_kst(at, unix_now())
                    ),
                    format!(
                        "Summary generated {} · KST",
                        crate::timing::format_kst(at, unix_now())
                    )
                ),
                inner,
                "",
            );
        }
    }

    render_freshness(snapshot, inner, lines);
    let state = match snapshot.agent_status.as_str() {
        "working" => crate::tr!("작업 중", "Working"),
        "idle" | "done" => crate::tr!("입력 대기", "Awaiting input"),
        "blocked" => crate::tr!("승인·입력 필요", "Approval/input needed"),
        _ => crate::tr!("상태 미확인", "Status unconfirmed"),
    };
    let state_detail = snapshot
        .agent_title
        .as_deref()
        .filter(|title| !title.trim().is_empty())
        .map_or_else(String::new, |title| format!(" · {}", sanitize(title)));
    lines.push(fit(&format!("[Codex · {state}]{state_detail}"), inner));
    if let Some(notice) = &snapshot.connection_notice {
        wrapped_bullet(lines, notice, inner, "");
    }
    if snapshot.agent_status == "working" {
        wrapped_bullet(
            lines,
            crate::tr!(
                "진행 중인 작업의 완료 내용은 아직 미반영",
                "Completion of the running task has not been reflected yet"
            ),
            inner,
            "",
        );
    }
    if let Some(request) = &snapshot.current_request {
        section(lines, crate::tr!("현재 요청", "Current request"), inner);
        wrapped_bullet(lines, request, inner, "");
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "Lazy source translations preserve complete original text while expanding the existing renderer"
)]
fn render_freshness(snapshot: &PanelSnapshot, inner: usize, lines: &mut Vec<String>) {
    let pending = &snapshot.pending;
    if let Some(preview) = &pending.preview {
        wrapped_bullet(
            lines,
            &crate::tr!(
                format!("[과거 맥락 미완료] 누적 정리 {}개 대기", pending.total),
                format!(
                    "[Earlier context incomplete] {} records awaiting cumulative processing",
                    pending.total
                )
            ),
            inner,
            "",
        );
        if let Some(at) = preview.cumulative_through {
            wrapped_bullet(
                lines,
                &crate::tr!(
                    format!(
                        "누적 맥락은 {}까지 반영",
                        crate::timing::format_kst(at, unix_now())
                    ),
                    format!(
                        "Cumulative context reflected through {}",
                        crate::timing::format_kst(at, unix_now())
                    )
                ),
                inner,
                "",
            );
        }
        if pending.newer_events > 0 {
            wrapped_bullet(
                lines,
                &crate::tr!(
                    format!("[새 기록 미반영] {}개", pending.newer_events),
                    format!("[New records pending] {}", pending.newer_events)
                ),
                inner,
                "",
            );
        }
        wrapped_bullet(
            lines,
            crate::tr!(
                "이전 승인·조건 미반영 가능 · 다음 행동은 잠정",
                "Earlier approvals/conditions may be missing · next actions are provisional"
            ),
            inner,
            "",
        );
    }
    if pending.total > 0 && pending.preview.is_none() {
        wrapped_bullet(
            lines,
            &crate::tr!(
                format!(
                    "[미반영] 기록 {}개 · 오늘 {}개",
                    pending.total, pending.today
                ),
                format!(
                    "[Pending] {} records · {} today",
                    pending.total, pending.today
                )
            ),
            inner,
            "",
        );
    }
    let state = snapshot.worker.state.as_str();
    let message = if pending.latest_work {
        match state {
            "error" => crate::tr!(
                format!("[최신 요약 멈춤] {}", snapshot.worker.message),
                format!("[Latest summary stopped] {}", snapshot.worker.message)
            ),
            "retry_wait" => crate::tr!(
                "[최신 요약 재시도 대기] 기록 보존",
                "[Latest summary retry pending] records retained"
            )
            .into(),
            "summarizing" if pending.worker_alive => crate::tr!(
                "[요약 · 최신 작업 정리 중]",
                "[Summary · organizing latest work]"
            )
            .into(),
            _ => crate::tr!(
                "[최신 요약 대기] 기록 보존",
                "[Latest summary pending] records retained"
            )
            .into(),
        }
    } else if pending.preview.is_some() {
        match state {
            "error" => crate::tr!(
                format!("[누적 정리 멈춤] {}", snapshot.worker.message),
                format!(
                    "[Cumulative processing stopped] {}",
                    snapshot.worker.message
                )
            ),
            "retry_wait" => crate::tr!(
                "[누적 정리 재시도 대기] 최신 요약 유지",
                "[Cumulative retry pending] latest summary retained"
            )
            .into(),
            "summarizing" if pending.worker_alive => crate::tr!(
                "[요약 · 과거 맥락 정리 중] 최신 요약 유지",
                "[Summary · organizing earlier context] latest summary retained"
            )
            .into(),
            _ => crate::tr!(
                "[누적 정리 대기] 최신 요약 유지",
                "[Cumulative processing pending] latest summary retained"
            )
            .into(),
        }
    } else if state == "error" {
        crate::tr!(
            format!("[요약 멈춤] {}", snapshot.worker.message),
            format!("[Summary stopped] {}", snapshot.worker.message)
        )
    } else if state == "retry_wait" {
        // Do not repeat a frozen countdown saved at the time of the failure.
        crate::tr!(
            "[재시도 대기] 기록 보존 · 자동 재시도 예정",
            "[Retry pending] records retained · automatic retry scheduled"
        )
        .into()
    } else if state == "summarizing" && pending.worker_alive {
        if pending.rebuild {
            crate::tr!(
                "[요약 · 정리 중] 기존 대화에서 맥락 재구성",
                "[Summary · organizing] rebuilding context from existing conversation"
            )
            .into()
        } else {
            crate::tr!(
                "[요약 · 정리 중] 미반영 대화를 순서대로 반영",
                "[Summary · organizing] reflecting pending conversation in order"
            )
            .into()
        }
    } else if pending.completed > 0 || pending.rebuild {
        crate::tr!(
            "[요약 대기] 기록 보존 · 순서대로 처리 예정",
            "[Summary pending] records retained · processing in order"
        )
        .into()
    } else if pending.total > 0 {
        crate::tr!(
            "[작업 완료 대기] 답변이 끝나면 요약에 반영",
            "[Waiting for completion] reflected after the response ends"
        )
        .into()
    } else if snapshot.current.revision == 0 {
        crate::tr!(
            "[맥락 없음] 아직 저장된 요약 없음",
            "[No context] no saved summary yet"
        )
        .into()
    } else if snapshot.connection_notice.is_some() {
        crate::tr!(
            "[연결 확인 필요] 최신 대화 수집 여부 미확인",
            "[Connection needs review] latest conversation collection unconfirmed"
        )
        .into()
    } else {
        crate::tr!(
            "[반영 완료] 수집된 대화까지 반영",
            "[Up to date] collected conversation reflected"
        )
        .into()
    };
    wrapped_bullet(lines, &message, inner, "");
}

fn render_live_facts(snapshot: &PanelSnapshot, inner: usize, lines: &mut Vec<String>) {
    section(lines, crate::tr!("실시간 사실", "Live facts"), inner);
    if snapshot.live_repo.available {
        let branch = snapshot.live_repo.branch.as_deref().unwrap_or("detached");
        lines.push(fit(
            &crate::tr!(
                format!(
                    "브랜치 {branch} · 변경 파일 {}개",
                    snapshot.live_repo.changed_file_count
                ),
                format!(
                    "Branch {branch} · {} changed files",
                    snapshot.live_repo.changed_file_count
                )
            ),
            inner,
        ));
        if snapshot.current.revision > 0 && snapshot.current.repo.head != snapshot.live_repo.head {
            lines.push(fit(
                crate::tr!(
                    "요약 이후 저장소 변경 감지",
                    "Repository changed since summary"
                ),
                inner,
            ));
        }
    } else {
        lines.push(crate::tr!("Git 저장소 아님", "No Git repository").to_string());
    }
}

fn render_verified_summary(snapshot: &PanelSnapshot, inner: usize, lines: &mut Vec<String>) {
    section(
        lines,
        &if snapshot.pending.preview.is_some() {
            crate::tr!(
                format!("최신 우선 맥락 · p{} · 잠정", snapshot.current.revision),
                format!(
                    "Latest-first context · p{} · provisional",
                    snapshot.current.revision
                )
            )
        } else {
            crate::tr!(
                format!("저장된 맥락 · r{}", snapshot.current.revision),
                format!("Saved context · r{}", snapshot.current.revision)
            )
        },
        inner,
    );

    if snapshot.current.revision == 0 {
        wrapped_bullet(
            lines,
            crate::tr!(
                "아직 확정된 맥락이 없습니다. 첫 작업이 끝나면 설정한 요약 모델이 기존 세션과 새 대화를 정리합니다.",
                "No confirmed context yet. After the first task finishes, the configured summary model organizes existing sessions and new conversation."
            ),
            inner,
            "",
        );
        return;
    }

    section(lines, crate::tr!("목표", "Objective"), inner);
    if snapshot.current.summary.objective.is_empty() {
        wrapped_bullet(
            lines,
            crate::tr!(
                "장기 목표를 아직 확인하지 못했습니다. 의미 있는 목표가 확인되면 자동으로 반영됩니다.",
                "No long-term objective confirmed yet. A meaningful objective is reflected automatically when confirmed."
            ),
            inner,
            "",
        );
    } else {
        wrapped_bullet(lines, &snapshot.current.summary.objective, inner, "");
    }

    render_decisions(snapshot, inner, lines);
    render_progress(snapshot, inner, lines);
    render_blockers(snapshot, inner, lines);
    render_next_actions(snapshot, inner, lines);
}

fn render_decisions(snapshot: &PanelSnapshot, inner: usize, lines: &mut Vec<String>) {
    let mut visible: Vec<_> = snapshot
        .current
        .summary
        .decisions
        .iter()
        .filter(|decision| {
            matches!(
                decision.status,
                DecisionStatus::Active | DecisionStatus::Proposed
            )
        })
        .collect();
    visible.sort_by_key(|decision| {
        std::cmp::Reverse(snapshot.current.timing.decisions.get(&decision.id).copied())
    });
    if !visible.is_empty() {
        section(lines, crate::tr!("결정", "Decisions"), inner);
        for decision in visible.into_iter().take(MAX_VISIBLE_DECISIONS) {
            let marker = match decision.status {
                DecisionStatus::Active => "",
                DecisionStatus::Proposed => crate::tr!("[제안] ", "[Proposed] "),
                DecisionStatus::Superseded | DecisionStatus::Reversed => continue,
            };
            let at = snapshot.current.timing.decisions.get(&decision.id).copied();
            wrapped_bullet(
                lines,
                &decision.text,
                inner,
                &format!("{} · {marker}", item_time(at)),
            );
        }
    }
}

fn render_progress(snapshot: &PanelSnapshot, inner: usize, lines: &mut Vec<String>) {
    if !snapshot.current.summary.progress.is_empty() {
        section(lines, crate::tr!("진행", "Progress"), inner);
        let mut visible: Vec<_> = snapshot.current.summary.progress.iter().collect();
        visible.sort_by_key(|item| {
            std::cmp::Reverse(snapshot.current.timing.progress.get(&item.text).copied())
        });
        for item in visible.into_iter().take(MAX_VISIBLE_PROGRESS) {
            let marker = match item.status {
                ProgressStatus::Done => crate::tr!("[확인] ", "[Confirmed] "),
                ProgressStatus::Reported => crate::tr!("[보고] ", "[Reported] "),
                ProgressStatus::InProgress => crate::tr!("[진행] ", "[In progress] "),
                ProgressStatus::Blocked => crate::tr!("[막힘] ", "[Blocked] "),
                ProgressStatus::Unverified => crate::tr!("[미검증] ", "[Unverified] "),
            };
            let at = snapshot.current.timing.progress.get(&item.text).copied();
            wrapped_bullet(
                lines,
                &item.text,
                inner,
                &format!("{} · {marker}", item_time(at)),
            );
        }
    }
}

fn item_time(at: Option<u64>) -> String {
    at.map_or_else(
        || crate::tr!("시각 미기록", "Time not recorded").into(),
        |at| crate::timing::format_kst(at, unix_now()),
    )
}

fn render_blockers(snapshot: &PanelSnapshot, inner: usize, lines: &mut Vec<String>) {
    if !snapshot.current.summary.blockers.is_empty() {
        section(lines, crate::tr!("막힌 부분", "Blockers"), inner);
        let mut visible: Vec<_> = snapshot.current.summary.blockers.iter().collect();
        visible.sort_by_key(|blocker| {
            (
                blocker_priority_rank(blocker.priority()),
                std::cmp::Reverse(
                    snapshot
                        .current
                        .timing
                        .blockers
                        .get(blocker.text())
                        .copied(),
                ),
            )
        });
        for blocker in visible.into_iter().take(MAX_VISIBLE_BLOCKERS) {
            let label = match blocker.priority() {
                crate::model::BlockerPriority::UserDecision => {
                    crate::tr!("사용자 결정", "User decision")
                }
                crate::model::BlockerPriority::Current => crate::tr!("현재 작업", "Current task"),
                crate::model::BlockerPriority::Later => crate::tr!("이후 단계", "Later stage"),
                crate::model::BlockerPriority::Unknown => crate::tr!("미분류", "Unclassified"),
            };
            wrapped_bullet(lines, blocker.text(), inner, &format!("[{label}] "));
        }
    }
}

fn render_next_actions(snapshot: &PanelSnapshot, inner: usize, lines: &mut Vec<String>) {
    if !snapshot.current.summary.next_actions.is_empty() {
        section(lines, crate::tr!("다음 행동", "Next actions"), inner);
        let mut visible: Vec<_> = snapshot.current.summary.next_actions.iter().collect();
        visible.sort_by_key(|action| {
            (
                action.step.unwrap_or(u8::MAX),
                std::cmp::Reverse(
                    snapshot
                        .current
                        .timing
                        .next_actions
                        .get(&action.text)
                        .copied(),
                ),
            )
        });
        for action in visible.into_iter().take(MAX_VISIBLE_ACTIONS) {
            let owner = if action.owner == "sol" {
                "Codex"
            } else {
                owner_label(&action.owner)
            };
            let when = if snapshot.pending.preview.is_some() {
                crate::tr!("잠정", "Provisional")
            } else {
                action
                    .when
                    .as_deref()
                    .unwrap_or(crate::tr!("순서 확인 필요", "Order needs review"))
            };
            wrapped_bullet(lines, &action.text, inner, &format!("[{when} · {owner}] "));
        }
    }
}

fn blocker_priority_rank(priority: crate::model::BlockerPriority) -> u8 {
    match priority {
        crate::model::BlockerPriority::UserDecision => 0,
        crate::model::BlockerPriority::Current => 1,
        crate::model::BlockerPriority::Later => 2,
        crate::model::BlockerPriority::Unknown => 3,
    }
}

fn main_footer(total: usize, scroll: usize, width: usize, height: usize) -> String {
    let visible = height.saturating_sub(1);
    let first = if total == 0 { 0 } else { scroll + 1 };
    let last = scroll.saturating_add(visible).min(total);
    let below = last < total;
    let cue = if below {
        "↓"
    } else if scroll > 0 {
        "↑"
    } else {
        ""
    };
    let position = format!("{first}-{last}/{total}");
    let status = if below {
        crate::tr!("↓ 아래 내용", "↓ more below")
    } else if scroll > 0 {
        crate::tr!("↑ 위 내용 · 끝", "↑ more above · end")
    } else {
        crate::tr!("전체 표시", "All shown")
    };
    [
        crate::tr!(
            format!("{status} · {position} · r 반영 · l 연결 · e 근거 · h 기록 · m 메뉴"),
            format!(
                "{status} · {position} · r reflect · l links · e evidence · h records · m menu"
            )
        ),
        crate::tr!(
            format!("{status} · {position} · r 반영 · h 기록 · m 메뉴"),
            format!("{status} · {position} · r reflect · h records · m menu")
        ),
        crate::tr!(
            format!("{cue} {position} · r 반영 · h 기록 · m 메뉴"),
            format!("{cue} {position} · r reflect · h records · m menu")
        ),
        crate::tr!(
            format!("{cue} {position} · r 반영 · h · m"),
            format!("{cue} {position} · r reflect · h · m")
        ),
        crate::tr!("r 반영 · h · m", "r reflect · h · m").to_string(),
        crate::tr!("r 반영 · m", "r reflect · m").to_string(),
        crate::tr!("r 반영", "r reflect").to_string(),
    ]
    .into_iter()
    .find(|footer| cell_width(footer) <= width)
    .unwrap_or_else(|| "r".to_string())
}

fn draw(
    lines: &[Line],
    scroll: usize,
    width: u16,
    height: u16,
    selection: Option<&crate::panel_selection::Selection>,
    footer: Option<&str>,
) -> AppResult<()> {
    let mut output = std::io::stdout();
    queue!(output, MoveTo(0, 0), Clear(ClearType::All))?;
    let body_height = usize::from(height).saturating_sub(1);
    for (row, line) in lines.iter().skip(scroll).take(body_height).enumerate() {
        let row = u16::try_from(row).unwrap_or(u16::MAX);
        queue!(
            output,
            MoveTo(0, row),
            SetAttribute(Attribute::Reset),
            SetForegroundColor(line.role.color()),
            SetAttribute(line.role.attribute())
        )?;
        if let Some(selected) = selection {
            let mut column = 0;
            let mut reversed = false;
            for ch in fit(line, usize::from(width)).chars() {
                let cells = ch.width().unwrap_or(0);
                let active = if cells == 0 {
                    reversed
                } else {
                    selected.selected(scroll + usize::from(row), column, cells)
                };
                if active != reversed {
                    queue!(
                        output,
                        SetAttribute(if active {
                            Attribute::Reverse
                        } else {
                            Attribute::NoReverse
                        })
                    )?;
                }
                queue!(output, Print(ch))?;
                reversed = active;
                column += cells;
            }
        } else {
            queue!(output, Print(fit(line, usize::from(width))))?;
        }
        queue!(output, SetAttribute(Attribute::Reset), ResetColor)?;
    }
    if height > 0 {
        queue!(
            output,
            MoveTo(0, height - 1),
            SetForegroundColor(panel_style::MUTED),
            Print(fit(
                if let Some(footer) = footer {
                    footer
                } else if let Some(selected) = selection {
                    if selected.dragging {
                        crate::tr!(
                            "드래그 선택 중 · 놓으면 복사 요청 · Esc 해제",
                            "Drag selection · release to request copy · Esc clear"
                        )
                    } else {
                        crate::tr!(
                            "선택 유지 · 복사 요청(64 KiB 이내, 터미널 지원 필요) · Esc 해제",
                            "Selection retained · copy requested (up to 64 KiB, requires terminal support) · Esc clear"
                        )
                    }
                } else {
                    if width < 52 {
                        crate::tr!("h 기록 · m 메뉴", "h records · m menu")
                    } else {
                        crate::tr!(
                            "l 연결 · e 근거 · h 전체 기록 · m 메뉴",
                            "l links · e evidence · h all records · m menu"
                        )
                    }
                },
                usize::from(width)
            )),
            ResetColor
        )?;
    }
    output.flush()?;
    Ok(())
}

fn draw_menu(menu: &crate::panel_menu::Menu, height: u16) -> AppResult<()> {
    if menu.y >= height.saturating_sub(1) {
        return Ok(());
    }
    let mut output = std::io::stdout();
    let width = usize::from(menu.width);
    if width < 15 || menu.height < 5 {
        queue!(
            output,
            MoveTo(menu.x, menu.y),
            Print(fit(
                crate::tr!("창을 넓혀주세요", "Enlarge the window"),
                width
            ))
        )?;
        output.flush()?;
        return Ok(());
    }
    let visible = menu.visible_items();
    let mut rows = vec![("Maeklog", ""), (crate::panel_menu::hint(), "")];
    rows.extend(visible.clone().map(|index| {
        (
            crate::panel_menu::item(index),
            crate::panel_menu::shortcut(index),
        )
    }));
    // Keep the existing box height; controls live in the fixed active footer.
    rows.extend([("", ""), ("", "")]);
    for (index, (text, shortcut)) in rows.iter().enumerate() {
        let row = menu.y + u16::try_from(index)?;
        if row >= height.saturating_sub(1) {
            break;
        }
        queue!(
            output,
            MoveTo(menu.x, row),
            SetBackgroundColor(panel_style::SURFACE),
            SetForegroundColor(panel_style::BODY)
        )?;
        if index == menu.selected.saturating_sub(visible.start) + 2 {
            queue!(
                output,
                SetForegroundColor(panel_style::YELLOW),
                SetAttribute(Attribute::Bold),
                SetAttribute(Attribute::Reverse)
            )?;
        } else if index == 0 {
            queue!(
                output,
                SetForegroundColor(panel_style::AQUA),
                SetAttribute(Attribute::Bold)
            )?;
        }
        let line = menu_row(text, shortcut, width.saturating_sub(2));
        let cells = cell_width(&line);
        queue!(
            output,
            Print(format!(
                " {line}{} ",
                " ".repeat(width.saturating_sub(cells + 2))
            )),
            SetAttribute(Attribute::Reset),
            ResetColor
        )?;
    }
    output.flush()?;
    Ok(())
}

fn menu_row(text: &str, shortcut: &str, width: usize) -> String {
    if shortcut.is_empty() || width < shortcut.len() + 3 {
        return fit(text, width);
    }
    let label = fit(text, width.saturating_sub(shortcut.len() + 2));
    let gap = width.saturating_sub(cell_width(&label) + shortcut.len());
    format!("{label}{}{shortcut}", " ".repeat(gap))
}

fn owner_label(owner: &str) -> &str {
    match owner {
        "user" => crate::tr!("사용자", "User"),
        "sol" => "Sol",
        "luna" => "Luna",
        _ => crate::tr!("담당 미정", "Owner unspecified"),
    }
}

fn section(lines: &mut Vec<String>, title: &str, width: usize) {
    lines.push(String::new());
    lines.push(fit(&format!("{title} {}", rule(width)), width));
}

pub(crate) fn wrapped_bullet(lines: &mut Vec<String>, value: &str, width: usize, prefix: &str) {
    let available = width.saturating_sub(cell_width(prefix)).max(1);
    let wrapped = wrap(&sanitize(value), available);
    for (index, line) in wrapped.into_iter().enumerate() {
        let lead = if index == 0 {
            prefix.to_string()
        } else {
            " ".repeat(cell_width(prefix))
        };
        lines.push(fit(&format!("{lead}{line}"), width));
    }
}

fn wrap(value: &str, width: usize) -> Vec<String> {
    if value.is_empty() {
        return vec![String::new()];
    }
    let width = width.max(1);
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_width = 0;
    for character in value.chars() {
        if character == '\n' {
            lines.push(std::mem::take(&mut current));
            current_width = 0;
            continue;
        }
        let character_width = character.width().unwrap_or(0);
        if current_width + character_width > width && !current.is_empty() {
            if character.is_whitespace() {
                lines.push(std::mem::take(&mut current));
                current_width = 0;
                continue;
            }
            // Keep prose words intact when a preceding whitespace boundary fits.
            // Boundary whitespace becomes a display-row separator, as before.
            let boundary = current
                .char_indices()
                .rev()
                .find(|(_, ch)| ch.is_whitespace())
                .filter(|(index, _)| current[..*index].chars().any(|ch| !ch.is_whitespace()))
                .map(|(index, ch)| index + ch.len_utf8())
                .or_else(|| punctuation_carry(&current, character, width));
            if let Some(boundary) = boundary {
                let tail = current.split_off(boundary);
                lines.push(std::mem::replace(&mut current, tail));
                current_width = cell_width(&current);
            } else {
                lines.push(std::mem::take(&mut current));
                current_width = 0;
            }
        }
        current.push(character);
        current_width += character_width;
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

fn punctuation_carry(value: &str, next: char, width: usize) -> Option<usize> {
    if !closing_punctuation(next) {
        return None;
    }
    // Carry one preceding visible character and any trailing zero-width marks
    // or closing punctuation. This is not full Unicode grapheme segmentation.
    let (start, _) = value.char_indices().rev().find(|(_, ch)| {
        ch.width().unwrap_or(0) > 0 && !ch.is_whitespace() && !closing_punctuation(*ch)
    })?;
    (start > 0 && cell_width(&value[start..]) + next.width().unwrap_or(0) <= width).then_some(start)
}

fn closing_punctuation(character: char) -> bool {
    matches!(
        character,
        '.' | ','
            | '!'
            | '?'
            | ';'
            | ':'
            | '…'
            | '。'
            | '、'
            | '，'
            | '！'
            | '？'
            | '；'
            | '：'
            | ')'
            | ']'
            | '}'
            | '>'
            | '）'
            | '］'
            | '｝'
            | '】'
            | '〕'
            | '〉'
            | '》'
            | '」'
            | '』'
            | '”'
            | '’'
            | '"'
            | '\''
    )
}

fn fit(value: &str, width: usize) -> String {
    let value = sanitize(value);
    let mut result = String::new();
    let mut used = 0;
    for character in value.chars() {
        let character_width = character.width().unwrap_or(0);
        if used + character_width > width {
            break;
        }
        result.push(character);
        used += character_width;
    }
    result
}

fn rule(width: usize) -> String {
    "─".repeat(width.min(80))
}

fn cell_width(value: &str) -> usize {
    value
        .chars()
        .map(|character| character.width().unwrap_or(0))
        .sum()
}

fn sanitize(value: &str) -> String {
    value
        .chars()
        .filter(|character| *character == '\n' || !character.is_control())
        .collect()
}

fn first_nonempty_line(value: &str) -> String {
    value
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or(crate::tr!("내용 없는 요청", "Empty request"))
        .to_string()
}

fn status_priority(status: &str) -> u8 {
    match status {
        "blocked" => 5,
        "working" => 4,
        "done" => 3,
        "idle" => 2,
        _ => 1,
    }
}

struct TerminalGuard {
    pane_id: Option<String>,
}

impl TerminalGuard {
    fn enter() -> AppResult<Self> {
        enable_raw_mode()?;
        let mut guard = Self { pane_id: None };
        execute!(
            std::io::stdout(),
            EnterAlternateScreen,
            Hide,
            EnableMouseCapture,
            EnableBracketedPaste
        )?;
        if let Ok(pane_id) = std::env::var("HERDR_PANE_ID") {
            herdr::route_panel_right_click(&pane_id, true)?;
            guard.pane_id = Some(pane_id);
        }
        Ok(guard)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        if let Some(pane_id) = &self.pane_id {
            let _ = herdr::route_panel_right_click(pane_id, false);
        }
        let _ = execute!(
            std::io::stdout(),
            DisableMouseCapture,
            DisableBracketedPaste,
            Show,
            LeaveAlternateScreen,
            ResetColor
        );
        let _ = disable_raw_mode();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Decision, NextAction, ProgressItem};

    #[test]
    fn menu_shortcut_column_keeps_the_key_visible_after_label_truncation() {
        for (label, shortcut) in [
            ("근거 확인·기록 수정", "e"),
            ("Summarize pending turns today", "r"),
            ("Close menu", "Esc"),
        ] {
            for width in [8, 28, 36, 80] {
                let row = menu_row(label, shortcut, width);
                assert_eq!(cell_width(&row), width);
                assert!(row.ends_with(shortcut));
                assert!(row[..row.len() - shortcut.len()].ends_with("  "));
            }
        }
    }

    #[test]
    fn korean_text_wraps_by_terminal_cells() {
        assert_eq!(wrap("한글테스트", 4), vec!["한글", "테스", "트"]);
    }

    #[test]
    fn prose_wrap_keeps_english_words_and_negation_intact() {
        let text = "maeklog · main · observed location\nResponse in progress\nAutomatic retry stopped\nPermission not granted";
        let expected: Vec<_> = text.split_whitespace().collect();
        for width in [14, 18, 28, 48, 78] {
            let rows = wrap(text, width);
            let words: Vec<_> = rows.iter().flat_map(|row| row.split_whitespace()).collect();
            assert_eq!(words, expected, "word split at width {width}");
            assert!(rows.iter().all(|row| cell_width(row) <= width));
        }
        assert_eq!(
            wrap("Response in progress", 18),
            ["Response in ", "progress"]
        );
        assert_eq!(
            wrap("Automatic retry stopped", 20),
            ["Automatic retry ", "stopped"]
        );
    }

    #[test]
    fn closing_punctuation_stays_with_preceding_visible_text() {
        for (text, width) in [
            ("진행하지 않았습니다.", 19),
            ("가나다라마바사아자차카타파하.", 28),
            ("가나\u{301}.", 4),
            ("가나.)", 5),
        ] {
            let rows = wrap(text, width);
            assert_eq!(rows.concat(), text);
            assert!(rows.iter().all(|row| cell_width(row) <= width));
            assert!(rows.iter().all(|row| !row.starts_with('\u{301}')));
            assert!(rows.iter().all(|row| {
                !row.chars()
                    .all(|ch| closing_punctuation(ch) || ch.is_whitespace())
            }));
        }
    }

    #[test]
    fn long_unbroken_source_tokens_round_trip_at_tiny_widths() {
        for text in [
            "https://example.invalid/source?hash=abcdef0123456789",
            "c7d58ef73e083143b4dcd0f985b412c35f60dc15a880548ec820e62160e9caf2",
            "한글원문전체보존",
        ] {
            for width in [2, 3, 4, 28, 48, 78] {
                let rows = wrap(text, width);
                assert_eq!(rows.concat(), text, "source loss at width {width}");
                assert!(rows.iter().all(|row| cell_width(row) <= width));
            }
        }
        for width in [0, 1] {
            let rows = wrap("abcde", width);
            assert_eq!(rows.concat(), "abcde");
            assert!(rows.iter().all(|row| cell_width(row) <= width.max(1)));
        }
    }

    #[test]
    fn public_wrap_preserves_paragraphs_sanitation_and_hanging_prefixes() {
        let mut rows = Vec::new();
        wrapped_bullet(
            &mut rows,
            "Keep\tNOT\x1bapproved.\n\n   \nnext\rsource\x07",
            20,
            "",
        );
        assert_eq!(rows, ["KeepNOTapproved.", "", "   ", "nextsource"]);
        assert!(
            rows.iter()
                .all(|row| row.chars().all(|ch| !ch.is_control()))
        );
        rows.clear();
        wrapped_bullet(&mut rows, "abcde", 2, "");
        assert_eq!(rows, ["ab", "cd", "e"]);
        rows.clear();
        wrapped_bullet(&mut rows, "abcde", 4, "• ");
        assert_eq!(rows, ["• ab", "  cd", "  e"]);
        assert!(rows.iter().all(|row| cell_width(row) <= 4));
        rows.clear();
        wrapped_bullet(&mut rows, "승인후에만실행.", 6, "• ");
        assert!(rows.iter().all(|row| cell_width(row) <= 6));
        let body: String = rows
            .iter()
            .enumerate()
            .map(|(i, row)| row.strip_prefix(if i == 0 { "• " } else { "  " }).unwrap())
            .collect();
        assert_eq!(body, "승인후에만실행.");
    }

    #[test]
    fn priorities_and_prerequisites_come_before_recency() {
        let summary: crate::model::Summary = serde_json::from_value(serde_json::json!({
            "blockers": [
                {"text":"future-blocker","priority":"later","evidence":[]},
                {"text":"current-blocker","priority":"current","evidence":[]},
                {"text":"older-user-blocker","priority":"user_decision","evidence":[]},
                {"text":"newer-user-blocker","priority":"user_decision","evidence":[]}
            ],
            "next_actions": [
                {"owner":"sol","text":"validate-action","step":2,"when":"구현 후","evidence":[]},
                {"owner":"sol","text":"build-action","step":1,"when":"승인 후","evidence":[]},
                {"owner":"user","text":"approve-action","step":0,"when":"지금","evidence":[]},
                {"owner":"sol","text":"newer-build-action","step":1,"when":"승인 후","evidence":[]},
                {"owner":"unknown","text":"legacy-action","evidence":[]}
            ]
        }))
        .unwrap();
        let snapshot = PanelSnapshot {
            branch: None,
            live: None,
            catch_up: None,
            connection_notice: None,
            label: "test".into(),
            agent_status: "idle".into(),
            agent_title: None,
            current_request: None,
            pending: Pending::default(),
            worker: WorkerStatus::default(),
            current: CurrentState {
                summary,
                timing: crate::model::ContextTiming {
                    blockers: [
                        ("future-blocker".into(), 900),
                        ("current-blocker".into(), 800),
                        ("older-user-blocker".into(), 100),
                        ("newer-user-blocker".into(), 200),
                    ]
                    .into(),
                    next_actions: [
                        ("validate-action".into(), 900),
                        ("build-action".into(), 200),
                        ("approve-action".into(), 100),
                        ("newer-build-action".into(), 300),
                    ]
                    .into(),
                    ..crate::model::ContextTiming::default()
                },
                ..CurrentState::default()
            },
            captured_at: 0,
            display_times: crate::local_time::LocalTimes::default(),
            live_repo: RepoSnapshot::default(),
        };
        let mut lines = Vec::new();
        render_blockers(&snapshot, 100, &mut lines);
        render_next_actions(&snapshot, 100, &mut lines);
        let rendered = lines.join("\n");
        let positions: Vec<_> = [
            "newer-user-blocker",
            "older-user-blocker",
            "current-blocker",
            "approve-action",
            "newer-build-action",
            " · Codex] build-action",
            "validate-action",
        ]
        .iter()
        .map(|text| rendered.find(text).unwrap())
        .collect();
        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(!rendered.contains("future-blocker"));
        assert!(!rendered.contains("legacy-action"));
        assert!(rendered.contains(crate::tr!(
            "[지금 · 사용자] approve-action",
            "[지금 · User] approve-action"
        )));
        assert!(rendered.contains("[승인 후 · Codex] build-action"));
        assert!(rendered.contains("[구현 후 · Codex] validate-action"));
    }

    #[test]
    fn legacy_items_are_not_guessed_ready_and_empty_sections_disappear() {
        let summary: crate::model::Summary = serde_json::from_value(serde_json::json!({
            "blockers":["old blocker"],
            "next_actions":[{"owner":"user","text":"old action","evidence":[]}]
        }))
        .unwrap();
        let mut snapshot = PanelSnapshot {
            branch: None,
            live: None,
            catch_up: None,
            connection_notice: None,
            label: "test".into(),
            agent_status: "idle".into(),
            agent_title: None,
            current_request: None,
            pending: Pending::default(),
            worker: WorkerStatus::default(),
            current: CurrentState {
                summary,
                ..CurrentState::default()
            },
            captured_at: 0,
            display_times: crate::local_time::LocalTimes::default(),
            live_repo: RepoSnapshot::default(),
        };
        let mut lines = Vec::new();
        render_blockers(&snapshot, 80, &mut lines);
        render_next_actions(&snapshot, 80, &mut lines);
        let rendered = lines.join("\n");
        assert!(rendered.contains(crate::tr!(
            "[미분류] old blocker",
            "[Unclassified] old blocker"
        )));
        assert!(rendered.contains(crate::tr!(
            "[순서 확인 필요 · 사용자] old action",
            "[Order needs review · User] old action"
        )));
        assert!(!rendered.contains("[지금"));
        snapshot.current.summary.blockers.clear();
        snapshot.current.summary.next_actions.clear();
        lines.clear();
        render_blockers(&snapshot, 80, &mut lines);
        render_next_actions(&snapshot, 80, &mut lines);
        assert!(lines.is_empty());
    }

    #[test]
    fn working_view_keeps_verified_revision_visible() {
        let snapshot = PanelSnapshot {
            branch: None,
            live: None,
            catch_up: None,
            connection_notice: None,
            label: "demo-shop-3".into(),
            agent_status: "working".into(),
            agent_title: Some("demo-shop-3".into()),
            current_request: Some("Issue 253 구현".into()),
            pending: Pending {
                total: 1,
                ..Pending::default()
            },
            worker: WorkerStatus::default(),
            current: CurrentState {
                revision: 3,
                base_revision: Some(2),
                source_seq: 8,
                updated_at_unix: Some(unix_now()),
                timing: crate::model::ContextTiming::default(),
                repo: RepoSnapshot::default(),
                summary: crate::model::Summary {
                    correction_revision: None,
                    record_updates: vec![],
                    state_updates: vec![],
                    objective: "상품 상세 기능 구현".into(),
                    objective_evidence: vec!["event:1".into()],
                    discussion: vec![],
                    decisions: vec![Decision {
                        id: "D-001".into(),
                        text: "exact code 사용".into(),
                        status: DecisionStatus::Active,
                        supersedes: None,
                        evidence: vec!["event:8".into()],
                    }],
                    progress: vec![ProgressItem {
                        text: "조사 완료".into(),
                        status: ProgressStatus::Done,
                        evidence: vec!["event:8".into()],
                    }],
                    blockers: vec![],
                    next_actions: vec![NextAction {
                        owner: "sol".into(),
                        text: "테스트 작성".into(),
                        evidence: vec!["event:8".into()],
                        step: None,
                        when: None,
                    }],
                    notes: vec![],
                },
            },
            captured_at: 0,
            display_times: crate::local_time::LocalTimes::default(),
            live_repo: RepoSnapshot::default(),
        };
        let rendered = render_lines(&snapshot, 48).join("\n");
        assert!(rendered.contains(crate::tr!("[Codex · 작업 중]", "[Codex · Working]")));
        assert!(rendered.contains("Issue 253"));
        assert!(rendered.contains(crate::tr!("저장된 맥락 · r3", "Saved context · r3")));
        assert!(rendered.contains("exact code 사용"));
        assert!(rendered.contains(crate::tr!("[확인] 조사 완료", "[Confirmed] 조사 완료")));
        assert!(rendered.contains("Codex] 테스트 작성"));
    }

    #[test]
    fn header_uses_conversation_cutoff_instead_of_summary_creation_time() {
        let at = crate::transcript::utc_timestamp_seconds("2026-09-06T00:12:00Z").unwrap();
        let snapshot = PanelSnapshot {
            branch: None,
            live: None,
            catch_up: None,
            connection_notice: None,
            label: "test".into(),
            agent_status: "working".into(),
            agent_title: None,
            current_request: None,
            pending: Pending {
                total: 1,
                ..Pending::default()
            },
            worker: WorkerStatus::default(),
            current: CurrentState {
                revision: 7,
                updated_at_unix: Some(at + 300),
                timing: crate::model::ContextTiming {
                    through_unix: Some(at),
                    ..crate::model::ContextTiming::default()
                },
                ..CurrentState::default()
            },
            captured_at: 0,
            display_times: crate::local_time::LocalTimes::default(),
            live_repo: RepoSnapshot::default(),
        };
        let rendered = render_lines(&snapshot, 70).join("\n");
        assert!(rendered.contains(crate::tr!(
            "09:12까지 대화 반영",
            "09:12 conversation reflected through"
        )));
        assert!(!rendered.contains("09:17"));
        assert!(rendered.contains(crate::tr!("한국시간", "KST")));
        assert!(rendered.contains(crate::tr!(
            "진행 중인 작업의 완료 내용은 아직 미반영",
            "Completion of the running task has not been reflected yet"
        )));
    }

    #[test]
    fn freshness_distinguishes_backlog_open_work_failure_and_completion() {
        let mut snapshot = PanelSnapshot {
            branch: None,
            live: None,
            catch_up: None,
            connection_notice: None,
            label: "test".into(),
            agent_status: "idle".into(),
            agent_title: None,
            current_request: None, // A final-only backlog still needs a freshness warning.
            pending: Pending {
                total: 66,
                completed: 64,
                today: 64,
                ..Pending::default()
            },
            worker: WorkerStatus {
                state: "idle".into(),
                ..WorkerStatus::default()
            },
            current: CurrentState {
                revision: 64,
                ..CurrentState::default()
            },
            captured_at: 0,
            display_times: crate::local_time::LocalTimes::default(),
            live_repo: RepoSnapshot::default(),
        };
        let render = |snapshot: &PanelSnapshot| render_lines(snapshot, 80).join("\n");
        let text = render(&snapshot);
        assert!(text.contains(crate::tr!(
            "[미반영] 기록 66개 · 오늘 64개",
            "[Pending] 66 records · 64 today"
        )));
        assert!(text.contains(crate::tr!("[요약 대기]", "[Summary pending]")));
        assert!(text.contains(crate::tr!(
            "[Codex · 입력 대기]",
            "[Codex · Awaiting input]"
        )));
        assert!(!text.contains(crate::tr!("[반영 완료]", "[Up to date]")));
        snapshot.worker.state = "summarizing".into();
        assert!(render(&snapshot).contains(crate::tr!("[요약 대기]", "[Summary pending]"))); // Interrupted worker.
        snapshot.pending.worker_alive = true;
        assert!(
            render(&snapshot).contains(crate::tr!("[요약 · 정리 중]", "[Summary · organizing]"))
        );
        for (state, label) in [
            ("retry_wait", crate::tr!("[재시도 대기]", "[Retry pending]")),
            ("error", crate::tr!("[요약 멈춤]", "[Summary stopped]")),
        ] {
            snapshot.worker.state = state.into();
            assert!(render(&snapshot).contains(label));
            assert!(!render(&snapshot).contains(crate::tr!("[반영 완료]", "[Up to date]")));
        }
        snapshot.worker.state = "idle".into();
        snapshot.pending = Pending {
            total: 1,
            ..Pending::default()
        };
        assert!(
            render(&snapshot).contains(crate::tr!("[작업 완료 대기]", "[Waiting for completion]"))
        );
        snapshot.pending = Pending::default();
        assert!(render(&snapshot).contains(crate::tr!("[반영 완료]", "[Up to date]")));
        snapshot.pending.rebuild = true;
        assert!(!render(&snapshot).contains(crate::tr!("[반영 완료]", "[Up to date]")));
        snapshot.pending.rebuild = false;
        snapshot.connection_notice = Some("새 세션 연결 미확인".into());
        assert!(!render(&snapshot).contains(crate::tr!("[반영 완료]", "[Up to date]")));
        snapshot.connection_notice = None;
        snapshot.current.revision = 0;
        assert!(render(&snapshot).contains(crate::tr!("[맥락 없음]", "[No context]")));
        for width in [24, 48, 80] {
            assert!(
                render_lines(&snapshot, width)
                    .iter()
                    .all(|line| cell_width(line) <= width)
            );
        }
    }

    #[test]
    fn pending_counts_use_korean_midnight_and_keep_open_prompts_separate() {
        let midnight = crate::transcript::utc_timestamp_seconds("2026-09-06T15:00:00Z").unwrap();
        let events: Vec<crate::model::TurnEvent> = [
            ("assistant_final", midnight - 1),
            ("user_prompt", midnight),
            ("assistant_final", midnight + 1),
            ("user_prompt", midnight + 2),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, (kind, at))| {
            serde_json::from_value(serde_json::json!({
                "seq":i+1,"kind":kind,"recorded_at_unix":at,"session_id":"test",
                "content":"test","truncated":false,"cwd":"/repo"
            }))
            .unwrap()
        })
        .collect();
        let pending = Pending::from_events(&events, midnight + 10);
        assert_eq!((pending.total, pending.completed, pending.today), (4, 3, 3));
        assert_eq!(
            Pending::from_events(&events[3..], midnight + 10).completed,
            0
        );
    }

    #[test]
    fn preview_labels_the_gap_and_never_displays_actions_as_ready() {
        let at = crate::transcript::utc_timestamp_seconds("2026-09-07T03:00:00Z").unwrap();
        let snapshot = PanelSnapshot {
            branch: None,
            live: None,
            catch_up: None,
            captured_at:0, display_times:crate::local_time::LocalTimes::default(),
            connection_notice:None, label:"test".into(), agent_status:"idle".into(), agent_title:None,
            current_request:None, worker:WorkerStatus::default(), live_repo:RepoSnapshot::default(),
            pending: Pending { total:60, completed:60, preview:Some(Preview { cumulative_through:Some(at-86_400) }), ..Pending::default() },
            current:CurrentState { revision:1, timing:crate::model::ContextTiming {through_unix:Some(at), ..crate::model::ContextTiming::default()},
                summary:serde_json::from_value(serde_json::json!({"objective":"goal", "next_actions":[{"owner":"sol","text":"deploy","step":0,"when":"지금","evidence":[]}]})).unwrap(),
                ..CurrentState::default() },
        };
        let rendered = render_lines(&snapshot, 80).join("\n");
        assert!(rendered.contains(crate::tr!(
            "12:00 최신 작업 반영",
            "12:00 latest work reflected"
        )));
        assert!(rendered.contains(crate::tr!(
            "[과거 맥락 미완료]",
            "[Earlier context incomplete]"
        )));
        assert!(rendered.contains(crate::tr!(
            "누적 정리 60개 대기",
            "60 records awaiting cumulative processing"
        )));
        assert!(rendered.contains(crate::tr!(
            "이전 승인·조건 미반영 가능",
            "Earlier approvals/conditions may be missing"
        )));
        assert!(rendered.contains(crate::tr!(
            "[잠정 · Codex] deploy",
            "[Provisional · Codex] deploy"
        )));
        assert!(!rendered.contains("[지금"));
        assert!(!rendered.contains(crate::tr!("[반영 완료]", "[Up to date]")));
        for width in [24, 48, 80] {
            assert!(
                render_lines(&snapshot, width)
                    .iter()
                    .all(|line| cell_width(line) <= width)
            );
        }
    }

    #[test]
    fn superseded_decisions_are_kept_out_of_the_default_panel() {
        let snapshot = PanelSnapshot {
            branch: None,
            live: None,
            catch_up: None,
            connection_notice: None,
            label: "project".into(),
            agent_status: "idle".into(),
            agent_title: None,
            current_request: None,
            pending: Pending::default(),
            worker: WorkerStatus::default(),
            current: CurrentState {
                revision: 2,
                summary: crate::model::Summary {
                    correction_revision: None,
                    objective: "사용자가 현재 결정을 빠르게 파악한다".into(),
                    decisions: vec![
                        Decision {
                            id: "D-001".into(),
                            text: "예전 선택".into(),
                            status: DecisionStatus::Superseded,
                            supersedes: None,
                            evidence: vec!["event:1".into()],
                        },
                        Decision {
                            id: "D-002".into(),
                            text: "현재 선택".into(),
                            status: DecisionStatus::Active,
                            supersedes: Some("D-001".into()),
                            evidence: vec!["event:2".into()],
                        },
                    ],
                    progress: vec![ProgressItem {
                        text: "구현 완료 주장".into(),
                        status: ProgressStatus::Reported,
                        evidence: vec!["event:2".into()],
                    }],
                    ..crate::model::Summary::default()
                },
                ..CurrentState::default()
            },
            captured_at: 0,
            display_times: crate::local_time::LocalTimes::default(),
            live_repo: RepoSnapshot::default(),
        };
        let rendered = render_lines(&snapshot, 48).join("\n");
        assert!(!rendered.contains("예전 선택"));
        assert!(rendered.contains("현재 선택"));
        assert!(rendered.contains(crate::tr!(
            "[보고] 구현 완료 주장",
            "[Reported] 구현 완료 주장"
        )));
    }

    #[test]
    fn newest_items_win_the_display_limit_and_equal_times_keep_their_order() {
        let mut current = CurrentState::default();
        for i in 0..9 {
            let id = format!("D-{i:03}");
            let decision = format!("decision-{i}");
            let progress = format!("progress-{i}");
            current.summary.decisions.push(Decision {
                id: id.clone(),
                text: decision,
                status: DecisionStatus::Active,
                supersedes: None,
                evidence: vec![],
            });
            current.summary.progress.push(ProgressItem {
                text: progress.clone(),
                status: ProgressStatus::Reported,
                evidence: vec![],
            });
            if i > 0 {
                let time = if i == 6 || i == 7 { 700 } else { i * 100 };
                current.timing.decisions.insert(id, time);
                current.timing.progress.insert(progress, time);
            }
        }
        let snapshot = PanelSnapshot {
            branch: None,
            live: None,
            catch_up: None,
            connection_notice: None,
            label: "test".into(),
            agent_status: "idle".into(),
            agent_title: None,
            current_request: None,
            pending: Pending::default(),
            worker: WorkerStatus::default(),
            current,
            captured_at: 0,
            display_times: crate::local_time::LocalTimes::default(),
            live_repo: RepoSnapshot::default(),
        };
        let mut lines = Vec::new();
        render_decisions(&snapshot, 100, &mut lines);
        render_progress(&snapshot, 100, &mut lines);
        let rendered = lines.join("\n");
        for prefix in ["decision", "progress"] {
            let positions: Vec<_> = [8, 6, 7, 5, 4, 3]
                .iter()
                .map(|i| rendered.find(&format!("{prefix}-{i}")).unwrap())
                .collect();
            assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
            for omitted in [0, 1, 2] {
                assert!(!rendered.contains(&format!("{prefix}-{omitted}")));
            }
        }
        // Sorting the view must not rewrite saved history.
        assert_eq!(snapshot.current.summary.decisions[0].text, "decision-0");
    }
    fn dialogue_snapshot() -> PanelSnapshot {
        let task: crate::task_links::TaskLinks = serde_json::from_value(serde_json::json!({
            "thread_id":"primary", "generation":0, "latest_source_seq":2,
            "current_request":"그래서 적용된 거야? 모델은 Sol max",
            "current":[{"id":"example/project#285","repository":"example/project","number":285,"kind":"pr","raw":"example/project#285","url":"https://github.com/example/project/pull/285","link":{"kind":"pr","repository":"example/project","number":285,"url":"https://github.com/example/project/pull/285","title":"근거 있는 작업 제목"},"verification":"verified","provenance":"user_prompt","source":{}}]
        })).unwrap();
        let live:crate::live_turn::Snapshot=serde_json::from_value(serde_json::json!({
            "generation":0,"thread_id":"primary","turn_id":"turn","context_key":"main","revision":2,"started_at":1,"updated_at":2,"ended":false,"status":"inProgress","request":"그래서 적용된 거야?","source_prompt_seq":2,
            "commentary":[{"id":"r1","text":"현재 턴 공개 진행 원문","at_unix":2}],"plan":[],"activities":[],"truncated":false
        })).unwrap();
        PanelSnapshot {
            branch: Some(crate::branch_panel::Snapshot {
                today: None,
                daily: None,
                scope: crate::branch_context::Context {
                    tab_id: Some("tab".into()),
                    branch: Some("main".into()),
                    ..Default::default()
                },
                journal: crate::journal::Journal::default(),
                links: crate::work_links::Links::default(),
                task: Some(task),
                legacy: vec![],
                unlinked_records: vec![],
                unlinked_states: vec![],
                times: crate::local_time::LocalTimes::default(),
                pending: 0,
                other_pending: 0,
                gap: false,
                history_limited: false,
                error: None,
            }),
            connection_notice: None,
            label: "project".into(),
            agent_status: "idle".into(),
            agent_title: None,
            current_request: None,
            live: Some(live),
            catch_up: None,
            pending: Pending::default(),
            worker: WorkerStatus::default(),
            current: CurrentState::default(),
            live_repo: RepoSnapshot::default(),
            captured_at: 2,
            display_times: crate::local_time::LocalTimes::default(),
        }
    }
    #[test]
    fn current_hero_uses_real_reference_or_same_primary_turn_explanation_not_latest_request_or_queue()
     {
        let mut snapshot = dialogue_snapshot();
        let text = render_main(&snapshot, 80, 40).join("\n");
        assert!(text.contains("근거 있는 작업 제목") && text.contains("현재 턴 공개 진행 원문"));
        assert!(
            !text.contains(crate::tr!("최근 요청", "Latest request"))
                && !text.contains("모델은 Sol max")
        );
        assert!(
            text.find(crate::tr!("진행 설명", "Progress commentary"))
                .unwrap()
                < text
                    .find(crate::tr!("이 탭의 저장 기록", "This tab’s saved records"))
                    .unwrap()
        );
        assert!(text.contains(crate::tr!(
            "이 탭의 저장 기록 · main",
            "This tab’s saved records · main"
        )));
        assert!(
            !text.contains(crate::tr!("새 대화 대기", "Waiting for new conversation"))
                && !text.contains(crate::tr!("요약 큐", "Summary queue"))
        );
        snapshot.live.as_mut().unwrap().thread_id = "other-primary".into();
        let text = render_main(&snapshot, 80, 40).join("\n");
        assert!(
            !text.contains("현재 턴 공개 진행 원문")
                && text.contains(crate::tr!("진행 정보 없음", "No progress information"))
        );
        snapshot.live.as_mut().unwrap().thread_id = "primary".into();
        snapshot
            .branch
            .as_mut()
            .unwrap()
            .task
            .as_mut()
            .unwrap()
            .latest_source_seq = 3;
        assert!(current_live(&snapshot).is_none());
    }
    #[test]
    fn record_queue_zero_is_quiet_but_failures_and_selected_dates_keep_feedback() {
        let mut snapshot = dialogue_snapshot();
        snapshot.branch.as_mut().unwrap().daily = Some(crate::daily::Queue {
            today: "2026-10-12".into(),
            today_turns: 0,
            manual: std::collections::BTreeMap::default(),
            open_events: 0,
            requested: None,
        });
        assert!(record_queue_preview(&snapshot, 80).is_empty());
        assert_eq!(
            observed_location(&snapshot),
            crate::tr!(
                "project · main · 관찰 위치",
                "project · main · observed location"
            )
        );
        let branch = snapshot.branch.as_mut().unwrap();
        branch.daily.as_mut().unwrap().requested = Some("2026-10-11".into());
        snapshot.worker = WorkerStatus {
            state: "error".into(),
            message: "실패 원문 · 승인 전 적용하지 못했습니다".into(),
            updated_at_unix: Some(2),
        };
        let lines = record_queue_preview(&snapshot, 80);
        assert!(lines.iter().any(|line| {
            line.role == Role::Error
                && line
                    .text
                    .contains(crate::tr!("요약 처리 실패", "Summary processing failed"))
        }));
        assert!(lines.join("\n").contains(crate::tr!(
            "선택한 날짜 · 2026-10-11",
            "Selected date · 2026-10-11"
        )));
        let main = render_main(&snapshot, 80, 40).join("\n");
        assert!(!main.contains("실패 원문"));
        assert!(
            record_worker_section(&snapshot)
                .rows
                .iter()
                .any(|line| line.text.contains("승인 전 적용하지 못했습니다"))
        );
        snapshot.worker.state = "retry_wait".into();
        assert!(
            record_queue_preview(&snapshot, 80)
                .iter()
                .any(|line| line.role == Role::Warning)
        );
        snapshot.worker.state = "summarizing".into();
        snapshot.pending.worker_alive = true;
        assert!(
            record_queue_preview(&snapshot, 80)
                .join("\n")
                .contains(crate::tr!("기록 요약 중", "Summarizing records"))
        );
    }
    #[test]
    fn main_without_a_public_report_keeps_the_whole_request_and_scroll_footer_is_discoverable() {
        let mut snapshot = dialogue_snapshot();
        let request = format!(
            "{} 요청 꼬리 · 승인 전에는 적용하지 마세요",
            "긴 현재 요청의 조건을 모두 읽습니다. ".repeat(24)
        );
        snapshot
            .branch
            .as_mut()
            .unwrap()
            .task
            .as_mut()
            .unwrap()
            .current_request = request.clone();
        snapshot.live.as_mut().unwrap().commentary.clear();
        let lines = render_main(&snapshot, 30, 28);
        let text = lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<String>();
        assert!(
            text.contains(crate::tr!("최근 요청", "Latest request"))
                && text.contains("요청 꼬리 · 승인 전에는 적용하지 마세요")
        );
        assert!(!text.contains('…'));
        let footer = main_footer(lines.len(), 0, 30, 28);
        assert!(
            footer.starts_with('↓')
                && footer.contains("1-27/")
                && footer.contains(crate::tr!("r 반영", "r reflect"))
                && footer.contains('h')
                && footer.contains('m')
        );
        assert!(cell_width(&footer) <= 30);
        let end = main_footer(lines.len(), lines.len().saturating_sub(27), 80, 28);
        assert!(
            end.contains(crate::tr!("끝", "end"))
                && end.contains(crate::tr!("r 반영", "r reflect"))
                && end.contains(crate::tr!("m 메뉴", "m menu"))
        );
        assert!(cell_width(&end) <= 80);
    }
    #[test]
    fn rejected_summary_has_one_actionable_block_and_exact_diagnostic_in_records_only() {
        let mut snapshot = dialogue_snapshot();
        let recorded_at = 1_792_000_099;
        let diagnostic = "Repeated validation error · decision_stage_required · raw exact source";
        snapshot.worker = WorkerStatus {
            state: "error".into(),
            message: diagnostic.into(),
            updated_at_unix: Some(recorded_at),
        };
        snapshot.display_times = crate::local_time::LocalTimes::new([recorded_at, 1, 2]);
        snapshot.branch.as_mut().unwrap().daily = Some(crate::daily::Queue {
            today: "2026-10-13".into(),
            today_turns: 1,
            manual: [("2026-10-12".into(), 1)].into(),
            open_events: 1,
            requested: None,
        });
        let normalize = |value: &str| value.split_whitespace().collect::<String>();
        for width in [28, 48, 78] {
            let lines = record_queue_preview(&snapshot, width);
            let rendered = normalize(&lines.join("\n"));
            assert_eq!(
                rendered
                    .matches(&normalize(crate::tr!(
                        "요약 처리 실패",
                        "Summary processing failed"
                    )))
                    .count(),
                1
            );
            assert!(rendered.contains(&normalize(crate::tr!("r 오늘 재시도", "r retry today"))));
            let main = normalize(&render_main(&snapshot, width + 2, 40).join("\n"));
            assert!(main.contains(&normalize(crate::tr!("오늘 1턴", "Today: 1 turn"))));
            assert!(main.contains(&normalize(crate::tr!("b 날짜 선택", "b select date"))));
            assert!(!rendered.contains("decision_stage_required"));
            assert!(!rendered.contains(&normalize(
                &snapshot.display_times.display(Some(recorded_at))
            )));
            assert!(
                lines
                    .iter()
                    .all(|line| unicode_width::UnicodeWidthStr::width(line.text.as_str()) <= width)
            );
            assert!(
                lines
                    .iter()
                    .filter(|line| line.role == Role::Error)
                    .all(|line| !line.text.contains("raw exact source"))
            );
        }
        let details = record_worker_section(&snapshot);
        assert_eq!(
            details
                .rows
                .iter()
                .filter(|line| line.text == diagnostic)
                .count(),
            1
        );
        assert!(details.rows.iter().any(|line| {
            line.text
                .contains(crate::tr!("상태 기록 시각", "Status recorded at"))
                && line
                    .text
                    .contains(&snapshot.display_times.display(Some(recorded_at)))
        }));
        snapshot.worker.state = "idle".into();
        let idle = record_queue_preview(&snapshot, 80).join("\n");
        assert!(!idle.contains(crate::tr!("요약 처리 실패", "Summary processing failed")));
        assert!(!idle.contains("raw exact source"));
        assert!(
            record_worker_section(&snapshot)
                .rows
                .iter()
                .any(|line| line.text == diagnostic)
        );
    }
    #[test]
    fn selected_date_failure_routes_recovery_to_date_selection() {
        let mut snapshot = dialogue_snapshot();
        snapshot.worker = WorkerStatus {
            state: "error".into(),
            message: "decision needs confirmed user evidence".into(),
            updated_at_unix: Some(2),
        };
        snapshot.branch.as_mut().unwrap().daily = Some(crate::daily::Queue {
            today: "2026-10-13".into(),
            today_turns: 1,
            manual: [("2026-10-12".into(), 1)].into(),
            open_events: 0,
            requested: Some("2026-10-12".into()),
        });
        let rendered = record_queue_preview(&snapshot, 80).join("\n");
        assert!(rendered.contains(crate::tr!("b 날짜 재선택", "b reselect date")));
        assert!(!rendered.contains(crate::tr!("r 오늘 재시도", "r retry today")));
        assert!(rendered.contains("2026-10-12"));
    }
}
