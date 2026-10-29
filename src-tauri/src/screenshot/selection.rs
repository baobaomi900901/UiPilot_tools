use super::model::*;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionInput {
    Move(PhysicalPoint),
    PointerDown(PhysicalPoint),
    PointerUp(PhysicalPoint),
    Arrow { direction: Direction, shift: bool },
    Space,
    Enter,
    Escape,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionOutcome {
    Continue,
    Selected(PhysicalRect),
    Cancelled,
}
#[derive(Debug)]
pub struct SelectionState {
    pub mode: ScreenshotMode,
    pub cursor: PhysicalPoint,
    pub anchor: Option<PhysicalPoint>,
    candidates: Vec<WindowCandidate>,
    selected: Option<usize>,
    candidate: Option<PhysicalRect>,
    locked: Option<PhysicalRect>,
    dragging: bool,
    pointer_pressed: bool,
    tolerance: (i32, i32),
}
impl SelectionState {
    pub fn new(
        mode: ScreenshotMode,
        cursor: PhysicalPoint,
        mut candidates: Vec<WindowCandidate>,
    ) -> Self {
        candidates.retain(|c| c.rect.dimensions().is_ok());
        candidates.sort_by_key(|c| c.z_order);
        let selected = candidates.iter().position(|c| c.rect.contains(cursor));
        Self {
            mode,
            cursor,
            anchor: None,
            candidates,
            selected,
            candidate: None,
            locked: None,
            dragging: false,
            pointer_pressed: false,
            tolerance: (4, 4),
        }
    }
    pub fn refresh_windows(&mut self, candidates: Vec<WindowCandidate>) {
        let selected_id = self.selected_window().map(|w| w.id);
        self.candidates = candidates;
        self.candidates.retain(|c| c.rect.dimensions().is_ok());
        self.candidates.sort_by_key(|c| c.z_order);
        self.selected = if self.mode == ScreenshotMode::Window {
            selected_id
                .and_then(|id| self.candidates.iter().position(|w| w.id == id))
                .or_else(|| {
                    self.candidates
                        .iter()
                        .position(|w| w.rect.contains(self.cursor))
                })
        } else {
            self.candidates
                .iter()
                .position(|w| w.rect.contains(self.cursor))
        };
    }
    /// SM_CXDRAG/SM_CYDRAG specify pixels on either side of the down point.
    /// Keep the boundary inclusive; crossing beyond it permanently starts drag.
    pub fn observe_motion(&mut self, dx: i64, dy: i64) {
        let (x, y) = (i64::from(self.tolerance.0), i64::from(self.tolerance.1));
        if self.pointer_pressed && (dx < -x || dx > x || dy < -y || dy > y) {
            self.dragging = true;
        }
    }
    pub fn set_drag_tolerance(&mut self, width: i32, height: i32) {
        self.tolerance = (width.max(1), height.max(1));
    }
    pub fn set_candidate(&mut self, point: PhysicalPoint, rect: Option<PhysicalRect>) {
        if self.hovering() && self.cursor == point {
            self.candidate = rect.filter(|r| r.dimensions().is_ok() && r.contains(point));
        }
    }
    pub fn hovering(&self) -> bool {
        self.mode == ScreenshotMode::Region && self.anchor.is_none()
    }
    pub fn highlight(&self) -> Option<PhysicalRect> {
        if self.mode == ScreenshotMode::Window {
            return self.selected_window().map(|c| c.rect);
        }
        if self.pointer_pressed && !self.dragging {
            self.locked
        } else if self.anchor.is_some() {
            self.region()
        } else {
            self.candidate
        }
    }
    pub fn selected_window(&self) -> Option<&WindowCandidate> {
        self.selected.and_then(|i| self.candidates.get(i))
    }
    pub fn region(&self) -> Option<PhysicalRect> {
        self.anchor
            .map(|a| PhysicalRect::from_points(a, self.cursor))
            .filter(|r| r.dimensions().is_ok())
    }
    fn update_pointer(&mut self, p: PhysicalPoint) {
        if self.cursor != p && self.hovering() {
            self.candidate = None;
        }
        self.cursor = p;
        if self.pointer_pressed {
            if let Some(a) = self.anchor {
                self.observe_motion(
                    i64::from(p.x) - i64::from(a.x),
                    i64::from(p.y) - i64::from(a.y),
                );
            }
        }
        self.selected = self.candidates.iter().position(|c| c.rect.contains(p));
    }
    pub fn reduce(&mut self, input: SelectionInput) -> SelectionOutcome {
        match input {
            SelectionInput::Escape => return SelectionOutcome::Cancelled,
            SelectionInput::Move(p) => self.update_pointer(p),
            SelectionInput::PointerDown(p) => {
                // Lock only the candidate actually displayed at mouse-down.
                let locked = if self.cursor == p {
                    self.candidate
                } else {
                    None
                };
                self.update_pointer(p);
                if self.mode == ScreenshotMode::Region {
                    self.anchor = Some(p);
                    self.locked = locked;
                    self.pointer_pressed = true;
                    self.dragging = false;
                    self.candidate = None;
                }
            }
            SelectionInput::PointerUp(p) => {
                self.update_pointer(p);
                if self.mode == ScreenshotMode::Region && self.pointer_pressed {
                    let rect = if self.dragging {
                        self.region()
                    } else {
                        self.locked
                    };
                    self.pointer_pressed = false;
                    self.anchor = None;
                    self.locked = None;
                    self.dragging = false;
                    return rect
                        .map(SelectionOutcome::Selected)
                        .unwrap_or(SelectionOutcome::Continue);
                }
                if self.mode == ScreenshotMode::Region {
                    return SelectionOutcome::Continue;
                }
                return self.commit();
            }
            SelectionInput::Space => {
                if self.mode == ScreenshotMode::Region {
                    self.anchor = Some(self.cursor);
                    self.pointer_pressed = false;
                    self.dragging = true;
                    self.candidate = None;
                }
            }
            SelectionInput::Enter => return self.commit(),
            SelectionInput::Arrow { direction, shift } => {
                if self.mode == ScreenshotMode::Window {
                    let len = self.candidates.len();
                    if len > 0 {
                        let forward = matches!(direction, Direction::Down | Direction::Right);
                        self.selected = Some(match self.selected {
                            None => 0,
                            Some(i) => {
                                if forward {
                                    (i + 1) % len
                                } else {
                                    (i + len - 1) % len
                                }
                            }
                        });
                    }
                } else {
                    let step = if shift { 10 } else { 1 };
                    let mut point = self.cursor;
                    match direction {
                        Direction::Left => point.x = point.x.saturating_sub(step),
                        Direction::Right => point.x = point.x.saturating_add(step),
                        Direction::Up => point.y = point.y.saturating_sub(step),
                        Direction::Down => point.y = point.y.saturating_add(step),
                    }
                    self.update_pointer(point);
                }
            }
        }
        SelectionOutcome::Continue
    }
    fn commit(&self) -> SelectionOutcome {
        match self.mode {
            ScreenshotMode::Region => self.region().or_else(|| {
                if self.hovering() {
                    self.candidate
                } else {
                    None
                }
            }),
            ScreenshotMode::Window => self.selected_window().map(|c| c.rect),
        }
        .map(SelectionOutcome::Selected)
        .unwrap_or(SelectionOutcome::Continue)
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_refresh_never_commits_old_moved_or_resized_rectangle() {
        for mode in [ScreenshotMode::Window, ScreenshotMode::Region] {
            let old = WindowCandidate {
                id: 42,
                rect: PhysicalRect {
                    left: 0,
                    top: 0,
                    right: 100,
                    bottom: 100,
                },
                z_order: 0,
            };
            let moved = WindowCandidate {
                id: 42,
                rect: PhysicalRect {
                    left: 10,
                    top: 10,
                    right: 200,
                    bottom: 200,
                },
                z_order: 0,
            };
            let mut s = SelectionState::new(mode, PhysicalPoint { x: 20, y: 20 }, vec![old]);
            s.refresh_windows(vec![moved.clone()]);
            assert_eq!(s.selected_window().unwrap().rect, moved.rect);
            if mode == ScreenshotMode::Region {
                s.set_candidate(s.cursor, Some(moved.rect));
            }
            assert_eq!(
                s.reduce(SelectionInput::Enter),
                SelectionOutcome::Selected(moved.rect)
            );
            s.refresh_windows(vec![]);
            assert!(s.selected_window().is_none());
        }
    }
    #[test]
    fn keyboard_region_uses_physical_steps_and_space_anchor() {
        let mut state = SelectionState::new(
            ScreenshotMode::Region,
            PhysicalPoint { x: -10, y: -20 },
            vec![],
        );
        assert_eq!(
            state.reduce(SelectionInput::Enter),
            SelectionOutcome::Continue
        );
        state.reduce(SelectionInput::Space);
        state.reduce(SelectionInput::Arrow {
            direction: Direction::Right,
            shift: true,
        });
        state.reduce(SelectionInput::Arrow {
            direction: Direction::Down,
            shift: false,
        });
        assert_eq!(
            state.reduce(SelectionInput::Enter),
            SelectionOutcome::Selected(PhysicalRect {
                left: -10,
                top: -20,
                right: 0,
                bottom: -19
            })
        );
        assert_eq!(
            state.reduce(SelectionInput::Escape),
            SelectionOutcome::Cancelled
        );
    }
    #[test]
    fn windows_sort_by_z_order_hit_and_keyboard_cycle() {
        let rect = PhysicalRect {
            left: 0,
            top: 0,
            right: 10,
            bottom: 10,
        };
        let mut state = SelectionState::new(
            ScreenshotMode::Window,
            PhysicalPoint { x: 1, y: 1 },
            vec![
                WindowCandidate {
                    id: 2,
                    rect,
                    z_order: 5,
                },
                WindowCandidate {
                    id: 1,
                    rect,
                    z_order: 0,
                },
            ],
        );
        assert_eq!(state.selected_window().unwrap().id, 1);
        state.reduce(SelectionInput::Arrow {
            direction: Direction::Down,
            shift: false,
        });
        assert_eq!(state.selected_window().unwrap().id, 2);
        state.reduce(SelectionInput::Move(PhysicalPoint { x: 10, y: 1 }));
        assert_eq!(
            state.reduce(SelectionInput::Enter),
            SelectionOutcome::Continue
        );
    }
}

#[cfg(test)]
mod gesture_tests {
    use super::*;
    #[test]
    fn mouse_empty_drag_can_be_reselected_and_keys_move_backwards() {
        let mut state =
            SelectionState::new(ScreenshotMode::Region, PhysicalPoint { x: 0, y: 0 }, vec![]);
        state.reduce(SelectionInput::PointerDown(PhysicalPoint { x: 2, y: 2 }));
        assert_eq!(
            state.reduce(SelectionInput::PointerUp(PhysicalPoint { x: 2, y: 9 })),
            SelectionOutcome::Continue
        );
        state.reduce(SelectionInput::PointerDown(PhysicalPoint { x: 9, y: 9 }));
        assert_eq!(
            state.reduce(SelectionInput::PointerUp(PhysicalPoint { x: -1, y: -1 })),
            SelectionOutcome::Selected(PhysicalRect {
                left: -1,
                top: -1,
                right: 9,
                bottom: 9
            })
        );
        state.reduce(SelectionInput::Space);
        state.reduce(SelectionInput::Arrow {
            direction: Direction::Left,
            shift: false,
        });
        state.reduce(SelectionInput::Arrow {
            direction: Direction::Up,
            shift: true,
        });
        assert_eq!(
            state.reduce(SelectionInput::Enter),
            SelectionOutcome::Selected(PhysicalRect {
                left: -2,
                top: -11,
                right: -1,
                bottom: -1
            })
        );
    }
}

#[cfg(test)]
mod smart_tests {
    use super::*;
    fn p(x: i32, y: i32) -> PhysicalPoint {
        PhysicalPoint { x, y }
    }
    fn r() -> PhysicalRect {
        PhysicalRect {
            left: -10,
            top: -10,
            right: 20,
            bottom: 20,
        }
    }
    #[test]
    fn jitter_commits_locked_candidate_not_late_reply() {
        let mut s = SelectionState::new(ScreenshotMode::Region, p(0, 0), vec![]);
        s.set_candidate(p(0, 0), Some(r()));
        s.reduce(SelectionInput::PointerDown(p(0, 0)));
        s.set_candidate(p(0, 0), None);
        assert_eq!(
            s.reduce(SelectionInput::PointerUp(p(1, 1))),
            SelectionOutcome::Selected(r())
        );
    }
    #[test]
    fn exceeded_drag_never_returns_to_click_and_empty_resumes() {
        let mut s = SelectionState::new(ScreenshotMode::Region, p(0, 0), vec![]);
        s.set_candidate(p(0, 0), Some(r()));
        s.reduce(SelectionInput::PointerDown(p(0, 0)));
        s.reduce(SelectionInput::Move(p(20, 20)));
        assert_eq!(
            s.reduce(SelectionInput::PointerUp(p(0, 0))),
            SelectionOutcome::Continue
        );
        assert!(s.anchor.is_none());
        assert!(s.hovering());
    }
    #[test]
    fn cursor_move_and_no_hit_clear_old_candidate() {
        let mut s = SelectionState::new(ScreenshotMode::Region, p(0, 0), vec![]);
        s.set_candidate(p(0, 0), Some(r()));
        s.reduce(SelectionInput::Move(p(-1, -1)));
        s.set_candidate(p(0, 0), Some(r()));
        assert!(s.highlight().is_none());
        s.set_candidate(p(-1, -1), Some(r()));
        s.set_candidate(p(-1, -1), None);
        assert!(s.highlight().is_none());
    }
}

#[cfg(test)]
mod frozen_motion_tests {
    use super::*;
    #[test]
    fn excursion_observed_while_frame_capture_blocks_still_becomes_drag() {
        let p = PhysicalPoint { x: -20, y: -20 };
        let mut s = SelectionState::new(ScreenshotMode::Region, p, vec![]);
        let rect = PhysicalRect {
            left: -30,
            top: -30,
            right: 0,
            bottom: 0,
        };
        s.set_candidate(p, Some(rect));
        s.reduce(SelectionInput::PointerDown(p));
        s.observe_motion(20, 20);
        assert_eq!(
            s.reduce(SelectionInput::PointerUp(p)),
            SelectionOutcome::Continue
        );
    }
    #[test]
    fn keyboard_hover_move_clears_candidate() {
        let p = PhysicalPoint { x: 0, y: 0 };
        let mut s = SelectionState::new(ScreenshotMode::Region, p, vec![]);
        s.set_candidate(
            p,
            Some(PhysicalRect {
                left: -10,
                top: -10,
                right: 10,
                bottom: 10,
            }),
        );
        s.reduce(SelectionInput::Arrow {
            direction: Direction::Right,
            shift: false,
        });
        assert_eq!(s.reduce(SelectionInput::Enter), SelectionOutcome::Continue);
    }
}

#[cfg(test)]
mod capture_input_tests {
    use super::*;
    #[test]
    fn orphan_release_cannot_submit_hover_candidate() {
        let p = PhysicalPoint { x: 0, y: 0 };
        let mut s = SelectionState::new(ScreenshotMode::Region, p, vec![]);
        s.set_candidate(
            p,
            Some(PhysicalRect {
                left: -10,
                top: -10,
                right: 10,
                bottom: 10,
            }),
        );
        assert_eq!(
            s.reduce(SelectionInput::PointerUp(p)),
            SelectionOutcome::Continue
        );
    }
    #[test]
    fn physical_tolerance_is_inclusive_and_click_keeps_down_candidate() {
        let p = PhysicalPoint { x: -20, y: -20 };
        let rect = PhysicalRect {
            left: -30,
            top: -30,
            right: 0,
            bottom: 0,
        };
        let mut s = SelectionState::new(ScreenshotMode::Region, p, vec![]);
        s.set_drag_tolerance(6, 10);
        s.set_candidate(p, Some(rect));
        s.reduce(SelectionInput::PointerDown(p));
        assert_eq!(
            s.reduce(SelectionInput::PointerUp(PhysicalPoint { x: -23, y: -25 })),
            SelectionOutcome::Selected(rect)
        );
    }
}

#[cfg(test)]
mod system_tolerance_tests {
    use super::*;
    #[test]
    fn system_drag_metric_is_inclusive_distance_on_either_side() {
        let p = PhysicalPoint { x: 0, y: 0 };
        let r = PhysicalRect {
            left: -20,
            top: -20,
            right: 20,
            bottom: 20,
        };
        for (dx, dy, click) in [
            (6, 0, true),
            (-6, 0, true),
            (0, 10, true),
            (0, -10, true),
            (7, 0, false),
            (-7, 0, false),
            (0, 11, false),
            (0, -11, false),
        ] {
            let mut s = SelectionState::new(ScreenshotMode::Region, p, vec![]);
            s.set_drag_tolerance(6, 10);
            s.set_candidate(p, Some(r));
            s.reduce(SelectionInput::PointerDown(p));
            s.observe_motion(dx, dy);
            assert_eq!(
                s.reduce(SelectionInput::PointerUp(p)),
                if click {
                    SelectionOutcome::Selected(r)
                } else {
                    SelectionOutcome::Continue
                },
                "excursion {dx},{dy}"
            );
        }
    }
}
