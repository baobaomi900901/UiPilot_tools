use crate::public_plugins::PluginRequestContext;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};

pub(crate) const MAX_ENCODED: usize = 8 * 1024 * 1024;
pub(crate) const MAX_PNG: usize = 3 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ImageDescriptor {
    pub(crate) id: String,
    pub(crate) mime_type: String,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) byte_length: usize,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LauncherImage {
    #[serde(flatten)]
    pub(crate) descriptor: ImageDescriptor,
    pub(crate) preview_data_url: String,
    #[serde(skip)]
    pub(crate) png: Arc<Vec<u8>>,
}
impl std::ops::Deref for LauncherImage {
    type Target = ImageDescriptor;
    fn deref(&self) -> &ImageDescriptor {
        &self.descriptor
    }
}
#[derive(Default)]
struct ImageState {
    sequence: u64,
    pending: Option<u64>,
    current: Option<LauncherImage>,
    bindings: HashMap<PluginRequestContext, LauncherImage>,
    leases: Vec<crate::public_plugins::result_action::ResultActionLease>,
    claimed: HashSet<PluginRequestContext>,
    intent_sequence: u64,
    intent_submit: bool,
}
#[derive(Default)]
pub(crate) struct LauncherImageStore {
    state: Mutex<ImageState>,
    ids: AtomicU64,
}
impl LauncherImageStore {
    pub(crate) fn begin_import(&self, sequence: u64) -> Result<(), &'static str> {
        let mut state = self.state.lock().map_err(|_| "imageUnavailable")?;
        if sequence == 0 || sequence <= state.sequence {
            return Err("expiredImage");
        }
        state.sequence = sequence;
        state.pending = Some(sequence);
        Ok(())
    }
    pub(crate) fn finish_import(
        &self,
        sequence: u64,
        image: LauncherImage,
    ) -> Result<LauncherImage, &'static str> {
        self.finish_import_with(sequence, image, |_| {})
    }
    pub(crate) fn finish_import_with(
        &self,
        sequence: u64,
        mut image: LauncherImage,
        retire: impl FnOnce(Vec<PluginRequestContext>),
    ) -> Result<LauncherImage, &'static str> {
        let mut state = self.state.lock().map_err(|_| "imageUnavailable")?;
        if state.pending != Some(sequence) {
            return Err("expiredImage");
        }
        state.pending = None;
        let number = self
            .ids
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| "imageUnavailable")?;
        image.descriptor.id = format!("launcher-image-{number:016x}");
        state.current = Some(image.clone());
        state.intent_sequence = 0;
        state.intent_submit = false;
        state.claimed.clear();
        for lease in state.leases.drain(..) {
            lease.revoke();
        }
        retire(state.bindings.drain().map(|(context, _)| context).collect());
        Ok(image)
    }
    pub(crate) fn current(&self, id: &str) -> Result<LauncherImage, &'static str> {
        self.state
            .lock()
            .map_err(|_| "imageUnavailable")?
            .current
            .as_ref()
            .filter(|i| i.id == id)
            .cloned()
            .ok_or("expiredImage")
    }
    pub(crate) fn clear(&self, sequence: u64) -> Result<(), &'static str> {
        self.clear_with(sequence, |_| {})
    }
    pub(crate) fn clear_with(
        &self,
        sequence: u64,
        retire: impl FnOnce(Vec<PluginRequestContext>),
    ) -> Result<(), &'static str> {
        let mut state = self.state.lock().map_err(|_| "imageUnavailable")?;
        if sequence < state.sequence {
            return Err("expiredImage");
        }
        state.sequence = sequence;
        state.pending = None;
        state.current = None;
        state.claimed.clear();
        for lease in state.leases.drain(..) {
            lease.revoke();
        }
        retire(state.bindings.drain().map(|(context, _)| context).collect());
        Ok(())
    }
    pub(crate) fn invalidate(&self) -> Vec<PluginRequestContext> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.pending = None;
        state.current = None;
        state.claimed.clear();
        for lease in state.leases.drain(..) {
            lease.revoke();
        }
        state.bindings.drain().map(|(context, _)| context).collect()
    }
    pub(crate) fn retire_bindings(&self) -> Vec<PluginRequestContext> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.claimed.clear();
        for lease in state.leases.drain(..) {
            lease.revoke();
        }
        state.bindings.drain().map(|(context, _)| context).collect()
    }
    pub(crate) fn bind(
        &self,
        id: &str,
        context: &PluginRequestContext,
        lease: crate::public_plugins::result_action::ResultActionLease,
    ) -> Result<(), &'static str> {
        let mut state = self.state.lock().map_err(|_| "imageUnavailable")?;
        Self::bind_locked(&mut state, id, context, lease)
    }
    pub(crate) fn admit_intent<T>(
        &self,
        id: &str,
        sequence: u64,
        submit: bool,
        admit_query: impl FnOnce() -> Option<T>,
        retire: impl FnOnce(Vec<PluginRequestContext>),
    ) -> Result<Option<T>, &'static str> {
        let mut state = self.state.lock().map_err(|_| "imageUnavailable")?;
        if !state.current.as_ref().is_some_and(|image| image.id == id) {
            return Err("expiredImage");
        }
        if sequence <= state.intent_sequence {
            return Ok(None);
        }
        // Registry admission and cancellation share the image lock. A stale window or
        // query cannot cancel newer work, and later binding must prove the same intent.
        let Some(token) = admit_query() else {
            return Ok(None);
        };
        state.intent_sequence = sequence;
        state.intent_submit = submit;
        state.claimed.clear();
        for lease in state.leases.drain(..) {
            lease.revoke();
        }
        retire(state.bindings.drain().map(|(context, _)| context).collect());
        Ok(Some(token))
    }
    pub(crate) fn bind_intent(
        &self,
        id: &str,
        sequence: u64,
        context: &PluginRequestContext,
        lease: crate::public_plugins::result_action::ResultActionLease,
    ) -> Result<(), &'static str> {
        let mut state = self.state.lock().map_err(|_| "imageUnavailable")?;
        if state.intent_sequence != sequence || !state.intent_submit {
            return Err("expiredRequest");
        }
        Self::bind_locked(&mut state, id, context, lease)
    }
    fn bind_locked(
        state: &mut ImageState,
        id: &str,
        context: &PluginRequestContext,
        lease: crate::public_plugins::result_action::ResultActionLease,
    ) -> Result<(), &'static str> {
        let mut image = state
            .current
            .as_ref()
            .filter(|image| image.id == id)
            .cloned()
            .ok_or("expiredImage")?;
        if state.bindings.len() >= 4096 && !state.bindings.contains_key(context) {
            return Err("imageUnavailable");
        }
        // The runtime reference is distinct from the launcher handle and scoped to one request.
        image.descriptor.id = format!(
            "{}-{}-{}",
            image.id, context.plugin_generation, context.request_id
        );
        state
            .bindings
            .retain(|old, _| old.plugin_id != context.plugin_id);
        state
            .claimed
            .retain(|old| old.plugin_id != context.plugin_id);
        state.bindings.insert(context.clone(), image);
        // Scheduler publications are shared by one plugin generation. Retain only live
        // generations, and deduplicate repeated requests from that generation.
        state.leases.retain(|old| old.live());
        if !state.leases.contains(&lease) {
            state.leases.push(lease);
        }
        Ok(())
    }
    pub(crate) fn bound(
        &self,
        context: &PluginRequestContext,
        id: Option<&str>,
    ) -> Result<LauncherImage, &'static str> {
        self.state
            .lock()
            .map_err(|_| "imageUnavailable")?
            .bindings
            .get(context)
            .filter(|image| id.is_none_or(|id| image.id == id))
            .cloned()
            .ok_or("expiredRequest")
    }
    pub(crate) fn plugin_context(&self, plugin: &str) -> Option<PluginRequestContext> {
        self.state
            .lock()
            .ok()?
            .bindings
            .keys()
            .find(|context| context.plugin_id == plugin)
            .cloned()
    }
    pub(crate) fn claim(
        &self,
        context: &PluginRequestContext,
        id: &str,
    ) -> Result<LauncherImage, &'static str> {
        let mut state = self.state.lock().map_err(|_| "imageUnavailable")?;
        let image = state
            .bindings
            .get(context)
            .filter(|image| image.id == id)
            .cloned()
            .ok_or("expiredRequest")?;
        if !state.claimed.insert(context.clone()) {
            return Err("serviceAlreadyUsed");
        }
        Ok(image)
    }
}

