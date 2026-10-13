//! Explicit conversation-reference selection; rendering and navigation do no network work.
use crate::AppResult;
use crate::branch_context::Context;
use crate::panel_style::{self, Line, Role};
use crate::storage::SpaceStore;
use crate::task_links::{self, Choice, ChoiceRole, TaskLinks};
use crate::work_links::{self, Kind, Link, Links};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEventKind};
use std::process::{Command, Stdio};
use std::thread::JoinHandle;
use std::time::Duration;

#[derive(Default)]
pub struct Outcome {
    pub close: bool,
    pub refresh: bool,
    pub changed: bool,
}

pub struct Picker {
    pub scope: Context,
    pub task: TaskLinks,
    repository: Option<String>,
    repository_from_request: bool,
    links: Links,
    selected: usize,
    input: Option<String>,
    cursor: usize,
    notice: Option<(String, Role)>,
    opening: Option<JoinHandle<AppResult<()>>>,
    invalidated: bool,
}

#[derive(Clone)]
enum Row {
    Task(Box<Choice>),
    Branch(Box<Link>),
    None,
}

impl Row {
    fn key(&self) -> String {
        match self {
            Self::Task(choice) => format!("task:{}", choice.reference.id),
            Self::Branch(link) => format!("branch:{}#{}", link.repository, link.number),
            Self::None => "none".into(),
        }
    }

    fn is_entity(&self, repository: &str, number: u64) -> bool {
        match self {
            Self::Task(choice) => {
                choice.reference.number == number
                    && choice
                        .reference
                        .repository
                        .as_deref()
                        .is_some_and(|repo| repo.eq_ignore_ascii_case(repository))
            }
            Self::Branch(link) => {
                link.number == number && link.repository.eq_ignore_ascii_case(repository)
            }
            Self::None => false,
        }
    }

    fn link(&self) -> AppResult<Link> {
        match self {
            Self::Branch(link) => Ok((**link).clone()),
            Self::Task(choice) => {
                let reference = &choice.reference;
                let link = if let Some(link) = &reference.link {
                    link.clone()
                } else {
                    let kind = reference.kind.ok_or(crate::tr!(
                        "참조 종류 확인 중 · v로 다시 확인하세요",
                        "Checking reference type · v to recheck"
                    ))?;
                    let url = reference.url.as_deref().ok_or(crate::tr!(
                        "열 링크 미확인 · v로 다시 확인하세요",
                        "Link to open unknown · v to recheck"
                    ))?;
                    work_links::parse(kind, url)?
                };
                if reference.number != link.number
                    || reference
                        .repository
                        .as_deref()
                        .is_none_or(|repo| !repo.eq_ignore_ascii_case(&link.repository))
                    || reference.kind.is_some_and(|kind| kind != link.kind)
                {
                    return Err(crate::tr!(
                        "참조의 저장소·번호가 일치하지 않습니다",
                        "Reference repository/number mismatch"
                    )
                    .into());
                }
                Ok(link)
            }
            Self::None => Err(crate::tr!(
                "연결 안 함에는 열 링크가 없습니다",
                "No link selected to open"
            )
            .into()),
        }
    }
}

impl Picker {
    /// Clear translated transient feedback without changing choices or user input.
    pub fn language_changed(&mut self) {
        self.notice = None;
    }

    pub fn open(scope: Context, links: Links, task: TaskLinks) -> Self {
        let repository_from_request = task.captured_repository.is_some();
        let repository = task
            .captured_repository
            .clone()
            .or_else(|| work_links::github_repository(&scope));
        let mut picker = Self {
            scope,
            task,
            repository,
            repository_from_request,
            links,
            selected: 0,
            input: None,
            cursor: 0,
            notice: None,
            opening: None,
            invalidated: false,
        };
        picker.select_current();
        picker
    }

    fn rows(&self) -> Vec<Row> {
        let mut rows = self
            .task
            .choices()
            .into_iter()
            .map(|choice| Row::Task(Box::new(choice)))
            .collect::<Vec<_>>();
        for kind in [Kind::Issue, Kind::Pr] {
            for link in self
                .links
                .primary(kind)
                .into_iter()
                .chain(self.links.candidates(kind))
            {
                if !rows
                    .iter()
                    .any(|row| row.is_entity(&link.repository, link.number))
                {
                    rows.push(Row::Branch(Box::new(link.clone())));
                }
            }
        }
        if self.task.thread_id.is_some() {
            rows.push(Row::None);
        }
        rows
    }

    fn select_current(&mut self) {
        let rows = self.rows();
        self.selected = if self.task.explicit_none {
            rows.len().saturating_sub(1)
        } else {
            self.current_rows()
                .first()
                .and_then(|current| rows.iter().position(|row| row.key() == current.key()))
                .unwrap_or(0)
        };
    }

    fn current_rows(&self) -> Vec<Row> {
        let mut rows = self
            .task
            .choices()
            .into_iter()
            .filter(|choice| matches!(choice.role, ChoiceRole::Current | ChoiceRole::Related))
            .map(|choice| Row::Task(Box::new(choice)))
            .collect::<Vec<_>>();
        if rows.is_empty() && !self.task.explicit_none {
            rows.extend(
                [Kind::Issue, Kind::Pr]
                    .iter()
                    .filter_map(|kind| self.links.primary(*kind))
                    .cloned()
                    .map(|link| Row::Branch(Box::new(link))),
            );
        }
        rows
    }

