//! Immutable physical-pixel annotations. History shares rasters; composition never changes frames.
use super::geometry::{self, BudgetedImage, PixelBudget};
use super::model::{MonitorFrame, PhysicalPoint, PhysicalRect, ScreenshotError};
use std::sync::Arc;

pub(crate) const MAX_HISTORY: usize = 256;
pub(crate) const MAX_ANNOTATIONS: usize = 256;
pub(crate) const MAX_STROKE_POINTS: usize = 16_384;
pub(crate) const MAX_TEXT_BYTES: usize = 65_536;

#[derive(Debug, Clone)]
pub(crate) struct TextAnnotation {
    pub origin: PhysicalPoint,
    pub content: String,
    pub font_size_dip: u32,
    pub color: [u8; 4],
    pub clip: PhysicalRect,
    /// Straight (not premultiplied) RGBA, shared by preview, history and output.
    pub raster: Arc<BudgetedImage>,
}
impl TextAnnotation {
    pub fn bounds(&self) -> Result<PhysicalRect, ScreenshotError> {
        Ok(PhysicalRect {
            left: self.origin.x,
            top: self.origin.y,
            right: self
                .origin
                .x
                .checked_add(
                    i32::try_from(self.raster.width())
                        .map_err(|_| ScreenshotError::InvalidGeometry)?,
                )
                .ok_or(ScreenshotError::InvalidGeometry)?,
            bottom: self
                .origin
                .y
                .checked_add(
                    i32::try_from(self.raster.height())
                        .map_err(|_| ScreenshotError::InvalidGeometry)?,
                )
                .ok_or(ScreenshotError::InvalidGeometry)?,
        })
    }
}
#[derive(Debug, Clone)]
pub(crate) struct MosaicStroke {
    pub clip: PhysicalRect,
    pub radius: u32,
    pub block_size: u32,
    points: Vec<Option<PhysicalPoint>>,
}
impl MosaicStroke {
    pub fn new(clip: PhysicalRect, radius: u32, block_size: u32) -> Result<Self, ScreenshotError> {
        clip.dimensions()?;
        if radius == 0 || radius > 1024 || block_size == 0 || block_size > 1024 {
            return Err(ScreenshotError::InvalidGeometry);
        }
        Ok(Self {
            clip,
            radius,
            block_size,
            points: Vec::new(),
        })
    }
    /// Outside samples insert a break; reentry cannot connect across the clipped area.
    pub fn push(&mut self, point: PhysicalPoint) -> Result<(), ScreenshotError> {
        let value = self.clip.contains(point).then_some(point);
        if self.points.last() == Some(&value) {
            return Ok(());
        }
        if self.points.len() >= MAX_STROKE_POINTS {
            return Err(ScreenshotError::ResourceLimit);
        }
        self.points
            .try_reserve(1)
            .map_err(|_| ScreenshotError::AllocationFailed)?;
        self.points.push(value);
        Ok(())
    }
    /// Grid-aligned bounds preserve the complete pre-stroke sample of edge cells.
    fn affected_bounds(&self, clip: PhysicalRect) -> Option<PhysicalRect> {
        let mut points = self.points.iter().flatten();
        let first = points.next()?;
        let (mut left, mut top, mut right, mut bottom) = (
            i64::from(first.x),
            i64::from(first.y),
            i64::from(first.x),
            i64::from(first.y),
        );
        for p in points {
            left = left.min(i64::from(p.x));
            top = top.min(i64::from(p.y));
            right = right.max(i64::from(p.x));
            bottom = bottom.max(i64::from(p.y));
        }
        let radius = i64::from(self.radius);
        let block = i64::from(self.block_size);
        left = (left - radius).div_euclid(block) * block;
        top = (top - radius).div_euclid(block) * block;
        right = (right + radius + block).div_euclid(block) * block;
        bottom = (bottom + radius + block).div_euclid(block) * block;
        let r = PhysicalRect {
            left: left.max(i64::from(clip.left)) as i32,
            top: top.max(i64::from(clip.top)) as i32,
            right: right.min(i64::from(clip.right)) as i32,
            bottom: bottom.min(i64::from(clip.bottom)) as i32,
        };
        r.dimensions().ok().map(|_| r)
    }
    fn mask(
        &self,
        clip: PhysicalRect,
        budget: &PixelBudget,
    ) -> Result<(Vec<u8>, super::geometry::BudgetReservation), ScreenshotError> {
        let (w, h) = clip.dimensions()?;
        let bytes = (w as usize)
            .checked_mul(h as usize)
            .ok_or(ScreenshotError::ResourceLimit)?;
        let lease = budget.reserve(bytes)?;
        let mut mask = Vec::new();
        mask.try_reserve_exact(bytes)
            .map_err(|_| ScreenshotError::AllocationFailed)?;
        mask.resize(bytes, 0);
        let mut previous = None;
        let radius = f64::from(self.radius);
        for sample in &self.points {
            let Some(point) = sample else {
                previous = None;
                continue;
            };
            let (bx, by) = (f64::from(point.x), f64::from(point.y));
            let (ax, ay) = previous.unwrap_or((bx, by));
            let (dx, dy) = (bx - ax, by - ay);
            let length = dx * dx + dy * dy;
            let top = ((ay.min(by) - radius).ceil() as i64).max(i64::from(clip.top));
            let bottom = ((ay.max(by) + radius).floor() as i64).min(i64::from(clip.bottom) - 1);
            for y in top..=bottom {
                // Only the narrow row interval of this capsule needs testing.
                let (t0, t1) = if dy == 0.0 {
                    (0.0, 1.0)
                } else {
                    let a = (y as f64 - radius - ay) / dy;
                    let b = (y as f64 + radius - ay) / dy;
                    (a.min(b).clamp(0.0, 1.0), a.max(b).clamp(0.0, 1.0))
                };
                let x0 = ax + t0 * dx;
                let x1 = ax + t1 * dx;
                let left = ((x0.min(x1) - radius).ceil() as i64).max(i64::from(clip.left));
                let right = ((x0.max(x1) + radius).floor() as i64).min(i64::from(clip.right) - 1);
                for x in left..=right {
                    let t = if length == 0.0 {
                        0.0
                    } else {
                        ((x as f64 - ax) * dx + (y as f64 - ay) * dy) / length
                    }
                    .clamp(0.0, 1.0);
                    if (x as f64 - ax - t * dx).powi(2) + (y as f64 - ay - t * dy).powi(2)
                        <= radius * radius
                    {
                        mask[(y - i64::from(clip.top)) as usize * w as usize
                            + (x - i64::from(clip.left)) as usize] = 1;
                    }
                }
            }
            previous = Some((bx, by));
        }
        Ok((mask, lease))
    }
}
#[derive(Debug, Clone)]
pub(crate) enum Annotation {
    Rectangle {
        rect: PhysicalRect,
        clip: PhysicalRect,
        color: [u8; 4],
    },
    Mosaic(MosaicStroke),
    Text(TextAnnotation),
}
impl Annotation {
    pub fn clip(&self) -> PhysicalRect {
        match self {
            Self::Rectangle { clip, .. } => *clip,
            Self::Mosaic(s) => s.clip,
            Self::Text(t) => t.clip,
        }
    }
    fn validate_stroke(s: &MosaicStroke) -> Result<(), ScreenshotError> {
        if s.radius == 0 || s.radius > 1024 || s.block_size == 0 || s.block_size > 1024 {
            return Err(ScreenshotError::InvalidGeometry);
        }
        Ok(())
    }
    fn valid(&self) -> Result<bool, ScreenshotError> {
        self.clip().dimensions()?;
        match self {
            Self::Rectangle { rect, clip, .. } => Ok(rect.intersection(*clip).is_some()),
            Self::Mosaic(s) => {
                Self::validate_stroke(s)?;
                Ok(s.points.iter().any(Option::is_some))
            }
            Self::Text(t) => {
                if t.content.len() > MAX_TEXT_BYTES || t.content.capacity() > MAX_TEXT_BYTES {
                    return Err(ScreenshotError::ResourceLimit);
                }
                if t.font_size_dip == 0 {
                    return Err(ScreenshotError::InvalidGeometry);
                }
                Ok(!t.content.trim().is_empty() && t.bounds()?.intersection(t.clip).is_some())
            }
        }
    }
}
#[derive(Debug, Clone)]
pub(crate) struct AnnotationSnapshot {
    selection: PhysicalRect,
    annotations: Arc<Vec<Arc<Annotation>>>,
}
impl AnnotationSnapshot {
    pub fn selection(&self) -> PhysicalRect {
        self.selection
    }
    pub fn annotations(&self) -> &[Arc<Annotation>] {
        &self.annotations
    }
    pub fn hit_text(&self, point: PhysicalPoint) -> Option<(usize, &TextAnnotation)> {
        if !self.selection.contains(point) {
            return None;
        }
        self.annotations
            .iter()
            .enumerate()
            .rev()
            .find_map(|(i, a)| match a.as_ref() {
                Annotation::Text(t)
                    if t.clip.contains(point) && t.bounds().ok()?.contains(point) =>
                {
                    Some((i, t))
                }
                _ => None,
            })
    }
    pub fn compose(
        &self,
        frames: &[MonitorFrame],
        budget: &PixelBudget,
    ) -> Result<BudgetedImage, ScreenshotError> {
        self.compose_preview(frames, budget, None, None, None)
    }
    /// A pending replacement retains its layer position. Pending additions are last.
    pub fn compose_preview(
        &self,
        frames: &[MonitorFrame],
        budget: &PixelBudget,
        selection: Option<PhysicalRect>,
        pending: Option<&Annotation>,
        replace_index: Option<usize>,
    ) -> Result<BudgetedImage, ScreenshotError> {
        let selection = selection.unwrap_or(self.selection);
        selection.dimensions()?;
        if replace_index.is_some_and(|i| i >= self.annotations.len()) {
            return Err(ScreenshotError::InvalidGeometry);
        }
        if let Some(a) = pending {
            a.valid()?;
        }
        let layers = self
            .annotations
            .iter()
            .enumerate()
            .filter_map(|(i, a)| {
                if Some(i) == replace_index {
                    pending
                } else {
                    Some(a.as_ref())
                }
            })
            .chain(pending.filter(|_| replace_index.is_none()));
        // Include creation clips, so shrinking the selection cannot change a mosaic's samples.
        let mut domain = selection;
        for a in layers.clone() {
            let c = a.clip();
            domain = PhysicalRect {
                left: domain.left.min(c.left),
                top: domain.top.min(c.top),
                right: domain.right.max(c.right),
                bottom: domain.bottom.max(c.bottom),
            };
        }
        let mut image = geometry::compose(frames, domain, budget)?;
        for a in layers {
            render(&mut image, domain, a, budget)?;
        }
        if domain == selection {
            return Ok(image);
        }
        let (w, h) = selection.dimensions()?;
        let mut out = BudgetedImage::allocate(w, h, budget)?;
        for y in selection.top..selection.bottom {
            let src = offset(domain, selection.left, y);
            let dest = offset(selection, selection.left, y);
            let n = w as usize * 4;
            out.pixels_mut()[dest..dest + n].copy_from_slice(&image.image().pixels[src..src + n]);
        }
        Ok(out)
    }
}
/// Hard caps bound metadata: 257 versions each contain at most 256 Arc handles;
/// unique annotations contain at most 16K points / 64KiB text. Rasters use PixelBudget.
#[derive(Debug)]
pub(crate) struct AnnotationHistory {
    versions: Vec<AnnotationSnapshot>,
    cursor: usize,
}
impl AnnotationHistory {
    pub fn new(selection: PhysicalRect) -> Result<Self, ScreenshotError> {
        selection.dimensions()?;
        Ok(Self {
            versions: vec![AnnotationSnapshot {
                selection,
                annotations: Arc::new(Vec::new()),
            }],
            cursor: 0,
        })
    }
    pub fn snapshot(&self) -> AnnotationSnapshot {
        self.versions[self.cursor].clone()
    }
    pub fn can_undo(&self) -> bool {
        self.cursor > 0
    }
    pub fn can_redo(&self) -> bool {
        self.cursor + 1 < self.versions.len()
    }
    pub fn undo(&mut self) -> bool {
        if !self.can_undo() {
            return false;
        }
        self.cursor -= 1;
        true
    }
    pub fn redo(&mut self) -> bool {
        if !self.can_redo() {
            return false;
        }
        self.cursor += 1;
        true
    }
    fn commit(&mut self, next: AnnotationSnapshot) -> Result<bool, ScreenshotError> {
        self.versions
            .try_reserve(1)
            .map_err(|_| ScreenshotError::AllocationFailed)?;
        self.versions.truncate(self.cursor + 1);
        self.versions.push(next);
        if self.versions.len() > MAX_HISTORY + 1 {
            self.versions.remove(0);
        }
        self.cursor = self.versions.len() - 1;
        Ok(true)
    }
    pub fn set_selection(&mut self, selection: PhysicalRect) -> Result<bool, ScreenshotError> {
        selection.dimensions()?;
        let mut s = self.snapshot();
        if s.selection == selection {
            return Ok(false);
        }
        s.selection = selection;
        self.commit(s)
    }
    pub fn append(&mut self, a: Annotation) -> Result<bool, ScreenshotError> {
        if !a.valid()? {
            return Ok(false);
        }
        let mut s = self.snapshot();
        if s.annotations.len() >= MAX_ANNOTATIONS {
            return Err(ScreenshotError::ResourceLimit);
        }
        let mut v = Vec::new();
        v.try_reserve_exact(s.annotations.len() + 1)
            .map_err(|_| ScreenshotError::AllocationFailed)?;
        v.extend(s.annotations.iter().cloned());
        v.push(Arc::new(a));
        s.annotations = Arc::new(v);
        self.commit(s)
    }
    pub fn replace_text(
        &mut self,
        index: usize,
        text: TextAnnotation,
    ) -> Result<bool, ScreenshotError> {
        let a = Annotation::Text(text);
        if !a.valid()? {
            return Ok(false);
        }
        let mut s = self.snapshot();
        let Some(old) = s.annotations.get(index) else {
            return Err(ScreenshotError::InvalidGeometry);
        };
        let (Annotation::Text(old), Annotation::Text(new)) = (old.as_ref(), &a) else {
            return Err(ScreenshotError::InvalidGeometry);
        };
        if old.origin == new.origin
            && old.content == new.content
            && old.font_size_dip == new.font_size_dip
            && old.color == new.color
            && old.clip == new.clip
            && old.raster.image() == new.raster.image()
        {
            return Ok(false);
        }
        let mut v = Vec::new();
        v.try_reserve_exact(s.annotations.len())
            .map_err(|_| ScreenshotError::AllocationFailed)?;
        v.extend(s.annotations.iter().cloned());
        v[index] = Arc::new(a);
        s.annotations = Arc::new(v);
        self.commit(s)
    }
}
/// Apply one new topmost annotation to a cached committed composite.
/// `base` must represent `domain`; replacements require full document replay.
/// Requiring the creation clip inside domain preserves complete mosaic cell samples.
pub(crate) fn compose_pending(
    base: &BudgetedImage,
    domain: PhysicalRect,
    pending: &Annotation,
    budget: &PixelBudget,
) -> Result<BudgetedImage, ScreenshotError> {
    let (width, height) = domain.dimensions()?;
    if (base.width(), base.height()) != (width, height) {
        return Err(ScreenshotError::InvalidGeometry);
    }
    let clip = pending.clip();
    if clip.intersection(domain) != Some(clip) || !pending.valid()? {
        return Err(ScreenshotError::InvalidGeometry);
    }
    let mut result = BudgetedImage::allocate(width, height, budget)?;
    result.pixels_mut().copy_from_slice(&base.image().pixels);
    render(&mut result, domain, pending, budget)?;
    Ok(result)
}

