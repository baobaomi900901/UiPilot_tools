use std::time::{Duration, Instant};

use everything_ipc::protocol::{
    decode_list2_payload, encode_query2, EverythingQueryResult, EverythingQuerySpec,
    EverythingSort, QueryReplyRoute,
};

const QUERY2_REPLY_HWND_OFFSET: usize = 0;
const QUERY2_REPLY_COPYDATA_MESSAGE_OFFSET: usize = 4;
const QUERY2_SEARCH_FLAGS_OFFSET: usize = 8;
const QUERY2_RESULT_OFFSET_OFFSET: usize = 12;
const QUERY2_MAX_RESULTS_OFFSET: usize = 16;
const QUERY2_REQUEST_FLAGS_OFFSET: usize = 20;
const QUERY2_SORT_TYPE_OFFSET: usize = 24;
const QUERY2_SEARCH_OFFSET: usize = 28;
const QUERY2_HEADER_LEN: usize = 28;

const LIST2_TOTAL_ITEMS_OFFSET: usize = 0;
const LIST2_NUM_ITEMS_OFFSET: usize = 4;
const LIST2_RESULT_OFFSET_OFFSET: usize = 8;
const LIST2_REQUEST_FLAGS_OFFSET: usize = 12;
const LIST2_SORT_TYPE_OFFSET: usize = 16;
const LIST2_HEADER_LEN: usize = 20;
const LIST2_ITEM_LEN: usize = 8;

const REQUEST_NAME: u32 = 0x0000_0001;
const REQUEST_PATH: u32 = 0x0000_0002;
const REQUEST_FULL_PATH_AND_NAME: u32 = 0x0000_0004;
const REQUEST_SIZE: u32 = 0x0000_0010;
const REQUEST_DATE_MODIFIED: u32 = 0x0000_0040;
const REQUEST_ATTRIBUTES: u32 = 0x0000_0100;
const SORT_DATE_MODIFIED_DESCENDING: u32 = 14;

