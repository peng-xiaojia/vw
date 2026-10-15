use vw_protocol::{EditSnapshot, TextFile};

#[test]
fn legacy_and_unicode_encodings_roundtrip_without_replacement() {
    for label in ["utf-8", "utf-8-bom", "utf-16le", "utf-16be", "gbk", "big5"] {
        let bytes = TextFile::encode("中文\r\nhello", label, false).unwrap();
        let file = TextFile::decode(&bytes, 4096, Some(label)).unwrap();
        assert!(file.editable(), "{label}");
        assert_eq!(file.text, "中文\r\nhello", "{label}");
        assert_eq!(file.serialize(&EditSnapshot::text(&file.text)).unwrap(), bytes, "{label}");
    }
    assert!(TextFile::encode("😀", "gbk", false).is_err());
    assert!(TextFile::decode(&[0xff], 1024, Some("utf-8")).is_err());
    assert!(TextFile::decode(b"hello", 1024, Some("invalid-codec")).is_err());
}

#[test]
fn utf16_bom_is_detected_automatically() {
    let bytes = TextFile::encode("你好", "utf-16le", true).unwrap();
    let file = TextFile::from_bytes(&bytes, 4096);
    assert!(file.editable());
    assert_eq!(file.text, "你好");
    assert_eq!(file.serialize(&EditSnapshot::text("你好")).unwrap(), bytes);
}
