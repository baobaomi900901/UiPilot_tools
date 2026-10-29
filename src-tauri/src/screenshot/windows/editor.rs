use crate::screenshot::{
    editing::{hit_handle_on_monitors, EditingSelection, Handle},
    model::{PhysicalPoint, PhysicalRect},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Focus {
    Handle(Handle),
    Button(Button),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Button {
    Cancel,
    Pin,
    Save,
    Copy,
}
impl Button {
    pub const ALL: [Self; 4] = [Self::Cancel, Self::Pin, Self::Save, Self::Copy];
    pub fn action(self) -> Action {
        match self {
            Self::Cancel => Action::Cancel,
            Self::Pin => Action::Pin,
            Self::Save => Action::Save,
            Self::Copy => Action::Copy,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Action {
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

pub(super) struct Editor {
    pub geometry: EditingSelection,
    bounds: PhysicalRect,
    pub focus: Focus,
    pub outputting: bool,
    pub output_action: Option<Action>,
    pub error: Option<&'static str>,
}

impl Editor {
    pub fn focus_button(&mut self, button: Button) {
        self.focus = Focus::Button(button);
    }
    pub fn new(rect: PhysicalRect, bounds: PhysicalRect) -> Option<Self> {
        EditingSelection::new(rect, bounds).map(|geometry| Self {
            geometry,
            bounds,
            focus: Focus::Button(Button::Save),
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
        if self.outputting {
            return false;
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
        let Some(handle) = self.geometry.drag_to(self.pointer_point(point)) else {
            return false;
        };
        self.focus = Focus::Handle(handle);
        true
    }
    pub fn up(&mut self, point: PhysicalPoint) -> bool {
        let changed = self.moved(point);
        self.geometry.end_resize();
        changed
    }
    /// Windows virtual-key values are kept here as integers so the complete
    /// keyboard contract can be tested without creating a desktop window.
    pub fn key(&mut self, key: u16, shift: bool, ctrl: bool) -> Action {
        if key == 27 {
            return Action::Cancel;
        }
        if self.outputting || self.geometry.resizing() {
            return Action::None;
        }
        match key {
            9 => {
                let index = match self.focus {
                    Focus::Button(button) => {
                        8 + Button::ALL.iter().position(|v| *v == button).unwrap()
                    }
                    Focus::Handle(h) => Handle::ALL.iter().position(|v| *v == h).unwrap(),
                };
                let next = (index + if shift { 11 } else { 1 }) % 12;
                self.focus = if next >= 8 {
                    Focus::Button(Button::ALL[next - 8])
                } else {
                    Focus::Handle(Handle::ALL[next])
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
            37..=40 => {
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
    pub fn begin_output(&mut self, action: Action) -> bool {
        if self.outputting || self.geometry.resizing() {
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
    fn toolbar_keyboard_focus_has_twelve_stops_and_space_activates_default_save() {
        let mut editor = editor();
        let initial = editor.focus;
        for _ in 0..9 {
            editor.key(9, false, false);
        }
        assert_ne!(
            editor.focus, initial,
            "four actions follow the eight handles"
        );
        for _ in 0..3 {
            editor.key(9, false, false);
        }
        assert_eq!(editor.focus, initial);
        assert_eq!(editor.key(32, false, false), Action::Save);
        assert!(!accepts_key(32, true), "held Space cannot repeat output");
    }
    #[test]
    fn every_toolbar_button_enter_and_space_route_to_its_own_action() {
        let mut editor = editor();
        for (button, action) in [
            (Button::Cancel, Action::Cancel),
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
        editor.focus_button(Button::Cancel);
        editor.key(9, true, false);
        assert_eq!(editor.focus, Focus::Handle(Handle::West));
        editor.key(9, false, false);
        for button in Button::ALL {
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
        assert_eq!(editor.focus, Focus::Button(Button::Cancel));
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
