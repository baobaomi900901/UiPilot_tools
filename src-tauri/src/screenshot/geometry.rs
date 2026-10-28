use super::model::*;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

impl PhysicalRect {
    pub fn from_points(a: PhysicalPoint, b: PhysicalPoint) -> Self {
        Self {
            left: a.x.min(b.x),
            top: a.y.min(b.y),
            right: a.x.max(b.x),
            bottom: a.y.max(b.y),
        }
    }
    pub fn dimensions(self) -> Result<(u32, u32), ScreenshotError> {
        let width = self
            .right
            .checked_sub(self.left)
            .filter(|v| *v > 0)
            .ok_or(ScreenshotError::InvalidGeometry)?;
        let height = self
            .bottom
            .checked_sub(self.top)
            .filter(|v| *v > 0)
            .ok_or(ScreenshotError::InvalidGeometry)?;
        Ok((width as u32, height as u32))
    }
    pub fn intersection(self, other: Self) -> Option<Self> {
        let rect = Self {
            left: self.left.max(other.left),
            top: self.top.max(other.top),
            right: self.right.min(other.right),
            bottom: self.bottom.min(other.bottom),
        };
        rect.dimensions().ok().map(|_| rect)
    }
    pub fn contains(self, p: PhysicalPoint) -> bool {
        p.x >= self.left && p.x < self.right && p.y >= self.top && p.y < self.bottom
    }
}

pub fn rgba_len(width: u32, height: u32) -> Result<usize, ScreenshotError> {
    if width == 0 || height == 0 {
        return Err(ScreenshotError::InvalidGeometry);
    }
    (width as usize)
        .checked_mul(height as usize)
        .and_then(|v| v.checked_mul(4))
        .filter(|v| *v <= isize::MAX as usize)
        .ok_or(ScreenshotError::ResourceLimit)
}

