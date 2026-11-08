//! Bounded standard-ZIP metadata preflight. zip 2.4.2 deduplicates central names
//! in an IndexMap, so these checks must precede ZipArchive construction. Actual
//! decompression/CRC remains the zip crate's job. No ZIP64/multi-disk/prefixes.
use super::super::{
    manifest::PublicPackageError,
    package_rules::{
        canonical_relative_path, fold_path, MAX_DIRECTORIES, MAX_FILES, MAX_FILE_BYTES,
    },
};
use std::collections::HashSet;
type Result<T> = std::result::Result<T, PublicPackageError>;
fn invalid<T>() -> Result<T> {
    Err(PublicPackageError::InvalidPackage)
}
fn part(bytes: &[u8], start: usize, len: usize) -> Result<&[u8]> {
    bytes
        .get(
            start
                ..start
                    .checked_add(len)
                    .ok_or(PublicPackageError::InvalidPackage)?,
        )
        .ok_or(PublicPackageError::InvalidPackage)
}
fn u16le(bytes: &[u8], offset: usize) -> Result<usize> {
    Ok(u16::from_le_bytes(part(bytes, offset, 2)?.try_into().unwrap()) as usize)
}
fn u32le(bytes: &[u8], offset: usize) -> Result<usize> {
    Ok(u32::from_le_bytes(part(bytes, offset, 4)?.try_into().unwrap()) as usize)
}
fn ordinary_extra(mut extra: &[u8]) -> Result<()> {
    while !extra.is_empty() {
        let tag = u16le(extra, 0)?;
        let size = u16le(extra, 2)?;
        if tag == 1 {
            return invalid();
        }
        part(extra, 4, size)?;
        extra = &extra[4 + size..];
    }
    Ok(())
}
pub(super) fn standard_directory(bytes: &[u8]) -> Result<(usize, usize)> {
    if bytes.len() < 22 {
        return invalid();
    }
    let mut candidates = Vec::new();
    for offset in bytes.len().saturating_sub(65557)..=bytes.len() - 22 {
        if &bytes[offset..offset + 4] == b"PK\x05\x06"
            && offset + 22 + u16le(bytes, offset + 20)? == bytes.len()
        {
            candidates.push(offset);
        }
    }
    if candidates.len() != 1 {
        return invalid();
    }
    let end = candidates[0];
    if u16le(bytes, end + 4)? != 0 || u16le(bytes, end + 6)? != 0 {
        return invalid();
    }
    let count = u16le(bytes, end + 10)?;
    if count == 0 || count > MAX_FILES + MAX_DIRECTORIES || u16le(bytes, end + 8)? != count {
        return invalid();
    }
    let size = u32le(bytes, end + 12)?;
    let start = u32le(bytes, end + 16)?;
    if start.checked_add(size) != Some(end) {
        return invalid();
    }
    let mut at = start;
    let mut names = HashSet::new();
    let mut offsets = HashSet::new();
    for _ in 0..count {
        let header = part(bytes, at, 46)?;
        if &header[..4] != b"PK\x01\x02" {
            return invalid();
        }
        let name_len = u16le(header, 28)?;
        let extra_len = u16le(header, 30)?;
        let comment_len = u16le(header, 32)?;
        if u16le(header, 34)? != 0 || u32le(header, 24)? as u64 > MAX_FILE_BYTES {
            return invalid();
        }
        let name = part(bytes, at + 46, name_len)?;
        let text = std::str::from_utf8(name).map_err(|_| PublicPackageError::InvalidPackage)?;
        let canonical = canonical_relative_path(text.strip_suffix('/').unwrap_or(text))?;
        if !names.insert(fold_path(&canonical)) {
            return invalid();
        }
        ordinary_extra(part(bytes, at + 46 + name_len, extra_len)?)?;
        let local = u32le(header, 42)?;
        if !offsets.insert(local) {
            return invalid();
        }
        let local_header = part(bytes, local, 30)?;
        if &local_header[..4] != b"PK\x03\x04"
            || u16le(local_header, 6)? != u16le(header, 8)?
            || u16le(local_header, 8)? != u16le(header, 10)?
        {
            return invalid();
        }
        let local_name = u16le(local_header, 26)?;
        let local_extra = u16le(local_header, 28)?;
        if part(bytes, local + 30, local_name)? != name {
            return invalid();
        }
        ordinary_extra(part(bytes, local + 30 + local_name, local_extra)?)?;
        let data = local
            .checked_add(30 + local_name + local_extra)
            .ok_or(PublicPackageError::InvalidPackage)?;
        if data
            .checked_add(u32le(header, 20)?)
            .is_none_or(|finish| finish > start)
        {
            return invalid();
        }
        at = at
            .checked_add(46 + name_len + extra_len + comment_len)
            .filter(|next| *next <= end)
            .ok_or(PublicPackageError::InvalidPackage)?;
    }
    if at != end || !offsets.contains(&0) {
        return invalid();
    }
    Ok((count, start))
}