fn push_u16(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_u64(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_utf16_field(bytes: &mut Vec<u8>, value: &str) {
    let units: Vec<u16> = value.encode_utf16().collect();
    push_u32(bytes, units.len() as u32);
    for unit in units {
        push_u16(bytes, unit);
    }
    push_u16(bytes, 0);
}

fn list2_header(
    total_items: u32,
    num_items: u32,
    result_offset: u32,
    request_flags: u32,
    sort_type: u32,
) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(LIST2_HEADER_LEN);
    push_u32(&mut bytes, total_items);
    push_u32(&mut bytes, num_items);
    push_u32(&mut bytes, result_offset);
    push_u32(&mut bytes, request_flags);
    push_u32(&mut bytes, sort_type);
    bytes
}

fn single_item_payload() -> Vec<u8> {
    let request_flags = REQUEST_NAME
        | REQUEST_FULL_PATH_AND_NAME
        | REQUEST_SIZE
        | REQUEST_DATE_MODIFIED
        | REQUEST_ATTRIBUTES;
    let mut bytes = list2_header(9, 1, 4, request_flags, SORT_DATE_MODIFIED_DESCENDING);
    push_u32(&mut bytes, 0);
    push_u32(&mut bytes, (LIST2_HEADER_LEN + LIST2_ITEM_LEN) as u32);
    push_utf16_field(&mut bytes, "needle.txt");
    push_utf16_field(&mut bytes, r"C:\tmp\needle.txt");
    push_u64(&mut bytes, 1_234_567_890);
    push_u64(&mut bytes, 0x01da_4f2b_89ab_cdef);
    push_u32(&mut bytes, 0x20);
    bytes
}

fn valid_query_spec() -> EverythingQuerySpec {
    EverythingQuerySpec {
        search: "A猫".encode_utf16().collect(),
        offset: 7,
        max_results: 200,
        request_flags: REQUEST_NAME
            | REQUEST_FULL_PATH_AND_NAME
            | REQUEST_SIZE
            | REQUEST_DATE_MODIFIED
            | REQUEST_ATTRIBUTES,
        sort: EverythingSort::DateModifiedDescending,
        deadline: Instant::now() + Duration::from_secs(1),
    }
}

fn valid_route() -> QueryReplyRoute {
    QueryReplyRoute {
        reply_hwnd: 0x1122_3344,
        reply_copydata_message: 0xa1b2_c3d4,
    }
}

#[test]
fn query2_encoding_matches_official_packed_offsets_and_total_length() {
    let spec = valid_query_spec();
    let route = valid_route();
    let encoded = encode_query2(&spec, route).expect("valid Query2 fixture must encode");

    let mut expected = Vec::new();
    push_u32(&mut expected, 0x1122_3344);
    push_u32(&mut expected, 0xa1b2_c3d4);
    push_u32(&mut expected, 0);
    push_u32(&mut expected, 7);
    push_u32(&mut expected, 200);
    push_u32(
        &mut expected,
        REQUEST_NAME
            | REQUEST_FULL_PATH_AND_NAME
            | REQUEST_SIZE
            | REQUEST_DATE_MODIFIED
            | REQUEST_ATTRIBUTES,
    );
    push_u32(&mut expected, SORT_DATE_MODIFIED_DESCENDING);
    push_u16(&mut expected, 0x0041);
    push_u16(&mut expected, 0x732b);
    push_u16(&mut expected, 0);

    assert_eq!(encoded, expected);
    assert_eq!(encoded.len(), QUERY2_HEADER_LEN + 6);
    assert_eq!(
        &encoded[QUERY2_REPLY_HWND_OFFSET..QUERY2_REPLY_HWND_OFFSET + 4],
        &0x1122_3344u32.to_le_bytes()
    );
    assert_eq!(
        &encoded[QUERY2_REPLY_COPYDATA_MESSAGE_OFFSET..QUERY2_REPLY_COPYDATA_MESSAGE_OFFSET + 4],
        &0xa1b2_c3d4u32.to_le_bytes()
    );
    assert_eq!(
        &encoded[QUERY2_SEARCH_FLAGS_OFFSET..QUERY2_SEARCH_FLAGS_OFFSET + 4],
        &0u32.to_le_bytes()
    );
    assert_eq!(
        &encoded[QUERY2_RESULT_OFFSET_OFFSET..QUERY2_RESULT_OFFSET_OFFSET + 4],
        &7u32.to_le_bytes()
    );
    assert_eq!(
        &encoded[QUERY2_MAX_RESULTS_OFFSET..QUERY2_MAX_RESULTS_OFFSET + 4],
        &200u32.to_le_bytes()
    );
    assert_eq!(
        &encoded[QUERY2_REQUEST_FLAGS_OFFSET..QUERY2_REQUEST_FLAGS_OFFSET + 4],
        &(REQUEST_NAME
            | REQUEST_FULL_PATH_AND_NAME
            | REQUEST_SIZE
            | REQUEST_DATE_MODIFIED
            | REQUEST_ATTRIBUTES)
            .to_le_bytes()
    );
    assert_eq!(
        &encoded[QUERY2_SORT_TYPE_OFFSET..QUERY2_SORT_TYPE_OFFSET + 4],
        &SORT_DATE_MODIFIED_DESCENDING.to_le_bytes()
    );
    assert_eq!(
        &encoded[QUERY2_SEARCH_OFFSET..],
        &[0x41, 0x00, 0x2b, 0x73, 0x00, 0x00]
    );
}

#[test]
fn query2_encoding_rejects_zero_reply_hwnd() {
    let route = QueryReplyRoute {
        reply_hwnd: 0,
        reply_copydata_message: 1,
    };
    assert!(encode_query2(&valid_query_spec(), route).is_err());
}

#[test]
fn query2_encoding_rejects_zero_reply_copydata_message() {
    let route = QueryReplyRoute {
        reply_hwnd: 1,
        reply_copydata_message: 0,
    };
    assert!(encode_query2(&valid_query_spec(), route).is_err());
}

#[test]
fn query2_encoding_rejects_embedded_utf16_terminator() {
    let mut spec = valid_query_spec();
    spec.search = vec![b'a' as u16, 0, b'b' as u16];
    assert!(encode_query2(&spec, valid_route()).is_err());
}

#[test]
fn list2_empty_result_preserves_actual_flags_and_sort_without_query_id() {
    let payload = list2_header(0, 0, 0, REQUEST_PATH, SORT_DATE_MODIFIED_DESCENDING);
    assert_eq!(payload.len(), LIST2_HEADER_LEN);
    assert_eq!(
        &payload[LIST2_TOTAL_ITEMS_OFFSET..LIST2_TOTAL_ITEMS_OFFSET + 4],
        &0u32.to_le_bytes()
    );
    assert_eq!(
        &payload[LIST2_NUM_ITEMS_OFFSET..LIST2_NUM_ITEMS_OFFSET + 4],
        &0u32.to_le_bytes()
    );
    assert_eq!(
        &payload[LIST2_RESULT_OFFSET_OFFSET..LIST2_RESULT_OFFSET_OFFSET + 4],
        &0u32.to_le_bytes()
    );
    assert_eq!(
        &payload[LIST2_REQUEST_FLAGS_OFFSET..LIST2_REQUEST_FLAGS_OFFSET + 4],
        &REQUEST_PATH.to_le_bytes()
    );
    assert_eq!(
        &payload[LIST2_SORT_TYPE_OFFSET..LIST2_SORT_TYPE_OFFSET + 4],
        &SORT_DATE_MODIFIED_DESCENDING.to_le_bytes()
    );

    let result = decode_list2_payload(&payload).expect("empty LIST2 fixture must decode");
    let EverythingQueryResult {
        total,
        request_flags,
        sort_type,
        items,
    } = result;
    assert_eq!(total, 0);
    assert_eq!(request_flags, REQUEST_PATH);
    assert_eq!(sort_type, SORT_DATE_MODIFIED_DESCENDING);
    assert!(items.is_empty());
}

#[test]
fn list2_single_item_decodes_requested_fields_in_wire_order() {
    let result = decode_list2_payload(&single_item_payload())
        .expect("single-item LIST2 fixture must decode");
    assert_eq!(result.total, 9);
    assert_eq!(
        result.request_flags,
        REQUEST_NAME
            | REQUEST_FULL_PATH_AND_NAME
            | REQUEST_SIZE
            | REQUEST_DATE_MODIFIED
            | REQUEST_ATTRIBUTES
    );
    assert_eq!(result.sort_type, SORT_DATE_MODIFIED_DESCENDING);
    assert_eq!(result.items.len(), 1);
    let item = &result.items[0];
    assert_eq!(item.file_name, "needle.txt");
    assert_eq!(item.full_path, r"C:\tmp\needle.txt");
    assert_eq!(item.attributes, 0x20);
    assert_eq!(item.size_bytes, Some(1_234_567_890));
    assert_eq!(item.modified_filetime, Some(0x01da_4f2b_89ab_cdef));
}

#[test]
fn list2_rejects_header_shorter_than_twenty_bytes() {
    assert!(decode_list2_payload(&[0u8; LIST2_HEADER_LEN - 1]).is_err());
}

#[test]
fn list2_rejects_odd_byte_utf16_field() {
    let mut payload = list2_header(1, 1, 0, REQUEST_NAME, 1);
    push_u32(&mut payload, 0);
    push_u32(&mut payload, (LIST2_HEADER_LEN + LIST2_ITEM_LEN) as u32);
    push_u32(&mut payload, 1);
    payload.push(b'A');
    assert!(decode_list2_payload(&payload).is_err());
}

#[test]
fn list2_rejects_item_data_offset_beyond_payload() {
    let mut payload = list2_header(1, 1, 0, REQUEST_NAME, 1);
    push_u32(&mut payload, 0);
    push_u32(&mut payload, 0x1000);
    assert!(decode_list2_payload(&payload).is_err());
}

#[test]
fn list2_rejects_truncated_later_field_offset() {
    let mut payload = list2_header(1, 1, 0, REQUEST_NAME | REQUEST_PATH, 1);
    push_u32(&mut payload, 0);
    push_u32(&mut payload, (LIST2_HEADER_LEN + LIST2_ITEM_LEN) as u32);
    push_utf16_field(&mut payload, "name");
    push_u32(&mut payload, 2);
    push_u16(&mut payload, b'C' as u16);
    assert!(decode_list2_payload(&payload).is_err());
}

#[test]
fn list2_rejects_huge_item_count_with_checked_multiply() {
    let payload = list2_header(0, u32::MAX, 0, 0, 1);
    assert!(decode_list2_payload(&payload).is_err());
}

#[test]
fn list2_rejects_max_data_offset_with_checked_add() {
    let mut payload = list2_header(1, 1, 0, REQUEST_NAME, 1);
    push_u32(&mut payload, 0);
    push_u32(&mut payload, u32::MAX);
    assert!(decode_list2_payload(&payload).is_err());
}

#[test]
fn list2_rejects_max_utf16_length_with_checked_multiply() {
    let mut payload = list2_header(1, 1, 0, REQUEST_NAME, 1);
    push_u32(&mut payload, 0);
    push_u32(&mut payload, (LIST2_HEADER_LEN + LIST2_ITEM_LEN) as u32);
    push_u32(&mut payload, u32::MAX);
    assert!(decode_list2_payload(&payload).is_err());
}

#[test]
fn list2_rejects_missing_utf16_terminator() {
    let mut payload = list2_header(1, 1, 0, REQUEST_NAME, 1);
    push_u32(&mut payload, 0);
    push_u32(&mut payload, (LIST2_HEADER_LEN + LIST2_ITEM_LEN) as u32);
    push_u32(&mut payload, 1);
    push_u16(&mut payload, b'A' as u16);
    assert!(decode_list2_payload(&payload).is_err());
}
