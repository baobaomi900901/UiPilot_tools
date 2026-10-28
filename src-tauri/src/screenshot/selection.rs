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
        self.cursor = p;
        self.selected = self.candidates.iter().position(|c| c.rect.contains(p));
    }
    pub fn reduce(&mut self, input: SelectionInput) -> SelectionOutcome {
        match input {
            SelectionInput::Escape => return SelectionOutcome::Cancelled,
            SelectionInput::Move(p) => self.update_pointer(p),
            SelectionInput::PointerDown(p) => {
                self.update_pointer(p);
                if self.mode == ScreenshotMode::Region {
                    self.anchor = Some(p);
                }
            }
            SelectionInput::PointerUp(p) => {
                self.update_pointer(p);
                return self.commit();
            }
            SelectionInput::Space => {
                if self.mode == ScreenshotMode::Region {
                    self.anchor = Some(self.cursor);
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
                    match direction {
                        Direction::Left => self.cursor.x = self.cursor.x.saturating_sub(step),
                        Direction::Right => self.cursor.x = self.cursor.x.saturating_add(step),
                        Direction::Up => self.cursor.y = self.cursor.y.saturating_sub(step),
                        Direction::Down => self.cursor.y = self.cursor.y.saturating_add(step),
                    }
                }
            }
        }
        SelectionOutcome::Continue
    }
    fn commit(&self) -> SelectionOutcome {
        match self.mode {
            ScreenshotMode::Region => self.region(),
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
