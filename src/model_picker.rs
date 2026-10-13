//! A local summary-settings view. Catalog discovery is read-only and runs off
//! the terminal thread; only the final Enter persists a Maeklog setting.
use crate::model_catalog::{self, Catalog, Model};
use crate::model_settings::{self, Settings};
use crate::panel_style::{Line, Role};
use crossterm::event::{Event, KeyCode, KeyEventKind, MouseButton, MouseEventKind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use unicode_width::UnicodeWidthChar;

#[derive(Default)]
pub struct Outcome {
    pub close: bool,
    pub saved: bool,
}

enum Stage {
    Models,
    Efforts { model: String },
}

struct Discovery {
    cancelled: Arc<AtomicBool>,
    receiver: mpsc::Receiver<Result<Catalog, String>>,
}

impl Drop for Discovery {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

pub struct Picker {
    current: Option<Settings>,
    settings_error: Option<String>,
    catalog: Option<Catalog>,
    verified: bool,
    discovery: Option<Discovery>,
    catalog_error: Option<String>,
    notice: Option<(String, Role)>,
    stage: Stage,
    selected: usize,
    first_visible: usize,
    row_targets: Vec<(usize, usize)>,
    frame_width: usize,
    page_size: usize,
}

impl Picker {
    /// Clear translated transient feedback without changing choices or user input.
    pub fn language_changed(&mut self) {
        self.notice = None;
    }

    pub fn open() -> Self {
        let (current, settings_error) = match model_settings::load() {
            Ok(settings) => (Some(settings), None),
            Err(error) => (None, Some(error.to_string())),
        };
        let mut picker = Self {
            current,
            settings_error,
            catalog: None,
            verified: false,
            discovery: None,
            catalog_error: None,
            notice: None,
            stage: Stage::Models,
            selected: 0,
            first_visible: 0,
            row_targets: Vec::new(),
            frame_width: 0,
            page_size: 1,
        };
        picker.refresh();
        picker
    }

    fn refresh(&mut self) {
        if self.discovery.is_some() {
            return;
        }
        self.back();
        self.stage = Stage::Models;
        self.first_visible = 0;
        self.notice = None;
        self.verified = false;
        self.catalog_error = None;
        let cancelled = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancelled);
        let (sender, receiver) = mpsc::channel();
        match std::thread::Builder::new()
            .name("maeklog-model-list".into())
            .spawn(move || {
                let result = model_catalog::discover_cancellable(|| flag.load(Ordering::Relaxed))
                    .map_err(|error| error.to_string());
                let _ = sender.send(result);
            }) {
            Ok(_) => {
                self.discovery = Some(Discovery {
                    cancelled,
                    receiver,
                });
            }
            Err(error) => self.catalog_error = Some(error.to_string()),
        }
    }

    pub fn poll(&mut self) {
        let Some(job) = &self.discovery else {
            return;
        };
        let result = match job.receiver.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => Err(crate::tr!(
                "모델 목록 확인 작업이 중단되었습니다",
                "Model list discovery stopped"
            )
            .into()),
        };
        self.discovery = None;
        let selected_model = self
            .catalog
            .as_ref()
            .and_then(|catalog| catalog.models.get(self.selected))
            .map(|model| model.model.clone());
        match result {
            Ok(catalog) => {
                self.selected = selected_model
                    .as_deref()
                    .and_then(|model| catalog.models.iter().position(|entry| entry.model == model))
                    .or_else(|| {
                        self.current.as_ref().and_then(|settings| {
                            catalog
                                .models
                                .iter()
                                .position(|entry| entry.model == settings.model)
                        })
                    })
                    .or_else(|| catalog.models.iter().position(|model| model.is_default))
                    .unwrap_or(0);
                self.catalog = Some(catalog);
                self.verified = true;
                self.catalog_error = None;
            }
            Err(error) => {
                self.verified = false;
                self.catalog_error = Some(error);
            }
        }
        self.first_visible = 0;
    }

    fn model(&self) -> Option<&Model> {
        let catalog = self.catalog.as_ref()?;
        match &self.stage {
            Stage::Models => catalog.models.get(self.selected),
            Stage::Efforts { model } => catalog.find(model),
        }
    }

    fn count(&self) -> usize {
        match self.stage {
            Stage::Models => self
                .catalog
                .as_ref()
                .map_or(0, |catalog| catalog.models.len()),
            Stage::Efforts { .. } => self
                .model()
                .map_or(0, |model| model.supported_reasoning_efforts.len().max(1)),
        }
    }

    fn choose_model(&mut self) {
        if !self.verified || self.discovery.is_some() || self.current.is_none() {
            return;
        }
        let Some(model) = self.model() else {
            return;
        };
        let selected = self
            .current
            .as_ref()
            .filter(|settings| settings.model == model.model)
            .and_then(|settings| settings.effort.as_deref())
            .and_then(|effort| {
                model
                    .supported_reasoning_efforts
                    .iter()
                    .position(|entry| entry.reasoning_effort == effort)
            })
            .or_else(|| {
                model
                    .default_reasoning_effort
                    .as_deref()
                    .and_then(|effort| {
                        model
                            .supported_reasoning_efforts
                            .iter()
                            .position(|entry| entry.reasoning_effort == effort)
                    })
            })
            .unwrap_or(0);
        self.stage = Stage::Efforts {
            model: model.model.clone(),
        };
        self.selected = selected;
        self.first_visible = 0;
        self.notice = None;
    }

    fn back(&mut self) {
        if let Stage::Efforts { model } = &self.stage {
            self.selected = self
                .catalog
                .as_ref()
                .and_then(|catalog| {
                    catalog
                        .models
                        .iter()
                        .position(|entry| entry.model == *model)
                })
                .unwrap_or(0);
            self.stage = Stage::Models;
            self.first_visible = 0;
            self.notice = None;
        }
    }

    fn save(&mut self) -> Outcome {
        if !self.verified || self.discovery.is_some() || self.current.is_none() {
            return Outcome::default();
        }
        let Stage::Efforts { .. } = &self.stage else {
            return Outcome::default();
        };
        let Some(model) = self.model() else {
            return Outcome::default();
        };
        let effort = if model.supported_reasoning_efforts.is_empty() {
            None
        } else {
            let Some(entry) = model.supported_reasoning_efforts.get(self.selected) else {
                return Outcome::default();
            };
            Some(entry.reasoning_effort.clone())
        };
        let settings = Settings {
            model: model.model.clone(),
            effort,
        };
        // A second panel may have saved since this chooser opened. Surface that
        // current setting and require a new deliberate Enter before replacing it.
        match model_settings::load() {
            Ok(latest) if self.current.as_ref() != Some(&latest) => {
                self.current = Some(latest);
                self.notice = Some((crate::tr!(
                    "다른 곳에서 설정이 바뀌었습니다 · 현재 설정을 확인하고 Enter로 저장하세요",
                    "Settings changed elsewhere · review the current setting, then Enter to save"
                ).into(), Role::Warning));
                return Outcome::default();
            }
            Err(error) => {
                self.notice = Some((
                    crate::tr!(
                        format!("현재 설정 확인 실패 · {error}"),
                        format!("Could not read current settings · {error}")
                    ),
                    Role::Error,
                ));
                return Outcome::default();
            }
            Ok(_) => {}
        }
        let Some(catalog) = &self.catalog else {
            return Outcome::default();
        };
        match model_settings::save(&settings, catalog) {
            Ok(()) => Outcome {
                close: true,
                saved: true,
            },
            Err(error) => {
                self.notice = Some((
                    crate::tr!(
                        format!("설정 저장 실패 · {error} · Enter 재시도 / Esc 취소"),
                        format!("Could not save settings · {error} · Enter retry / Esc cancel")
                    ),
                    Role::Error,
                ));
                Outcome::default()
            }
        }
    }

    pub fn handle(&mut self, event: &Event) -> Outcome {
        match event {
            Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::Esc | KeyCode::Char('q') => {
                    return Outcome {
                        close: true,
                        saved: false,
                    };
                }
                KeyCode::Char('r' | 'R') => self.refresh(),
                KeyCode::Up | KeyCode::Char('k') => self.selected = self.selected.saturating_sub(1),
                KeyCode::Down | KeyCode::Char('j') => {
                    self.selected = self
                        .selected
                        .saturating_add(1)
                        .min(self.count().saturating_sub(1));
                }
                KeyCode::PageUp => self.selected = self.selected.saturating_sub(self.page_size),
                KeyCode::PageDown | KeyCode::Char(' ') => {
                    self.selected = self
                        .selected
                        .saturating_add(self.page_size)
                        .min(self.count().saturating_sub(1));
                }
                KeyCode::Home => self.selected = 0,
                KeyCode::End => self.selected = self.count().saturating_sub(1),
                KeyCode::Left | KeyCode::Backspace => self.back(),
                KeyCode::Right if matches!(self.stage, Stage::Models) => self.choose_model(),
                KeyCode::Enter => {
                    if matches!(self.stage, Stage::Models) {
                        self.choose_model();
                    } else {
                        return self.save();
                    }
                }
                _ => {}
            },
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::ScrollUp => self.selected = self.selected.saturating_sub(1),
                MouseEventKind::ScrollDown => {
                    self.selected = self
                        .selected
                        .saturating_add(1)
                        .min(self.count().saturating_sub(1));
                }
                MouseEventKind::Down(MouseButton::Left)
                    if usize::from(mouse.column) < self.frame_width =>
                {
                    if let Some((_, index)) = self
                        .row_targets
                        .iter()
                        .find(|(row, _)| *row == usize::from(mouse.row))
                    {
                        self.selected = *index;
                    }
                }
                _ => {}
            },
            _ => {}
        }
        Outcome::default()
    }

    fn can_confirm(&self) -> bool {
        self.verified && self.discovery.is_none() && self.current.is_some() && self.count() > 0
    }

    fn current_lines(&self, width: usize, compact: bool) -> Vec<Line> {
        let mut lines = Vec::new();
        append(
            &mut lines,
            crate::tr!("요약 모델", "Summary model"),
            width,
            1,
            Role::Title,
        );
        if let Some(settings) = &self.current {
            let current = if width < 40 {
                crate::tr!(
                    format!("현재 모델: {}", settings.model),
                    format!("Current: {}", settings.model)
                )
            } else {
                crate::tr!(
                    format!("현재 모델: {}", settings.model),
                    format!("Current model: {}", settings.model)
                )
            };
            append(
                &mut lines,
                &current,
                width,
                if compact { 1 } else { 2 },
                Role::Body,
            );
            let effort = settings
                .effort
                .as_deref()
                .unwrap_or(crate::tr!("지정 안 함", "no override"));
            append(
                &mut lines,
                &crate::tr!(
                    format!("현재 추론 강도: {effort}"),
                    format!("Current reasoning: {effort}")
                ),
                width,
                1,
                Role::Body,
            );
        } else {
            append(
                &mut lines,
                crate::tr!(
                    "현재 설정을 읽지 못했습니다 · 설정 유지",
                    "Current settings unreadable · retained"
                ),
                width,
                2,
                Role::Error,
            );
        }
        if !compact {
            append(
                &mut lines,
                crate::tr!(
                    "모든 Maeklog 탭 · 다음 요약 요청부터 적용",
                    "All Maeklog tabs · from the next summary request"
                ),
                width,
                2,
                Role::Muted,
            );
            append(
                &mut lines,
                crate::tr!(
                    "저장 기록은 유지 · 진행 요약은 같은 모델·강도·언어끼리 재사용",
                    "Saved records retained · live summary reuse requires the same model, reasoning and language"
                ),
                width,
                3,
                Role::Muted,
            );
        }
        lines
    }

    fn status_lines(&self, width: usize, compact: bool) -> Vec<Line> {
        let mut lines = Vec::new();
        if let Some(error) = &self.settings_error {
            append(&mut lines, error, width, 2, Role::Error);
            append(
                &mut lines,
                crate::tr!(
                    "설정 파일을 확인한 뒤 Esc로 닫고 다시 여세요",
                    "Check the settings file, then Esc and reopen"
                ),
                width,
                2,
                Role::Warning,
            );
        }
        let (status, role) = if self.discovery.is_some() {
            (
                crate::tr!(
                    "Codex 모델 목록 확인 중… · 설정 유지",
                    "Loading Codex model list… · settings retained"
                ),
                Role::Muted,
            )
        } else if self.catalog_error.is_some() {
            (
                if self.catalog.is_some() {
                    crate::tr!(
                        "목록 확인 실패 · 이전 목록은 미확인 · 저장 불가 · r 재시도",
                        "List check failed · prior list unverified · saving disabled · r retry"
                    )
                } else {
                    crate::tr!(
                        "모델 목록 확인 실패 · 설정 유지 · r 재시도",
                        "Model list unavailable · settings retained · r retry"
                    )
                },
                Role::Warning,
            )
        } else if self.count() == 0 {
            (
                crate::tr!(
                    "Codex가 반환한 모델이 없습니다 · 설정 유지 · r 재시도",
                    "Codex returned no models · settings retained · r retry"
                ),
                Role::Warning,
            )
        } else {
            (
                crate::tr!("Codex가 반환한 모델 목록", "Models returned by Codex"),
                Role::Muted,
            )
        };
        append(&mut lines, status, width, if compact { 1 } else { 3 }, role);
        if !compact {
            if let Some(error) = &self.catalog_error {
                append(&mut lines, error, width, 2, Role::Body);
            }
            if let Some(warning) = self.current_catalog_warning() {
                append(&mut lines, warning, width, 2, Role::Warning);
            }
        }
        if let Some((notice, role)) = &self.notice {
            append(
                &mut lines,
                notice,
                width,
                if compact { 1 } else { 3 },
                *role,
            );
        }
        lines
    }

    fn current_catalog_warning(&self) -> Option<&'static str> {
        if !self.verified {
            return None;
        }
        let settings = self.current.as_ref()?;
        let catalog = self.catalog.as_ref()?;
        let Some(model) = catalog.find(&settings.model) else {
            return Some(crate::tr!(
                "현재 모델이 목록에 없습니다 · 설정 유지",
                "Current model is not listed · retained"
            ));
        };
        let supported = match &settings.effort {
            Some(effort) => model
                .supported_reasoning_efforts
                .iter()
                .any(|entry| entry.reasoning_effort == *effort),
            None => model.supported_reasoning_efforts.is_empty(),
        };
        (!supported).then_some(crate::tr!(
            "현재 추론 설정이 목록에 없습니다 · 설정 유지",
            "Current reasoning setting is not listed · retained"
        ))
    }

    fn row(&self, index: usize) -> String {
        let pointer = if index == self.selected { ">" } else { " " };
        match &self.stage {
            Stage::Models => {
                let Some(model) = self
                    .catalog
                    .as_ref()
                    .and_then(|catalog| catalog.models.get(index))
                else {
                    return String::new();
                };
                let label = if model.display_name.is_empty() {
                    &model.model
                } else {
                    &model.display_name
                };
                let badge = if self
                    .current
                    .as_ref()
                    .is_some_and(|settings| settings.model == model.model)
                {
                    crate::tr!(" · 현재", " · current")
                } else if model.is_default {
                    crate::tr!(" · 목록 기본값", " · list default")
                } else {
                    ""
                };
                format!("{pointer} {label}{badge}")
            }
            Stage::Efforts { .. } => {
                let Some(model) = self.model() else {
                    return String::new();
                };
                let Some(effort) = model.supported_reasoning_efforts.get(index) else {
                    return crate::tr!(
                        format!("{pointer} 추론 강도 지정 안 함"),
                        format!("{pointer} No reasoning override")
                    );
                };
                let badge = if self.current.as_ref().is_some_and(|settings| {
                    settings.model == model.model
                        && settings.effort.as_deref() == Some(&effort.reasoning_effort)
                }) {
                    crate::tr!(" · 현재", " · current")
                } else if model.default_reasoning_effort.as_deref()
                    == Some(&effort.reasoning_effort)
                {
                    crate::tr!(" · 목록 기본값", " · list default")
                } else {
                    ""
                };
                format!("{pointer} {}{badge}", effort.reasoning_effort)
            }
        }
    }

    fn detail_lines(&self, width: usize, compact: bool) -> Vec<Line> {
        let mut lines = Vec::new();
        let Some(model) = self.model() else {
            return lines;
        };
        append(
            &mut lines,
            &crate::tr!(
                format!("모델 ID: {}", model.model),
                format!("Model ID: {}", model.model)
            ),
            width,
            if compact { 1 } else { 2 },
            Role::Muted,
        );
        if matches!(self.stage, Stage::Efforts { .. }) && !compact {
            if let Some(effort) = model.supported_reasoning_efforts.get(self.selected) {
                append(&mut lines, &effort.description, width, 3, Role::Body);
            } else {
                append(
                    &mut lines,
                    crate::tr!(
                        "이 모델은 추론 강도 옵션을 반환하지 않았습니다. 강도 설정 없이 저장합니다.",
                        "This model returned no reasoning options. Save without a reasoning override."
                    ),
                    width,
                    3,
                    Role::Body,
                );
            }
            append(
                &mut lines,
                crate::tr!(
                    "Enter로 이 모델·강도를 저장합니다 · Esc 취소",
                    "Enter saves this model and reasoning · Esc cancels"
                ),
                width,
                2,
                Role::Action,
            );
        }
        lines
    }

    pub fn frame(&mut self, width: usize, height: usize) -> (Vec<Line>, String) {
        self.frame_width = width;
        self.row_targets.clear();
        let body = height.saturating_sub(1);
        let compact = height < 18;
        let mut lines = self.current_lines(width, compact);
        lines.extend(self.status_lines(width, compact));
        let heading = if !self.can_confirm() {
            crate::tr!("모델 목록 · r 다시 확인", "Model list · r retry")
        } else if matches!(self.stage, Stage::Models) {
            crate::tr!(
                "모델 선택 · Enter로 추론 강도 선택",
                "Choose model · Enter for reasoning"
            )
        } else {
            crate::tr!(
                "추론 강도 선택 · Enter로 저장",
                "Choose reasoning · Enter to save"
            )
        };
        let detail = self.detail_lines(width, compact);
        // Reserve one list row and the stage heading even in a short terminal.
        let header_limit = body.saturating_sub(2 + detail.len().min(2));
        lines.truncate(header_limit);
        append(&mut lines, heading, width, 1, Role::Heading);
        let slots = body
            .saturating_sub(lines.len() + detail.len().min(body.saturating_sub(lines.len() + 1)));
        self.page_size = slots.max(1);
        let count = self.count();
        self.selected = self.selected.min(count.saturating_sub(1));
        if self.selected < self.first_visible {
            self.first_visible = self.selected;
        } else if self.selected >= self.first_visible.saturating_add(slots.max(1)) {
            self.first_visible = self.selected.saturating_add(1).saturating_sub(slots.max(1));
        }
        self.first_visible = self.first_visible.min(count.saturating_sub(slots.max(1)));
        for index in self.first_visible..self.first_visible.saturating_add(slots).min(count) {
            self.row_targets.push((lines.len(), index));
            append(
                &mut lines,
                &self.row(index),
                width,
                1,
                if index == self.selected {
                    Role::Selected
                } else {
                    Role::Body
                },
            );
        }
        if count > 0 {
            lines.extend(detail.into_iter().take(body.saturating_sub(lines.len())));
        }
        lines.truncate(body);
        (lines, self.footer(width))
    }

    fn footer(&self, width: usize) -> String {
        let full = if !self.can_confirm() {
            crate::tr!(
                "↑↓ 보기 · r 다시 확인 · Esc 취소",
                "↑↓ browse · r retry · Esc cancel"
            )
        } else if matches!(self.stage, Stage::Models) {
            crate::tr!(
                "↑↓ 선택 · Enter 강도 · r 새로 확인 · Esc 취소",
                "↑↓ choose · Enter reasoning · r refresh · Esc cancel"
            )
        } else {
            crate::tr!(
                "↑↓ 선택 · Enter 저장 · ← 모델 · Esc 취소",
                "↑↓ choose · Enter save · ← models · Esc cancel"
            )
        };
        let medium = if !self.can_confirm() {
            crate::tr!("r 다시 확인 · Esc 취소", "r retry · Esc cancel")
        } else if matches!(self.stage, Stage::Models) {
            crate::tr!(
                "Enter 선택 · r 확인 · Esc 취소",
                "Enter · r refresh · Esc cancel"
            )
        } else {
            crate::tr!("Enter 저장 · ← 모델 · Esc", "Enter save · ← models · Esc")
        };
        [full, medium, crate::tr!("Esc 취소", "Esc cancel")]
            .into_iter()
            .find(|text| cells(text) <= width)
            .map_or_else(|| clipped("Esc", width), str::to_owned)
    }
}

