//! A modal, keyboard-first evidence reader and user correction flow.
//! Viewing and drafting do not call a model or mutate the journal.
use crate::{
    AppResult,
    branch_context::Context,
    corrections::{self, Edit},
    journal::{self, Journal, Kind, Record},
    local_time::LocalTimes,
    panel_style::{self, Line, Role},
    storage::SpaceStore,
};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEventKind};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    List,
    Detail,
    Form,
    Confirm,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Intent {
    Amend,
    Exclude,
    Undo,
}

pub struct Editor {
    scope: Context,
    journal: Journal,
    ids: Vec<String>,
    selected: usize,
    mode: Mode,
    intent: Intent,
    fields: [String; 3],
    field: usize,
    cursor: usize,
    scroll: usize,
    notice: Option<String>,
    times: LocalTimes,
}
impl Editor {
    /// Clear translated transient feedback without changing choices or user input.
    pub fn language_changed(&mut self) {
        self.notice = None;
    }

    pub fn open(scope: Context, journal: Journal) -> Self {
        let mut editor = Self {
            scope,
            journal,
            ids: Vec::new(),
            selected: 0,
            mode: Mode::List,
            intent: Intent::Amend,
            fields: Default::default(),
            field: 0,
            cursor: 0,
            scroll: 0,
            notice: None,
            times: LocalTimes::default(),
        };
        editor.index();
        editor
    }
    fn index(&mut self) {
        let selected = self.ids.get(self.selected).cloned();
        let mut records: Vec<_> = self.journal.records.values().collect();
        records.sort_by_key(|r| (std::cmp::Reverse(r.latest().order), &r.id));
        self.ids = records.iter().map(|r| r.id.clone()).collect();
        self.selected = selected
            .and_then(|id| self.ids.iter().position(|v| *v == id))
            .unwrap_or(0);
        self.times = LocalTimes::new(records.iter().flat_map(|r| {
            r.versions
                .iter()
                .flat_map(|v| v.proof.iter().filter_map(|p| p.source.at_unix_utc))
                .chain(r.versions.iter().filter_map(|v| v.at_unix_utc))
                .chain(r.corrections.iter().map(|c| c.at_unix_utc))
        }));
    }
    fn record(&self) -> Option<&Record> {
        self.ids
            .get(self.selected)
            .and_then(|id| self.journal.records.get(id))
    }
    fn start(&mut self, intent: Intent) {
        let Some(record) = self.record() else {
            return;
        };
        self.fields = [
            record.text().into(),
            record.condition().unwrap_or_default().into(),
            String::new(),
        ];
        self.intent = intent;
        self.field = if intent == Intent::Amend { 0 } else { 2 };
        self.cursor = self.fields[self.field].chars().count();
        self.mode = Mode::Form;
        self.scroll = 0;
        self.notice = None;
    }
    fn edit(&self) -> Edit {
        match self.intent {
            Intent::Amend => Edit::Amend {
                text: self.fields[0].clone(),
                condition: (!self.fields[1].trim().is_empty()).then(|| self.fields[1].clone()),
            },
            Intent::Exclude => Edit::Exclude,
            Intent::Undo => Edit::Undo,
        }
    }
    fn check(&self, store: &SpaceStore) -> AppResult<()> {
        corrections::prepare(
            self.record()
                .ok_or(crate::tr!("선택된 항목이 없습니다.", "No record selected."))?,
            &self.edit(),
            &self.fields[2],
            &store.key,
        )?;
        Ok(())
    }
    fn commit(&mut self, store: &SpaceStore) -> AppResult<()> {
        let _participation = crate::participation::lock(store)?;
        crate::participation::ensure_enabled(store)?;
        let meta = store.meta()?.ok_or(crate::tr!(
            "Space 정보가 없습니다.",
            "Space information is missing."
        ))?;
        if crate::branch_context::detect_for_store(store, &meta.cwd)?.key != self.scope.key {
            return Err(crate::tr!(
                "브랜치가 바뀌었습니다. Esc로 닫고 현재 브랜치에서 다시 여세요.",
                "Branch changed. Press Esc and reopen on the current branch."
            )
            .into());
        }
        let record = self
            .record()
            .ok_or(crate::tr!("선택된 항목이 없습니다.", "No record selected."))?;
        self.journal = corrections::save(
            &store.state_root,
            &self.scope,
            &record.id,
            self.journal.revision,
            &self.edit(),
            &self.fields[2],
            &store.key,
        )?;
        self.index();
        self.mode = Mode::Detail;
        self.scroll = 0;
        self.notice = Some(
            crate::tr!(
                "저장했습니다. 현재 기록 범위에 반영됩니다. 원문은 유지됩니다.",
                "Saved to the current record scope. Original text is retained."
            )
            .into(),
        );
        Ok(())
    }
    /// True closes just this reader, never the parent Maeklog pane.
    pub fn handle(&mut self, store: &SpaceStore, event: &Event) -> bool {
        match event {
            Event::Key(key) if key.kind == KeyEventKind::Press => return self.key(store, *key),
            Event::Paste(text) if self.mode == Mode::Form => self.insert(text),
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::ScrollDown => self.scroll = self.scroll.saturating_add(3),
                MouseEventKind::ScrollUp => self.scroll = self.scroll.saturating_sub(3),
                _ => {}
            },
            _ => {}
        }
        false
    }
    fn key(&mut self, store: &SpaceStore, key: KeyEvent) -> bool {
        if key.code == KeyCode::Esc {
            self.scroll = 0;
            self.notice = None;
            self.mode = match self.mode {
                Mode::List => return true,
                Mode::Detail => Mode::List,
                Mode::Form => Mode::Detail,
                Mode::Confirm => Mode::Form,
            };
            return false;
        }
        if self.mode == Mode::Form {
            self.input(store, key);
            return false;
        }
        match key.code {
            KeyCode::PageDown => self.scroll = self.scroll.saturating_add(10),
            KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(10),
            KeyCode::Down | KeyCode::Char('j') if self.mode == Mode::List => {
                self.selected = (self.selected + 1).min(self.ids.len().saturating_sub(1));
                self.scroll = 0;
            }
            KeyCode::Up | KeyCode::Char('k') if self.mode == Mode::List => {
                self.selected = self.selected.saturating_sub(1);
                self.scroll = 0;
            }
            KeyCode::Down | KeyCode::Char('j') => self.scroll = self.scroll.saturating_add(1),
            KeyCode::Up | KeyCode::Char('k') => self.scroll = self.scroll.saturating_sub(1),
            KeyCode::Enter if self.mode == Mode::List && self.record().is_some() => {
                self.mode = Mode::Detail;
                self.scroll = 0;
            }
            KeyCode::Char('e') if self.mode == Mode::Detail => self.start(Intent::Amend),
            KeyCode::Char('x') if self.mode == Mode::Detail => self.start(Intent::Exclude),
            KeyCode::Char('u') if self.mode == Mode::Detail => self.start(Intent::Undo),
            KeyCode::Char('r') if self.mode == Mode::Detail || self.mode == Mode::List => {
                match journal::load(&store.state_root, &self.scope) {
                    Ok(journal) => {
                        self.journal = journal;
                        self.index();
                        self.notice = Some(
                            crate::tr!("최신 기록을 다시 읽었습니다.", "Latest records reloaded.")
                                .into(),
                        );
                    }
                    Err(error) => self.notice = Some(error.to_string()),
                }
            }
            KeyCode::Enter if self.mode == Mode::Confirm => {
                if let Err(error) = self.commit(store) {
                    self.notice = Some(error.to_string());
                    self.mode = Mode::Detail;
                    self.scroll = 0;
                }
            }
            _ => {}
        }
        false
    }
    fn input(&mut self, store: &SpaceStore, key: KeyEvent) {
        let len = self.fields[self.field].chars().count();
        match key.code {
            KeyCode::Tab | KeyCode::BackTab if self.intent == Intent::Amend => {
                self.field = (self.field + if key.code == KeyCode::Tab { 1 } else { 2 }) % 3;
                self.cursor = self.fields[self.field].chars().count();
            }
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => self.cursor = (self.cursor + 1).min(len),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = len,
            KeyCode::Backspace if self.cursor > 0 => {
                self.cursor -= 1;
                self.remove();
            }
            KeyCode::Delete => self.remove(),
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.fields[self.field].clear();
                self.cursor = 0;
            }
            KeyCode::Char(ch)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.insert(&ch.to_string());
            }
            KeyCode::Enter => match self.check(store) {
                Ok(()) => {
                    self.mode = Mode::Confirm;
                    self.scroll = 0;
                    self.notice = None;
                }
                Err(error) => self.notice = Some(error.to_string()),
            },
            _ => {}
        }
    }
    fn remove(&mut self) {
        let mut chars: Vec<_> = self.fields[self.field].chars().collect();
        if self.cursor < chars.len() {
            chars.remove(self.cursor);
        }
        self.fields[self.field] = chars.into_iter().collect();
    }
    fn insert(&mut self, text: &str) {
        let text: String = text
            .chars()
            .map(|ch| if ch.is_whitespace() { ' ' } else { ch })
            .filter(|ch| !ch.is_control())
            .collect();
        let limit = if self.field == 0 { 1200 } else { 600 };
        if self.fields[self.field].chars().count() + text.chars().count() > limit {
            self.notice = Some(
                crate::tr!("입력 한도 {limit}자를 초과합니다. 붙여넣은 내용은 반영하지 않았습니다.", "Input exceeds {limit} characters. Pasted content was not applied.";
                ),
            );
            return;
        }
        let byte = self.fields[self.field]
            .char_indices()
            .nth(self.cursor)
            .map_or(self.fields[self.field].len(), |(i, _)| i);
        self.fields[self.field].insert_str(byte, &text);
        self.cursor += text.chars().count();
        self.notice = None;
    }
    pub fn footer(&self) -> &'static str {
        match self.mode {
            Mode::List => crate::tr!(
                "↑↓ 선택 · Enter 근거 · Esc 닫기 · r 새로 읽기",
                "↑↓ select · Enter evidence · Esc close · r reload"
            ),
            Mode::Detail => {
                crate::tr!(
                    "e 정정 · x 오기록 제외 · u 되돌리기 · Esc 목록 · j/k 스크롤 · r 새로 읽기",
                    "e correct · x exclude incorrect · u undo · Esc list · j/k scroll · r reload"
                )
            }
            Mode::Form => crate::tr!(
                "Tab 다음 칸 · ←→ 이동 · Ctrl+U 비우기 · Enter 검토 · Esc 취소",
                "Tab next field · ←→ move · Ctrl+U clear · Enter review · Esc cancel"
            ),
            Mode::Confirm => crate::tr!(
                "Enter 공유 기록에 저장 · Esc 편집 · j/k 스크롤",
                "Enter save to shared records · Esc edit · j/k scroll"
            ),
        }
    }
    #[allow(
        clippy::too_many_lines,
        reason = "Four modal states share one wrapped, scrollable rendering pipeline"
    )]
    pub fn frame(&mut self, width: usize, height: usize) -> (Vec<Line>, usize) {
        let width = width.saturating_sub(2).max(2);
        let mut rows: Vec<(String, Role)> = vec![
            (
                crate::tr!("근거 보기 · 사용자 정정", "Evidence and user corrections").into(),
                Role::Title,
            ),
            (
                format!(
                    "{} · {}",
                    self.scope
                        .branch
                        .as_deref()
                        .unwrap_or(crate::tr!("미연결 기록", "Unlinked records")),
                    self.scope.worktree
                ),
                Role::Muted,
            ),
        ];
        if let Some(notice) = &self.notice {
            rows.push((notice.clone(), Role::Warning));
        }
        let mut focus = None;
        match self.mode {
            Mode::List => {
                rows.push((crate::tr!("결정·완료 기록을 선택하세요. 오기록으로 제외한 항목도 여기서 확인할 수 있습니다.", "Select a decision or completion record. Excluded records are also available here.").into(), Role::Body));
                if self.ids.is_empty() {
                    rows.push((
                        crate::tr!("저장된 결정·완료가 없습니다. 새 대화가 요약되면 항목이 표시됩니다.", "No saved decisions or completions. Records appear after a conversation is summarized.").into(),
                        Role::Muted,
                    ));
                }
                for (index, id) in self
                    .ids
                    .iter()
                    .enumerate()
                    .skip(self.selected.saturating_sub(2))
                    .take(5)
                {
                    let r = &self.journal.records[id];
                    let preview: String = r.text().chars().take(72).collect();
                    rows.push((
                        format!(
                            "{} {} · {}{}{}",
                            if index == self.selected { "▶" } else { " " },
                            kind(r),
                            preview,
                            if r.text().chars().count() > 72 {
                                "…"
                            } else {
                                ""
                            },
                            status(r)
                        ),
                        if index == self.selected {
                            Role::Selected
                        } else {
                            Role::Body
                        },
                    ));
                }
                rows.push((
                    crate::tr!("{}/{} · ↑↓ 선택 · Enter 근거 보기", "{}/{} · ↑↓ select · Enter evidence";
                        if self.ids.is_empty() {
                            0
                        } else {
                            self.selected + 1
                        },
                        self.ids.len()
                    ),
                    Role::Muted,
                ));
            }
            Mode::Detail => {
                if let Some(record) = self.record() {
                    self.detail(record, &mut rows);
                }
            }
            Mode::Form => {
                rows.push((self.action_title().into(), Role::Heading));
                rows.push((
                    crate::tr!("정정은 원문을 바꾸지 않습니다. 실제 결정 변경·철회는 대화에 남겨 주세요.", "Corrections retain the original. Record actual decision changes or withdrawals in the conversation.")
                        .into(),
                    Role::Muted,
                ));
                for i in if self.intent == Intent::Amend {
                    0..3
                } else {
                    2..3
                } {
                    rows.push((
                        format!(
                            "{}{}",
                            if i == self.field { "▶ " } else { "" },
                            [
                                crate::tr!("내용 (필수)", "Content (required)"),
                                crate::tr!("조건 (빈칸 = 조건 없음)", "Condition (blank = none)"),
                                crate::tr!("정정 사유 (필수)", "Correction reason (required)")
                            ][i]
                        ),
                        Role::Heading,
                    ));
                    let mut value = self.fields[i].clone();
                    if i == self.field {
                        let byte = value
                            .char_indices()
                            .nth(self.cursor)
                            .map_or(value.len(), |(j, _)| j);
                        value.insert(byte, '▏');
                        focus = Some(rows.len());
                    }
                    rows.push((value, Role::Body));
                }
            }
            Mode::Confirm => {
                if let Some(record) = self.record() {
                    rows.push((
                        crate::tr!("저장 전 확인", "Review before saving").into(),
                        Role::Heading,
                    ));
                    rows.push((
                        if self.scope.tab_id.is_some() {
                            crate::tr!("이 탭의 현재 브랜치 기록에만 반영됩니다. 다른 탭의 기록은 바뀌지 않습니다.", "Applies to this tab’s current branch records.").into()
                        } else if self.scope.repository.is_some() {
                            crate::tr!("같은 저장소·브랜치 기록을 공유하는 모든 Space에 반영됩니다.", "Applies to all Spaces sharing this repository and branch’s records.").into()
                        } else {
                            crate::tr!("현재 Space의 미연결 기록에 반영됩니다.", "Applies to this Space’s unlinked records.").into()
                        },
                        Role::Warning,
                    ));
                    rows.push((format!("{} · {}", kind(record), record.id), Role::Muted));
                    rows.push((crate::tr!("변경 전", "Before").into(), Role::Heading));
                    rows.push((describe(&corrections::content(record)), Role::Body));
                    if let Ok(change) =
                        corrections::prepare(record, &self.edit(), &self.fields[2], "preview")
                    {
                        let after = change.after.unwrap_or_else(|| corrections::Content {
                            text: record.latest().text.clone(),
                            condition: record.latest().condition.clone(),
                            excluded: false,
                        });
                        rows.push((crate::tr!("변경 후", "After").into(), Role::Heading));
                        rows.push((describe(&after), Role::Body));
                    }
                    rows.push((
                        crate::tr!("사유: {}", "Reason: {}"; self.fields[2]),
                        Role::Body,
                    ));
                    rows.push((crate::tr!("원문·발언 시각은 유지됩니다. 사용자 정정으로 표시되며 새 승인이나 독립 검증을 뜻하지 않습니다.", "Original text and statement time are retained. Marked as a user correction; no new approval or independent verification.").into(), Role::Muted));
                }
            }
        }
        let mut lines = Vec::new();
        let mut focus_line = None;
        for (index, (text, role)) in rows.into_iter().enumerate() {
            let start = lines.len();
            panel_style::wrap(&mut lines, &safe(&text), width, role);
            if focus == Some(index) {
                focus_line = Some(
                    (start..lines.len())
                        .find(|i| lines[*i].contains('▏'))
                        .unwrap_or(start),
                );
            }
        }
        let body = height.saturating_sub(1).max(1);
        if let Some(line) = focus_line {
            if line < self.scroll {
                self.scroll = line.saturating_sub(1);
            }
            if line >= self.scroll + body {
                self.scroll = line.saturating_sub(body.saturating_sub(1));
            }
        }
        self.scroll = self.scroll.min(lines.len().saturating_sub(body));
        (lines, self.scroll)
    }
    fn action_title(&self) -> &'static str {
        match self.intent {
            Intent::Amend => crate::tr!("내용·조건 정정", "Correct content/condition"),
            Intent::Exclude => crate::tr!(
                "오기록으로 제외 · 실제 결정 철회 아님",
                "Exclude as incorrect · Decision not withdrawn"
            ),
            Intent::Undo => crate::tr!("마지막 사용자 정정 되돌리기", "Undo last user correction"),
        }
    }
    #[allow(
        clippy::too_many_lines,
        reason = "Keeps evidence and correction detail ordering together; localized labels add presentation lines"
    )]
    fn detail(&self, record: &Record, rows: &mut Vec<(String, Role)>) {
        rows.push((format!("{}{}", kind(record), status(record)), Role::Heading));
        rows.push((
            format!("{} · {:?} / {:?}", record.id, record.stage, record.outcome),
            Role::Muted,
        ));
        rows.push((describe(&corrections::content(record)), Role::Body));
        rows.push((
            crate::tr!(
                "e 내용·조건 정정 · x 오기록 제외 · u 정정 되돌리기",
                "e correct content/condition · x exclude incorrect · u undo correction"
            )
            .into(),
            Role::Action,
        ));
        rows.push((
            crate::tr!(
                "저장된 원문 인용 · 발언 시각",
                "Saved original quotations and statement times"
            )
            .into(),
            Role::Heading,
        ));
        rows.push((
            crate::tr!(
                "아래는 저장된 근거 발췌입니다. 전체 대화 원문이 아닙니다.",
                "Saved evidence excerpts follow. They are not the full transcript."
            )
            .into(),
            Role::Muted,
        ));
        for version in &record.versions {
            rows.push((
                crate::tr!("원래 기록: {} · 조건: {}", "Original record: {} · Condition: {}";
                    version.text,
                    version.condition.as_deref().unwrap_or(crate::tr!("없음", "None"))
                ),
                Role::Body,
            ));
            for proof in &version.proof {
                let speaker = match proof.source.kind {
                    crate::model::EventKind::UserPrompt => crate::tr!("사용자", "User"),
                    crate::model::EventKind::AssistantFinal => {
                        crate::tr!("어시스턴트", "Assistant")
                    }
                    crate::model::EventKind::Interrupted => {
                        crate::tr!("중단 기록", "Interruption record")
                    }
                };
                rows.push((
                    format!(
                        "{speaker} · {}",
                        self.times.display(proof.source.at_unix_utc)
                    ),
                    Role::Heading,
                ));
                rows.push((format!("“{}”", proof.quote), Role::Body));
                rows.push((
                    crate::tr!("Space: {} · 세션: {} · 턴: {}", "Space: {} · Session: {} · Turn: {}";
                        proof.source.space,
                        proof.source.session,
                        proof.source.turn.as_deref().unwrap_or(crate::tr!("미확인", "Unknown"))
                    ),
                    Role::Muted,
                ));
            }
        }
        rows.push((
            crate::tr!(
                "사용자 정정 이력 · 정정 시각",
                "User correction history and correction times"
            )
            .into(),
            Role::Heading,
        ));
        if record.corrections.is_empty() {
            rows.push((
                crate::tr!("정정 이력 없음", "No correction history").into(),
                Role::Muted,
            ));
        }
        for entry in &record.corrections {
            rows.push((
                crate::tr!("#{} · 사용자 · {} · {}", "#{} · User · {} · {}";
                    entry.sequence,
                    self.times.display(Some(entry.at_unix_utc)),
                    entry
                        .undo_of
                        .map_or_else(|| crate::tr!("정정", "Correction").into(), |id| crate::tr!("#{id} 정정 되돌림", "Correction #{id} undone";))
                ),
                Role::Heading,
            ));
            let original = &entry.original;
            rows.push((
                crate::tr!("전: {}", "Before: {}";
                    describe(entry.before.as_ref().unwrap_or(original))
                ),
                Role::Body,
            ));
            rows.push((
                crate::tr!("후: {}", "After: {}"; describe(entry.after.as_ref().unwrap_or(original))),
                Role::Body,
            ));
            rows.push((
                crate::tr!("사유: {} · Space: {}", "Reason: {} · Space: {}"; entry.reason, entry.space),
                Role::Muted,
            ));
        }
    }
}
fn kind(record: &Record) -> &'static str {
    if record.kind == Kind::Decision {
        crate::tr!("결정", "Decision")
    } else {
        crate::tr!("완료 [보고]", "Completion [Reported]")
    }
}
fn status(record: &Record) -> &'static str {
    if record.excluded() {
        crate::tr!(" · 오기록 제외", " · Excluded as incorrect")
    } else if record.withdrawn {
        crate::tr!(" · 실제 철회", " · Withdrawn")
    } else if record.replaced_by.is_some() {
        crate::tr!(" · 이전 결정", " · Earlier decision")
    } else if corrections::current(record).is_some() {
        crate::tr!(" · 사용자 정정", " · User correction")
    } else {
        ""
    }
}
fn describe(content: &corrections::Content) -> String {
    crate::tr!("{} · 조건: {}{}", "{} · Condition: {}{}";
        content.text,
        content.condition.as_deref().unwrap_or(crate::tr!("없음", "None")),
        if content.excluded {
            crate::tr!(" · 오기록 제외", " · Excluded as incorrect")
        } else {
            ""
        }
    )
}
fn safe(text: &str) -> String {
    text.chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect()
}
