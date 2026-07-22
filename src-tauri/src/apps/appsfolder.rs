use std::{collections::HashSet, ffi::c_void};

use windows::{
    core::{Interface, PCWSTR, PWSTR},
    Win32::{
        Foundation::{ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS},
        Storage::{
            EnhancedStorage::PKEY_AppUserModel_ID, Packaging::Appx::ParseApplicationUserModelId,
        },
        System::Com::{CoCreateInstance, CoTaskMemFree, IBindCtx, CLSCTX_INPROC_SERVER},
        UI::Shell::{
            BHID_EnumItems, FOLDERID_AppsFolder, IEnumShellItems, IKnownFolderManager, IShellItem,
            IShellItem2, KnownFolderManager, SIGDN_NORMALDISPLAY,
        },
    },
};

use super::{
    packaged_app_id, Application, ApplicationLaunchTarget, DiscoveryDiagnostics, DiscoveryError,
    DiscoverySnapshot,
};

struct RawPackagedEntry {
    display_name: Option<String>,
    aumid: Option<String>,
    icon: Option<String>,
}

struct ShellString<F: FnOnce(*mut u16)> {
    pointer: *mut u16,
    free: Option<F>,
}

impl<F: FnOnce(*mut u16)> Drop for ShellString<F> {
    fn drop(&mut self) {
        self.free
            .take()
            .expect("shell string deallocator is missing")(self.pointer);
    }
}

fn shell_string_with<F>(raw: PWSTR, free: F) -> Result<String, ()>
where
    F: FnOnce(*mut u16),
{
    let owned = ShellString {
        pointer: raw.0,
        free: Some(free),
    };
    if owned.pointer.is_null() {
        return Err(());
    }

    let mut length = 0;
    unsafe {
        while *owned.pointer.add(length) != 0 {
            length += 1;
        }
        if length == 0 {
            return Err(());
        }
        String::from_utf16(std::slice::from_raw_parts(owned.pointer, length)).map_err(|_| ())
    }
}

fn is_packaged_aumid(aumid: &str) -> bool {
    let encoded: Vec<u16> = aumid.encode_utf16().chain([0]).collect();
    let mut family_length = 0_u32;
    let mut application_length = 0_u32;
    let first = unsafe {
        ParseApplicationUserModelId(
            PCWSTR(encoded.as_ptr()),
            &mut family_length,
            None,
            &mut application_length,
            None,
        )
    };
    if first != ERROR_INSUFFICIENT_BUFFER || family_length == 0 || application_length == 0 {
        return false;
    }

    let mut family = vec![0_u16; family_length as usize];
    let mut application = vec![0_u16; application_length as usize];
    unsafe {
        ParseApplicationUserModelId(
            PCWSTR(encoded.as_ptr()),
            &mut family_length,
            Some(PWSTR(family.as_mut_ptr())),
            &mut application_length,
            Some(PWSTR(application.as_mut_ptr())),
        ) == ERROR_SUCCESS
    }
}

fn enumerate_with<T, N>(mut next: N) -> Result<Vec<T>, DiscoveryError>
where
    N: FnMut() -> Result<Option<T>, DiscoveryError>,
{
    let mut items = Vec::new();
    loop {
        match next()? {
            Some(item) => items.push(item),
            None => return Ok(items),
        }
    }
}

fn classify_next<T>(
    status: windows::core::Result<()>,
    fetched: u32,
    item: Option<T>,
) -> Result<Option<T>, DiscoveryError> {
    status.map_err(|_| DiscoveryError::AppsFolderEnumeration)?;
    match (fetched, item) {
        (0, None) => Ok(None),
        (1, Some(item)) => Ok(Some(item)),
        _ => Err(DiscoveryError::AppsFolderEnumeration),
    }
}

fn next_shell_item(items: &IEnumShellItems) -> Result<Option<IShellItem>, DiscoveryError> {
    let mut slot = [None];
    let mut fetched = 0_u32;
    let status = unsafe { items.Next(&mut slot, Some(&mut fetched)) };
    classify_next(status, fetched, slot[0].take())
}

fn read_shell_string(result: windows::core::Result<PWSTR>) -> Option<String> {
    let raw = result.ok()?;
    shell_string_with(raw, |pointer| unsafe {
        CoTaskMemFree(Some(pointer.cast::<c_void>()))
    })
    .ok()
}