fn dimensions(width: u32, height: u32) -> Result<usize, &'static str> {
    if width == 0 || height == 0 {
        return Err("invalidImage");
    }
    if width > 4096 || height > 4096 || u64::from(width) * u64::from(height) > 16_000_000 {
        return Err("imageTooLarge");
    }
    Ok(width as usize * height as usize * 4)
}
pub(crate) fn normalize_rgba(
    width: u32,
    height: u32,
    rgba: &[u8],
) -> Result<LauncherImage, &'static str> {
    if dimensions(width, height)? != rgba.len() {
        return Err("invalidImage");
    }
    let mut png = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|_| "invalidImage")?;
        writer.write_image_data(rgba).map_err(|_| "invalidImage")?;
    }
    if png.len() > MAX_PNG {
        return Err("imageTooLarge");
    }
    // Derived display-only PNG: at most 512px and 40KiB; OCR keeps the original PNG.
    let mut edge = 512u32;
    let preview = loop {
        let ratio = (width.max(height) as f64 / f64::from(edge)).max(1.0);
        let (pw, ph) = ((width as f64 / ratio).max(1.0) as u32, (height as f64 / ratio).max(1.0) as u32);
        let mut pixels = Vec::with_capacity(pw as usize * ph as usize * 4);
        for y in 0..ph {
            for x in 0..pw {
                let offset = (((y * height / ph) * width + x * width / pw) * 4) as usize;
                pixels.extend_from_slice(&rgba[offset..offset + 4]);
            }
        }
        let mut preview = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut preview, pw, ph);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.write_header().map_err(|_| "invalidImage")?
                .write_image_data(&pixels).map_err(|_| "invalidImage")?;
        }
        if preview.len() <= 40 * 1024 { break preview; }
        edge /= 2;
        if edge == 0 { return Err("imageTooLarge"); }
    };
    Ok(LauncherImage {
        descriptor: ImageDescriptor {
            id: String::new(),
            mime_type: "image/png".into(),
            width,
            height,
            byte_length: png.len(),
        },
        preview_data_url: format!("data:image/png;base64,{}", base64(&preview)),
        png: Arc::new(png),
    })
}
pub(crate) fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for group in bytes.chunks(3) {
        let value = (u32::from(group[0]) << 16)
            | (u32::from(*group.get(1).unwrap_or(&0)) << 8)
            | u32::from(*group.get(2).unwrap_or(&0));
        result.push(ALPHABET[((value >> 18) & 63) as usize] as char);
        result.push(ALPHABET[((value >> 12) & 63) as usize] as char);
        result.push(if group.len() > 1 {
            ALPHABET[((value >> 6) & 63) as usize] as char
        } else {
            '='
        });
        result.push(if group.len() > 2 {
            ALPHABET[(value & 63) as usize] as char
        } else {
            '='
        });
    }
    result
}
pub(crate) fn normalize_encoded(bytes: &[u8], mime: &str) -> Result<LauncherImage, &'static str> {
    if bytes.is_empty() || bytes.len() > MAX_ENCODED {
        return Err("imageTooLarge");
    }
    let signature_matches = match mime {
        "image/png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/jpeg" => bytes.starts_with(&[255, 216, 255]),
        "image/bmp" => bytes.starts_with(b"BM"),
        _ => false,
    };
    if !signature_matches {
        return Err("invalidImage");
    }
    decode_wic(bytes)
}
fn decode_wic(bytes: &[u8]) -> Result<LauncherImage, &'static str> {
    use windows::{
        core::Interface,
        Win32::{
            Graphics::Imaging::{
                CLSID_WICImagingFactory, GUID_WICPixelFormat32bppRGBA, IWICImagingFactory,
                WICBitmapDitherTypeNone, WICBitmapPaletteTypeCustom,
                WICDecodeMetadataCacheOnDemand,
            },
            System::Com::{
                CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
                COINIT_APARTMENTTHREADED,
            },
        },
    };
    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe { CoUninitialize() };
        }
    }
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }
        .ok()
        .map_err(|_| "invalidImage")?;
    let _apartment = Apartment;
    let result = (|| -> windows::core::Result<_> {
        let factory: IWICImagingFactory =
            unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER) }?;
        let stream = unsafe { factory.CreateStream() }?;
        unsafe {
            (Interface::vtable(&stream).InitializeFromMemory)(
                Interface::as_raw(&stream),
                bytes.as_ptr(),
                bytes.len() as u32,
            )
        }
        .ok()?;
        let decoder = unsafe {
            factory.CreateDecoderFromStream(
                &stream,
                std::ptr::null(),
                WICDecodeMetadataCacheOnDemand,
            )
        }?;
        if unsafe { decoder.GetFrameCount() }? != 1 {
            return Err(windows::core::Error::from_win32());
        }
        let frame = unsafe { decoder.GetFrame(0) }?;
        let (mut width, mut height) = (0, 0);
        unsafe { frame.GetSize(&mut width, &mut height) }?;
        // No pixel buffer is allocated until dimensions have passed the product limits.
        let Ok(length) = dimensions(width, height) else {
            return Ok(Err("imageTooLarge"));
        };
        let converter = unsafe { factory.CreateFormatConverter() }?;
        unsafe {
            converter.Initialize(
                &frame,
                &GUID_WICPixelFormat32bppRGBA,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeCustom,
            )
        }?;
        let mut rgba = vec![0; length];
        unsafe { converter.CopyPixels(std::ptr::null(), width * 4, &mut rgba) }?;
        Ok(normalize_rgba(width, height, &rgba))
    })();
    result.map_err(|_| "invalidImage")?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn panel_preview_is_bounded_and_does_not_replace_ocr_pixels() {
        let mut seed = 13u32;
        let pixels: Vec<u8> = (0..512*512*4).map(|_| { seed = seed.wrapping_mul(1664525).wrapping_add(1013904223); (seed >> 24) as u8 }).collect();
        let image = normalize_rgba(512,512,&pixels).unwrap();
        assert!(image.preview_data_url.len() <= 40 * 1024 * 4 / 3 + 26);
        assert!(image.png.len() > 40 * 1024);
        assert_eq!((image.width,image.height),(512,512));
        let event = crate::panel_image::PanelImages::new(1,Some(image)).event();
        assert!(serde_json::to_vec(&event).unwrap().len() <= 64 * 1024);
    }
    #[test]
    fn late_import_and_clear_cannot_replace_newer_image() {
        let store = LauncherImageStore::default();
        store.begin_import(1).unwrap();
        store.begin_import(2).unwrap();
        let image = normalize_rgba(1, 1, &[255, 0, 0, 255]).unwrap();
        let b = store.finish_import(2, image.clone()).unwrap();
        assert_eq!(
            store.finish_import(1, image.clone()).unwrap_err(),
            "expiredImage"
        );
        assert!(store.current(&b.id).is_ok());
        store.clear(3).unwrap();
        assert_eq!(store.finish_import(2, image).unwrap_err(), "expiredImage");
        assert!(store.current(&b.id).is_err());
    }
    #[test]
    fn resource_limits_and_mime_are_checked_before_decode() {
        assert_eq!(
            normalize_encoded(&vec![0; MAX_ENCODED + 1], "image/png").unwrap_err(),
            "imageTooLarge"
        );
        assert_eq!(
            normalize_encoded(b"BMnot a png", "image/png").unwrap_err(),
            "invalidImage"
        );
        assert_eq!(normalize_rgba(4097, 1, &[]).unwrap_err(), "imageTooLarge");
    }
    #[test]
    fn request_binding_is_not_a_cross_plugin_image_capability_and_hide_revokes_copy() {
        let store = LauncherImageStore::default();
        store.begin_import(1).unwrap();
        let image = store
            .finish_import(1, normalize_rgba(1, 1, &[0, 0, 0, 255]).unwrap())
            .unwrap();
        let context = PluginRequestContext {
            plugin_id: "com.example.first".into(),
            plugin_generation: 1,
            request_id: "public-request-0000000000000001".into(),
        };
        let lease = crate::public_plugins::result_action::ResultActionLease::default();
        store.bind(&image.id, &context, lease.clone()).unwrap();
        let reference = store.bound(&context, None).unwrap();
        assert_ne!(reference.id, image.id);
        assert!(store.bound(&context, Some(&image.id)).is_err());
        let mut other = context.clone();
        other.plugin_id = "com.example.second".into();
        assert!(store.bound(&other, Some(&reference.id)).is_err());
        assert_eq!(store.invalidate(), vec![context.clone()]);
        assert!(!lease.live());
        assert!(store.bound(&context, Some(&reference.id)).is_err());
        // Native hide cancels pending imports without consuming the frontend sequence.
        store.begin_import(2).unwrap();
        store.invalidate();
        assert!(store
            .finish_import(2, normalize_rgba(1, 1, &[0, 0, 0, 255]).unwrap())
            .is_err());
        store.begin_import(3).unwrap();
    }
    #[test]
    fn each_request_can_claim_provider_work_only_once() {
        let store = LauncherImageStore::default();
        store.begin_import(1).unwrap();
        let image = store
            .finish_import(1, normalize_rgba(1, 1, &[0, 0, 0, 255]).unwrap())
            .unwrap();
        let context = PluginRequestContext {
            plugin_id: "com.example.first".into(),
            plugin_generation: 1,
            request_id: "public-request-0000000000000001".into(),
        };
        store
            .bind(
                &image.id,
                &context,
                crate::public_plugins::result_action::ResultActionLease::default(),
            )
            .unwrap();
        let reference = store.bound(&context, None).unwrap();
        assert!(store.claim(&context, &reference.id).is_ok());
        assert_eq!(
            store.claim(&context, &reference.id).unwrap_err(),
            "serviceAlreadyUsed"
        );
    }
    #[test]
    fn stale_list_intents_cannot_cancel_newer_ocr_and_cancelled_admission_cannot_bind() {
        let store = LauncherImageStore::default();
        store.begin_import(1).unwrap();
        let image = store
            .finish_import(1, normalize_rgba(1, 1, &[0, 0, 0, 255]).unwrap())
            .unwrap();
        let context = PluginRequestContext {
            plugin_id: "com.example.first".into(),
            plugin_generation: 1,
            request_id: "public-request-0000000000000001".into(),
        };
        assert_eq!(
            store
                .admit_intent(&image.id, 2, true, || Some(()), |_| {})
                .unwrap(),
            Some(())
        );
        let lease = crate::public_plugins::result_action::ResultActionLease::default();
        store
            .bind_intent(&image.id, 2, &context, lease.clone())
            .unwrap();
        let retired = std::cell::Cell::new(false);
        assert_eq!(
            store
                .admit_intent(&image.id, 1, false, || Some(()), |_| retired.set(true))
                .unwrap(),
            None
        );
        assert!(!retired.get());
        assert!(lease.live());
        assert!(store.bound(&context, None).is_ok());
        // The list/Esc intent arrives after query admission but before request binding.
        store
            .admit_intent(&image.id, 3, false, || Some(()), |_| {})
            .unwrap();
        assert!(!lease.live());
        assert!(store
            .bind_intent(
                &image.id,
                2,
                &context,
                crate::public_plugins::result_action::ResultActionLease::default()
            )
            .is_err());
        // A registry-rejected stale window/query never retires the current image authority.
        store
            .admit_intent(&image.id, 4, true, || Some(()), |_| {})
            .unwrap();
        let lease = crate::public_plugins::result_action::ResultActionLease::default();
        store
            .bind_intent(&image.id, 4, &context, lease.clone())
            .unwrap();
        assert!(store
            .admit_intent(
                &image.id,
                5,
                false,
                || None::<()>,
                |_| panic!("stale registry must not cancel OCR")
            )
            .unwrap()
            .is_none());
        assert!(lease.live());
    }
}
