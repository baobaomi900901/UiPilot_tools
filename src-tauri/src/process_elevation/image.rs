//! Held file/ancestor identity and digest, not a package ACL or publisher verdict.
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::Read,
    os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
    path::{Path, PathBuf},
};
use windows::{
    core::{PCWSTR, PWSTR},
    Win32::{
        Foundation::{GENERIC_READ, HANDLE, WAIT_TIMEOUT},
        Storage::FileSystem::{
            GetDriveTypeW, GetFileInformationByHandle, GetFinalPathNameByHandleW,
            GetVolumeInformationByHandleW, BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY,
            FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
            FILE_READ_ATTRIBUTES, FILE_SHARE_READ, VOLUME_NAME_GUID,
        },
        System::Threading::{QueryFullProcessImageNameW, WaitForSingleObject, PROCESS_NAME_WIN32},
    },
};

const MAX_IMAGE_BYTES: u64 = 512 * 1024 * 1024;
type ImageResult<T> = Result<T, &'static str>;

pub(crate) struct ImageLease {
    file: File,
    directories: Vec<File>,
    path: PathBuf,
    identity: (u32, u64),
}

impl ImageLease {
    pub(crate) fn acquire(path: &Path, digest: &str) -> ImageResult<Self> {
        if digest.len() != 64
            || !digest
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err("UntrustedInstallation");
        }
        let mut lease = lock_path(path)?;
        let size = lease.file.metadata().map_err(|_| "ImageUnavailable")?.len();
        if size > MAX_IMAGE_BYTES {
            return Err("ImageTooLarge");
        }
        let mut hasher = Sha256::new();
        let read = std::io::copy(
            &mut (&mut lease.file).take(MAX_IMAGE_BYTES + 1),
            &mut hasher,
        )
        .map_err(|_| "ImageUnavailable")?;
        if read != size || read > MAX_IMAGE_BYTES {
            return Err("ImageChanged");
        }
        let actual = hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        if actual != digest {
            return Err("ImageDigestMismatch");
        }
        if identity(&lease.file)? != lease.identity {
            return Err("ImageChanged");
        }
        Ok(lease)
    }
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
    pub(crate) fn verify_installation_permissions(&self) -> ImageResult<()> {
        use super::permissions::{verify_held_file, ObjectRole};
        for (index, directory) in self.directories.iter().enumerate() {
            let role = if index + 1 == self.directories.len() {
                ObjectRole::PackageDirectory
            } else {
                ObjectRole::Ancestor
            };
            verify_held_file(directory, role)?;
        }
        verify_held_file(&self.file, ObjectRole::Image)
    }
    pub(crate) fn verify_process_image(&self, process: HANDLE) -> ImageResult<()> {
        if unsafe { WaitForSingleObject(process, 0) } != WAIT_TIMEOUT {
            return Err("PeerNotAlive");
        }
        let mut buffer = vec![0u16; 32768];
        let mut len = buffer.len() as u32;
        unsafe {
            QueryFullProcessImageNameW(
                process,
                PROCESS_NAME_WIN32,
                PWSTR(buffer.as_mut_ptr()),
                &mut len,
            )
        }
        .map_err(|_| "PeerImageUnavailable")?;
        if len == 0 || len as usize >= buffer.len() {
            return Err("PeerImageUnavailable");
        }
        let image =
            String::from_utf16(&buffer[..len as usize]).map_err(|_| "PeerImageUnavailable")?;
        let observed = lock_path(Path::new(&image))?;
        if observed.identity != self.identity
            || !observed
                .path
                .to_string_lossy()
                .eq_ignore_ascii_case(&self.path.to_string_lossy())
        {
            return Err("PeerImageMismatch");
        }
        if unsafe { WaitForSingleObject(process, 0) } != WAIT_TIMEOUT {
            return Err("PeerNotAlive");
        }
        Ok(())
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}
fn raw(file: &File) -> HANDLE {
    HANDLE(file.as_raw_handle())
}