pub const DEFAULT_PIXEL_BUDGET: usize = 512 * 1024 * 1024;
#[derive(Debug, Clone)]
pub struct PixelBudget {
    limit: usize,
    used: Arc<AtomicUsize>,
}
impl Default for PixelBudget {
    fn default() -> Self {
        Self::new(DEFAULT_PIXEL_BUDGET)
    }
}
impl PixelBudget {
    pub fn new(limit: usize) -> Self {
        Self {
            limit,
            used: Arc::new(AtomicUsize::new(0)),
        }
    }
    pub fn used(&self) -> usize {
        self.used.load(Ordering::Acquire)
    }
    pub fn reserve(&self, bytes: usize) -> Result<BudgetReservation, ScreenshotError> {
        self.used
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                current
                    .checked_add(bytes)
                    .filter(|total| *total <= self.limit)
            })
            .map_err(|_| ScreenshotError::ResourceLimit)?;
        Ok(BudgetReservation {
            used: Arc::clone(&self.used),
            bytes,
        })
    }
}
#[derive(Debug)]
pub struct BudgetReservation {
    used: Arc<AtomicUsize>,
    bytes: usize,
}
impl Drop for BudgetReservation {
    fn drop(&mut self) {
        self.used.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}
#[derive(Debug)]
pub struct BudgetedImage {
    image: RgbaImage,
    _reservation: BudgetReservation,
}
impl BudgetedImage {
    /// Borrowing keeps the allocation and its lease bound to this owner.
    pub fn image(&self) -> &RgbaImage {
        &self.image
    }
    /// Fixed-length storage may be filled, but never resized or moved out.
    pub fn pixels_mut(&mut self) -> &mut [u8] {
        &mut self.image.pixels
    }
    pub fn width(&self) -> u32 {
        self.image.width
    }
    pub fn height(&self) -> u32 {
        self.image.height
    }
    pub fn allocate(
        width: u32,
        height: u32,
        budget: &PixelBudget,
    ) -> Result<Self, ScreenshotError> {
        let bytes = rgba_len(width, height)?;
        let reservation = budget.reserve(bytes)?;
        let mut pixels = Vec::new();
        pixels
            .try_reserve_exact(bytes)
            .map_err(|_| ScreenshotError::AllocationFailed)?;
        pixels.resize(bytes, 0);
        Ok(Self {
            image: RgbaImage {
                width,
                height,
                pixels,
            },
            _reservation: reservation,
        })
    }
}

/// Frozen frames carry their aggregate allocation leases until the capture is dropped.
#[derive(Debug)]
pub struct FrameSet {
    frames: Vec<MonitorFrame>,
    _reservations: Vec<BudgetReservation>,
}
impl FrameSet {
    pub fn new(images: Vec<(PhysicalRect, BudgetedImage)>) -> Result<Self, ScreenshotError> {
        let mut frames = Vec::new();
        let mut reservations = Vec::new();
        frames
            .try_reserve_exact(images.len())
            .map_err(|_| ScreenshotError::AllocationFailed)?;
        reservations
            .try_reserve_exact(images.len())
            .map_err(|_| ScreenshotError::AllocationFailed)?;
        for (rect, owned) in images {
            if rect.dimensions()? != (owned.image.width, owned.image.height)
                || owned.image.pixels.len() != rgba_len(owned.image.width, owned.image.height)?
            {
                return Err(ScreenshotError::InvalidPixels);
            }
            frames.push(MonitorFrame {
                rect,
                image: owned.image,
            });
            reservations.push(owned._reservation);
        }
        Ok(Self {
            frames,
            _reservations: reservations,
        })
    }
    pub fn frames(&self) -> &[MonitorFrame] {
        &self.frames
    }
}
fn pixel_offset(rect: PhysicalRect, x: i32, y: i32, width: u32) -> Result<usize, ScreenshotError> {
    let x = x
        .checked_sub(rect.left)
        .and_then(|v| usize::try_from(v).ok())
        .ok_or(ScreenshotError::InvalidGeometry)?;
    let y = y
        .checked_sub(rect.top)
        .and_then(|v| usize::try_from(v).ok())
        .ok_or(ScreenshotError::InvalidGeometry)?;
    y.checked_mul(width as usize)
        .and_then(|v| v.checked_add(x))
        .and_then(|v| v.checked_mul(4))
        .ok_or(ScreenshotError::InvalidGeometry)
}

/// Frames must retain their budget reservations for the full capture lifetime.
pub fn compose(
    frames: &[MonitorFrame],
    selection: PhysicalRect,
    budget: &PixelBudget,
) -> Result<BudgetedImage, ScreenshotError> {
    for frame in frames {
        if frame.rect.dimensions()? != (frame.image.width, frame.image.height)
            || frame.image.pixels.len() != rgba_len(frame.image.width, frame.image.height)?
        {
            return Err(ScreenshotError::InvalidPixels);
        }
    }
    let (width, height) = selection.dimensions()?;
    let mut output = BudgetedImage::allocate(width, height, budget)?;
    for pixel in output.image.pixels.chunks_exact_mut(4) {
        pixel[3] = 255;
    }
    for frame in frames {
        let Some(overlap) = selection.intersection(frame.rect) else {
            continue;
        };
        let (row_width, _) = overlap.dimensions()?;
        let bytes = (row_width as usize)
            .checked_mul(4)
            .ok_or(ScreenshotError::InvalidGeometry)?;
        for y in overlap.top..overlap.bottom {
            let src = pixel_offset(frame.rect, overlap.left, y, frame.image.width)?;
            let dest = pixel_offset(selection, overlap.left, y, width)?;
            let src_end = src
                .checked_add(bytes)
                .ok_or(ScreenshotError::InvalidGeometry)?;
            let dest_end = dest
                .checked_add(bytes)
                .ok_or(ScreenshotError::InvalidGeometry)?;
            let source = frame
                .image
                .pixels
                .get(src..src_end)
                .ok_or(ScreenshotError::InvalidPixels)?;
            output
                .image
                .pixels
                .get_mut(dest..dest_end)
                .ok_or(ScreenshotError::InvalidPixels)?
                .copy_from_slice(source);
        }
    }
    Ok(output)
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directions_normalize_and_empty_does_not_commit() {
        let expected = PhysicalRect {
            left: -3,
            top: -2,
            right: 7,
            bottom: 8,
        };
        for (a, b) in [
            ((-3, -2), (7, 8)),
            ((7, -2), (-3, 8)),
            ((-3, 8), (7, -2)),
            ((7, 8), (-3, -2)),
        ] {
            assert_eq!(
                PhysicalRect::from_points(
                    PhysicalPoint { x: a.0, y: a.1 },
                    PhysicalPoint { x: b.0, y: b.1 }
                ),
                expected
            );
        }
        assert!(PhysicalRect::from_points(
            PhysicalPoint { x: 0, y: 0 },
            PhysicalPoint { x: 0, y: 4 }
        )
        .dimensions()
        .is_err());
    }
    #[test]
    fn physical_frames_preserve_pixels_and_black_gaps() {
        let frames = vec![
            MonitorFrame {
                rect: PhysicalRect {
                    left: -2,
                    top: 0,
                    right: 0,
                    bottom: 1,
                },
                image: RgbaImage {
                    width: 2,
                    height: 1,
                    pixels: vec![255, 0, 0, 255, 1, 2, 3, 255],
                },
            },
            MonitorFrame {
                rect: PhysicalRect {
                    left: 1,
                    top: 0,
                    right: 2,
                    bottom: 1,
                },
                image: RgbaImage {
                    width: 1,
                    height: 1,
                    pixels: vec![0, 255, 0, 255],
                },
            },
        ];
        let budget = PixelBudget::new(32);
        let image = compose(
            &frames,
            PhysicalRect {
                left: -2,
                top: 0,
                right: 2,
                bottom: 1,
            },
            &budget,
        )
        .unwrap();
        assert_eq!(
            image.image.pixels,
            vec![255, 0, 0, 255, 1, 2, 3, 255, 0, 0, 0, 255, 0, 255, 0, 255]
        );
        assert_eq!(budget.used(), 16);
        drop(image);
        assert_eq!(budget.used(), 0);
    }
    #[test]
    fn checked_overflow_and_retained_peak_are_rejected() {
        assert!(rgba_len(u32::MAX, u32::MAX).is_err());
        assert!(PhysicalRect {
            left: i32::MIN,
            top: 0,
            right: i32::MAX,
            bottom: 1
        }
        .dimensions()
        .is_err());
        let budget = PixelBudget::new(24);
        let old = budget.reserve(16).unwrap();
        assert!(budget.reserve(12).is_err());
        assert_eq!(budget.used(), 16);
        drop(old);
        assert!(budget.reserve(24).is_ok());
    }
}

#[cfg(test)]
mod ownership_tests {
    use super::*;
    #[test]
    fn frozen_frame_reservation_counts_against_new_result() {
        let budget = PixelBudget::new(12);
        let frame = BudgetedImage::allocate(2, 1, &budget).unwrap();
        let frames = FrameSet::new(vec![(
            PhysicalRect {
                left: 0,
                top: 0,
                right: 2,
                bottom: 1,
            },
            frame,
        )])
        .unwrap();
        assert!(compose(
            frames.frames(),
            PhysicalRect {
                left: 0,
                top: 0,
                right: 2,
                bottom: 1
            },
            &budget
        )
        .is_err());
        drop(frames);
        assert_eq!(budget.used(), 0);
    }
    #[test]
    fn malformed_source_rejected_before_output_allocation() {
        let budget = PixelBudget::new(100);
        let frames = [MonitorFrame {
            rect: PhysicalRect {
                left: 0,
                top: 0,
                right: 2,
                bottom: 1,
            },
            image: RgbaImage {
                width: 2,
                height: 1,
                pixels: vec![0; 4],
            },
        }];
        assert_eq!(
            compose(&frames, frames[0].rect, &budget).unwrap_err(),
            ScreenshotError::InvalidPixels
        );
        assert_eq!(budget.used(), 0);
    }
}

#[cfg(test)]
mod borrowing_tests {
    use super::*;
    #[test]
    fn borrowed_pixels_keep_fixed_storage_and_live_reservation() {
        let budget = PixelBudget::new(8);
        let mut owner = BudgetedImage::allocate(2, 1, &budget).unwrap();
        owner
            .pixels_mut()
            .copy_from_slice(&[1, 2, 3, 255, 4, 5, 6, 255]);
        assert_eq!((owner.width(), owner.height()), (2, 1));
        let image = owner.image();
        assert_eq!(image.pixels, &[1, 2, 3, 255, 4, 5, 6, 255]);
        assert_eq!(budget.used(), 8);
        assert!(budget.reserve(1).is_err());
        drop(owner);
        assert_eq!(budget.used(), 0);
    }
}
