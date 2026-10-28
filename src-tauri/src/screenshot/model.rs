use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScreenshotMode {
    Region,
    Window,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PhysicalPoint {
    pub x: i32,
    pub y: i32,
}

/// A half-open rectangle in signed physical desktop pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysicalRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[derive(Debug, PartialEq, Eq)]
pub struct RgbaImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

#[derive(Debug)]
pub struct MonitorFrame {
    pub rect: PhysicalRect,
    pub image: RgbaImage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowCandidate {
    pub id: isize,
    pub rect: PhysicalRect,
    pub z_order: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenshotError {
    InvalidGeometry,
    InvalidPixels,
    ResourceLimit,
    AllocationFailed,
    EncodingFailed,
    ClipboardBusy,
    ClipboardWriteFailed,
    SaveFailed,
}
impl std::fmt::Display for ScreenshotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for ScreenshotError {}
