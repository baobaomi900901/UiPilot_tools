use std::fmt;
use std::time::Instant;

const QUERY2_HEADER_LEN: usize = 28;
const LIST2_HEADER_LEN: usize = 20;
const LIST2_ITEM_LEN: usize = 8;

const REQUEST_NAME: u32 = 0x0000_0001;
const REQUEST_PATH: u32 = 0x0000_0002;
const REQUEST_FULL_PATH_AND_NAME: u32 = 0x0000_0004;
const REQUEST_SIZE: u32 = 0x0000_0010;
const REQUEST_DATE_MODIFIED: u32 = 0x0000_0040;
const REQUEST_ATTRIBUTES: u32 = 0x0000_0100;
const SUPPORTED_REQUEST_FLAGS: u32 = REQUEST_NAME
    | REQUEST_PATH
    | REQUEST_FULL_PATH_AND_NAME
    | REQUEST_SIZE
    | REQUEST_DATE_MODIFIED
    | REQUEST_ATTRIBUTES;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EverythingQuerySpec {
    pub search: Vec<u16>,
    pub offset: u32,
    pub max_results: u32,
    pub request_flags: u32,
    pub sort: EverythingSort,
    pub deadline: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueryReplyRoute {
    pub reply_hwnd: u32,
    pub reply_copydata_message: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct List2ReplyContract {
    pub offset: u32,
    pub max_results: u32,
    pub request_flags: u32,
    pub sort_type: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EverythingSort {
    DateModifiedAscending,
    DateModifiedDescending,
}

impl EverythingSort {
    fn wire_value(self) -> u32 {
        match self {
            Self::DateModifiedAscending => 13,
            Self::DateModifiedDescending => 14,
        }
    }
}

impl From<&EverythingQuerySpec> for List2ReplyContract {
    fn from(spec: &EverythingQuerySpec) -> Self {
        Self {
            offset: spec.offset,
            max_results: spec.max_results,
            request_flags: spec.request_flags,
            sort_type: spec.sort.wire_value(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EverythingQueryResult {
    pub total: u32,
    pub request_flags: u32,
    pub sort_type: u32,
    pub items: Vec<EverythingResultItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EverythingResultItem {
    pub full_path: String,
    pub file_name: String,
    pub attributes: u32,
    pub size_bytes: Option<u64>,
    pub modified_filetime: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolError {
    InvalidReplyRoute,
    EmbeddedQueryNul,
    LengthOverflow,
    PayloadTooShort,
    ReplyContractMismatch,
    UnsupportedRequestFlags,
    ItemTableOutOfBounds,
    ItemDataOffsetOutOfBounds,
    DuplicateItemDataOffset,
    FieldOutOfBounds,
    MissingUtf16Terminator,
    InvalidUtf16,
    EmbeddedUtf16Nul,
    MissingFileName,
    MissingFullPath,
    MissingAttributes,
    TrailingItemData,
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidReplyRoute => "invalid reply route",
            Self::EmbeddedQueryNul => "query contains an embedded nul",
            Self::LengthOverflow => "protocol length overflow",
            Self::PayloadTooShort => "protocol payload is too short",
            Self::ReplyContractMismatch => "reply does not match the query contract",
            Self::UnsupportedRequestFlags => "unsupported request flags",
            Self::ItemTableOutOfBounds => "item table is out of bounds",
            Self::ItemDataOffsetOutOfBounds => "item data offset is out of bounds",
            Self::DuplicateItemDataOffset => "duplicate item data offset",
            Self::FieldOutOfBounds => "item field is out of bounds",
            Self::MissingUtf16Terminator => "utf-16 field terminator is missing",
            Self::InvalidUtf16 => "utf-16 field is invalid",
            Self::EmbeddedUtf16Nul => "utf-16 field contains an embedded nul",
            Self::MissingFileName => "result item is missing its file name",
            Self::MissingFullPath => "result item is missing its full path",
            Self::MissingAttributes => "result item is missing its attributes",
            Self::TrailingItemData => "result item contains trailing data",
        })
    }
}

impl std::error::Error for ProtocolError {}

pub fn encode_query2(
    spec: &EverythingQuerySpec,
    route: QueryReplyRoute,
) -> Result<Vec<u8>, ProtocolError> {
    if route.reply_hwnd == 0 || route.reply_copydata_message == 0 {
        return Err(ProtocolError::InvalidReplyRoute);
    }
    if spec.search.contains(&0) {
        return Err(ProtocolError::EmbeddedQueryNul);
    }
    if spec.request_flags & !SUPPORTED_REQUEST_FLAGS != 0 {
        return Err(ProtocolError::UnsupportedRequestFlags);
    }

    let search_units = spec
        .search
        .len()
        .checked_add(1)
        .ok_or(ProtocolError::LengthOverflow)?;
    let search_bytes = search_units
        .checked_mul(2)
        .ok_or(ProtocolError::LengthOverflow)?;
    let total_len = QUERY2_HEADER_LEN
        .checked_add(search_bytes)
        .ok_or(ProtocolError::LengthOverflow)?;
    if let Ok(max_wire_len) = usize::try_from(u32::MAX) {
        if total_len > max_wire_len {
            return Err(ProtocolError::LengthOverflow);
        }
    }

    let mut encoded = Vec::with_capacity(total_len);
    push_u32(&mut encoded, route.reply_hwnd);
    push_u32(&mut encoded, route.reply_copydata_message);
    push_u32(&mut encoded, 0);
    push_u32(&mut encoded, spec.offset);
    push_u32(&mut encoded, spec.max_results);
    push_u32(&mut encoded, spec.request_flags);
    push_u32(&mut encoded, spec.sort.wire_value());
    for unit in &spec.search {
        encoded.extend_from_slice(&unit.to_le_bytes());
    }
    encoded.extend_from_slice(&0u16.to_le_bytes());
    Ok(encoded)
}

pub fn decode_list2_payload(
    payload: &[u8],
    contract: List2ReplyContract,
) -> Result<EverythingQueryResult, ProtocolError> {
    if payload.len() < LIST2_HEADER_LEN {
        return Err(ProtocolError::PayloadTooShort);
    }

    let total = read_u32_at(payload, 0, payload.len())?;
    let num_items = read_u32_at(payload, 4, payload.len())?;
    let result_offset = read_u32_at(payload, 8, payload.len())?;
    let request_flags = read_u32_at(payload, 12, payload.len())?;
    let sort_type = read_u32_at(payload, 16, payload.len())?;
    if result_offset != contract.offset
        || num_items > contract.max_results
        || request_flags != contract.request_flags
        || sort_type != contract.sort_type
    {
        return Err(ProtocolError::ReplyContractMismatch);
    }
    if request_flags & !SUPPORTED_REQUEST_FLAGS != 0 {
        return Err(ProtocolError::UnsupportedRequestFlags);
    }

    let item_count = usize::try_from(num_items).map_err(|_| ProtocolError::LengthOverflow)?;
    let item_table_bytes = item_count
        .checked_mul(LIST2_ITEM_LEN)
        .ok_or(ProtocolError::LengthOverflow)?;
    let item_table_end = LIST2_HEADER_LEN
        .checked_add(item_table_bytes)
        .ok_or(ProtocolError::LengthOverflow)?;
    if item_table_end > payload.len() {
        return Err(ProtocolError::ItemTableOutOfBounds);
    }
    if item_count == 0 {
        if payload.len() != LIST2_HEADER_LEN {
            return Err(ProtocolError::TrailingItemData);
        }
        return Ok(EverythingQueryResult {
            total,
            request_flags,
            sort_type,
            items: Vec::new(),
        });
    }

    let mut records = Vec::with_capacity(item_count);
    for index in 0..item_count {
        let table_offset = index
            .checked_mul(LIST2_ITEM_LEN)
            .and_then(|value| LIST2_HEADER_LEN.checked_add(value))
            .ok_or(ProtocolError::LengthOverflow)?;
        let flags = read_u32_at(payload, table_offset, item_table_end)?;
        let data_offset_field = table_offset
            .checked_add(4)
            .ok_or(ProtocolError::LengthOverflow)?;
        let data_offset = read_u32_at(payload, data_offset_field, item_table_end)?;
        let data_offset =
            usize::try_from(data_offset).map_err(|_| ProtocolError::LengthOverflow)?;
        if data_offset < item_table_end || data_offset >= payload.len() {
            return Err(ProtocolError::ItemDataOffsetOutOfBounds);
        }
        records.push(ItemRecord {
            flags,
            data_offset,
            data_end: payload.len(),
        });
    }

    let mut ordered_offsets: Vec<(usize, usize)> = records
        .iter()
        .enumerate()
        .map(|(index, record)| (record.data_offset, index))
        .collect();
    ordered_offsets.sort_unstable_by_key(|entry| entry.0);
    for pair in ordered_offsets.windows(2) {
        if pair[0].0 == pair[1].0 {
            return Err(ProtocolError::DuplicateItemDataOffset);
        }
        records[pair[0].1].data_end = pair[1].0;
    }

    let mut items = Vec::with_capacity(item_count);
    for record in records {
        items.push(parse_item(payload, record, request_flags)?);
    }

    Ok(EverythingQueryResult {
        total,
        request_flags,
        sort_type,
        items,
    })
}

#[derive(Debug, Clone, Copy)]
struct ItemRecord {
    flags: u32,
    data_offset: usize,
    data_end: usize,
}

fn parse_item(
    payload: &[u8],
    record: ItemRecord,
    request_flags: u32,
) -> Result<EverythingResultItem, ProtocolError> {
    let _item_flags = record.flags;
    let mut cursor = record.data_offset;
    let mut file_name = None;
    let mut full_path = None;
    let mut size_bytes = None;
    let mut modified_filetime = None;
    let mut attributes = None;

    if request_flags & REQUEST_NAME != 0 {
        let (value, next) = read_utf16_field(payload, cursor, record.data_end)?;
        file_name = Some(value);
        cursor = next;
    }
    if request_flags & REQUEST_PATH != 0 {
        let (_, next) = read_utf16_field(payload, cursor, record.data_end)?;
        cursor = next;
    }
    if request_flags & REQUEST_FULL_PATH_AND_NAME != 0 {
        let (value, next) = read_utf16_field(payload, cursor, record.data_end)?;
        full_path = Some(value);
        cursor = next;
    }
    if request_flags & REQUEST_SIZE != 0 {
        size_bytes = Some(read_u64_at(payload, cursor, record.data_end)?);
        cursor = cursor.checked_add(8).ok_or(ProtocolError::LengthOverflow)?;
    }
    if request_flags & REQUEST_DATE_MODIFIED != 0 {
        modified_filetime = Some(read_u64_at(payload, cursor, record.data_end)?);
        cursor = cursor.checked_add(8).ok_or(ProtocolError::LengthOverflow)?;
    }
    if request_flags & REQUEST_ATTRIBUTES != 0 {
        attributes = Some(read_u32_at(payload, cursor, record.data_end)?);
        cursor = cursor.checked_add(4).ok_or(ProtocolError::LengthOverflow)?;
    }
    if cursor != record.data_end {
        return Err(ProtocolError::TrailingItemData);
    }

    Ok(EverythingResultItem {
        full_path: full_path.ok_or(ProtocolError::MissingFullPath)?,
        file_name: file_name.ok_or(ProtocolError::MissingFileName)?,
        attributes: attributes.ok_or(ProtocolError::MissingAttributes)?,
        size_bytes,
        modified_filetime,
    })
}

fn read_utf16_field(
    payload: &[u8],
    offset: usize,
    limit: usize,
) -> Result<(String, usize), ProtocolError> {
    let length = read_u32_at(payload, offset, limit)?;
    let length = usize::try_from(length).map_err(|_| ProtocolError::LengthOverflow)?;
    let content_offset = offset.checked_add(4).ok_or(ProtocolError::LengthOverflow)?;
    let content_bytes = length.checked_mul(2).ok_or(ProtocolError::LengthOverflow)?;
    let content_end = content_offset
        .checked_add(content_bytes)
        .ok_or(ProtocolError::LengthOverflow)?;
    let field_end = content_end
        .checked_add(2)
        .ok_or(ProtocolError::LengthOverflow)?;
    if field_end > limit || field_end > payload.len() {
        return Err(ProtocolError::FieldOutOfBounds);
    }
    if payload[content_end..field_end] != [0, 0] {
        return Err(ProtocolError::MissingUtf16Terminator);
    }

    let content = &payload[content_offset..content_end];
    let mut units = Vec::with_capacity(length);
    for chunk in content.chunks_exact(2) {
        units.push(u16::from_le_bytes([chunk[0], chunk[1]]));
    }
    if units.contains(&0) {
        return Err(ProtocolError::EmbeddedUtf16Nul);
    }
    let value = String::from_utf16(&units).map_err(|_| ProtocolError::InvalidUtf16)?;
    Ok((value, field_end))
}

fn read_u32_at(payload: &[u8], offset: usize, limit: usize) -> Result<u32, ProtocolError> {
    let bytes = read_bytes(payload, offset, 4, limit)?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn read_u64_at(payload: &[u8], offset: usize, limit: usize) -> Result<u64, ProtocolError> {
    let bytes = read_bytes(payload, offset, 8, limit)?;
    Ok(u64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ]))
}

fn read_bytes(
    payload: &[u8],
    offset: usize,
    length: usize,
    limit: usize,
) -> Result<&[u8], ProtocolError> {
    let end = offset
        .checked_add(length)
        .ok_or(ProtocolError::LengthOverflow)?;
    if end > limit || end > payload.len() {
        return Err(ProtocolError::FieldOutOfBounds);
    }
    Ok(&payload[offset..end])
}

fn push_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}