fn offset(rect: PhysicalRect, x: i32, y: i32) -> usize {
    ((i64::from(y) - i64::from(rect.top)) as usize
        * (i64::from(rect.right) - i64::from(rect.left)) as usize
        + (i64::from(x) - i64::from(rect.left)) as usize)
        * 4
}
fn render(
    image: &mut BudgetedImage,
    domain: PhysicalRect,
    a: &Annotation,
    budget: &PixelBudget,
) -> Result<(), ScreenshotError> {
    let Some(clip) = domain.intersection(a.clip()) else {
        return Ok(());
    };
    match a {
        Annotation::Rectangle { rect, color, .. } => {
            if let Some(r) = clip.intersection(*rect) {
                for y in r.top..r.bottom {
                    for x in r.left..r.right {
                        let i = offset(domain, x, y);
                        image.pixels_mut()[i..i + 4]
                            .copy_from_slice(&[color[0], color[1], color[2], 255]);
                    }
                }
            }
        }
        Annotation::Text(t) => {
            if let Some(r) = clip.intersection(t.bounds()?) {
                let bounds = t.bounds()?;
                for y in r.top..r.bottom {
                    for x in r.left..r.right {
                        let i = offset(domain, x, y);
                        let j = offset(bounds, x, y);
                        let src = &t.raster.image().pixels[j..j + 4];
                        let dest = &mut image.pixels_mut()[i..i + 4];
                        let alpha = u32::from(src[3]);
                        for c in 0..3 {
                            dest[c] = ((u32::from(src[c]) * alpha
                                + u32::from(dest[c]) * (255 - alpha)
                                + 127)
                                / 255) as u8;
                        }
                        dest[3] = 255;
                    }
                }
            }
        }
        Annotation::Mosaic(s) => {
            let Some(clip) = s.affected_bounds(clip) else {
                return Ok(());
            };
            let (mask, _lease) = s.mask(clip, budget)?;
            let mask_width = clip.dimensions()?.0 as usize;
            let block = i64::from(s.block_size);
            let mut cy = i64::from(clip.top).div_euclid(block) * block;
            while cy < i64::from(clip.bottom) {
                let top = cy.max(i64::from(clip.top)) as i32;
                let bottom = (cy + block).min(i64::from(clip.bottom)) as i32;
                let mut cx = i64::from(clip.left).div_euclid(block) * block;
                while cx < i64::from(clip.right) {
                    let left = cx.max(i64::from(clip.left)) as i32;
                    let right = (cx + block).min(i64::from(clip.right)) as i32;
                    let touched = (top..bottom).any(|y| {
                        let start =
                            (y - clip.top) as usize * mask_width + (left - clip.left) as usize;
                        mask[start..start + (right - left) as usize]
                            .iter()
                            .any(|v| *v != 0)
                    });
                    if !touched {
                        cx += block;
                        continue;
                    }
                    let mut sum = [0u64; 3];
                    let mut count = 0u64;
                    // Every disjoint grid cell is sampled before any pixel in it is changed.
                    for y in top..bottom {
                        for x in left..right {
                            let i = offset(domain, x, y);
                            for c in 0..3 {
                                sum[c] += u64::from(image.image().pixels[i + c]);
                            }
                            count += 1;
                        }
                    }
                    let color = [
                        (sum[0] / count) as u8,
                        (sum[1] / count) as u8,
                        (sum[2] / count) as u8,
                        255,
                    ];
                    for y in top..bottom {
                        for x in left..right {
                            if mask[(y - clip.top) as usize * mask_width + (x - clip.left) as usize]
                                != 0
                            {
                                let i = offset(domain, x, y);
                                image.pixels_mut()[i..i + 4].copy_from_slice(&color);
                            }
                        }
                    }
                    cx += block;
                }
                cy += block;
            }
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn r(l: i32, t: i32, rr: i32, b: i32) -> PhysicalRect {
        PhysicalRect {
            left: l,
            top: t,
            right: rr,
            bottom: b,
        }
    }
    fn p(x: i32, y: i32) -> PhysicalPoint {
        PhysicalPoint { x, y }
    }
    fn frame(rect: PhysicalRect) -> MonitorFrame {
        let (w, h) = rect.dimensions().unwrap();
        let mut pixels = Vec::new();
        for _y in rect.top..rect.bottom {
            for x in rect.left..rect.right {
                pixels.extend([x.rem_euclid(256) as u8, 0, 0, 255]);
            }
        }
        MonitorFrame {
            rect,
            image: super::super::model::RgbaImage {
                width: w,
                height: h,
                pixels,
            },
        }
    }
    fn red(rect: PhysicalRect, clip: PhysicalRect) -> Annotation {
        Annotation::Rectangle {
            rect,
            clip,
            color: [255, 0, 0, 1],
        }
    }
    fn pixel(image: &BudgetedImage, x: usize, y: usize) -> [u8; 4] {
        image.image().pixels[(y * image.width() as usize + x) * 4..][..4]
            .try_into()
            .unwrap()
    }
    #[test]
    fn rectangles_clip_and_selection_history_restores_hidden_pixels() {
        let b = PixelBudget::new(10000);
        let bounds = r(-4, 0, 8, 4);
        let mut h = AnnotationHistory::new(bounds).unwrap();
        assert!(h.append(red(r(-8, 0, 4, 4), bounds)).unwrap());
        h.set_selection(r(0, 0, 2, 2)).unwrap();
        h.undo();
        let image = h.snapshot().compose(&[frame(bounds)], &b).unwrap();
        assert_eq!(pixel(&image, 0, 0), [255, 0, 0, 255]);
        assert_eq!(pixel(&image, 8, 0), [4, 0, 0, 255]);
        h.redo();
        assert_eq!(h.snapshot().selection(), r(0, 0, 2, 2));
    }
    #[test]
    fn undo_branch_and_invalid_operations_are_atomic() {
        let rect = r(0, 0, 10, 10);
        let mut h = AnnotationHistory::new(rect).unwrap();
        assert!(!h.append(red(r(0, 0, 0, 1), rect)).unwrap());
        assert!(!h.can_undo());
        h.append(red(r(0, 0, 1, 1), rect)).unwrap();
        h.set_selection(r(1, 1, 9, 9)).unwrap();
        h.undo();
        h.append(red(r(2, 2, 3, 3), rect)).unwrap();
        assert!(!h.can_redo());
        let before = h.snapshot();
        assert!(h.set_selection(r(0, 0, 0, 0)).is_err());
        assert_eq!(h.snapshot().selection(), before.selection());
    }
    #[test]
    fn mosaic_is_continuous_breaks_outside_and_samples_prior_layers() {
        let rect = r(-8, 0, 24, 8);
        let b = PixelBudget::new(100000);
        let mut h = AnnotationHistory::new(rect).unwrap();
        h.append(red(r(-8, 0, 8, 8), rect)).unwrap();
        let mut s = MosaicStroke::new(rect, 1, 8).unwrap();
        s.push(p(-7, 2)).unwrap();
        s.push(p(6, 2)).unwrap();
        s.push(p(30, 2)).unwrap();
        s.push(p(22, 2)).unwrap();
        h.append(Annotation::Mosaic(s)).unwrap();
        let img = h.snapshot().compose(&[frame(rect)], &b).unwrap();
        assert_eq!(pixel(&img, 9, 2), [255, 0, 0, 255]);
        assert_eq!(pixel(&img, 20, 2), [12, 0, 0, 255]);
        assert_eq!(pixel(&img, 30, 2), [19, 0, 0, 255]);
    }
    #[test]
    fn text_alpha_replace_and_snapshot_release_budget() {
        let rect = r(0, 0, 3, 1);
        let b = PixelBudget::new(1000);
        let mut raster = BudgetedImage::allocate(1, 1, &b).unwrap();
        raster.pixels_mut().copy_from_slice(&[0, 200, 0, 128]);
        let text = TextAnnotation {
            origin: p(1, 0),
            content: "中文\n文字".into(),
            font_size_dip: 20,
            color: [0, 200, 0, 255],
            clip: rect,
            raster: Arc::new(raster),
        };
        let mut h = AnnotationHistory::new(rect).unwrap();
        h.append(Annotation::Text(text.clone())).unwrap();
        let snapshot = h.snapshot();
        assert_eq!(snapshot.hit_text(p(1, 0)).unwrap().0, 0);
        let img = snapshot.compose(&[frame(rect)], &b).unwrap();
        assert_eq!(pixel(&img, 1, 0), [0, 100, 0, 255]);
        drop(img);
        let mut replacement = text.clone();
        replacement.content = "修改".into();
        h.replace_text(0, replacement).unwrap();
        h.undo();
        assert_eq!(
            h.snapshot().hit_text(p(1, 0)).unwrap().1.content,
            "中文\n文字"
        );
        drop(h);
        drop(text);
        assert_eq!(b.used(), 4);
        drop(snapshot);
        assert_eq!(b.used(), 0);
    }
    #[test]
    fn preview_matches_commit_and_budget_failure_retains_state() {
        let rect = r(0, 0, 8, 8);
        let frames = [frame(rect)];
        let b = PixelBudget::new(10000);
        let mut h = AnnotationHistory::new(rect).unwrap();
        let a = red(r(1, 1, 5, 5), rect);
        let pre = h
            .snapshot()
            .compose_preview(&frames, &b, None, Some(&a), None)
            .unwrap();
        h.append(a).unwrap();
        let post = h.snapshot().compose(&frames, &b).unwrap();
        assert_eq!(pre.image(), post.image());
        let tiny = PixelBudget::new(1);
        assert!(h.snapshot().compose(&frames, &tiny).is_err());
        assert_eq!(tiny.used(), 0);
        assert!(h.can_undo());
    }
    #[test]
    fn exact_capsule_mask_matches_reference_without_point_times_pixel_scan() {
        let rect = r(-20, -20, 40, 40);
        let b = PixelBudget::new(10000);
        for end in [p(30, 30), p(-10, 30), p(30, -10), p(0, 0)] {
            let mut s = MosaicStroke::new(rect, 3, 8).unwrap();
            s.push(p(0, 0)).unwrap();
            s.push(end).unwrap();
            let (mask, lease) = s.mask(rect, &b).unwrap();
            for y in rect.top..rect.bottom {
                for x in rect.left..rect.right {
                    let dx = f64::from(end.x);
                    let dy = f64::from(end.y);
                    let n = dx * dx + dy * dy;
                    let t = if n == 0.0 {
                        0.0
                    } else {
                        (f64::from(x) * dx + f64::from(y) * dy) / n
                    }
                    .clamp(0.0, 1.0);
                    let expected =
                        (f64::from(x) - t * dx).powi(2) + (f64::from(y) - t * dy).powi(2) <= 9.0;
                    assert_eq!(
                        mask[(y - rect.top) as usize * 60 + (x - rect.left) as usize] != 0,
                        expected,
                        "{x},{y} -> {end:?}"
                    );
                }
            }
            assert_eq!(b.used(), 3600);
            drop(mask);
            drop(lease);
            assert_eq!(b.used(), 0);
        }
    }
    #[test]
    fn mosaic_fixed_negative_grid_is_stable_when_crop_changes_and_revisits() {
        let rect = r(-7, 0, 9, 4);
        let b = PixelBudget::new(100000);
        let frames = [frame(rect)];
        let mut h = AnnotationHistory::new(rect).unwrap();
        let mut s = MosaicStroke::new(rect, 3, 4).unwrap();
        s.push(p(-3, 1)).unwrap();
        s.push(p(6, 1)).unwrap();
        s.push(p(-3, 1)).unwrap();
        h.append(Annotation::Mosaic(s)).unwrap();
        let full = h.snapshot().compose(&frames, &b).unwrap();
        h.set_selection(r(-3, 0, 3, 3)).unwrap();
        let crop = h.snapshot().compose(&frames, &b).unwrap();
        for y in 0..3 {
            for x in 0..6 {
                assert_eq!(pixel(&crop, x, y), pixel(&full, x + 4, y));
            }
        }
        assert_eq!(pixel(&full, 4, 1), [253, 0, 0, 255]);
    }
    #[test]
    fn bounded_history_and_failed_mask_allocation_release_working_image() {
        let rect = r(0, 0, 4, 4);
        let mut h = AnnotationHistory::new(rect).unwrap();
        for i in 0..600 {
            h.set_selection(r(0, 0, 4 + (i % 2), 4)).unwrap();
        }
        let mut count = 0;
        while h.undo() {
            count += 1;
        }
        assert_eq!(count, MAX_HISTORY);
        let mut h = AnnotationHistory::new(rect).unwrap();
        let mut s = MosaicStroke::new(rect, 2, 4).unwrap();
        s.push(p(1, 1)).unwrap();
        h.append(Annotation::Mosaic(s)).unwrap();
        let b = PixelBudget::new(64);
        assert_eq!(
            h.snapshot().compose(&[frame(rect)], &b).unwrap_err(),
            ScreenshotError::ResourceLimit
        );
        assert_eq!(b.used(), 0);
        assert_eq!(h.snapshot().annotations().len(), 1);
    }
    #[test]
    fn small_stroke_only_reserves_affected_grid_and_keeps_full_boundary_cell_mean() {
        let rect = r(-7, 0, 93, 100);
        let b = PixelBudget::new(40_064);
        let frames = [frame(rect)];
        let mut h = AnnotationHistory::new(rect).unwrap();
        let mut s = MosaicStroke::new(rect, 1, 4).unwrap();
        s.push(p(-6, 2)).unwrap();
        h.append(Annotation::Mosaic(s)).unwrap();
        let full = h.snapshot().compose(&frames, &b).unwrap();
        assert_eq!(pixel(&full, 1, 2), [250, 0, 0, 255]); // clipped cell [-7,-4) averages 249,250,251
        assert_eq!(pixel(&full, 3, 2), [252, 0, 0, 255]); // untouched neighboring cell
        drop(full);
        h.set_selection(r(-6, 1, -4, 4)).unwrap();
        let crop = h.snapshot().compose(&frames, &b).unwrap();
        assert_eq!(pixel(&crop, 0, 1), [250, 0, 0, 255]);
    }
    #[test]
    fn cached_pending_matches_full_append_and_rejects_mismatched_domains() {
        let rect = r(-7, 0, 9, 8);
        let frames = [frame(rect)];
        let b = PixelBudget::new(100000);
        let mut h = AnnotationHistory::new(rect).unwrap();
        h.append(red(r(-7, 0, 0, 8), rect)).unwrap();
        let mut s = MosaicStroke::new(rect, 2, 4).unwrap();
        s.push(p(-5, 3)).unwrap();
        s.push(p(5, 3)).unwrap();
        let mut raster = BudgetedImage::allocate(1, 1, &b).unwrap();
        raster.pixels_mut().copy_from_slice(&[0, 200, 0, 128]);
        let text = TextAnnotation {
            origin: p(1, 1),
            content: "字".into(),
            font_size_dip: 20,
            color: [0, 200, 0, 255],
            clip: rect,
            raster: Arc::new(raster),
        };
        for a in [
            red(r(-1, 1, 4, 4), rect),
            Annotation::Mosaic(s),
            Annotation::Text(text),
        ] {
            let base = h.snapshot().compose(&frames, &b).unwrap();
            let before = base.image().pixels.clone();
            let cached = compose_pending(&base, rect, &a, &b).unwrap();
            h.append(a).unwrap();
            let full = h.snapshot().compose(&frames, &b).unwrap();
            assert_eq!(cached.image(), full.image());
            assert_eq!(base.image().pixels, before);
        }
        let base = h.snapshot().compose(&frames, &b).unwrap();
        assert!(compose_pending(&base, r(-7, 0, 10, 8), &red(rect, rect), &b).is_err());
        assert!(compose_pending(&base, rect, &red(rect, r(-8, 0, 9, 8)), &b).is_err());
        let tiny = PixelBudget::new(1);
        assert!(compose_pending(&base, rect, &red(rect, rect), &tiny).is_err());
        assert_eq!(tiny.used(), 0);
    }
}