fn raw_entry(item: &IShellItem) -> RawPackagedEntry {
    let display_name = read_shell_string(unsafe { item.GetDisplayName(SIGDN_NORMALDISPLAY) });
    let aumid = display_name.as_ref().and_then(|_| {
        let item: IShellItem2 = item.cast().ok()?;
        read_shell_string(unsafe { item.GetString(&PKEY_AppUserModel_ID) })
    });
    RawPackagedEntry {
        display_name,
        aumid,
        icon: super::icon::from_shell_item(item),
    }
}

pub(super) fn discover() -> Result<DiscoverySnapshot, DiscoveryError> {
    let manager: IKnownFolderManager = unsafe {
        CoCreateInstance(&KnownFolderManager, None, CLSCTX_INPROC_SERVER)
            .map_err(|_| DiscoveryError::AppsFolderUnavailable)?
    };
    let folder = unsafe { manager.GetFolder(&FOLDERID_AppsFolder) }
        .map_err(|_| DiscoveryError::AppsFolderUnavailable)?;
    let root: IShellItem =
        unsafe { folder.GetShellItem(0) }.map_err(|_| DiscoveryError::AppsFolderUnavailable)?;
    let items: IEnumShellItems = unsafe {
        root.BindToHandler(None::<&IBindCtx>, &BHID_EnumItems)
            .map_err(|_| DiscoveryError::AppsFolderUnavailable)?
    };
    let raw = enumerate_with(|| next_shell_item(&items))?
        .iter()
        .map(raw_entry)
        .collect::<Vec<_>>();
    snapshot_from_raw(raw, is_packaged_aumid)
}