fn metadata(file: &File) -> ImageResult<BY_HANDLE_FILE_INFORMATION> {
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    unsafe { GetFileInformationByHandle(raw(file), &mut info) }.map_err(|_| "ImageUnavailable")?;
    Ok(info)
}
fn identity(file: &File) -> ImageResult<(u32, u64)> {
    let info = metadata(file)?;
    Ok((
        info.dwVolumeSerialNumber,
        (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
    ))
}
fn final_path(file: &File) -> ImageResult<String> {
    let mut buffer = vec![0u16; 32768];
    let len =
        unsafe { GetFinalPathNameByHandleW(raw(file), &mut buffer, VOLUME_NAME_GUID) } as usize;
    if len == 0 || len >= buffer.len() {
        return Err("ImagePathUnavailable");
    }
    String::from_utf16(&buffer[..len]).map_err(|_| "ImagePathRejected")
}
fn open_component(path: &str, directory: bool) -> ImageResult<File> {
    if path.encode_utf16().count() >= 32768 {
        return Err("ImagePathRejected");
    }
    let flags = FILE_FLAG_OPEN_REPARSE_POINT.0
        | if directory {
            FILE_FLAG_BACKUP_SEMANTICS.0
        } else {
            0
        };
    let file = OpenOptions::new()
        .read(true)
        .access_mode(if directory {
            FILE_READ_ATTRIBUTES.0 | 0x00020000 // READ_CONTROL for held-handle ACL inspection.
        } else {
            GENERIC_READ.0
        })
        .share_mode(FILE_SHARE_READ.0)
        .custom_flags(flags)
        .open(path)
        .map_err(|_| "ImageUnavailable")?;
    let info = metadata(&file)?;
    if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        return Err("ImagePathRejected");
    }
    if (info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0) != directory {
        return Err("ImagePathRejected");
    }
    Ok(file)
}
fn lock_path(path: &Path) -> ImageResult<ImageLease> {
    // Input is a local absolute DOS path; use the opened root's volume-GUID path
    // for all subsequent opens and launch. No dependence on a mutable drive alias.
    let value = path.to_str().ok_or("ImagePathRejected")?.replace('/', "\\");
    let bytes = value.as_bytes();
    if bytes.len() < 4
        || !bytes[0].is_ascii_alphabetic()
        || bytes[1..3] != *b":\\"
        || value.contains('\0')
    {
        return Err("ImagePathRejected");
    }
    let parts: Vec<_> = value[3..].split('\\').collect();
    if parts.iter().any(|part| {
        part.is_empty()
            || *part == "."
            || *part == ".."
            || part.contains(':')
            || part.ends_with(['.', ' '])
    }) {
        return Err("ImagePathRejected");
    }
    let root = open_component(&value[..3], true)?;
    let guid = final_path(&root)?;
    if !guid.starts_with(r"\\?\Volume{") || !guid.ends_with("}\\") {
        return Err("ImagePathRejected");
    }
    if unsafe { GetDriveTypeW(PCWSTR(wide(&guid).as_ptr())) } != 3 {
        return Err("ImagePathRejected");
    }
    let mut filesystem = [0u16; 64];
    unsafe {
        GetVolumeInformationByHandleW(raw(&root), None, None, None, None, Some(&mut filesystem))
    }
    .map_err(|_| "ImageVolumeUnavailable")?;
    let end = filesystem
        .iter()
        .position(|c| *c == 0)
        .ok_or("ImageVolumeUnavailable")?;
    if String::from_utf16_lossy(&filesystem[..end]) != "NTFS" {
        return Err("ImagePathRejected");
    }
    let mut directories = vec![root];
    let mut current = guid.trim_end_matches('\\').to_owned();
    for (index, part) in parts.iter().enumerate() {
        current.push('\\');
        current.push_str(part);
        let last = index + 1 == parts.len();
        let file = open_component(&current, !last)?;
        if !final_path(&file)?.eq_ignore_ascii_case(&current) {
            return Err("ImageChanged");
        }
        if last {
            let identity = identity(&file)?;
            return Ok(ImageLease {
                file,
                directories,
                path: current.into(),
                identity,
            });
        }
        directories.push(file);
    }
    Err("ImagePathRejected")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
    };
    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(1);
            let root = std::env::temp_dir().join(format!(
                "uipilot-image-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            let fixture = Self(root);
            fs::create_dir(fixture.0.join("sub")).unwrap();
            fs::write(fixture.path(), b"abc").unwrap();
            fixture
        }
        fn path(&self) -> PathBuf {
            self.0.join("sub/helper.exe")
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            for dir in ["sub", "moved"] {
                for file in [
                    "helper.exe",
                    "renamed.exe",
                    "uipilot-process-elevation-helper.exe",
                ] {
                    let _ = fs::remove_file(self.0.join(dir).join(file));
                }
                let _ = fs::remove_dir(self.0.join(dir));
            }
            let _ = fs::remove_dir(self.0.join("link"));
            let _ = fs::remove_dir(&self.0);
        }
    }
    #[test]
    fn verified_image_and_ancestors_remain_held_until_lease_drop() {
        let fixture = Fixture::new();
        let lease =
            ImageLease::acquire(&fixture.path(), ABC).unwrap_or_else(|code| panic!("{code}"));
        assert!(fs::OpenOptions::new()
            .write(true)
            .open(fixture.path())
            .is_err());
        assert!(fs::rename(fixture.path(), fixture.0.join("sub/renamed.exe")).is_err());
        assert!(fs::rename(fixture.0.join("sub"), fixture.0.join("moved")).is_err());
        drop(lease);
        fs::write(fixture.path(), b"changed").unwrap();
    }
    #[test]
    fn digest_mismatch_releases_all_file_and_directory_leases() {
        let fixture = Fixture::new();
        assert!(matches!(
            ImageLease::acquire(&fixture.path(), &"0".repeat(64)),
            Err("ImageDigestMismatch")
        ));
        fs::rename(fixture.0.join("sub"), fixture.0.join("moved")).unwrap();
    }

    #[test]
    fn matching_digest_in_user_owned_package_is_rejected_before_launch() {
        let fixture = Fixture::new();
        let helper = fixture.0.join("sub/uipilot-process-elevation-helper.exe");
        fs::rename(fixture.path(), &helper).unwrap();
        let package = crate::process_elevation::trust::TrustPackage::new(
            fixture.0.join("sub"),
            ABC.into(),
            ABC.into(),
        )
        .unwrap();
        assert!(matches!(
            package.lease_helper_image(),
            Err("UntrustedOwner" | "UntrustedWriteGrant")
        ));
        // Even the digest was correct; rejection must release the previously held file.
        fs::write(&helper, b"released").unwrap();
    }

    #[test]
    fn namespace_escape_and_stream_paths_are_rejected_before_open() {
        for path in [
            r"relative.exe",
            r"\\server\share\helper.exe",
            r"\\?\GLOBALROOT\Device\x",
            r"C:\temp\..\helper.exe",
            r"C:\temp\.\helper.exe",
            r"C:\temp\helper.exe:stream",
            r"C:\temp\helper.exe.",
            r"C:\temp\helper.exe ",
        ] {
            assert!(matches!(
                ImageLease::acquire(Path::new(path), ABC),
                Err("ImagePathRejected")
            ));
        }
    }

    #[test]
    fn junction_in_ancestor_chain_is_rejected() {
        use std::os::windows::ffi::OsStrExt;
        use windows::Win32::System::IO::DeviceIoControl;
        let fixture = Fixture::new();
        let link = fixture.0.join("link");
        fs::create_dir(&link).unwrap();
        let target = fixture.0.join("sub");
        let print: Vec<u8> = target
            .as_os_str()
            .encode_wide()
            .flat_map(u16::to_le_bytes)
            .collect();
        let substitute: Vec<u8> = format!(r"\??\{}", target.display())
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        let mut buffer = Vec::new();
        buffer.extend_from_slice(&0xa0000003u32.to_le_bytes()); // IO_REPARSE_TAG_MOUNT_POINT
        buffer.extend_from_slice(&((8 + substitute.len() + print.len() + 4) as u16).to_le_bytes());
        buffer.extend_from_slice(&0u16.to_le_bytes());
        for field in [
            0u16,
            substitute.len() as u16,
            (substitute.len() + 2) as u16,
            print.len() as u16,
        ] {
            buffer.extend_from_slice(&field.to_le_bytes());
        }
        buffer.extend_from_slice(&substitute);
        buffer.extend_from_slice(&[0, 0]);
        buffer.extend_from_slice(&print);
        buffer.extend_from_slice(&[0, 0]);
        let directory = OpenOptions::new()
            .write(true)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0 | FILE_FLAG_OPEN_REPARSE_POINT.0)
            .open(&link)
            .unwrap();
        let mut returned = 0;
        unsafe {
            DeviceIoControl(
                raw(&directory),
                0x000900a4,
                Some(buffer.as_ptr().cast()),
                buffer.len() as u32,
                None,
                0,
                Some(&mut returned),
                None,
            )
        }
        .unwrap(); // FSCTL_SET_REPARSE_POINT
        drop(directory);
        assert!(matches!(
            ImageLease::acquire(&link.join("helper.exe"), ABC),
            Err("ImagePathRejected")
        ));
    }

    #[test]
    fn process_image_binding_uses_the_held_volume_and_file_identity() {
        use windows::Win32::System::Threading::GetCurrentProcess;
        let current = std::env::current_exe().unwrap();
        let mut hasher = Sha256::new();
        std::io::copy(&mut File::open(&current).unwrap(), &mut hasher).unwrap();
        let digest = hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let lease = ImageLease::acquire(&current, &digest).unwrap_or_else(|code| panic!("{code}"));
        assert_eq!(
            lease.verify_process_image(unsafe { GetCurrentProcess() }),
            Ok(())
        );
        drop(lease);
        let fixture = Fixture::new();
        let other =
            ImageLease::acquire(&fixture.path(), ABC).unwrap_or_else(|code| panic!("{code}"));
        assert_eq!(
            other.verify_process_image(unsafe { GetCurrentProcess() }),
            Err("PeerImageMismatch")
        );
    }
}
