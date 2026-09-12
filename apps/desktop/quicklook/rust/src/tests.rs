use super::*;

#[test]
fn rejects_empty_preview_input() {
    let result = hop_ql_render_preview_pdf(ptr::null(), 0);
    assert_eq!(result.status, HOP_QL_INVALID_INPUT);
    assert!(result.bytes.ptr.is_null());
}

#[test]
fn rejects_non_document_preview_input() {
    let data = b"not a hwp document";
    let result = hop_ql_render_preview_pdf(data.as_ptr(), data.len());
    assert_eq!(result.status, HOP_QL_PARSE_FAILED);
    assert!(result.bytes.ptr.is_null());
}

#[test]
fn reports_missing_embedded_thumbnail() {
    let data = b"not a hwp document";
    let result = hop_ql_extract_embedded_thumbnail(data.as_ptr(), data.len());
    assert_eq!(result.status, HOP_QL_NO_THUMBNAIL);
    assert!(result.bytes.ptr.is_null());
}

#[test]
fn free_accepts_null() {
    unsafe {
        hop_ql_free_bytes(ptr::null_mut(), 0);
    }
}

#[test]
fn renders_sample_preview_pdf() {
    let data = sample_hwp_bytes();
    let result = hop_ql_render_preview_pdf(data.as_ptr(), data.len());
    assert_eq!(result.status, HOP_QL_OK);
    assert!(result.page_count > 0);
    assert!(result.bytes.len > 0);
    assert!(!result.bytes.ptr.is_null());
    let pdf = unsafe { std::slice::from_raw_parts(result.bytes.ptr, result.bytes.len) };
    assert!(pdf.starts_with(b"%PDF-"));
    unsafe {
        hop_ql_free_bytes(result.bytes.ptr, result.bytes.len);
    }
}

#[test]
fn renders_sample_first_page_png() {
    let data = sample_hwp_bytes();
    let result = hop_ql_render_first_page_png(data.as_ptr(), data.len(), 512);
    assert_eq!(result.status, HOP_QL_OK);
    assert!(result.page_count > 0);
    assert!(result.bytes.len > 0);
    assert!(!result.bytes.ptr.is_null());
    unsafe {
        hop_ql_free_bytes(result.bytes.ptr, result.bytes.len);
    }
}

fn sample_hwp_bytes() -> Vec<u8> {
    let mut core = hop_rhwp_adapter::DocumentCore::new_empty();
    core.create_blank_document_native()
        .expect("create blank fixture document");
    core.export_hwp_native()
        .expect("serialize fixture hwp")
}
