pub(super) const TRUSTED_INSTALLER: &str =
    "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464";

pub(super) fn trusted_writer(sid: &str) -> bool {
    matches!(sid, "S-1-5-18" | "S-1-5-32-544") || sid == TRUSTED_INSTALLER
}

pub(super) fn unsafe_grant(mask: u32, sid: &str, ancestor: bool) -> bool {
    // Sibling creation does not replace an existing protected child.
    let mutation = if ancestor { 0x500d0150 } else { 0x500d0156 };
    !trusted_writer(sid) && mask & mutation != 0
}