    pub fn update(&mut self, scope: &Context, links: &Links, task: Option<&TaskLinks>) {
        let Some(task) = task else {
            self.invalidated = true;
            self.notice = Some((
                crate::tr!(
                    "대화 연결을 읽지 못했습니다 · Esc로 닫고 다시 여세요",
                    "Conversation links unavailable · Esc to close and reopen"
                )
                .into(),
                Role::Warning,
            ));
            return;
        };
        if !same_task(&self.task, task) {
            self.invalidated = true;
            self.notice = Some((
                crate::tr!(
                    "현재 대화의 연결이 바뀌었습니다 · Esc로 닫고 다시 선택하세요",
                    "Conversation links changed · Esc to close and select again"
                )
                .into(),
                Role::Warning,
            ));
            return;
        }
        let selected = self.rows().get(self.selected).map(Row::key);
        self.scope = scope.clone();
        self.links = links.clone();
        self.task = task.clone();
        if let Some(index) = selected
            .as_ref()
            .and_then(|key| self.rows().iter().position(|row| row.key() == *key))
        {
            self.selected = index;
        } else if selected.is_none() || self.input.is_some() {
            // Direct input is bound to the dialogue and displayed repository,
            // not the branch candidate that was selected before editing.
            self.select_current();
        } else {
            self.invalidated = true;
            self.notice = Some((
                crate::tr!(
                    "선택한 후보가 바뀌었습니다 · Esc로 닫고 다시 선택하세요",
                    "Selected candidate changed · Esc to close and select again"
                )
                .into(),
                Role::Warning,
            ));
        }
    }

    fn check_task(&self, store: &SpaceStore) -> AppResult<()> {
        if self.invalidated || !same_task(&self.task, &task_links::load(store)?) {
            return Err(crate::tr!(
                "현재 대화의 연결이 바뀌었습니다 · Esc로 닫고 다시 선택하세요",
                "Conversation links changed · Esc to close and select again"
            )
            .into());
        }
        Ok(())
    }

    pub fn poll_open(&mut self) {
        if !self.opening.as_ref().is_some_and(JoinHandle::is_finished) {
            return;
        }
        let result = self
            .opening
            .take()
            .expect("completed opener")
            .join()
            .unwrap_or_else(|_| {
                Err(crate::tr!(
                    "링크 열기 작업이 중단되었습니다",
                    "Link opening interrupted"
                )
                .into())
            });
        self.notice = Some(match result {
            Ok(()) => (
                crate::tr!("브라우저 열기를 요청했습니다", "Browser opening requested").into(),
                Role::Muted,
            ),
            Err(error) => (
                crate::tr!("링크 열기 실패 · {error}", "Link opening failed · {error}";),
                Role::Warning,
            ),
        });
    }

    pub fn refresh_finished(
        &mut self,
        key: &crate::link_refresh::TaskKey,
        result: &AppResult<work_links::RefreshOutcome>,
    ) {
        if self.invalidated {
            return;
        }
        if !key.matches(&self.task) {
            if self.notice.as_ref().is_some_and(|(notice, _)| {
                notice == crate::tr!("작업 연결 확인 중…", "Checking work links…")
            }) {
                self.notice = None;
            }
            return;
        }
        self.notice = Some(match result {
            Ok(work_links::RefreshOutcome::Refreshed) => (
                crate::tr!("연결 정보를 다시 확인했습니다", "Links rechecked").into(),
                Role::Muted,
            ),
            Ok(work_links::RefreshOutcome::Busy) => (
                crate::tr!("다른 패널에서 확인 중…", "Another panel is checking…").into(),
                Role::Muted,
            ),
            Ok(work_links::RefreshOutcome::Skipped) => (
                crate::tr!(
                    "최근 연결 정보를 사용합니다",
                    "Using recent link information"
                )
                .into(),
                Role::Muted,
            ),
            Err(error) => (
                crate::tr!("연결 확인 실패 · {error}", "Link check failed · {error}";),
                Role::Warning,
            ),
        });
    }

    /// The base-panel shortcut opens only a single current target. Otherwise it
    /// leaves the chooser open so the user can deliberately select the target.
    pub fn open_current(&mut self, store: &SpaceStore) {
        let current = self.current_rows();
        let count = current.len();
        if count != 1 {
            self.notice = Some((
                if count > 1 {
                    crate::tr!(
                        "연결이 여러 개입니다 · 대상을 선택하고 o로 여세요",
                        "Multiple links · Select a target and press o to open"
                    )
                } else {
                    crate::tr!(
                        "현재 연결이 없습니다 · 후보를 선택하고 o로 여세요",
                        "No current link · Select a candidate and press o to open"
                    )
                }
                .into(),
                Role::Muted,
            ));
            return;
        }
        if let Some(index) = self
            .rows()
            .iter()
            .position(|row| row.key() == current[0].key())
        {
            self.selected = index;
        }
        if let Err(error) = self.open_selected(store) {
            self.notice = Some((error.to_string(), Role::Warning));
        }
    }