fn snapshot_from_raw<I, V>(entries: I, mut validate: V) -> Result<DiscoverySnapshot, DiscoveryError>
where
    I: IntoIterator<Item = RawPackagedEntry>,
    V: FnMut(&str) -> bool,
{
    let mut diagnostics = DiscoveryDiagnostics::default();
    let mut candidates = Vec::new();
    for entry in entries {
        let Some(display_name) = entry
            .display_name
            .filter(|display_name| !display_name.trim().is_empty())
        else {
            diagnostics.invalid_packaged_names += 1;
            continue;
        };
        let Some(aumid) = entry.aumid.filter(|aumid| validate(aumid)) else {
            diagnostics.invalid_packaged_aumids += 1;
            continue;
        };
        candidates.push((aumid.to_lowercase(), display_name, aumid, entry.icon));
    }
    candidates.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| left.1.to_lowercase().cmp(&right.1.to_lowercase()))
            .then_with(|| left.1.cmp(&right.1))
    });

    let mut aumids = HashSet::with_capacity(candidates.len());
    let mut applications = Vec::with_capacity(candidates.len());
    for (normalized_aumid, display_name, aumid, icon) in candidates {
        if !aumids.insert(normalized_aumid) {
            continue;
        }
        applications.push(Application {
            app_id: packaged_app_id(&aumid)?,
            display_name,
            target: ApplicationLaunchTarget::PackagedApp { aumid },
            icon,
            use_count: 0,
        });
    }

    Ok(DiscoverySnapshot {
        applications,
        diagnostics,
    })
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use windows::{
        core::{Error, PWSTR},
        Win32::Foundation::E_FAIL,
    };

    use super::{
        classify_next, enumerate_with, is_packaged_aumid, shell_string_with, snapshot_from_raw,
        RawPackagedEntry,
    };
    use crate::apps::{ApplicationLaunchTarget, DiscoveryError};

    fn raw(display_name: Option<&str>, aumid: Option<&str>) -> RawPackagedEntry {
        RawPackagedEntry {
            display_name: display_name.map(str::to_owned),
            aumid: aumid.map(str::to_owned),
            icon: None,
        }
    }

    fn candidates() -> Vec<RawPackagedEntry> {
        vec![
            raw(None, Some("MissingName_family!App")),
            raw(Some("  "), Some("EmptyName_family!App")),
            raw(Some("Missing AUMID"), None),
            raw(Some("Invalid AUMID"), Some("not-packaged")),
            raw(
                Some("Calculator"),
                Some("Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"),
            ),
            raw(
                Some("calculator duplicate"),
                Some("MICROSOFT.WINDOWSCALCULATOR_8WEKYB3D8BBWE!APP"),
            ),
            raw(
                Some("Calculator"),
                Some("Contoso.Calculator_1234567890abc!App"),
            ),
        ]
    }

    #[test]
    fn bad_entries_are_counted_and_valid_aumids_are_deduplicated() {
        let first = snapshot_from_raw(candidates(), |aumid| aumid.contains('!')).unwrap();
        let mut reversed = candidates();
        reversed.reverse();
        let second = snapshot_from_raw(reversed, |aumid| aumid.contains('!')).unwrap();

        assert_eq!(first.diagnostics.invalid_packaged_names, 2);
        assert_eq!(first.diagnostics.invalid_packaged_aumids, 2);
        assert_eq!(first.applications, second.applications);
        assert_eq!(first.applications.len(), 2);
        assert_eq!(
            first
                .applications
                .iter()
                .filter(|application| application.display_name == "Calculator")
                .count(),
            2
        );
        assert!(first
            .applications
            .iter()
            .all(|application| application.icon.is_none()));
        assert!(first.applications.iter().all(|application| matches!(
            application.target,
            ApplicationLaunchTarget::PackagedApp { .. }
        )));
        assert_ne!(first.applications[0].app_id, first.applications[1].app_id);
    }

    #[test]
    fn packaged_icon_survives_validation_and_deduplication() {
        let retained_icon = "data:image/png;base64,QWxwaGE=".to_owned();
        let snapshot = snapshot_from_raw(
            [
                RawPackagedEntry {
                    display_name: Some("Zulu Calculator".into()),
                    aumid: Some("Microsoft.WindowsCalculator_8wekyb3d8bbwe!App".into()),
                    icon: Some("data:image/png;base64,WnVsdQ==".into()),
                },
                RawPackagedEntry {
                    display_name: Some("Alpha Calculator".into()),
                    aumid: Some("MICROSOFT.WINDOWSCALCULATOR_8WEKYB3D8BBWE!APP".into()),
                    icon: Some(retained_icon.clone()),
                },
            ],
            |_| true,
        )
        .unwrap();

        assert_eq!(snapshot.applications.len(), 1);
        assert_eq!(snapshot.applications[0].display_name, "Alpha Calculator");
        assert_eq!(snapshot.applications[0].icon, Some(retained_icon));
    }

    #[test]
    fn shell_strings_are_freed_on_every_return_path() {
        let mut valid = ['A' as u16, 0];
        let frees = Cell::new(0);
        assert_eq!(
            shell_string_with(PWSTR(valid.as_mut_ptr()), |_| frees.set(frees.get() + 1)),
            Ok("A".into())
        );
        assert_eq!(frees.get(), 1);

        let mut empty = [0_u16];
        let mut invalid = [0xd800_u16, 0];
        for pointer in [
            std::ptr::null_mut(),
            empty.as_mut_ptr(),
            invalid.as_mut_ptr(),
        ] {
            let frees = Cell::new(0);
            assert_eq!(
                shell_string_with(PWSTR(pointer), |_| frees.set(frees.get() + 1)),
                Err(())
            );
            assert_eq!(frees.get(), 1);
        }
    }

    #[test]
    fn packaged_aumid_validation_rejects_non_packaged_values() {
        assert!(is_packaged_aumid(
            "Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"
        ));
        for invalid in [
            "",
            "Microsoft.WindowsCalculator",
            "shell:AppsFolder",
            "Microsoft.AutoGenerated.{FB09042A-F244-C006-E678-F30D488ABF7C}",
        ] {
            assert!(
                !is_packaged_aumid(invalid),
                "unexpected valid AUMID: {invalid}"
            );
        }
    }

    #[test]
    fn enumeration_distinguishes_end_from_error_without_partial_success() {
        let mut complete = [Ok(Some("first")), Ok(Some("second")), Ok(None)].into_iter();
        assert_eq!(
            enumerate_with(|| complete.next().unwrap()).unwrap(),
            ["first", "second"]
        );

        let mut failed = [
            Ok(Some("partial")),
            Err(DiscoveryError::AppsFolderEnumeration),
        ]
        .into_iter();
        assert_eq!(
            enumerate_with(|| failed.next().unwrap()),
            Err(DiscoveryError::AppsFolderEnumeration)
        );
    }

    #[test]
    fn native_next_status_requires_a_consistent_fetched_item() {
        assert_eq!(classify_next(Ok(()), 0, None::<&str>), Ok(None));
        assert_eq!(classify_next(Ok(()), 1, Some("item")), Ok(Some("item")));
        assert_eq!(
            classify_next(Err(Error::from_hresult(E_FAIL)), 0, None::<&str>),
            Err(DiscoveryError::AppsFolderEnumeration)
        );
        for (fetched, item) in [(0, Some("item")), (1, None), (2, Some("item"))] {
            assert_eq!(
                classify_next(Ok(()), fetched, item),
                Err(DiscoveryError::AppsFolderEnumeration)
            );
        }
    }
}
