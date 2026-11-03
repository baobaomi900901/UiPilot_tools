//! Temporary copy transaction. Clipboard contents never leave process memory.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ClipboardStamp {
    sequence: u32,
    owner: isize,
    process: u32,
}

fn fresh_copy(before: u32, observed: ClipboardStamp, process: u32, input_unchanged: bool) -> bool {
    before != 0
        && observed.sequence != 0
        && observed.sequence != before
        && observed.owner != 0
        && observed.process == process
        && input_unchanged
}

fn still_owned(expected: ClipboardStamp, observed: ClipboardStamp, input_unchanged: bool) -> bool {
    expected.sequence != 0 && expected.owner != 0 && expected == observed && input_unchanged
}

fn synthetic_keys_down(inserted: u32) -> (bool, bool) {
    ((1..4).contains(&inserted), inserted == 2)
}

fn registered_format_is_serialized(name: &str) -> bool {
    [
        "HTML Format",
        "Rich Text Format",
        "Rich Text Format Without Objects",
        "PNG",
        "image/png",
        "Preferred DropEffect",
        "Performed DropEffect",
        "Paste Succeeded",
        "FileName",
        "FileNameW",
        "Shell IDList Array",
        "UniformResourceLocator",
        "UniformResourceLocatorW",
        "CanIncludeInClipboardHistory",
        "CanUploadToCloudClipboard",
        "ExcludeClipboardContentFromMonitorProcessing",
    ]
    .iter()
    .any(|known| name.eq_ignore_ascii_case(known))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RegisteredFormatKind {
    Payload,
    OlePayload,
    OleTransport,
    Unsupported,
}

fn ole_payloads_cover(required: &[u32], declared: &[u32]) -> bool {
    required.iter().all(|format| declared.contains(format))
}

fn registered_format_kind(name: &str) -> RegisteredFormatKind {
    if registered_format_is_serialized(name) {
        RegisteredFormatKind::Payload
    } else if [
        "WeChat_RichEdit_Format",
        "QQ_Unicode_RichEdit_Format",
        "x-application-mimetype-xwechat",
    ]
    .iter()
    .any(|known| name.eq_ignore_ascii_case(known))
    {
        // Preserve these rich-text payloads byte-for-byte only after the source
        // advertises them as ordinary, non-indexed HGLOBAL OLE data.
        RegisteredFormatKind::OlePayload
    } else if ["DataObject", "Ole Private Data"]
        .iter()
        .any(|known| name.eq_ignore_ascii_case(known))
    {
        RegisteredFormatKind::OleTransport
    } else {
        RegisteredFormatKind::Unsupported
    }
}

fn ole_format_is_materializable(
    format: u32,
    aspect: u32,
    index: i32,
    medium: u32,
    device: bool,
    clipboard_formats: &[u32],
    transport_formats: &[u32],
) -> bool {
    // FORMATETC.tymed is a bitmask of alternative media, not an exact type.
    // Windows' OLE wrapper exposes HGLOBAL | ISTREAM after Win32 restoration;
    // GetClipboardData still supplies the HGLOBAL that we copy below.
    let required_medium = if matches!(format, 2 | 9) { 16 } else { 1 };
    aspect == 1
        && index == -1
        && !device
        && medium & required_medium != 0
        && clipboard_formats.contains(&format)
        && !transport_formats.contains(&format)
}

trait TransactionDriver {
    type Backup;
    type Error;
    fn prepare(&mut self) -> Result<Self::Backup, Self::Error>;
    fn copy(&mut self) -> Result<String, Self::Error>;
    fn restore(&mut self, backup: Self::Backup) -> Result<(), Self::Error>;
}

fn run_transaction<D: TransactionDriver>(driver: &mut D) -> Result<String, D::Error> {
    let backup = driver.prepare()?;
    let copied = driver.copy();
    driver.restore(backup)?;
    copied
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn restored_clipboard_accepts_ole_alternative_media_on_repeated_capture() {
        let formats = [13, 1, 50021, 50119, 50102, 16, 7, 49848, 49847];
        // WeChat initially advertises HGLOBAL. After our Win32 restoration,
        // Windows' OLE adapter advertises HGLOBAL | ISTREAM for the same bytes.
        // Both snapshots must remain eligible for the next copy transaction.
        for medium in [1, 1 | 4] {
            for format in formats {
                assert!(ole_format_is_materializable(
                    format,
                    1,
                    -1,
                    medium,
                    false,
                    &formats,
                    &[]
                ));
            }
        }
        assert!(!ole_format_is_materializable(
            13,
            1,
            -1,
            4,
            false,
            &formats,
            &[]
        ));
        assert!(!ole_format_is_materializable(
            13,
            1,
            -1,
            0,
            false,
            &formats,
            &[]
        ));
        assert!(ole_format_is_materializable(
            2,
            1,
            -1,
            16 | 1,
            false,
            &[2],
            &[]
        ));
        assert!(!ole_format_is_materializable(2, 1, -1, 1, false, &[2], &[]));
    }
    #[test]
    fn wechat_rich_text_formats_require_ole_payload_validation() {
        for name in [
            "WeChat_RichEdit_Format",
            "QQ_Unicode_RichEdit_Format",
            "x-application-mimetype-xwechat",
        ] {
            assert_eq!(
                registered_format_kind(name),
                RegisteredFormatKind::OlePayload
            );
            assert!(!registered_format_is_serialized(name));
        }
        assert_eq!(
            registered_format_kind("Application Pointer"),
            RegisteredFormatKind::Unsupported
        );
    }
    #[test]
    fn custom_payloads_must_all_appear_in_validated_ole_descriptors() {
        assert!(ole_payloads_cover(
            &[50021, 50119, 50102],
            &[13, 1, 50021, 50119, 50102]
        ));
        assert!(!ole_payloads_cover(
            &[50021, 50119, 50102],
            &[13, 1, 50021, 50119]
        ));
        assert!(!ole_payloads_cover(&[50021], &[]));
    }
    #[test]
    fn ole_transport_is_distinct_from_restorable_payload_and_unknown_formats() {
        for name in ["DataObject", "Ole Private Data"] {
            assert_eq!(
                registered_format_kind(name),
                RegisteredFormatKind::OleTransport
            );
            assert!(!registered_format_is_serialized(name));
        }
        assert_eq!(registered_format_kind("PNG"), RegisteredFormatKind::Payload);
        assert_eq!(
            registered_format_kind("Application Pointer"),
            RegisteredFormatKind::Unsupported
        );
    }
    #[test]
    fn ole_backup_requires_every_advertised_payload_to_be_materializable() {
        let formats = [8, 49349, 49161, 49171];
        let transport = [49161, 49171];
        assert!(ole_format_is_materializable(
            8, 1, -1, 1, false, &formats, &transport
        ));
        assert!(ole_format_is_materializable(
            49349, 1, -1, 1, false, &formats, &transport
        ));
        // No omission of virtual-file streams, indexed data, device-specific data,
        // formats absent from Win32, or the source object's own transport handles.
        for (format, aspect, index, medium, device) in [
            (8, 1, -1, 4, false),
            (8, 1, 0, 1, false),
            (8, 2, -1, 1, false),
            (8, 1, -1, 1, true),
            (60000, 1, -1, 1, false),
            (49161, 1, -1, 1, false),
        ] {
            assert!(!ole_format_is_materializable(
                format, aspect, index, medium, device, &formats, &transport
            ));
        }
    }
    struct Fake {
        events: Vec<&'static str>,
        backup_error: bool,
        copy_error: bool,
        restore_error: bool,
    }
    impl TransactionDriver for Fake {
        type Backup = ();
        type Error = &'static str;
        fn prepare(&mut self) -> Result<(), Self::Error> {
            self.events.push("backup");
            if self.backup_error {
                Err("backup")
            } else {
                Ok(())
            }
        }
        fn copy(&mut self) -> Result<String, Self::Error> {
            self.events.push("copy");
            if self.copy_error {
                Err("copy")
            } else {
                Ok("selection".into())
            }
        }
        fn restore(&mut self, _: ()) -> Result<(), Self::Error> {
            self.events.push("restore");
            if self.restore_error {
                Err("restore")
            } else {
                Ok(())
            }
        }
    }
    fn fake() -> Fake {
        Fake {
            events: vec![],
            backup_error: false,
            copy_error: false,
            restore_error: false,
        }
    }
    fn stamp(sequence: u32) -> ClipboardStamp {
        ClipboardStamp {
            sequence,
            owner: 10,
            process: 20,
        }
    }
    #[test]
    fn backup_failure_never_injects_copy() {
        let mut driver = fake();
        driver.backup_error = true;
        assert_eq!(run_transaction(&mut driver), Err("backup"));
        assert_eq!(driver.events, ["backup"]);
    }
    #[test]
    fn restores_after_caching_text_and_before_returning_it() {
        let mut driver = fake();
        assert_eq!(run_transaction(&mut driver), Ok("selection".into()));
        assert_eq!(driver.events, ["backup", "copy", "restore"]);
    }
    #[test]
    fn failed_copy_still_restores_and_reports_restore_failure() {
        let mut driver = fake();
        driver.copy_error = true;
        assert_eq!(run_transaction(&mut driver), Err("copy"));
        assert_eq!(driver.events, ["backup", "copy", "restore"]);
        driver.restore_error = true;
        assert_eq!(run_transaction(&mut driver), Err("restore"));
    }
    #[test]
    fn old_clipboard_and_ambiguous_ownership_are_never_selected() {
        assert!(!fresh_copy(2, stamp(2), 20, true));
        assert!(!fresh_copy(2, stamp(0), 20, true));
        assert!(!fresh_copy(2, stamp(3), 21, true));
        assert!(!fresh_copy(2, stamp(3), 20, false));
        assert!(fresh_copy(2, stamp(3), 20, true));
    }
    #[test]
    fn user_copy_before_first_observation_invalidates_restore_even_in_same_process() {
        assert!(!still_owned(stamp(3), stamp(3), false));
        assert!(!still_owned(stamp(3), stamp(4), true));
        assert!(!still_owned(
            stamp(3),
            ClipboardStamp {
                owner: 11,
                ..stamp(3)
            },
            true
        ));
        assert!(still_owned(stamp(3), stamp(3), true));
    }

    #[test]
    fn partial_input_releases_only_keys_that_its_inserted_prefix_pressed() {
        assert_eq!(synthetic_keys_down(0), (false, false));
        assert_eq!(synthetic_keys_down(1), (true, false));
        assert_eq!(synthetic_keys_down(2), (true, true));
        assert_eq!(synthetic_keys_down(3), (true, false));
        assert_eq!(synthetic_keys_down(4), (false, false));
    }

    #[test]
    fn registered_format_backup_accepts_only_known_self_contained_payloads() {
        for name in [
            "HTML Format",
            "Rich Text Format",
            "Rich Text Format Without Objects",
            "PNG",
            "image/png",
            "Preferred DropEffect",
            "Performed DropEffect",
            "Paste Succeeded",
            "FileName",
            "FileNameW",
            "Shell IDList Array",
            "UniformResourceLocator",
            "UniformResourceLocatorW",
            "CanIncludeInClipboardHistory",
            "CanUploadToCloudClipboard",
        ] {
            assert!(
                registered_format_is_serialized(name),
                "known serialized format rejected"
            );
        }
        for name in [
            "DataObject",
            "Ole Private Data",
            "Application Pointer",
            "",
            "PNG-pointers",
        ] {
            assert!(!registered_format_is_serialized(name));
        }
        assert!(registered_format_is_serialized("html format"));
    }
}

// Windows backend. Policy tests above can run without invoking any native API.
#[cfg(windows)]
pub(super) use platform::capture;

#[cfg(windows)]
mod platform {
    use super::super::{
        native, windows_history::WindowsHistory, CaptureTarget, SelectionCaptureError,
    };
    use super::{
        fresh_copy, ole_format_is_materializable, ole_payloads_cover, registered_format_kind,
        run_transaction, still_owned, synthetic_keys_down, ClipboardStamp, RegisteredFormatKind,
        TransactionDriver,
    };
    use crate::clipboard_history::{ClipboardHistoryService, TemporaryClipboardCopy};
    use std::{
        ffi::c_void,
        ptr,
        sync::{
            atomic::{AtomicBool, AtomicU64, Ordering},
            mpsc, Arc,
        },
        thread,
        time::{Duration, Instant},
    };

    const POLL: Duration = Duration::from_millis(5);
    const MAX_BACKUP_BYTES: usize = 64 * 1024 * 1024;
    const MAX_FORMATS: usize = 256;

    // Enumerate advertised OLE payloads, not the raw object-reference formats.
    // No retained IDataObject is used as the backup: every supported payload is
    // still materialized into owned resources before Ctrl+C is injected.
    fn validate_ole_payloads(
        formats: &[u32],
        transport: &[u32],
        required: &[u32],
        mut check: impl FnMut() -> Result<(), SelectionCaptureError>,
    ) -> Result<(), SelectionCaptureError> {
        use windows::{
            core::Interface,
            Win32::System::Com::{CoTaskMemFree, IDataObject, DATADIR_GET, FORMATETC},
        };
        #[link(name = "ole32")]
        unsafe extern "system" {
            fn OleInitialize(reserved: *const c_void) -> i32;
            fn OleUninitialize();
            fn OleGetClipboard(data: *mut *mut c_void) -> i32;
        }
        struct Ole;
        impl Drop for Ole {
            fn drop(&mut self) {
                unsafe {
                    OleUninitialize();
                }
            }
        }
        let fail = SelectionCaptureError::ClipboardBackup;
        unsafe {
            check()?;
            let hr = OleInitialize(ptr::null());
            if hr < 0 {
                native::trace("backup_ole_init", i64::from(hr));
                return Err(fail);
            }
            let _ole = Ole;
            let mut raw = ptr::null_mut();
            let hr = OleGetClipboard(&mut raw);
            if hr < 0 || raw.is_null() {
                native::trace("backup_ole_object", i64::from(hr));
                return Err(fail);
            }
            let data = IDataObject::from_raw(raw);
            check()?;
            let enumerator = data.EnumFormatEtc(DATADIR_GET.0 as u32).map_err(|_| fail)?;
            let mut count = 0;
            let mut declared = Vec::new();
            loop {
                check()?;
                let mut item = [FORMATETC::default()];
                let mut fetched = 0;
                let hr = enumerator.Next(&mut item, Some(&mut fetched));
                let descriptor = &item[0];
                let valid = ole_format_is_materializable(
                    u32::from(descriptor.cfFormat),
                    descriptor.dwAspect,
                    descriptor.lindex,
                    descriptor.tymed,
                    !descriptor.ptd.is_null(),
                    formats,
                    transport,
                );
                CoTaskMemFree(Some(descriptor.ptd.cast()));
                if hr.0 == 1 && fetched == 0 {
                    return if count > 0 && ole_payloads_cover(required, &declared) {
                        Ok(())
                    } else {
                        Err(fail)
                    };
                }
                if hr.0 != 0 || fetched != 1 || !valid || count >= MAX_FORMATS {
                    native::trace("backup_ole_payload", i64::from(descriptor.cfFormat));
                    return Err(fail);
                }
                count += 1;
                declared.push(u32::from(descriptor.cfFormat));
            }
        }
    }
    const MAX_TEXT_BYTES: usize = 16 * 1024 * 1024;
    const OWN_INPUT: usize = 0x5549_5043;
    static INPUT_EPOCH: AtomicU64 = AtomicU64::new(0);

    #[repr(C)]
    #[derive(Default)]
    struct Point {
        x: i32,
        y: i32,
    }
    #[repr(C)]
    #[derive(Default)]
    struct Message {
        window: isize,
        message: u32,
        wparam: usize,
        lparam: isize,
        time: u32,
        point: Point,
        private: u32,
    }
    #[repr(C)]
    struct KeyboardHook {
        vk: u32,
        scan: u32,
        flags: u32,
        time: u32,
        extra: usize,
    }
    #[repr(C)]
    struct MouseHook {
        point: Point,
        data: u32,
        flags: u32,
        time: u32,
        extra: usize,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct KeyboardInput {
        vk: u16,
        scan: u16,
        flags: u32,
        time: u32,
        extra: usize,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct MouseInput {
        x: i32,
        y: i32,
        data: u32,
        flags: u32,
        time: u32,
        extra: usize,
    }
    #[repr(C)]
    #[derive(Default)]
    struct BitmapInfo {
        kind: i32,
        width: i32,
        height: i32,
        stride: i32,
        planes: u16,
        bits_per_pixel: u16,
        bits: *mut c_void,
    }
    #[repr(C)]
    union InputData {
        keyboard: KeyboardInput,
        mouse: MouseInput,
    }
    #[repr(C)]
    struct Input {
        kind: u32,
        data: InputData,
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn OpenClipboard(window: isize) -> i32;
        fn CloseClipboard() -> i32;
        fn EmptyClipboard() -> i32;
        fn GetClipboardData(format: u32) -> isize;
        fn SetClipboardData(format: u32, data: isize) -> isize;
        fn EnumClipboardFormats(format: u32) -> u32;
        fn GetClipboardSequenceNumber() -> u32;
        fn GetClipboardOwner() -> isize;
        fn GetWindowThreadProcessId(window: isize, process: *mut u32) -> u32;
        fn RegisterClipboardFormatW(name: *const u16) -> u32;
        fn GetClipboardFormatNameW(format: u32, name: *mut u16, capacity: i32) -> i32;
        fn GetAsyncKeyState(key: i32) -> i16;
        fn SendInput(count: u32, inputs: *const Input, size: i32) -> u32;
        fn CreateWindowExW(
            ex: u32,
            class: *const u16,
            name: *const u16,
            style: u32,
            x: i32,
            y: i32,
            width: i32,
            height: i32,
            parent: isize,
            menu: isize,
            instance: isize,
            parameter: *const c_void,
        ) -> isize;
        fn DestroyWindow(window: isize) -> i32;
        fn CopyImage(image: isize, kind: u32, width: i32, height: i32, flags: u32) -> isize;
        fn SetWindowsHookExW(
            kind: i32,
            callback: Option<unsafe extern "system" fn(i32, usize, isize) -> isize>,
            module: isize,
            thread: u32,
        ) -> isize;
        fn UnhookWindowsHookEx(hook: isize) -> i32;
        fn CallNextHookEx(hook: isize, code: i32, wparam: usize, lparam: isize) -> isize;
        fn MsgWaitForMultipleObjectsEx(
            count: u32,
            handles: *const isize,
            millis: u32,
            wake_mask: u32,
            flags: u32,
        ) -> u32;
        fn PeekMessageW(
            message: *mut Message,
            window: isize,
            low: u32,
            high: u32,
            remove: u32,
        ) -> i32;
        fn PostThreadMessageW(thread: u32, message: u32, wparam: usize, lparam: isize) -> i32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GlobalAlloc(flags: u32, bytes: usize) -> isize;
        fn GlobalSize(memory: isize) -> usize;
        fn GlobalFlags(memory: isize) -> u32;
        fn GlobalLock(memory: isize) -> *mut c_void;
        fn GlobalUnlock(memory: isize) -> i32;
        fn GlobalFree(memory: isize) -> isize;
        fn GetLastError() -> u32;
        fn SetLastError(error: u32);
        fn GetCurrentThreadId() -> u32;
        fn GetModuleHandleW(name: *const u16) -> isize;
    }
    #[link(name = "gdi32")]
    unsafe extern "system" {
        fn DeleteObject(object: isize) -> i32;
        fn GetObjectW(object: isize, size: i32, data: *mut c_void) -> i32;
        fn GetPaletteEntries(palette: isize, start: u32, count: u32, entries: *mut u8) -> u32;
        fn CreatePalette(palette: *const u8) -> isize;
    }

    struct ClipboardLock;
    impl ClipboardLock {
        fn open(window: isize, deadline: Instant) -> Result<Self, SelectionCaptureError> {
            loop {
                if unsafe { OpenClipboard(window) } != 0 {
                    return Ok(Self);
                }
                if Instant::now() >= deadline {
                    return Err(SelectionCaptureError::Busy);
                }
                thread::sleep(POLL.min(deadline.saturating_duration_since(Instant::now())));
            }
        }
    }
    impl Drop for ClipboardLock {
        fn drop(&mut self) {
            unsafe {
                CloseClipboard();
            }
        }
    }

    struct Window(isize);
    impl Window {
        fn new() -> Result<Self, SelectionCaptureError> {
            let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
            let handle = unsafe {
                CreateWindowExW(
                    0,
                    class.as_ptr(),
                    ptr::null(),
                    0,
                    0,
                    0,
                    0,
                    0,
                    -3,
                    0,
                    GetModuleHandleW(ptr::null()),
                    ptr::null(),
                )
            };
            if handle == 0 {
                Err(SelectionCaptureError::ClipboardBackup)
            } else {
                Ok(Self(handle))
            }
        }
    }
    impl Drop for Window {
        fn drop(&mut self) {
            unsafe {
                DestroyWindow(self.0);
            }
        }
    }

    enum Resource {
        Global(isize),
        Bitmap(isize),
    }
    impl Resource {
        fn handle(&self) -> isize {
            match self {
                Self::Global(h) | Self::Bitmap(h) => *h,
            }
        }
        fn relinquish(&mut self) {
            match self {
                Self::Global(h) | Self::Bitmap(h) => *h = 0,
            }
        }
        fn global(bytes: &[u8]) -> Result<Self, SelectionCaptureError> {
            if bytes.is_empty() {
                return Err(SelectionCaptureError::ClipboardBackup);
            }
            let handle = unsafe { GlobalAlloc(2, bytes.len()) };
            if handle == 0 {
                return Err(SelectionCaptureError::ClipboardBackup);
            }
            let resource = Self::Global(handle);
            let destination = unsafe { GlobalLock(handle) };
            if destination.is_null() {
                return Err(SelectionCaptureError::ClipboardBackup);
            }
            unsafe {
                ptr::copy_nonoverlapping(bytes.as_ptr(), destination.cast::<u8>(), bytes.len());
                GlobalUnlock(handle);
            }
            Ok(resource)
        }
    }
    impl Drop for Resource {
        fn drop(&mut self) {
            unsafe {
                match *self {
                    Self::Global(h) if h != 0 => {
                        GlobalFree(h);
                    }
                    Self::Bitmap(h) if h != 0 => {
                        DeleteObject(h);
                    }
                    _ => {}
                }
            }
        }
    }
    struct Snapshot {
        baseline: u32,
        formats: Vec<(u32, Resource)>,
    }

    fn stamp() -> ClipboardStamp {
        unsafe {
            let owner = GetClipboardOwner();
            let mut process = 0;
            if owner != 0 {
                GetWindowThreadProcessId(owner, &mut process);
            }
            ClipboardStamp {
                sequence: GetClipboardSequenceNumber(),
                owner,
                process,
            }
        }
    }

    // The caller holds OpenClipboard throughout access and the stamp recheck.
    fn read_unicode() -> Result<String, SelectionCaptureError> {
        let handle = unsafe { GetClipboardData(13) };
        if handle == 0 {
            return Err(SelectionCaptureError::Failed);
        }
        let size = unsafe { GlobalSize(handle) };
        if size < 2 || size > MAX_TEXT_BYTES || size % 2 != 0 {
            return Err(SelectionCaptureError::TooLong);
        }
        let source = unsafe { GlobalLock(handle) };
        if source.is_null() {
            return Err(SelectionCaptureError::Failed);
        }
        let units = unsafe { std::slice::from_raw_parts(source.cast::<u16>(), size / 2) };
        let text = units
            .iter()
            .position(|unit| *unit == 0)
            .ok_or(SelectionCaptureError::Invalid)
            .and_then(|end| {
                String::from_utf16(&units[..end]).map_err(|_| SelectionCaptureError::Invalid)
            });
        unsafe {
            GlobalUnlock(handle);
        }
        text
    }

    // A dedicated message-pump thread observes physical input before the receiving
    // application can process it. Comparing clipboard owner PID alone cannot detect
    // a user's second Ctrl+C in the same process before our first clipboard poll.
    unsafe extern "system" fn keyboard_hook(code: i32, wparam: usize, lparam: isize) -> isize {
        if code >= 0 && (*(lparam as *const KeyboardHook)).extra != OWN_INPUT {
            INPUT_EPOCH.fetch_add(1, Ordering::AcqRel);
        }
        CallNextHookEx(0, code, wparam, lparam)
    }
    unsafe extern "system" fn mouse_hook(code: i32, wparam: usize, lparam: isize) -> isize {
        if code >= 0 && wparam != 0x200 && (*(lparam as *const MouseHook)).extra != OWN_INPUT {
            INPUT_EPOCH.fetch_add(1, Ordering::AcqRel);
        }
        CallNextHookEx(0, code, wparam, lparam)
    }
    struct InputGuard {
        thread: u32,
        done: mpsc::Receiver<()>,
        stop: Arc<AtomicBool>,
    }
    impl InputGuard {
        fn start(deadline: Instant) -> Result<Self, SelectionCaptureError> {
            let (ready_tx, ready_rx) = mpsc::sync_channel(1);
            let (done_tx, done_rx) = mpsc::sync_channel(1);
            let stop = Arc::new(AtomicBool::new(false));
            let worker_stop = Arc::clone(&stop);
            thread::Builder::new()
                .name("selection-input-guard".into())
                .spawn(move || unsafe {
                    let mut message = Message::default();
                    // Create a message queue before reporting the thread ID.
                    PeekMessageW(&mut message, 0, 0, 0, 0);
                    let module = GetModuleHandleW(ptr::null());
                    let keyboard = SetWindowsHookExW(13, Some(keyboard_hook), module, 0);
                    let mouse = SetWindowsHookExW(14, Some(mouse_hook), module, 0);
                    let installed = keyboard != 0 && mouse != 0;
                    let accepted = ready_tx
                        .send(if installed { GetCurrentThreadId() } else { 0 })
                        .is_ok();
                    if installed && accepted {
                        while !worker_stop.load(Ordering::Acquire) {
                            // A bounded pump also tears down safely if admission
                            // timed out before the caller received our thread ID.
                            MsgWaitForMultipleObjectsEx(0, ptr::null(), 20, 0x4ff, 4);
                            while PeekMessageW(&mut message, 0, 0, 0, 1) != 0 {
                                if message.message == 0x12 {
                                    worker_stop.store(true, Ordering::Release);
                                }
                            }
                        }
                    }
                    if keyboard != 0 {
                        UnhookWindowsHookEx(keyboard);
                    }
                    if mouse != 0 {
                        UnhookWindowsHookEx(mouse);
                    }
                    let _ = done_tx.send(());
                })
                .map_err(|_| SelectionCaptureError::Failed)?;
            let id = match ready_rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
            {
                Ok(id) => id,
                Err(_) => {
                    stop.store(true, Ordering::Release);
                    return Err(SelectionCaptureError::Timeout);
                }
            };
            if id == 0 {
                return Err(SelectionCaptureError::Failed);
            }
            Ok(Self {
                thread: id,
                done: done_rx,
                stop,
            })
        }
        fn epoch(&self) -> u64 {
            INPUT_EPOCH.load(Ordering::Acquire)
        }
    }
    impl Drop for InputGuard {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Release);
            unsafe {
                PostThreadMessageW(self.thread, 0x12, 0, 0);
            }
            let _ = self.done.recv_timeout(Duration::from_millis(100));
        }
    }

    fn key(vk: u16, up: bool) -> Input {
        Input {
            kind: 1,
            data: InputData {
                keyboard: KeyboardInput {
                    vk,
                    scan: 0,
                    flags: if up { 2 } else { 0 },
                    time: 0,
                    extra: OWN_INPUT,
                },
            },
        }
    }
    fn inject_copy() -> (bool, Result<(), SelectionCaptureError>) {
        let inputs = [
            key(0x11, false),
            key(0x43, false),
            key(0x43, true),
            key(0x11, true),
        ];
        let inserted =
            unsafe { SendInput(4, inputs.as_ptr(), std::mem::size_of::<Input>() as i32) };
        if inserted == 4 {
            return (true, Ok(()));
        }
        // SendInput inserts a prefix; release only synthetic keys still down.
        let (control_down, c_down) = synthetic_keys_down(inserted);
        if c_down {
            let release = key(0x43, true);
            unsafe {
                SendInput(1, &release, std::mem::size_of::<Input>() as i32);
            }
        }
        if control_down {
            let release = key(0x11, true);
            unsafe {
                SendInput(1, &release, std::mem::size_of::<Input>() as i32);
            }
        }
        (inserted >= 2, Err(SelectionCaptureError::Failed))
    }

    struct Driver<'a> {
        target: CaptureTarget,
        cancelled: &'a AtomicBool,
        copy_deadline: Instant,
        restore_deadline: Instant,
        window: &'a Window,
        input: &'a InputGuard,
        epoch: u64,
        scope: &'a mut TemporaryClipboardCopy,
        observed: Option<ClipboardStamp>,
        final_stamp: Option<ClipboardStamp>,
        text: Option<String>,
        injected: bool,
    }
    impl Driver<'_> {
        fn input_unchanged(&self) -> bool {
            self.input.epoch() == self.epoch
        }
        fn check(&self) -> Result<(), SelectionCaptureError> {
            native::check(self.target, self.cancelled, self.copy_deadline)?;
            if !self.input_unchanged() {
                return Err(SelectionCaptureError::ClipboardChanged);
            }
            Ok(())
        }
        fn snapshot(&self) -> Result<Snapshot, SelectionCaptureError> {
            self.check()?;
            let mut lock = Some(
                ClipboardLock::open(self.window.0, self.copy_deadline)
                    .map_err(|_| SelectionCaptureError::ClipboardBackup)?,
            );
            self.check()?;
            let mut ids = Vec::new();
            let mut previous = 0;
            loop {
                self.check()?;
                unsafe {
                    SetLastError(0);
                }
                let next = unsafe { EnumClipboardFormats(previous) };
                if next == 0 {
                    if unsafe { GetLastError() } != 0 {
                        return Err(SelectionCaptureError::ClipboardBackup);
                    }
                    break;
                }
                if ids.len() >= MAX_FORMATS || ids.contains(&next) {
                    return Err(SelectionCaptureError::ClipboardBackup);
                }
                ids.push(next);
                previous = next;
            }
            let mut transport = Vec::new();
            let mut required_ole_payloads = Vec::new();
            for &format in &ids {
                self.check()?;
                // These formats carry process/GDI handles rather than HGLOBAL bytes.
                // CF_BITMAP is copied with the GDI ownership rules below.
                if matches!(format, 3 | 14 | 0x80 | 0x82 | 0x83 | 0x8e | 0x200..=0x3ff) {
                    native::trace("backup_handle_format", i64::from(format));
                    return Err(SelectionCaptureError::ClipboardBackup);
                }
                if format >= 0xc000 {
                    // HGLOBAL alone does not prove an opaque payload is portable:
                    // OLE formats may embed process-local pointers. Never log names.
                    let mut name = [0u16; 256];
                    let length = unsafe {
                        GetClipboardFormatNameW(format, name.as_mut_ptr(), name.len() as i32)
                    };
                    if length <= 0 || length as usize >= name.len() {
                        return Err(SelectionCaptureError::ClipboardBackup);
                    }
                    let name = String::from_utf16(&name[..length as usize])
                        .map_err(|_| SelectionCaptureError::ClipboardBackup)?;
                    match registered_format_kind(&name) {
                        RegisteredFormatKind::Payload => {}
                        RegisteredFormatKind::OlePayload => required_ole_payloads.push(format),
                        RegisteredFormatKind::OleTransport => transport.push(format),
                        RegisteredFormatKind::Unsupported => {
                            native::trace("backup_unknown_format", i64::from(format));
                            return Err(SelectionCaptureError::ClipboardBackup);
                        }
                    }
                }
            }
            if !transport.is_empty() || !required_ole_payloads.is_empty() {
                let before = stamp();
                // OLE may call into the source application. Do not hold its
                // clipboard lock across COM calls; reject any intervening copy.
                drop(lock.take());
                validate_ole_payloads(&ids, &transport, &required_ole_payloads, || self.check())?;
                lock = Some(
                    ClipboardLock::open(self.window.0, self.copy_deadline)
                        .map_err(|_| SelectionCaptureError::ClipboardBackup)?,
                );
                self.check()?;
                if stamp() != before {
                    return Err(SelectionCaptureError::ClipboardChanged);
                }
            }
            let _lock = lock;
            let mut formats = Vec::new();
            let mut total = 0usize;
            for format in ids.into_iter().filter(|id| !transport.contains(id)) {
                self.check()?;
                let handle = unsafe { GetClipboardData(format) };
                if handle == 0 {
                    native::trace("backup_read_format", i64::from(format));
                    return Err(SelectionCaptureError::ClipboardBackup);
                }
                let resource = if format == 2 {
                    let mut bitmap_info = BitmapInfo::default();
                    if unsafe {
                        GetObjectW(
                            handle,
                            std::mem::size_of::<BitmapInfo>() as i32,
                            (&mut bitmap_info as *mut BitmapInfo).cast(),
                        )
                    } == 0
                    {
                        return Err(SelectionCaptureError::ClipboardBackup);
                    }
                    let size = usize::try_from(bitmap_info.stride)
                        .ok()
                        .and_then(|stride| {
                            stride.checked_mul(bitmap_info.height.unsigned_abs() as usize)
                        })
                        .filter(|size| *size != 0)
                        .ok_or(SelectionCaptureError::ClipboardBackup)?;
                    total = total
                        .checked_add(size)
                        .ok_or(SelectionCaptureError::ClipboardBackup)?;
                    if total > MAX_BACKUP_BYTES {
                        return Err(SelectionCaptureError::ClipboardBackup);
                    }
                    let bitmap = unsafe { CopyImage(handle, 0, 0, 0, 0x2000) };
                    if bitmap == 0 {
                        return Err(SelectionCaptureError::ClipboardBackup);
                    }
                    Resource::Bitmap(bitmap)
                } else if format == 9 {
                    let count = unsafe { GetPaletteEntries(handle, 0, 0, ptr::null_mut()) };
                    if count == 0 || count > u32::from(u16::MAX) {
                        return Err(SelectionCaptureError::ClipboardBackup);
                    }
                    let size = count as usize * 4 + 4;
                    total = total
                        .checked_add(size)
                        .ok_or(SelectionCaptureError::ClipboardBackup)?;
                    if total > MAX_BACKUP_BYTES {
                        return Err(SelectionCaptureError::ClipboardBackup);
                    }
                    let mut data = vec![0u8; size];
                    data[..2].copy_from_slice(&0x300u16.to_ne_bytes());
                    data[2..4].copy_from_slice(&(count as u16).to_ne_bytes());
                    if unsafe { GetPaletteEntries(handle, 0, count, data.as_mut_ptr().add(4)) }
                        != count
                    {
                        return Err(SelectionCaptureError::ClipboardBackup);
                    }
                    let palette = unsafe { CreatePalette(data.as_ptr()) };
                    if palette == 0 {
                        return Err(SelectionCaptureError::ClipboardBackup);
                    }
                    // Both bitmap and palette ownership transfer via SetClipboardData
                    // and use DeleteObject if transfer never happens.
                    Resource::Bitmap(palette)
                } else {
                    let size = unsafe { GlobalSize(handle) };
                    total = total
                        .checked_add(size)
                        .ok_or(SelectionCaptureError::ClipboardBackup)?;
                    if size == 0
                        || total > MAX_BACKUP_BYTES
                        || unsafe { GlobalFlags(handle) } == 0x8000
                    {
                        return Err(SelectionCaptureError::ClipboardBackup);
                    }
                    let source = unsafe { GlobalLock(handle) };
                    if source.is_null() {
                        return Err(SelectionCaptureError::ClipboardBackup);
                    }
                    let copied = Resource::global(unsafe {
                        std::slice::from_raw_parts(source.cast::<u8>(), size)
                    });
                    unsafe {
                        GlobalUnlock(handle);
                    }
                    copied?
                };
                formats.push((format, resource));
            }
            // Restore writes must not create a new Windows history/cloud record.
            for name in ["CanIncludeInClipboardHistory", "CanUploadToCloudClipboard"] {
                let name: Vec<u16> = name.encode_utf16().chain([0]).collect();
                let format = unsafe { RegisterClipboardFormatW(name.as_ptr()) };
                if format == 0 {
                    return Err(SelectionCaptureError::ClipboardBackup);
                }
                formats.retain(|(existing, _)| *existing != format);
                formats.push((format, Resource::global(&0u32.to_ne_bytes())?));
            }
            self.check()?;
            let baseline = unsafe { GetClipboardSequenceNumber() };
            if baseline == 0 {
                return Err(SelectionCaptureError::ClipboardBackup);
            }
            Ok(Snapshot { baseline, formats })
        }
        fn read_text(&mut self, baseline: u32) -> Result<String, SelectionCaptureError> {
            loop {
                // Even cancellation or a target switch must still notice an already
                // completed copy so the subsequent restore can be conditional.
                let current = stamp();
                if current.sequence != baseline && current.sequence != 0 {
                    let _lock = ClipboardLock::open(self.window.0, self.copy_deadline)?;
                    let current = stamp();
                    if !fresh_copy(
                        baseline,
                        current,
                        self.target.process,
                        self.input_unchanged(),
                    ) {
                        return Err(SelectionCaptureError::ClipboardChanged);
                    }
                    self.observed = Some(current);
                    self.scope.exclude_sequence(current.sequence);
                    let status = self.check();
                    let text = read_unicode();
                    // Rendering a delayed/synthesized Unicode format can advance
                    // its sequence while this same lock remains held.
                    let rendered = stamp();
                    if !fresh_copy(
                        baseline,
                        rendered,
                        self.target.process,
                        self.input_unchanged(),
                    ) || rendered.owner != current.owner
                    {
                        return Err(SelectionCaptureError::ClipboardChanged);
                    }
                    self.observed = Some(rendered);
                    self.scope.exclude_sequence(rendered.sequence);
                    let text = text?;
                    self.text = Some(text.clone());
                    status?;
                    self.check()?;
                    return Ok(text);
                }
                self.check()?;
                thread::sleep(POLL);
            }
        }
    }
    // The baseline is carried by prepare separately from the native resources so
    // no post-copy allocation is needed to restore the original clipboard formats.
    impl TransactionDriver for Driver<'_> {
        type Backup = Snapshot;
        type Error = SelectionCaptureError;
        fn prepare(&mut self) -> Result<Snapshot, Self::Error> {
            let snapshot = self.snapshot()?;
            self.final_stamp = Some(ClipboardStamp {
                sequence: snapshot.baseline,
                owner: 0,
                process: 0,
            });
            Ok(snapshot)
        }
        fn copy(&mut self) -> Result<String, Self::Error> {
            self.check()?;
            let baseline = self
                .final_stamp
                .take()
                .ok_or(SelectionCaptureError::ClipboardBackup)?
                .sequence;
            if unsafe { GetClipboardSequenceNumber() } != baseline {
                return Err(SelectionCaptureError::ClipboardChanged);
            }
            let (injected, injection_result) = inject_copy();
            self.injected = injected;
            native::trace("copy_input_sent", i64::from(injection_result.is_ok()));
            if !injected {
                return injection_result.and(Err(SelectionCaptureError::Failed));
            }
            // A partial injection may still have delivered Ctrl+C. Always observe
            // an update before returning its error, then conditionally restore it.
            let read = self.read_text(baseline);
            injection_result.and(read)
        }
        fn restore(&mut self, mut backup: Snapshot) -> Result<(), Self::Error> {
            if !self.injected {
                return Ok(());
            }
            if self.observed.is_none() {
                let grace =
                    (Instant::now() + Duration::from_millis(150)).min(self.restore_deadline);
                while unsafe { GetClipboardSequenceNumber() } == backup.baseline
                    && Instant::now() < grace
                {
                    if !self.input_unchanged() {
                        return Err(SelectionCaptureError::ClipboardChanged);
                    }
                    thread::sleep(POLL);
                }
            }
            let _lock = ClipboardLock::open(self.window.0, self.restore_deadline)
                .map_err(|_| SelectionCaptureError::ClipboardRestore)?;
            if self.observed.is_none() {
                let current = stamp();
                if current.sequence == backup.baseline {
                    return Ok(());
                }
                if !fresh_copy(
                    backup.baseline,
                    current,
                    self.target.process,
                    self.input_unchanged(),
                ) {
                    return Err(SelectionCaptureError::ClipboardChanged);
                }
                self.observed = Some(current);
                self.scope.exclude_sequence(current.sequence);
                // A cancelled/timed-out read may still have completed its Ctrl+C.
                // Cache solely for targeted cleanup, never for later dispatch.
                if let Ok(text) = read_unicode() {
                    self.text = Some(text);
                }
                let rendered = stamp();
                if !fresh_copy(
                    backup.baseline,
                    rendered,
                    self.target.process,
                    self.input_unchanged(),
                ) {
                    return Err(SelectionCaptureError::ClipboardChanged);
                }
                self.observed = Some(rendered);
                self.scope.exclude_sequence(rendered.sequence);
            }
            let expected = self
                .observed
                .ok_or(SelectionCaptureError::ClipboardRestore)?;
            if !still_owned(expected, stamp(), self.input_unchanged()) {
                return Err(SelectionCaptureError::ClipboardChanged);
            }
            if unsafe { EmptyClipboard() } == 0 {
                return Err(SelectionCaptureError::ClipboardRestore);
            }
            let mut failed = false;
            for (format, resource) in &mut backup.formats {
                if unsafe { SetClipboardData(*format, resource.handle()) } == 0 {
                    failed = true;
                } else {
                    resource.relinquish();
                }
            }
            let restored = stamp();
            self.scope.exclude_sequence(restored.sequence);
            self.final_stamp = Some(restored);
            native::trace("clipboard_restored", i64::from(!failed));
            if failed {
                Err(SelectionCaptureError::ClipboardRestore)
            } else {
                Ok(())
            }
        }
    }

    pub(crate) fn capture(
        target: CaptureTarget,
        cancelled: &AtomicBool,
        deadline: Instant,
        history: &ClipboardHistoryService,
    ) -> Result<String, SelectionCaptureError> {
        let copy_deadline = deadline
            .checked_sub(Duration::from_millis(1200))
            .unwrap_or(deadline);
        let restore_deadline = deadline
            .checked_sub(Duration::from_millis(850))
            .unwrap_or(deadline);
        native::check(target, cancelled, copy_deadline)?;
        let input = InputGuard::start(copy_deadline)?;
        loop {
            native::check(target, cancelled, copy_deadline)?;
            if [
                0x10, 0x11, 0x12, 0x5b, 0x5c, 0x43, 0x01, 0x02, 0x04, 0x05, 0x06,
            ]
            .iter()
            .all(|key| unsafe { GetAsyncKeyState(*key) } >= 0)
            {
                break;
            }
            thread::sleep(POLL);
        }
        let epoch = input.epoch();
        let window = Window::new()?;
        let windows_history = WindowsHistory::begin(copy_deadline)
            .map_err(|_| SelectionCaptureError::HistoryUnavailable)?;
        let mut final_stamp = None;
        let mut copied_text = None;
        let result = history
            .with_temporary_copy(|scope| {
                let mut driver = Driver {
                    target,
                    cancelled,
                    copy_deadline,
                    restore_deadline,
                    window: &window,
                    input: &input,
                    epoch,
                    scope,
                    observed: None,
                    final_stamp: None,
                    text: None,
                    injected: false,
                };
                let result = run_transaction(&mut driver);
                final_stamp = driver.final_stamp.or(driver.observed);
                copied_text = driver.text;
                result
            })
            .map_err(|_| SelectionCaptureError::HistoryUnavailable)?;
        if let Some(text) = copied_text.as_deref() {
            let cleanup = windows_history
                .cleanup(
                    text,
                    || {
                        final_stamp
                            .map(|expected| still_owned(expected, stamp(), input.epoch() == epoch))
                            .unwrap_or(false)
                    },
                    deadline,
                )
                .map_err(|_| SelectionCaptureError::HistoryCleanup);
            // Preserve the more serious restoration error while still attempting
            // cleanup, so a second failure cannot hide lost original formats.
            if matches!(result, Err(SelectionCaptureError::ClipboardRestore)) {
                return result;
            }
            cleanup?;
        } else if final_stamp.is_some_and(|value| value.owner != 0) {
            // A copy was observed but could not be decoded; never claim that its
            // system-history entry was cleaned by matching unknown contents.
            if matches!(result, Err(SelectionCaptureError::ClipboardRestore)) {
                return result;
            }
            return Err(SelectionCaptureError::HistoryCleanup);
        }
        result
    }
}
