const TRUSTED_INSTALLER: &str = "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464";

fn trusted_writer(sid: &str) -> bool {
    matches!(sid, "S-1-5-18" | "S-1-5-32-544") || sid == TRUSTED_INSTALLER
}

fn unsafe_grant(mask: u32, sid: &str, ancestor: bool) -> bool {
    // WRITE_DATA/APPEND_DATA on an ancestor can create siblings, but cannot
    // replace an existing protected child. DELETE_CHILD still must be denied.
    let mutation = if ancestor { 0x500d0150 } else { 0x500d0156 };
    !trusted_writer(sid) && mask & mutation != 0
}

#[cfg(windows)]
mod native;

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
