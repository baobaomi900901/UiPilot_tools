use crate::screenshot::{
    annotations::{Annotation, AnnotationHistory, MosaicStroke, TextAnnotation},
    editing::{hit_handle_on_monitors, EditingSelection, Handle},
    geometry::{BudgetedImage, PixelBudget},
    model::{MonitorFrame, PhysicalPoint, PhysicalRect, ScreenshotError},
};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Focus {
    Handle(Handle),
    Button(Button),
    Property(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Button {
    Mosaic,
    Text,
    Rectangle,
    Undo,
    Redo,
    Cancel,
    Pin,
    Save,
    Copy,
}
impl Button {
    pub const ALL: [Self; 9] = [
        Self::Mosaic,
        Self::Text,
        Self::Rectangle,
        Self::Undo,
        Self::Redo,
        Self::Cancel,
        Self::Pin,
        Self::Save,
        Self::Copy,
    ];
    pub fn action(self) -> Action {
        match self {
            Self::Mosaic => Action::Tool(Tool::Mosaic),
            Self::Text => Action::Tool(Tool::Text),
            Self::Rectangle => Action::Tool(Tool::Rectangle),
            Self::Undo => Action::Undo,
            Self::Redo => Action::Redo,
            Self::Cancel => Action::Terminate,
            Self::Pin => Action::Pin,
            Self::Save => Action::Save,
            Self::Copy => Action::Copy,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Tool {
    Mosaic,
    Text,
    Rectangle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Action {
    Terminate,
    Tool(Tool),
    Undo,
    Redo,
    None,
    Changed,
    Save,
    Pin,
    Copy,
    Cancel,
}

pub(super) fn accepts_key(key: u16, repeated: bool) -> bool {
    !repeated || !matches!(key, 13 | 32 | 67)
}

fn drag_endpoint(mut point: PhysicalPoint, bounds: PhysicalRect) -> PhysicalPoint {
    point.x = point.x.clamp(bounds.left, bounds.right);
    point.y = point.y.clamp(bounds.top, bounds.bottom);
    if point.x == bounds.right - 1 {
        point.x = bounds.right;
    }
    if point.y == bounds.bottom - 1 {
        point.y = bounds.bottom;
    }
    point
}

enum Gesture {
    Mosaic(MosaicStroke),
    Rectangle {
        start: PhysicalPoint,
        end: PhysicalPoint,
        clip: PhysicalRect,
        color: [u8; 4],
        tolerance: (i32, i32),
    },
}
pub(super) struct Editor {
    pub history: AnnotationHistory,
    pub tool: Option<Tool>,
    pub color: [u8; 4],
    pub font_size: u32,
    pub brush_size: u32,
    pub cursor: Option<PhysicalPoint>,
    pub pending: Option<Annotation>,
    gesture: Option<Gesture>,
    pub preview: Option<Arc<BudgetedImage>>,
    pub preview_rect: PhysicalRect,
    pub committed_preview: Option<Arc<BudgetedImage>>,
    pub preview_dirty: bool,
    pub preview_revision: u64,
    pub preview_damage: Option<PhysicalRect>,
    preview_pending_bounds: Option<PhysicalRect>,
    mosaic_processed: Option<usize>,
    nudge_active: bool,
    pub drag_tolerance: Option<(i32, i32)>,
    pub geometry: EditingSelection,
    bounds: PhysicalRect,
    pub focus: Focus,
    pub keyboard_focus: bool,
    pub outputting: bool,
    pub output_action: Option<Action>,
    pub error: Option<&'static str>,
}

impl Editor {
    fn note_preview_change(&mut self, damage: PhysicalRect) {
        self.preview_revision = self.preview_revision.wrapping_add(1);
        self.preview_damage = Some(match self.preview_damage {
            Some(old) => PhysicalRect {
                left: old.left.min(damage.left),
                top: old.top.min(damage.top),
                right: old.right.max(damage.right),
                bottom: old.bottom.max(damage.bottom),
            },
            None => damage,
        });
    }
    /// A skipped/failed upload cannot discard changes from a newer revision.
    pub fn acknowledge_preview(&mut self, revision: u64) {
        if revision == self.preview_revision {
            self.preview_damage = None;
        }
    }
    pub fn focus_button(&mut self, button: Button) {
        self.focus = Focus::Button(button);
    }
    pub fn new(rect: PhysicalRect, bounds: PhysicalRect) -> Option<Self> {
        EditingSelection::new(rect, bounds).map(|geometry| Self {
            geometry,
            history: AnnotationHistory::new(rect).expect("validated selection"),
            tool: None,
            color: [239, 68, 68, 255],
            font_size: 20,
            brush_size: 24,
            cursor: None,
            pending: None,
            gesture: None,
            preview: None,
            committed_preview: None,
            preview_rect: rect,
            preview_dirty: true,
            preview_revision: 0,
            preview_damage: Some(rect),
            preview_pending_bounds: None,
            mosaic_processed: None,
            nudge_active: false,
            drag_tolerance: None,
            bounds,
            focus: Focus::Button(Button::Save),
            keyboard_focus: false,
            outputting: false,
            output_action: None,
            error: None,
        })
    }
    pub fn pointer_point(&self, point: PhysicalPoint) -> PhysicalPoint {
        if self.geometry.resizing() {
            drag_endpoint(point, self.bounds)
        } else {
            PhysicalPoint {
                x: point.x.clamp(self.bounds.left, self.bounds.right),
                y: point.y.clamp(self.bounds.top, self.bounds.bottom),
            }
        }
    }
    pub fn down(&mut self, point: PhysicalPoint, monitors: &[(PhysicalRect, u32)]) -> bool {
        self.keyboard_focus = false;
        if self.outputting || self.gesture.is_some() {
            return false;
        }
        let _ = self.finish_nudge();
        if let Some(tool) = self.tool {
            if !self.geometry.rect().contains(point) {
                return false;
            }
            self.cursor = Some(point);
            let dpi = monitors
                .iter()
                .find(|(r, _)| r.contains(point))
                .map(|(_, d)| *d)
                .unwrap_or(96);
            self.gesture = match tool {
                Tool::Mosaic => {
                    let Ok(mut stroke) = MosaicStroke::new(
                        self.geometry.rect(),
                        (self.brush_size * dpi / 96).max(1),
                        (8 * dpi / 96).max(1),
                    ) else {
                        return false;
                    };
                    if stroke.push(point).is_err() {
                        return false;
                    }
                    Some(Gesture::Mosaic(stroke))
                }
                Tool::Rectangle => Some(Gesture::Rectangle {
                    start: point,
                    end: point,
                    clip: self.geometry.rect(),
                    color: self.color,
                    tolerance: self
                        .drag_tolerance
                        .unwrap_or(((4 * dpi / 96).max(1) as i32, (4 * dpi / 96).max(1) as i32)),
                }),
                Tool::Text => return false,
            };
            self.update_pending();
            return true;
        }
        let Some(handle) = hit_handle_on_monitors(self.geometry.rect(), point, monitors) else {
            return false;
        };
        // Hit testing uses the physical pixel under the pointer. The drag
        // anchor uses the same half-open endpoint mapping as every later move
        // and release, so a stationary edge click cannot resize by one pixel.
        if !self
            .geometry
            .begin_resize(handle, drag_endpoint(point, self.bounds))
        {
            return false;
        }
        self.focus = Focus::Handle(handle);
        true
    }
    pub fn moved(&mut self, point: PhysicalPoint) -> bool {
        let cursor_changed = self.cursor != Some(point);
        self.cursor = Some(point);
        if let Some(g) = self.gesture.as_mut() {
            match g {
                Gesture::Mosaic(s) => {
                    if s.push(point).is_err() {
                        self.error = Some("标注过多，请结束当前笔画");
                    }
                }
                Gesture::Rectangle { end, .. } => *end = point,
            }
            self.update_pending();
            return true;
        }
        if self.tool.is_some() {
            return cursor_changed;
        }

        let Some(handle) = self.geometry.drag_to(self.pointer_point(point)) else {
            return false;
        };
        self.focus = Focus::Handle(handle);
        true
    }
    pub fn up(&mut self, point: PhysicalPoint) -> bool {
        let changed = self.moved(point);
        self.geometry.end_resize();
        self.gesture = None;
        changed
    }
    /// Windows virtual-key values are kept here as integers so the complete
    /// keyboard contract can be tested without creating a desktop window.
    pub fn key(&mut self, key: u16, shift: bool, ctrl: bool) -> Action {
        if key == 27 {
            return Action::Cancel;
        }
        if self.outputting || self.dragging() {
            return Action::None;
        }
        match key {
            9 => {
                self.keyboard_focus = true;
                let mut stops = Vec::with_capacity(28);
                if self.tool.is_none() {
                    stops.extend(Handle::ALL.into_iter().map(Focus::Handle));
                }
                stops.extend(
                    Button::ALL
                        .into_iter()
                        .filter(|b| self.enabled(*b))
                        .map(Focus::Button),
                );
                stops.extend(self.property_indices().into_iter().map(Focus::Property));
                self.focus = match stops.iter().position(|v| *v == self.focus) {
                    Some(index) => {
                        stops[(index + if shift { stops.len() - 1 } else { 1 }) % stops.len()]
                    }
                    None => {
                        if shift {
                            *stops.last().unwrap()
                        } else {
                            stops[0]
                        }
                    }
                };
                Action::Changed
            }
            13 => match self.focus {
                Focus::Button(button) => button.action(),
                _ => Action::Save,
            },
            32 => match self.focus {
                Focus::Button(button) => button.action(),
                _ => Action::None,
            },
            67 if ctrl => Action::Copy,
            83 if ctrl => Action::Save,
            90 if ctrl && shift => Action::Redo,
            90 if ctrl => Action::Undo,
            89 if ctrl => Action::Redo,
            37..=40 if self.tool.is_none() => {
                self.nudge_active = true;
                let Focus::Handle(handle) = self.focus else {
                    return Action::None;
                };
                let step = if shift { 10 } else { 1 };
                let (dx, dy) = match key {
                    37 => (-step, 0),
                    38 => (0, -step),
                    39 => (step, 0),
                    _ => (0, step),
                };
                self.focus = Focus::Handle(self.geometry.nudge(handle, dx, dy));
                Action::Changed
            }
            _ => Action::None,
        }
    }
    pub fn brush_radius(&self, pointer_dpi: u32) -> u32 {
        match self.gesture.as_ref() {
            Some(Gesture::Mosaic(s)) => s.radius,
            _ => (self.brush_size * pointer_dpi / 96).max(1),
        }
    }
    pub fn normalize_focus(&mut self) -> bool {
        if self.outputting || self.dragging() {
            return false;
        }
        let next = match self.focus {
            Focus::Button(b) if !self.enabled(b) => {
                let index = Button::ALL.iter().position(|v| *v == b).unwrap();
                (1..=Button::ALL.len())
                    .map(|n| Button::ALL[(index + n) % Button::ALL.len()])
                    .find(|b| self.enabled(*b))
                    .map(Focus::Button)
            }
            Focus::Property(i) if !self.property_indices().contains(&i) => {
                Some(Focus::Button(Button::Save))
            }
            Focus::Handle(_) if self.tool.is_some() => {
                Some(Focus::Button(match self.tool.unwrap() {
                    Tool::Mosaic => Button::Mosaic,
                    Tool::Text => Button::Text,
                    Tool::Rectangle => Button::Rectangle,
                }))
            }
            _ => None,
        };
        if let Some(next) = next {
            self.focus = next;
            return true;
        }
        false
    }
    pub fn property_indices(&self) -> Vec<usize> {
        match self.tool {
            Some(Tool::Mosaic) => vec![0, 1, 2],
            Some(Tool::Text) => vec![3, 4, 5, 6, 7, 8, 9, 10],
            Some(Tool::Rectangle) => vec![6, 7, 8, 9, 10],
            None => vec![],
        }
    }
    pub fn finish_selection_rendered(
        &mut self,
        frames: &[MonitorFrame],
        budget: &PixelBudget,
    ) -> Result<bool, ScreenshotError> {
        self.nudge_active = false;
        if self.history.snapshot().selection() == self.geometry.rect() {
            return Ok(false);
        }
        let snapshot = self.history.snapshot();
        let image = if snapshot.annotations().is_empty() {
            Ok(None)
        } else {
            snapshot
                .compose_preview(frames, budget, Some(self.geometry.rect()), None, None)
                .map(|i| Some(Arc::new(i)))
        };
        match image {
            Ok(image) => {
                let result = self.history.set_selection(self.geometry.rect());
                if result.is_err() {
                    self.restore_geometry();
                    return result;
                }
                self.note_preview_change(self.geometry.rect());
                self.preview = image;
                self.mosaic_processed = None;
                self.committed_preview = self.preview.clone();
                self.preview_rect = self.geometry.rect();
                self.preview_dirty = self.preview.is_none();
                result
            }
            Err(e) => {
                self.restore_geometry();
                Err(e)
            }
        }
    }
    pub fn activate(&mut self, button: Button) -> Action {
        if self.outputting || self.dragging() {
            return Action::None;
        }
        let action = button.action();
        if let Action::Tool(tool) = action {
            self.tool = if self.tool == Some(tool) {
                None
            } else {
                Some(tool)
            };
            self.cursor = None;
            return Action::Changed;
        }
        action
    }
    pub fn dragging(&self) -> bool {
        self.geometry.resizing() || self.gesture.is_some()
    }
    pub fn enabled(&self, button: Button) -> bool {
        !self.outputting
            && !self.dragging()
            && match button {
                Button::Undo => self.history.can_undo(),
                Button::Redo => self.history.can_redo(),
                _ => true,
            }
    }
    fn update_pending(&mut self) {
        self.pending = match self.gesture.as_ref() {
            Some(Gesture::Mosaic(s)) => Some(Annotation::Mosaic(s.clone())),
            Some(Gesture::Rectangle {
                start,
                end,
                clip,
                color,
                tolerance,
            }) => {
                if (i64::from(start.x) - i64::from(end.x)).abs() < i64::from(tolerance.0)
                    && (i64::from(start.y) - i64::from(end.y)).abs() < i64::from(tolerance.1)
                {
                    None
                } else {
                    PhysicalRect {
                        left: start.x.min(end.x),
                        right: start.x.max(end.x),
                        top: start.y.min(end.y),
                        bottom: start.y.max(end.y),
                    }
                    .intersection(*clip)
                    .map(|rect| Annotation::Rectangle {
                        rect,
                        clip: *clip,
                        color: *color,
                    })
                }
            }
            None => None,
        };
        self.preview_dirty = true;
    }
    pub fn cancel_drag(&mut self) {
        self.mosaic_processed = None;
        self.preview_pending_bounds = None;
        self.gesture = None;
        self.pending = None;
        if self.geometry.resizing() {
            self.restore_geometry();
        }
        self.note_preview_change(self.geometry.rect());
        self.preview = self.committed_preview.clone();
        self.preview_dirty = false;
    }
    fn restore_geometry(&mut self) {
        self.geometry =
            EditingSelection::new(self.history.snapshot().selection(), self.bounds).unwrap();
    }
    pub fn finish_selection(&mut self) -> Result<bool, ScreenshotError> {
        let result = self.history.set_selection(self.geometry.rect());
        if matches!(result, Ok(true)) {
            self.mosaic_processed = None;
            self.committed_preview = None;
            self.note_preview_change(self.geometry.rect());
            self.preview = None;
            self.preview_dirty = true;
        }
        result
    }
    pub fn finish_nudge(&mut self) -> Result<bool, ScreenshotError> {
        if !std::mem::replace(&mut self.nudge_active, false) {
            return Ok(false);
        }
        self.finish_selection()
    }
    pub fn refresh_preview(
        &mut self,
        frames: &[MonitorFrame],
        budget: &PixelBudget,
    ) -> Result<(), ScreenshotError> {
        if !self.preview_dirty {
            return Ok(());
        }
        let snapshot = self.history.snapshot();
        if self.committed_preview.is_none() || self.preview_rect != self.geometry.rect() {
            let image =
                snapshot.compose_preview(frames, budget, Some(self.geometry.rect()), None, None)?;
            self.mosaic_processed = None;
            self.committed_preview = Some(Arc::new(image));
            self.preview_rect = self.geometry.rect();
        }
        let image = if let Some(pending) = self.pending.as_ref() {
            if let (Annotation::Mosaic(stroke), Some(processed)) = (pending, self.mosaic_processed)
            {
                if let Some(mut previous) = self.preview.take() {
                    if let Some(output) = Arc::get_mut(&mut previous) {
                        let updated = crate::screenshot::annotations::apply_mosaic_incremental(
                            self.committed_preview.as_ref().unwrap(),
                            output,
                            self.geometry.rect(),
                            stroke,
                            processed,
                            budget,
                        );
                        self.preview = Some(previous);
                        self.mosaic_processed = Some(updated?);
                        if let Some(damage) = stroke.damage_since(processed) {
                            self.note_preview_change(damage);
                        }
                        self.preview_dirty = false;
                        return Ok(());
                    }
                    self.preview = Some(previous);
                }
            }
            crate::screenshot::annotations::compose_pending(
                self.committed_preview.as_ref().unwrap(),
                self.geometry.rect(),
                pending,
                budget,
            )?
        } else {
            self.note_preview_change(self.geometry.rect());
            self.preview = self.committed_preview.clone();
            self.preview_dirty = false;
            return Ok(());
        };
        let damage = match self.pending.as_ref() {
            Some(Annotation::Rectangle { rect, .. }) => match self.preview_pending_bounds {
                Some(old) => PhysicalRect {
                    left: old.left.min(rect.left),
                    top: old.top.min(rect.top),
                    right: old.right.max(rect.right),
                    bottom: old.bottom.max(rect.bottom),
                },
                None => *rect,
            },
            Some(Annotation::Mosaic(stroke)) => {
                stroke.damage_since(0).unwrap_or(self.geometry.rect())
            }
            _ => self.geometry.rect(),
        };
        self.note_preview_change(damage);
        self.preview_pending_bounds = match self.pending.as_ref() {
            Some(Annotation::Rectangle { rect, .. }) => Some(*rect),
            _ => None,
        };
        self.preview = Some(Arc::new(image));
        self.mosaic_processed = match self.pending.as_ref() {
            Some(Annotation::Mosaic(stroke)) => Some(stroke.point_count()),
            _ => None,
        };
        self.preview_rect = self.geometry.rect();
        if self.pending.is_none() {
            self.mosaic_processed = None;
            self.committed_preview = self.preview.clone();
        }
        self.preview_dirty = false;
        Ok(())
    }
    pub fn commit_pending(
        &mut self,
        frames: &[MonitorFrame],
        budget: &PixelBudget,
    ) -> Result<bool, ScreenshotError> {
        if self.pending.is_none() {
            return Ok(false);
        }
        if let Err(e) = self.refresh_preview(frames, budget) {
            self.cancel_drag();
            return Err(e);
        }
        let result = self.history.append(self.pending.take().unwrap());
        if result.is_ok() {
            self.error = None;
            self.mosaic_processed = None;
            self.committed_preview = self.preview.clone();
        } else {
            self.cancel_drag();
        }
        result
    }
    pub fn commit_text(
        &mut self,
        text: TextAnnotation,
        index: Option<usize>,
        frames: &[MonitorFrame],
        budget: &PixelBudget,
    ) -> Result<bool, ScreenshotError> {
        if text.content.trim().is_empty() {
            return Ok(false);
        }
        let snapshot = self.history.snapshot();
        let pending = Annotation::Text(text.clone());
        let image = snapshot.compose_preview(frames, budget, None, Some(&pending), index)?;
        let changed = if let Some(i) = index {
            self.history.replace_text(i, text)?
        } else {
            self.history.append(pending)?
        };
        if changed {
            self.error = None;
            self.note_preview_change(self.geometry.rect());
            self.preview = Some(Arc::new(image));
            self.mosaic_processed = None;
            self.committed_preview = self.preview.clone();
            self.preview_rect = self.geometry.rect();
            self.preview_dirty = false;
        }
        Ok(changed)
    }
    pub fn travel(
        &mut self,
        redo: bool,
        frames: &[MonitorFrame],
        budget: &PixelBudget,
    ) -> Result<bool, ScreenshotError> {
        self.finish_nudge()?;
        let changed = if redo {
            self.history.redo()
        } else {
            self.history.undo()
        };
        if !changed {
            return Ok(false);
        }
        let snapshot = self.history.snapshot();
        let image = if snapshot.annotations().is_empty() {
            Ok(None)
        } else {
            snapshot.compose(frames, budget).map(|i| Some(Arc::new(i)))
        };
        match image {
            Ok(image) => {
                self.restore_geometry();
                self.note_preview_change(self.geometry.rect());
                self.preview = image;
                self.mosaic_processed = None;
                self.committed_preview = self.preview.clone();
                self.preview_rect = self.geometry.rect();
                self.preview_dirty = false;
                Ok(true)
            }
            Err(e) => {
                if redo {
                    self.history.undo();
                } else {
                    self.history.redo();
                }
                Err(e)
            }
        }
    }
    pub fn begin_output(&mut self, action: Action) -> bool {
        if self.outputting || self.dragging() {
            return false;
        }
        self.outputting = true;
        self.output_action = Some(action);
        self.error = None;
        true
    }
    pub fn failed(&mut self, error: &str) {
        self.outputting = false;
        self.output_action = None;
        self.error = Some(match error {
            "SaveDirectoryUnavailable" | "SaveFailed" => "保存失败，请检查保存目录后重试",
            "ClipboardBusy" | "ClipboardWriteFailed" => "复制失败，请重试 Ctrl+C",
            "ResourceLimit" | "AllocationFailed" => "截图过大，请缩小选区后重试",
            "PinFailed" => "贴图失败，请重试",
            _ => "输出失败，请重试",
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn r(a: i32, b: i32, c: i32, d: i32) -> PhysicalRect {
        PhysicalRect {
            left: a,
            top: b,
            right: c,
            bottom: d,
        }
    }
    fn editor() -> Editor {
        Editor::new(r(20, 20, 80, 80), r(0, 0, 100, 100)).unwrap()
    }
    #[test]
    fn toolbar_keyboard_focus_skips_unavailable_history_and_space_activates_default_save() {
        let mut editor = editor();
        let initial = editor.focus;
        for _ in 0..9 {
            editor.key(9, false, false);
        }
        assert_ne!(
            editor.focus, initial,
            "four actions follow the eight handles"
        );
        for _ in 0..6 {
            editor.key(9, false, false);
        }
        assert_eq!(editor.focus, initial);
        assert_eq!(editor.key(32, false, false), Action::Save);
        assert!(!accepts_key(32, true), "held Space cannot repeat output");
    }
    #[test]
    fn toolbar_focus_hint_only_appears_after_keyboard_navigation() {
        let mut editor = editor();
        assert!(!editor.keyboard_focus);
        assert_eq!(editor.key(9, false, false), Action::Changed);
        assert!(editor.keyboard_focus);
        editor.focus_button(Button::Copy);
        assert!(
            editor.keyboard_focus,
            "native focus notification must retain keyboard mode"
        );
        editor.down(PhysicalPoint { x: 0, y: 0 }, &[(r(0, 0, 100, 100), 96)]);
        assert!(
            !editor.keyboard_focus,
            "mouse input clears the keyboard-only hint"
        );
    }
    #[test]
    fn every_toolbar_button_enter_and_space_route_to_its_own_action() {
        let mut editor = editor();
        for (button, action) in [
            (Button::Cancel, Action::Terminate),
            (Button::Pin, Action::Pin),
            (Button::Save, Action::Save),
            (Button::Copy, Action::Copy),
        ] {
            editor.focus_button(button);
            assert_eq!(editor.key(13, false, false), action);
            assert_eq!(editor.key(32, false, false), action);
        }
        editor.focus = Focus::Handle(Handle::East);
        assert_eq!(editor.key(13, false, false), Action::Save);
        assert_eq!(editor.key(32, false, false), Action::None);
        editor.focus_button(Button::Mosaic);
        editor.key(9, true, false);
        assert_eq!(editor.focus, Focus::Handle(Handle::West));
        editor.key(9, false, false);
        let enabled = Button::ALL
            .into_iter()
            .filter(|b| editor.enabled(*b))
            .collect::<Vec<_>>();
        for button in enabled {
            assert_eq!(editor.focus, Focus::Button(button));
            editor.key(9, false, false);
        }
        assert_eq!(editor.focus, Focus::Handle(Handle::NorthWest));
    }
    #[test]
    fn pin_creation_failure_retains_selection_and_can_retry() {
        let mut editor = editor();
        let rect = editor.geometry.rect();
        assert!(editor.begin_output(Action::Save));
        editor.failed("PinFailed");
        assert_eq!(editor.geometry.rect(), rect);
        assert_eq!(editor.error, Some("贴图失败，请重试"));
        assert!(editor.begin_output(Action::Save));
    }
    #[test]
    fn pending_output_retains_action_identity_for_status_feedback() {
        for action in [Action::Save, Action::Copy, Action::Pin] {
            let mut editor = editor();
            assert!(editor.begin_output(action));
            assert_eq!(editor.output_action, Some(action));
            assert!(!editor.begin_output(Action::Pin));
            editor.failed("PinFailed");
            assert_eq!(editor.output_action, None);
        }
    }
    #[test]
    fn deleted_or_unwritable_save_directory_keeps_selection_and_offers_retry() {
        for code in ["SaveDirectoryUnavailable", "SaveFailed"] {
            let mut editor = editor();
            let rect = editor.geometry.rect();
            assert!(editor.begin_output(Action::Save));
            editor.failed(code);
            assert_eq!(editor.geometry.rect(), rect);
            assert_eq!(editor.error, Some("保存失败，请检查保存目录后重试"));
            assert!(editor.begin_output(Action::Save));
        }
    }
    #[test]
    fn tab_visits_all_eight_handles_then_four_actions() {
        let mut editor = editor();
        editor.focus = Focus::Button(Button::Copy);
        for handle in Handle::ALL {
            assert_eq!(editor.key(9, false, false), Action::Changed);
            assert_eq!(editor.focus, Focus::Handle(handle));
        }
        editor.key(9, false, false);
        assert_eq!(editor.focus, Focus::Button(Button::Mosaic));
        editor.key(9, true, false);
        assert_eq!(editor.focus, Focus::Handle(Handle::West));
    }
    #[test]
    fn keyboard_nudges_focused_handle_and_shift_uses_ten_physical_pixels() {
        let mut editor = editor();
        editor.focus = Focus::Handle(Handle::East);
        assert_eq!(editor.key(39, true, false), Action::Changed);
        assert_eq!(editor.geometry.rect(), r(20, 20, 90, 80));
        editor.key(38, false, false);
        assert_eq!(editor.geometry.rect(), r(20, 20, 90, 80));
        assert_eq!(editor.key(13, false, false), Action::Save);
        assert_eq!(editor.key(67, false, true), Action::Copy);
    }
    #[test]
    fn ordinary_clicks_do_not_restart_selection_and_drag_blocks_output() {
        let mut editor = editor();
        let monitors = [(r(0, 0, 100, 100), 96)];
        assert!(!editor.down(PhysicalPoint { x: 50, y: 50 }, &monitors));
        assert!(!editor.down(PhysicalPoint { x: 1, y: 1 }, &monitors));
        assert!(editor.down(PhysicalPoint { x: 80, y: 50 }, &monitors));
        assert_eq!(editor.key(13, false, false), Action::None);
        assert!(!editor.begin_output(Action::Save));
        assert!(editor.up(PhysicalPoint { x: 90, y: 50 }));
        assert_eq!(editor.geometry.rect(), r(20, 20, 90, 80));
    }
    #[test]
    fn output_is_single_flight_and_failure_preserves_geometry_for_retry() {
        let mut editor = editor();
        let original = editor.geometry.rect();
        assert!(editor.begin_output(Action::Copy));
        assert!(!editor.begin_output(Action::Save));
        assert_eq!(editor.key(67, false, true), Action::None);
        assert_eq!(editor.key(27, false, false), Action::Cancel);
        editor.failed("ClipboardBusy");
        assert_eq!(editor.geometry.rect(), original);
        assert!(editor.error.is_some());
        assert!(!editor.outputting);
        assert!(editor.begin_output(Action::Save));
        assert!(editor.error.is_none());
    }
    #[test]
    fn native_save_focus_after_cancelled_click_updates_keyboard_target_without_output() {
        let mut editor = editor();
        editor.focus = Focus::Handle(Handle::East);
        let rect = editor.geometry.rect();
        editor.focus_button(Button::Save);
        assert_eq!(editor.focus, Focus::Button(Button::Save));
        assert!(!editor.outputting);
        editor.key(39, false, false);
        assert_eq!(editor.geometry.rect(), rect);
        editor.key(9, false, false);
        assert_eq!(editor.focus, Focus::Button(Button::Copy));
    }
    #[test]
    fn held_initial_confirmation_needs_release_before_editing_can_save_or_copy() {
        assert!(!accepts_key(13, true));
        assert!(!accepts_key(67, true));
        assert!(accepts_key(13, false));
        assert!(accepts_key(67, false));
        assert!(accepts_key(39, true));
    }
    #[test]
    fn zero_motion_edge_handle_click_preserves_selection_and_drag_still_reaches_bounds() {
        let bounds = r(0, 0, 400, 300);
        let rect = r(50, 50, 399, 299);
        let monitors = [(bounds, 96)];
        for down in [
            PhysicalPoint { x: 399, y: 174 },
            PhysicalPoint { x: 224, y: 299 },
            PhysicalPoint { x: 399, y: 299 },
        ] {
            let mut editor = Editor::new(rect, bounds).unwrap();
            assert!(editor.down(down, &monitors));
            let up = editor.pointer_point(down);
            assert!(editor.up(up));
            assert_eq!(
                editor.geometry.rect(),
                rect,
                "zero-motion native edge click must preserve {down:?}"
            );
        }
        let mut editor = Editor::new(r(50, 50, 200, 180), bounds).unwrap();
        assert!(editor.down(PhysicalPoint { x: 200, y: 115 }, &monitors));
        let up = editor.pointer_point(PhysicalPoint { x: 399, y: 115 });
        editor.up(up);
        assert_eq!(editor.geometry.rect(), r(50, 50, 400, 180));
    }
}

#[cfg(test)]
mod annotation_tests {
    use super::*;
    use crate::screenshot::{
        geometry::PixelBudget,
        model::{MonitorFrame, RgbaImage},
    };
    fn rect() -> PhysicalRect {
        PhysicalRect {
            left: 0,
            top: 0,
            right: 100,
            bottom: 100,
        }
    }
    fn point(x: i32, y: i32) -> PhysicalPoint {
        PhysicalPoint { x, y }
    }
    #[test]
    fn annotation_tools_toggle_and_pending_drag_blocks_switch_and_output() {
        let mut e = Editor::new(rect(), rect()).unwrap();
        assert_eq!(e.activate(Button::Rectangle), Action::Changed);
        assert_eq!(e.tool, Some(Tool::Rectangle));
        assert!(e.down(point(10, 10), &[(rect(), 96)]));
        assert_eq!(e.activate(Button::Mosaic), Action::None);
        assert!(!e.begin_output(Action::Save));
        e.cancel_drag();
        assert_eq!(e.activate(Button::Rectangle), Action::Changed);
        assert_eq!(e.tool, None);
        assert!(!e.history.can_undo());
    }
    #[test]
    fn jitter_ignored_and_rectangle_preview_is_exact_output_with_creation_clip() {
        let mut e = Editor::new(rect(), rect()).unwrap();
        e.activate(Button::Rectangle);
        e.down(point(20, 20), &[(rect(), 96)]);
        e.up(point(21, 21));
        assert!(e.pending.is_none());
        e.down(point(20, 20), &[(rect(), 96)]);
        e.up(point(150, 150));
        let b = PixelBudget::new(1000000);
        let frames = [MonitorFrame {
            rect: rect(),
            image: RgbaImage {
                width: 100,
                height: 100,
                pixels: vec![255; 40000],
            },
        }];
        assert!(e.commit_pending(&frames, &b).unwrap());
        let preview = e.preview.as_ref().unwrap().image().pixels.clone();
        assert_eq!(
            preview,
            e.history
                .snapshot()
                .compose(&frames, &b)
                .unwrap()
                .image()
                .pixels
        );
        assert!(e.history.can_undo());
        assert_eq!(e.history.snapshot().annotations()[0].clip(), rect());
    }
    #[test]
    fn render_budget_rejects_pending_preserving_document_and_redo() {
        let mut e = Editor::new(rect(), rect()).unwrap();
        e.activate(Button::Rectangle);
        e.down(point(10, 10), &[(rect(), 96)]);
        e.up(point(30, 30));
        let b = PixelBudget::new(1);
        assert!(e.commit_pending(&[], &b).is_err());
        assert!(!e.history.can_undo());
        assert!(e.history.snapshot().annotations().is_empty());
        assert!(e.pending.is_none());
    }
    #[test]
    fn selection_drag_and_arrow_sequence_are_each_one_history_step() {
        let bounds = PhysicalRect {
            left: 0,
            top: 0,
            right: 200,
            bottom: 200,
        };
        let mut e = Editor::new(rect(), bounds).unwrap();
        e.down(point(100, 50), &[(bounds, 96)]);
        e.moved(point(110, 50));
        e.up(point(120, 50));
        e.finish_selection().unwrap();
        assert_eq!(e.history.snapshot().selection().right, 120);
        e.focus = Focus::Handle(Handle::East);
        e.key(39, false, false);
        e.key(39, false, false);
        e.key(39, false, false);
        e.finish_nudge().unwrap();
        assert_eq!(e.history.snapshot().selection().right, 123);
        assert!(e.history.undo());
        assert_eq!(e.history.snapshot().selection().right, 120);
        assert!(e.history.undo());
        assert_eq!(e.history.snapshot().selection().right, 100);
    }
}

#[cfg(test)]
mod cache_and_focus_tests {
    use super::*;
    fn r() -> PhysicalRect {
        PhysicalRect {
            left: 0,
            top: 0,
            right: 100,
            bottom: 100,
        }
    }
    #[test]
    fn cursor_movement_keeps_cached_image_and_properties_are_keyboard_stops() {
        let mut e = Editor::new(r(), r()).unwrap();
        e.activate(Button::Mosaic);
        let frames = [MonitorFrame {
            rect: r(),
            image: crate::screenshot::model::RgbaImage {
                width: 100,
                height: 100,
                pixels: vec![255; 40000],
            },
        }];
        let b = PixelBudget::new(1000000);
        e.refresh_preview(&frames, &b).unwrap();
        let image = e.preview.as_ref().unwrap().clone();
        e.moved(PhysicalPoint { x: 30, y: 40 });
        e.refresh_preview(&frames, &b).unwrap();
        assert!(Arc::ptr_eq(&image, e.preview.as_ref().unwrap()));
        e.focus = Focus::Button(Button::Copy);
        e.key(9, false, false);
        assert_eq!(e.focus, Focus::Property(0));
        e.key(9, false, false);
        assert_eq!(e.focus, Focus::Property(1));
        e.key(9, true, false);
        assert_eq!(e.focus, Focus::Property(0));
    }
    #[test]
    fn failed_selection_render_preserves_history_and_geometry() {
        let mut e = Editor::new(
            r(),
            PhysicalRect {
                right: 200,
                bottom: 200,
                ..r()
            },
        )
        .unwrap();
        e.history
            .append(Annotation::Rectangle {
                rect: PhysicalRect {
                    right: 10,
                    bottom: 10,
                    ..r()
                },
                clip: r(),
                color: [1, 2, 3, 255],
            })
            .unwrap();
        e.focus = Focus::Handle(Handle::East);
        e.key(39, false, false);
        assert!(e
            .finish_selection_rendered(&[], &PixelBudget::new(1))
            .is_err());
        assert_eq!(e.geometry.rect(), r());
        assert_eq!(e.history.snapshot().selection(), r());
    }
}

#[cfg(test)]
mod availability_focus_tests {
    use super::*;
    #[test]
    fn tab_skips_initial_undo_redo_and_disabled_redo_after_new_edit() {
        let r = PhysicalRect {
            left: 0,
            top: 0,
            right: 100,
            bottom: 100,
        };
        let mut e = Editor::new(r, r).unwrap();
        e.focus = Focus::Button(Button::Rectangle);
        e.key(9, false, false);
        assert_eq!(e.focus, Focus::Button(Button::Cancel));
        e.key(9, true, false);
        assert_eq!(e.focus, Focus::Button(Button::Rectangle));
        e.history
            .append(Annotation::Rectangle {
                rect: PhysicalRect {
                    right: 10,
                    bottom: 10,
                    ..r
                },
                clip: r,
                color: [1, 2, 3, 255],
            })
            .unwrap();
        e.focus = Focus::Button(Button::Undo);
        e.key(9, false, false);
        assert_eq!(e.focus, Focus::Button(Button::Cancel));
        assert_eq!(e.key(13, false, false), Action::Terminate);
    }
}

#[cfg(test)]
mod review_regressions {
    use super::*;
    fn r() -> PhysicalRect {
        PhysicalRect {
            left: 0,
            top: 0,
            right: 200,
            bottom: 100,
        }
    }
    #[test]
    fn last_undo_and_redo_normalize_to_enabled_keyboard_target() {
        let mut e = Editor::new(r(), r()).unwrap();
        e.history
            .append(Annotation::Rectangle {
                rect: PhysicalRect {
                    right: 10,
                    bottom: 10,
                    ..r()
                },
                clip: r(),
                color: [1, 2, 3, 255],
            })
            .unwrap();
        e.focus = Focus::Button(Button::Undo);
        e.history.undo();
        assert!(e.normalize_focus());
        assert_eq!(e.focus, Focus::Button(Button::Redo));
        e.history.redo();
        assert!(e.normalize_focus());
        assert_eq!(e.focus, Focus::Button(Button::Cancel));
    }
    #[test]
    fn mixed_dpi_outline_uses_gesture_radius_then_pointer_dpi_after_release() {
        let mut e = Editor::new(r(), r()).unwrap();
        e.activate(Button::Mosaic);
        let monitors = [
            (PhysicalRect { right: 100, ..r() }, 96),
            (PhysicalRect { left: 100, ..r() }, 192),
        ];
        e.down(PhysicalPoint { x: 50, y: 50 }, &monitors);
        e.moved(PhysicalPoint { x: 150, y: 50 });
        assert_eq!(e.brush_radius(192), 24);
        e.up(PhysicalPoint { x: 150, y: 50 });
        assert_eq!(e.brush_radius(192), 48);
    }
    #[test]
    fn explicit_cancel_action_is_distinct_from_escape_draft_cancel() {
        let mut e = Editor::new(r(), r()).unwrap();
        assert_eq!(Button::Cancel.action(), Action::Terminate);
        assert_eq!(e.key(27, false, false), Action::Cancel);
    }
    #[test]
    fn mosaic_updates_reuse_pixels_but_advance_content_revision() {
        let rect = PhysicalRect {
            left: 0,
            top: 0,
            right: 100,
            bottom: 100,
        };
        let b = PixelBudget::new(1000000);
        let frames = [MonitorFrame {
            rect,
            image: crate::screenshot::model::RgbaImage {
                width: 100,
                height: 100,
                pixels: vec![255; 40000],
            },
        }];
        let mut e = Editor::new(rect, rect).unwrap();
        e.activate(Button::Mosaic);
        e.down(PhysicalPoint { x: 20, y: 20 }, &[(rect, 96)]);
        e.refresh_preview(&frames, &b).unwrap();
        let address = Arc::as_ptr(e.preview.as_ref().unwrap());
        let revision = e.preview_revision;
        e.moved(PhysicalPoint { x: 40, y: 35 });
        e.refresh_preview(&frames, &b).unwrap();
        assert_eq!(address, Arc::as_ptr(e.preview.as_ref().unwrap()));
        assert!(e.preview_revision > revision);
        let expected = crate::screenshot::annotations::compose_pending(
            e.committed_preview.as_ref().unwrap(),
            rect,
            e.pending.as_ref().unwrap(),
            &b,
        )
        .unwrap();
        assert_eq!(e.preview.as_ref().unwrap().image(), expected.image());
        let revision = e.preview_revision;
        e.cancel_drag();
        assert!(e.preview_revision > revision);
        let revision = e.preview_revision;
        e.moved(PhysicalPoint { x: 60, y: 60 });
        e.refresh_preview(&frames, &b).unwrap();
        assert_eq!(e.preview_revision, revision);
    }
    #[test]
    fn preview_damage_accumulates_until_matching_revision_is_acknowledged() {
        let rect = PhysicalRect {
            left: 0,
            top: 0,
            right: 100,
            bottom: 100,
        };
        let b = PixelBudget::new(1000000);
        let frames = [MonitorFrame {
            rect,
            image: crate::screenshot::model::RgbaImage {
                width: 100,
                height: 100,
                pixels: vec![255; 40000],
            },
        }];
        let mut e = Editor::new(rect, rect).unwrap();
        e.activate(Button::Mosaic);
        e.down(PhysicalPoint { x: 20, y: 20 }, &[(rect, 96)]);
        e.refresh_preview(&frames, &b).unwrap();
        e.acknowledge_preview(e.preview_revision);
        assert_eq!(e.preview_damage, None);
        e.moved(PhysicalPoint { x: 30, y: 20 });
        e.refresh_preview(&frames, &b).unwrap();
        let first = e.preview_damage.unwrap();
        let old_revision = e.preview_revision;
        e.moved(PhysicalPoint { x: 60, y: 40 });
        e.refresh_preview(&frames, &b).unwrap();
        let combined = e.preview_damage.unwrap();
        assert!(
            combined.left <= first.left
                && combined.top <= first.top
                && combined.right >= first.right
                && combined.bottom >= first.bottom
        );
        assert!(combined.right < rect.right);
        e.acknowledge_preview(old_revision);
        assert_eq!(e.preview_damage, Some(combined));
        e.acknowledge_preview(e.preview_revision);
        assert_eq!(e.preview_damage, None);
        let revision = e.preview_revision;
        e.moved(PhysicalPoint { x: 60, y: 40 });
        e.refresh_preview(&frames, &b).unwrap();
        assert_eq!(e.preview_revision, revision);
        assert_eq!(e.preview_damage, None);
    }
}