    fn open_selected(&mut self, store: &SpaceStore) -> AppResult<()> {
        self.check_task(store)?;
        if self.opening.is_some() {
            self.notice = Some((
                crate::tr!("브라우저 열기 요청 중…", "Requesting browser opening…").into(),
                Role::Muted,
            ));
            return Ok(());
        }
        let row = self
            .rows()
            .get(self.selected)
            .cloned()
            .ok_or(crate::tr!("열 참조가 없습니다", "No reference to open"))?;
        self.opening = Some(open_link(&row.link()?)?);
        self.notice = Some((
            crate::tr!("브라우저 열기 요청 중…", "Requesting browser opening…").into(),
            Role::Muted,
        ));
        Ok(())
    }

    fn apply(&mut self, store: &SpaceStore) -> AppResult<Outcome> {
        self.check_task(store)?;
        let row = self.rows().get(self.selected).cloned().ok_or(crate::tr!(
            "선택할 후보가 없습니다 · a로 직접 지정하세요",
            "No candidates · a to enter a reference"
        ))?;
        let refresh = match row {
            Row::Task(choice) => {
                task_links::select(store, self.task.revision, &choice.reference.id)?;
                true
            }
            Row::Branch(link) => {
                task_links::set_manual(
                    store,
                    self.task.revision,
                    &format!("{}#{}", link.repository, link.number),
                )?;
                true
            }
            Row::None => {
                task_links::clear(store, self.task.revision)?;
                false
            }
        };
        self.task = task_links::load(store)?;
        Ok(Outcome {
            close: true,
            changed: true,
            refresh,
        })
    }

    pub fn handle(&mut self, store: &SpaceStore, event: &Event) -> Outcome {
        let result = match event {
            Event::Key(key) if key.kind == KeyEventKind::Press => self.key(store, *key),
            Event::Paste(text) if self.input.is_some() => {
                self.insert(text);
                Ok(Outcome::default())
            }
            Event::Mouse(mouse) => {
                match mouse.kind {
                    MouseEventKind::ScrollDown => {
                        self.selected =
                            (self.selected + 1).min(self.rows().len().saturating_sub(1));
                    }
                    MouseEventKind::ScrollUp => self.selected = self.selected.saturating_sub(1),
                    _ => {}
                }
                Ok(Outcome::default())
            }
            _ => Ok(Outcome::default()),
        };
        result.unwrap_or_else(|error| {
            self.notice = Some((error.to_string(), Role::Warning));
            Outcome::default()
        })
    }

    fn key(&mut self, store: &SpaceStore, key: KeyEvent) -> AppResult<Outcome> {
        if self.invalidated && key.code == KeyCode::Esc {
            return Ok(Outcome {
                close: true,
                ..Outcome::default()
            });
        }
        if self.input.is_some() {
            return self.input_key(store, key);
        }
        if self.task.thread_id.is_none()
            && matches!(key.code, KeyCode::Enter | KeyCode::Char('a' | 'n' | 'u'))
        {
            self.notice = Some((
                crate::tr!(
                    "대화 연결 필요 · 지금은 o로 참조 보기만 가능합니다",
                    "Conversation connection needed · o can open references"
                )
                .into(),
                Role::Muted,
            ));
            return Ok(Outcome::default());
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('q' | 'l') => {
                return Ok(Outcome {
                    close: true,
                    ..Outcome::default()
                });
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.selected = (self.selected + 1).min(self.rows().len().saturating_sub(1));
            }
            KeyCode::Up | KeyCode::Char('k') => self.selected = self.selected.saturating_sub(1),
            KeyCode::Char('n') => self.selected = self.rows().len().saturating_sub(1),
            KeyCode::Char('a') => {
                self.input = Some(String::new());
                self.cursor = 0;
                self.notice = None;
            }
            KeyCode::Enter => return self.apply(store),
            KeyCode::Char('u') => {
                self.check_task(store)?;
                task_links::undo(store, self.task.revision)?;
                self.task = task_links::load(store)?;
                return Ok(Outcome {
                    close: true,
                    changed: true,
                    refresh: !self.task.explicit_none,
                });
            }
            KeyCode::Char('v') if key.modifiers.is_empty() => {
                self.check_task(store)?;
                self.notice = Some((
                    crate::tr!("작업 연결 확인 중…", "Checking work links…").into(),
                    Role::Muted,
                ));
                return Ok(Outcome {
                    refresh: true,
                    ..Outcome::default()
                });
            }
            KeyCode::Char('o') => self.open_selected(store)?,
            _ => {}
        }
        Ok(Outcome::default())
    }

