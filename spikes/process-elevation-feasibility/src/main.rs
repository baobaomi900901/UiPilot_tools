mod policy;
#[cfg(test)]
use policy::TRUSTED_INSTALLER;
use policy::{trusted_writer, unsafe_grant};

#[cfg(windows)]
mod native;

#[cfg(all(test, windows))]
mod pipe_probe;

#[cfg(all(test, windows, feature = "uac-probe"))]
mod uac_identity;

fn main() {
    #[cfg(windows)]
    let result = native::run();
    #[cfg(not(windows))]
    let result: Result<(), &'static str> = Err("PlatformUnsupported");
    match result {
        Ok(()) => println!("INSTALL_DIRECTORY_CHECK_PASS"),
        Err(code) => {
            eprintln!("{code}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_untrusted_content_delete_and_acl_writers() {
        for mask in [
            2, 4, 0x10, 0x40, 0x100, 0x10000, 0x40000, 0x80000, 0x40000000, 0x10000000,
        ] {
            assert!(unsafe_grant(mask, "S-1-5-21-123-456-789-1001", false));
        }
    }

    #[test]
    fn ancestors_allow_child_creation_but_not_replacement_or_acl_changes() {
        assert!(!unsafe_grant(6, "S-1-5-11", true));
        for mask in [0x40, 0x100, 0x10000, 0x40000, 0x80000] {
            assert!(unsafe_grant(mask, "S-1-5-11", true));
        }
    }

    #[test]
    fn read_access_and_trusted_writers_are_allowed() {
        assert!(!unsafe_grant(0x120089, "S-1-1-0", false));
        for sid in ["S-1-5-18", "S-1-5-32-544", TRUSTED_INSTALLER] {
            assert!(trusted_writer(sid));
            assert!(!unsafe_grant(u32::MAX, sid, false));
        }
        assert!(!trusted_writer("S-1-5-21-123-456-789-1001"));
    }
}