fn cells(text: &str) -> usize {
    text.chars().map(|ch| ch.width().unwrap_or(0)).sum()
}

fn clipped(text: &str, width: usize) -> String {
    let clean: String = text.chars().filter(|ch| !ch.is_control()).collect();
    if cells(&clean) <= width {
        return clean;
    }
    let budget = width.saturating_sub(1);
    let mut result = String::new();
    let mut used = 0;
    for ch in clean.chars() {
        let size = ch.width().unwrap_or(0);
        if used + size > budget {
            break;
        }
        result.push(ch);
        used += size;
    }
    if width > 0 {
        result.push('…');
    }
    result
}

fn append(lines: &mut Vec<Line>, text: &str, width: usize, limit: usize, role: Role) {
    if width == 0 || limit == 0 {
        return;
    }
    let mut wrapped = Vec::new();
    crate::panel::wrapped_bullet(&mut wrapped, text, width, "");
    let overflow = wrapped.len() > limit;
    for (index, text) in wrapped.into_iter().take(limit).enumerate() {
        let text = if index + 1 == limit && overflow {
            clipped(&(text + "…"), width)
        } else {
            clipped(&text, width)
        };
        lines.push(Line { text, role });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_catalog::ReasoningEffort;
    use crossterm::event::{KeyEvent, KeyModifiers, MouseEvent};

    fn picker() -> Picker {
        Picker {
            current: Some(Settings {
                model: "fixture-current".into(),
                effort: Some("max".into()),
            }),
            settings_error: None,
            catalog: Some(Catalog {
                models: (0..40)
                    .map(|index| Model {
                        id: format!("fixture-{index}"),
                        model: format!("fixture-{index}"),
                        display_name: format!(
                            "선택 가능한 긴 모델 이름 {index} · 원문 {}",
                            "한글🐱".repeat(30)
                        ),
                        supported_reasoning_efforts: vec![ReasoningEffort {
                            reasoning_effort: "max".into(),
                            description: "Returned effort description".repeat(20),
                        }],
                        default_reasoning_effort: Some("max".into()),
                        is_default: index == 0,
                    })
                    .collect(),
            }),
            verified: true,
            discovery: None,
            catalog_error: None,
            notice: None,
            stage: Stage::Models,
            selected: 0,
            first_visible: 0,
            row_targets: Vec::new(),
            frame_width: 0,
            page_size: 1,
        }
    }

    fn key(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    #[test]
    fn sizes_preserve_a_visible_cursor_and_bounded_rows() {
        for (width, height) in [(80, 40), (30, 28), (30, 8), (10, 5), (2, 2), (0, 0)] {
            let mut picker = picker();
            for code in [
                KeyCode::Home,
                KeyCode::End,
                KeyCode::PageUp,
                KeyCode::PageDown,
            ] {
                picker.handle(&key(code));
                let (frame, footer) = picker.frame(width, height);
                assert!(frame.len() <= height.saturating_sub(1));
                assert!(frame.iter().all(|line| cells(line) <= width));
                assert!(cells(&footer) <= width);
                if height >= 5 && width >= 10 {
                    assert!(frame.iter().any(|line| line.text.starts_with('>')));
                }
            }
        }
    }

    #[test]
    fn cancel_navigation_and_clicks_never_save_or_rewrite_current() {
        let mut picker = picker();
        let current = picker.current.clone();
        let (frame, _) = picker.frame(30, 28);
        let row = frame
            .iter()
            .position(|line| line.text.starts_with('>'))
            .unwrap();
        let click = Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 2,
            row: u16::try_from(row).unwrap(),
            modifiers: KeyModifiers::NONE,
        });
        assert!(!picker.handle(&click).saved);
        assert!(!picker.handle(&key(KeyCode::Enter)).saved);
        assert!(matches!(picker.stage, Stage::Efforts { .. }));
        let outcome = picker.handle(&key(KeyCode::Esc));
        assert!(outcome.close && !outcome.saved);
        assert_eq!(picker.current, current);
    }

    #[test]
    fn stale_and_empty_catalogs_block_confirmation_and_preserve_settings() {
        let mut picker = picker();
        let current = picker.current.clone();
        picker.verified = false;
        picker.catalog_error = Some("offline fixture".into());
        assert!(!picker.handle(&key(KeyCode::Enter)).saved);
        assert!(matches!(picker.stage, Stage::Models));
        let text = picker
            .frame(80, 40)
            .0
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains(crate::tr!("미확인", "unverified")));
        assert!(text.contains("fixture-current") && text.contains("offline fixture"));
        picker.verified = true;
        picker.catalog = Some(Catalog { models: vec![] });
        assert!(!picker.handle(&key(KeyCode::Enter)).saved);
        assert_eq!(picker.current, current);
    }

    #[test]
    fn effortless_model_uses_a_separate_no_override_confirmation() {
        let mut picker = picker();
        picker.catalog.as_mut().unwrap().models[0]
            .supported_reasoning_efforts
            .clear();
        picker.catalog.as_mut().unwrap().models[0].default_reasoning_effort = None;
        assert!(!picker.handle(&key(KeyCode::Enter)).saved);
        assert!(matches!(picker.stage, Stage::Efforts { .. }));
        assert_eq!(picker.count(), 1);
        assert!(
            picker
                .row(0)
                .contains(crate::tr!("지정 안 함", "No reasoning override"))
        );
        assert_eq!(picker.current.as_ref().unwrap().model, "fixture-current");
    }
}