    fn input_key(&mut self, store: &SpaceStore, key: KeyEvent) -> AppResult<Outcome> {
        match key.code {
            KeyCode::Esc => {
                self.input = None;
                self.notice = None;
            }
            KeyCode::Enter => {
                self.check_task(store)?;
                let input = self.input.as_deref().unwrap_or_default().trim();
                // Make the displayed repository binding explicit before passing
                // a bare number; a later checkout cannot silently retarget it.
                let scoped = if input
                    .strip_prefix('#')
                    .unwrap_or(input)
                    .chars()
                    .all(|c| c.is_ascii_digit())
                    && !input.strip_prefix('#').unwrap_or(input).is_empty()
                {
                    let repository = self.repository.as_deref().ok_or(crate::tr!(
                        "저장소 미확인 · OWNER/REPO#N 또는 전체 GitHub URL을 입력하세요",
                        "Repository unknown · Enter OWNER/REPO#N or a full GitHub URL"
                    ))?;
                    format!("{repository}#{}", input.strip_prefix('#').unwrap_or(input))
                } else {
                    input.to_string()
                };
                task_links::set_manual(store, self.task.revision, &scoped)?;
                self.task = task_links::load(store)?;
                self.input = None;
                return Ok(Outcome {
                    close: true,
                    changed: true,
                    refresh: true,
                });
            }
            KeyCode::Backspace if self.cursor > 0 => {
                let input = self.input.as_mut().expect("input mode");
                input.replace_range(
                    char_offset(input, self.cursor - 1)..char_offset(input, self.cursor),
                    "",
                );
                self.cursor -= 1;
            }
            KeyCode::Delete => {
                let input = self.input.as_mut().expect("input mode");
                input.replace_range(
                    char_offset(input, self.cursor)..char_offset(input, self.cursor + 1),
                    "",
                );
            }
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => {
                self.cursor = (self.cursor + 1)
                    .min(self.input.as_deref().unwrap_or_default().chars().count());
            }
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.input.as_deref().unwrap_or_default().chars().count(),
            KeyCode::Char(character)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.insert(&character.to_string());
            }
            _ => {}
        }
        Ok(Outcome::default())
    }

    fn insert(&mut self, text: &str) {
        let Some(input) = &mut self.input else {
            return;
        };
        let remaining = 1024_usize.saturating_sub(input.chars().count());
        let text = text
            .chars()
            .filter(|c| !c.is_control())
            .take(remaining)
            .collect::<String>();
        input.insert_str(char_offset(input, self.cursor), &text);
        self.cursor += text.chars().count();
        self.notice = None;
    }

    pub fn frame(&self, width: usize, height: usize) -> (Vec<Line>, usize) {
        let width = width.saturating_sub(2).max(2);
        let mut lines = Vec::new();
        panel_style::wrap(
            &mut lines,
            crate::tr!("작업 연결 정정", "Edit work links"),
            width,
            Role::Title,
        );
        if self.invalidated {
            if let Some((notice, role)) = &self.notice {
                panel_style::wrap(&mut lines, notice, width, *role);
            }
            panel_style::wrap(
                &mut lines,
                crate::tr!(
                    "선택은 적용되지 않았습니다 · Esc로 닫기",
                    "Selection not applied · Esc to close"
                ),
                width,
                Role::Muted,
            );
            return (lines, 0);
        }
        panel_style::wrap(
            &mut lines,
            &if self.task.thread_id.is_none() {
                crate::tr!(
                    "대화 연결 필요 · 지금은 참조 보기만 가능합니다",
                    "Conversation connection needed · References can be viewed"
                )
                .into()
            } else {
                crate::tr!("현재 연결과 이 대화의 후보 · 관찰 {}", "Current links and conversation candidates · Observed {}";
                    self.scope.branch.as_deref().unwrap_or(crate::tr!("브랜치 미확인", "Branch unknown"))
                )
            },
            width,
            Role::Muted,
        );
        if let Some((notice, role)) = &self.notice
            && self.input.is_none()
        {
            panel_style::wrap(&mut lines, notice, width, *role);
        }
        if let Some(input) = &self.input {
            self.input_frame(&mut lines, input, width);
        } else {
            self.choices_frame(&mut lines, width);
        }
        let focus = if self.input.is_some() {
            lines.iter().position(|line| line.contains('│'))
        } else {
            lines.iter().position(|line| line.role == Role::Selected)
        };
        let scroll = focus
            .unwrap_or(0)
            .saturating_sub(height.saturating_sub(4))
            .min(lines.len().saturating_sub(height.saturating_sub(1)));
        (lines, scroll)
    }

    fn input_frame(&self, lines: &mut Vec<Line>, input: &str, width: usize) {
        panel_style::wrap(
            lines,
            crate::tr!(
                "이슈·PR 번호 또는 GitHub URL",
                "Issue/PR number or GitHub URL"
            ),
            width,
            Role::Heading,
        );
        panel_style::wrap(
            lines,
            &self.repository.as_ref().map_or_else(
                || crate::tr!("저장소 미확인 · OWNER/REPO#N 또는 전체 URL 입력", "Repository unknown · Enter OWNER/REPO#N or a full URL").into(),
                |repo| {
                    crate::tr!("번호는 {repo} 기준 · {}", "Numbers refer to {repo} · {}";
                        if self.repository_from_request {
                            crate::tr!("요청 수집 당시 확인", "Checked when the request was collected")
                        } else {
                            crate::tr!("화면을 열 때 Git 확인", "Git checked when this view opened")
                        }
                    )
                },
            ),
            width,
            Role::Muted,
        );
        panel_style::wrap(
            lines,
            &crate::tr!("입력 > {}", "Input > {}"; input_window(input, self.cursor, width)),
            width,
            Role::Selected,
        );
        if let Some((notice, role)) = &self.notice {
            panel_style::wrap(lines, notice, width, *role);
        }
        panel_style::wrap(
            lines,
            crate::tr!(
                "번호만 입력해도 이슈·PR 종류를 확인합니다",
                "A number alone resolves the issue/PR type"
            ),
            width,
            Role::Muted,
        );
    }

    fn choices_frame(&self, lines: &mut Vec<Line>, width: usize) {
        let rows = self.rows();
        if !rows.iter().any(|row| !matches!(row, Row::None)) {
            panel_style::wrap(
                lines,
                crate::tr!(
                    "발견한 이슈·PR 후보가 없습니다",
                    "No issue/PR candidates found"
                ),
                width,
                Role::Muted,
            );
        }
        for (index, row) in rows
            .iter()
            .enumerate()
            .skip(self.selected.saturating_sub(1))
            .take(4)
        {
            self.row_frame(lines, row, index == self.selected, width);
        }
        if self.task.thread_id.is_none() {
            panel_style::wrap(
                lines,
                crate::tr!(
                    "o 참조 열기 · v 브랜치 다시 확인",
                    "o open reference · v recheck branch"
                ),
                width,
                Role::Muted,
            );
            return;
        }
        let selected = rows.get(self.selected);
        let note = match selected {
            Some(Row::None) => crate::tr!(
                "이 작업의 연결을 해제합니다 · 저장된 기록 유지",
                "Unlink this work · Saved records retained"
            )
            .to_string(),
            Some(Row::Task(choice))
                if choice.role == ChoiceRole::Current && self.task.current.len() == 1 =>
            {
                crate::tr!("현재 연결을 유지합니다", "Keep the current link").into()
            }
            Some(Row::Task(choice)) => {
                crate::tr!("이 참조 1개로 현재 작업 연결 · {}", "Link current work to this reference · {}";
                    crate::branch_panel::task_reference_label(&choice.reference)
                )
            }
            Some(Row::Branch(link)) => {
                crate::tr!("현재 작업을 {}#{}에 직접 연결", "Link current work directly to {}#{}";
                    link.repository, link.number
                )
            }
            None => crate::tr!("후보를 선택하세요", "Select a candidate").into(),
        };
        panel_style::wrap(lines, &note, width, Role::Action);
        panel_style::wrap(
            lines,
            crate::tr!(
                "a 번호·URL 입력 · o 열기 · v 다시 확인",
                "a enter number/URL · o open · v recheck"
            ),
            width,
            Role::Muted,
        );
        if self.task.can_undo() {
            panel_style::wrap(
                lines,
                crate::tr!("u 이전 연결로 되돌리기", "u restore previous link"),
                width,
                Role::Muted,
            );
        }
    }

    fn row_frame(&self, lines: &mut Vec<Line>, row: &Row, selected: bool, width: usize) {
        let pointer = if selected { ">" } else { " " };
        let role = if selected { Role::Selected } else { Role::Body };
        match row {
            Row::Task(choice) => Self::task_row_frame(lines, choice, selected, width),
            Row::Branch(link) => {
                panel_style::wrap(
                    lines,
                    &crate::tr!("{pointer} {} {}#{} · 브랜치 후보", "{pointer} {} {}#{} · Branch candidate";
                        if link.kind == Kind::Issue {
                            crate::tr!("이슈", "Issue")
                        } else {
                            "PR"
                        },
                        link.repository,
                        link.number
                    ),
                    width,
                    role,
                );
                panel_style::wrap(
                    lines,
                    &crate::branch_panel::short_text(
                        link.title
                            .as_deref()
                            .unwrap_or(crate::tr!("제목 조회 전", "Title not checked")),
                        width * 2,
                    ),
                    width,
                    Role::Body,
                );
                panel_style::wrap(
                    lines,
                    &crate::tr!("관찰 {}에서 확인 · 대화 작업으로 미지정", "Found on observed {} · Not selected for conversation work";
                        self.scope.branch.as_deref().unwrap_or(crate::tr!("브랜치 미확인", "Branch unknown"))
                    ),
                    width,
                    Role::Muted,
                );
                if selected {
                    panel_style::wrap(lines, &link.url, width, Role::Muted);
                }
            }
            Row::None => {
                panel_style::wrap(
                    lines,
                    &crate::tr!("{pointer} 연결 안 함{}", "{pointer} No link{}";
                        if self.task.explicit_none {
                            crate::tr!(" · 현재 선택", " · Current selection")
                        } else {
                            ""
                        }
                    ),
                    width,
                    role,
                );
                panel_style::wrap(
                    lines,
                    crate::tr!(
                        "이 작업의 이슈·PR 연결 해제",
                        "Remove this work’s issue/PR link"
                    ),
                    width,
                    Role::Muted,
                );
            }
        }
    }

    fn task_row_frame(lines: &mut Vec<Line>, choice: &Choice, selected: bool, width: usize) {
        let pointer = if selected { ">" } else { " " };
        let role = if selected { Role::Selected } else { Role::Body };
        let reference = &choice.reference;
        let badge = match choice.role {
            ChoiceRole::Current => crate::tr!("현재 연결", "Current link"),
            ChoiceRole::Related => crate::tr!("관련 참조", "Related reference"),
            ChoiceRole::Reference => crate::tr!("참고 후보", "Reference candidate"),
            ChoiceRole::Previous => crate::tr!("이전 연결", "Previous link"),
        };
        panel_style::wrap(
            lines,
            &format!(
                "{pointer} {} · {badge}",
                crate::branch_panel::task_reference_label(reference)
            ),
            width,
            role,
        );
        panel_style::wrap(
            lines,
            &crate::branch_panel::short_text(
                reference
                    .link
                    .as_ref()
                    .and_then(|link| link.title.as_deref())
                    .unwrap_or(crate::tr!("제목 확인 전", "Title not checked")),
                width * 2,
            ),
            width,
            Role::Body,
        );
        let source = if choice.role == ChoiceRole::Reference
            && reference.provenance == task_links::Provenance::UserPrompt
        {
            crate::tr!(
                "사용자 발언 · 작업 지정 아님",
                "User mention · Work not selected"
            )
        } else {
            crate::branch_panel::task_reference_source(reference.provenance)
        };
        panel_style::wrap(
            lines,
            &format!(
                "{} · {}",
                source,
                crate::branch_panel::task_reference_status(reference)
            ),
            width,
            Role::Muted,
        );
        if selected {
            if !reference.source.quote.is_empty() {
                panel_style::wrap(
                    lines,
                    &crate::tr!("근거: {}", "Evidence: {}";
                        crate::branch_panel::short_text(&reference.source.quote, width * 2)
                    ),
                    width,
                    Role::Muted,
                );
            }
            if let Some(url) = reference
                .url
                .as_deref()
                .or_else(|| reference.link.as_ref().map(|link| link.url.as_str()))
            {
                panel_style::wrap(lines, url, width, Role::Muted);
            }
            if let Some(error) = &reference.error {
                panel_style::wrap(
                    lines,
                    &crate::branch_panel::short_text(error, width * 2),
                    width,
                    Role::Warning,
                );
            }
        }
    }

    pub fn footer(&self) -> &'static str {
        if self.task.thread_id.is_none() {
            crate::tr!("o 열기 · Esc 닫기", "o open · Esc close")
        } else if self.input.is_some() {
            crate::tr!("Enter 저장 · Esc 취소", "Enter save · Esc cancel")
        } else {
            crate::tr!("Enter 적용 · Esc 취소", "Enter apply · Esc cancel")
        }
    }
}

fn same_task(a: &TaskLinks, b: &TaskLinks) -> bool {
    a.revision == b.revision && a.generation == b.generation && a.thread_id == b.thread_id
}

fn char_offset(text: &str, cursor: usize) -> usize {
    text.char_indices()
        .nth(cursor)
        .map_or(text.len(), |(index, _)| index)
}

fn input_window(input: &str, cursor: usize, width: usize) -> String {
    let chars: Vec<_> = input.chars().collect();
    let context = width.saturating_sub(8).div_ceil(2).max(1);
    let start = cursor.saturating_sub(context);
    let end = (cursor + context).min(chars.len());
    format!(
        "{}{}│{}{}",
        if start > 0 { "…" } else { "" },
        chars[start..cursor].iter().collect::<String>(),
        chars[cursor..end].iter().collect::<String>(),
        if end < chars.len() { "…" } else { "" },
    )
}

fn open_link(link: &Link) -> AppResult<JoinHandle<AppResult<()>>> {
    let canonical = work_links::parse(link.kind, &link.url)?;
    if canonical.repository != link.repository || canonical.number != link.number {
        return Err(crate::tr!(
            "링크의 저장소·번호가 일치하지 않습니다",
            "Link repository/number mismatch"
        )
        .into());
    }
    let executable = std::env::var_os("MAEKLOG_OPEN_BIN").unwrap_or_else(|| {
        if cfg!(target_os = "macos") {
            "open".into()
        } else if cfg!(target_os = "windows") {
            "explorer".into()
        } else {
            "xdg-open".into()
        }
    });
    Ok(std::thread::Builder::new()
        .name("pm-link-open".into())
        .spawn(move || {
            let result = crate::process::run_with_timeout(
                Command::new(executable)
                    .arg(canonical.url)
                    .stdin(Stdio::null()),
                None,
                Duration::from_secs(5),
                8192,
            )?;
            if result.success {
                Ok(())
            } else {
                Err(crate::tr!(
                    "OS의 기본 브라우저 연결을 확인하세요",
                    "Check the system’s default browser association"
                )
                .into())
            }
        })?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use unicode_width::UnicodeWidthChar;

    fn picker() -> Picker {
        let mut task = TaskLinks::default();
        task.captured_repository = Some("example/project".into());
        task.thread_id = Some("thread-a".into());
        Picker::open(
            Context {
                key: "branch-a".into(),
                branch: Some("feature/links".into()),
                ..Context::default()
            },
            Links::default(),
            task,
        )
    }

    fn store() -> SpaceStore {
        SpaceStore {
            state_root: "/unused".into(),
            key: "unused".into(),
        }
    }

    fn reference(number: u64) -> task_links::TaskReference {
        serde_json::from_value(serde_json::json!({
            "id": format!("example/project#{number}"), "repository": "example/project",
            "number": number, "kind": null, "raw": format!("example/project#{number}"),
            "url": null, "link": null, "verification": "checking", "error": null,
            "provenance": "user_prompt", "source": { "quote": "이 참조도 참고해" }
        }))
        .unwrap()
    }

    #[test]
    fn paste_is_bounded_and_unicode_editing_and_esc_keep_parent_panel_open() {
        let mut picker = picker();
        picker.input = Some(String::new());
        picker.insert("한글\n\x1b[31m");
        assert_eq!(picker.input.as_deref(), Some("한글[31m"));
        picker.cursor = 2;
        picker
            .input_key(
                &store(),
                KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
            )
            .unwrap();
        assert_eq!(picker.input.as_deref(), Some("한[31m"));
        picker.insert(&"x".repeat(2000));
        assert_eq!(picker.input.as_ref().unwrap().chars().count(), 1024);
        let outcome = picker
            .key(&store(), KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .unwrap();
        assert!(!outcome.close && picker.input.is_none());
        let outcome = picker
            .key(&store(), KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .unwrap();
        assert!(outcome.close && !outcome.refresh && !outcome.changed);
    }

    #[test]
    fn old_refresh_aliases_are_inert_and_refresh_letters_remain_typeable() {
        let mut picker = picker();
        for ch in ['r', 'R'] {
            let outcome = picker
                .key(
                    &store(),
                    KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
                )
                .unwrap();
            assert!(!outcome.refresh && !outcome.changed && !outcome.close);
        }
        picker.input = Some(String::new());
        for ch in ['v', 'r', 'R'] {
            let outcome = picker
                .key(
                    &store(),
                    KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
                )
                .unwrap();
            assert!(!outcome.refresh && !outcome.changed && !outcome.close);
        }
        assert_eq!(picker.input.as_deref(), Some("vrR"));
    }

    #[test]
    fn invalid_input_remains_editable_with_a_local_error_and_no_fetch() {
        let mut picker = picker();
        picker.input = Some("https://example.com/issues/10".into());
        picker.cursor = picker.input.as_ref().unwrap().chars().count();
        let outcome = picker.handle(
            &store(),
            &Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        );
        assert!(!outcome.refresh && !outcome.changed && !outcome.close);
        assert!(
            picker
                .notice
                .as_ref()
                .is_some_and(|(_, role)| *role == Role::Warning)
        );
        assert_eq!(
            picker.input.as_deref(),
            Some("https://example.com/issues/10")
        );
    }

    #[test]
    fn narrow_input_view_keeps_cursor_visible_and_wraps_by_terminal_cells() {
        let mut picker = picker();
        picker.input = Some("한".repeat(1000));
        for cursor in [0, 500, 1000] {
            picker.cursor = cursor;
            for (width, height) in [(12, 6), (24, 10), (52, 20)] {
                let (lines, scroll) = picker.frame(width, height);
                assert!(
                    lines
                        .iter()
                        .skip(scroll)
                        .take(height - 1)
                        .any(|line| line.contains('│'))
                );
                assert!(lines.iter().all(|line| {
                    line.chars().map(|c| c.width().unwrap_or(0)).sum::<usize>() <= width - 2
                }));
                assert!(!lines.iter().any(|line| line.contains('\x1b')));
            }
        }
        assert!(picker.frame(52, 20).0.join("\n").contains(crate::tr!(
            "번호는 example/project 기준",
            "Numbers refer to example/project"
        )));
    }

    #[test]
    fn none_shortcut_only_selects_and_escape_does_not_mutate_current() {
        let mut picker = picker();
        let outcome = picker
            .key(
                &store(),
                KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE),
            )
            .unwrap();
        assert!(!outcome.changed && !outcome.refresh && !picker.task.explicit_none);
        let outcome = picker
            .key(&store(), KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .unwrap();
        assert!(outcome.close && !outcome.changed);
    }

    #[test]
    fn thread_revision_change_requires_a_new_choice_and_canonical_open_is_checked() {
        let mut picker = picker();
        let mut changed = picker.task.clone();
        changed.thread_id = Some("thread-b".into());
        picker.update(&Context::default(), &Links::default(), Some(&changed));
        assert_eq!(picker.task.thread_id.as_deref(), Some("thread-a"));
        assert!(picker.invalidated);
        assert!(picker.notice.as_ref().unwrap().0.contains(crate::tr!(
            "현재 대화의 연결이 바뀌었습니다",
            "Conversation links changed"
        )));
        let mut link = work_links::parse(Kind::Issue, "example/project#10").unwrap();
        link.number = 11;
        assert!(open_link(&link).is_err());
        link.url = "https://example.com/issues/11".into();
        assert!(open_link(&link).is_err());
    }

    #[test]
    fn direct_input_survives_branch_rows_but_rejects_dialogue_identity_changes() {
        let initial = picker();
        let scope = initial.scope;
        let task = initial.task;
        let links = |number| {
            let link = work_links::parse(Kind::Pr, &format!("example/project#{number}")).unwrap();
            let mut links = Links::default();
            links.primary_pr = Some(link.id());
            links.entries.insert(link.id(), link);
            links
        };
        let changed_scope = Context {
            key: "branch-b".into(),
            branch: Some("feature/other".into()),
            ..scope.clone()
        };
        let changed_links = links(167);
        let mut editing = Picker::open(scope.clone(), links(168), task.clone());
        editing
            .key(
                &store(),
                KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE),
            )
            .unwrap();
        editing.insert("#144");
        editing.update(&changed_scope, &changed_links, Some(&task));
        assert!(!editing.invalidated);
        assert_eq!(editing.input.as_deref(), Some("#144"));
        assert_eq!(editing.repository.as_deref(), Some("example/project"));
        assert_eq!(editing.scope.branch, changed_scope.branch);
        assert!(
            editing
                .frame(80, 42)
                .0
                .join("\n")
                .contains(crate::tr!("번호만 입력해도", "A number alone"))
        );
        let cancel = editing
            .key(&store(), KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .unwrap();
        assert!(!cancel.close && !cancel.changed && editing.input.is_none());
        assert!(editing.rows()[editing.selected].is_entity("example/project", 167));

        for change in 0..3 {
            let mut editing = Picker::open(scope.clone(), links(168), task.clone());
            editing.input = Some("#144".into());
            let mut changed_task = task.clone();
            match change {
                0 => changed_task.revision += 1,
                1 => changed_task.thread_id = Some("thread-b".into()),
                _ => changed_task.generation += 1,
            }
            editing.update(&changed_scope, &changed_links, Some(&changed_task));
            assert!(editing.invalidated);
            assert!(
                editing
                    .key(&store(), KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
                    .is_err()
            );
        }
        let mut choosing = Picker::open(scope, links(168), task.clone());
        choosing.update(&changed_scope, &changed_links, Some(&task));
        assert!(choosing.invalidated);
    }

    #[test]
    fn reference_candidate_is_not_described_as_an_assigned_task() {
        let mut picker = picker();
        picker.task.candidates.push(reference(10));
        let text = picker.frame(80, 42).0.join("\n");
        assert!(text.contains(crate::tr!("참고 후보", "Reference candidate")));
        assert!(text.contains(crate::tr!(
            "사용자 발언 · 작업 지정 아님 · 확인 중",
            "User mention · Work not selected · Checking"
        )));
        assert!(!text.contains(crate::tr!("지정됨", "Selected")));
        assert!(!text.contains(crate::tr!("사용자 지정", "User selected")));
    }

    #[test]
    fn multiple_current_targets_require_a_deliberate_open_selection() {
        let mut picker = picker();
        picker.task.current = vec![reference(10), reference(11)];
        picker.open_current(&store());
        assert!(picker.opening.is_none());
        assert!(picker.notice.as_ref().unwrap().0.contains(crate::tr!(
            "대상을 선택하고 o",
            "Select a target and press o"
        )));
        assert_eq!(picker.task.choices().len(), 2);
    }

    #[test]
    fn selected_reference_rejects_metadata_for_a_different_entity() {
        let mut reference = reference(10);
        reference.kind = Some(Kind::Issue);
        reference.link = Some(work_links::parse(Kind::Issue, "example/project#11").unwrap());
        let row = Row::Task(Box::new(Choice {
            reference,
            role: ChoiceRole::Current,
        }));
        assert!(row.link().is_err());
    }

    #[test]
    fn cached_branch_repository_never_completes_an_unresolved_manual_number() {
        let mut task = TaskLinks::default();
        task.thread_id = Some("thread-a".into());
        let mut links = Links::default();
        links.repository = Some("old/repository".into());
        let picker = Picker::open(Context::default(), links, task);
        assert!(picker.repository.is_none());
    }

    #[test]
    fn unbound_chooser_is_read_only_before_any_store_access() {
        let mut picker = Picker::open(Context::default(), Links::default(), TaskLinks::default());
        for code in [
            KeyCode::Enter,
            KeyCode::Char('a'),
            KeyCode::Char('n'),
            KeyCode::Char('u'),
        ] {
            let outcome = picker
                .key(&store(), KeyEvent::new(code, KeyModifiers::NONE))
                .unwrap();
            assert!(!outcome.changed && !outcome.refresh && !outcome.close);
            assert!(picker.input.is_none());
        }
        assert_eq!(
            picker.footer(),
            crate::tr!("o 열기 · Esc 닫기", "o open · Esc close")
        );
        let text = picker.frame(80, 42).0.join("\n");
        assert!(text.contains(crate::tr!(
            "지금은 참조 보기만 가능합니다",
            "References can be viewed"
        )));
        assert!(!text.contains(crate::tr!("Enter 적용", "Enter apply")));
        assert!(!text.contains(crate::tr!("번호·URL 입력", "enter number/URL")));
        assert!(picker.rows().is_empty());
    }
}
