/// Clamp string to max IRC message length (512 bytes).
/// Duplicated from fuzz_structured.rs for testing (fuzz binaries cannot run tests).
fn clamp_len(s: &str) -> String {
    if s.len() <= 512 {
        s.to_string()
    } else {
        let mut end = 512;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        s[..end].to_string()
    }
}

#[test]
fn clamp_len_multibyte_utf8_at_boundary() {
    // U+00E9 (é) is 2 bytes in UTF-8 (0xC3 0xA9).
    // 511 ASCII bytes + 'é' = 513 bytes total. Byte 512 falls inside 'é'.
    let s = "a".repeat(511) + "é";
    assert_eq!(s.len(), 513);

    // Should not panic, and should clamp to the char boundary before 512
    let result = clamp_len(&s);
    assert_eq!(result.len(), 511);
    assert!(result.is_char_boundary(result.len()));
}

#[test]
fn clamp_len_within_limit() {
    let s = "hello";
    assert_eq!(clamp_len(s), "hello");
}

#[test]
fn clamp_len_exactly_512() {
    let s = "x".repeat(512);
    assert_eq!(clamp_len(&s), s);
}

#[test]
fn clamp_len_ascii_over_512() {
    let s = "x".repeat(600);
    let result = clamp_len(&s);
    assert_eq!(result.len(), 512);
}

#[test]
fn clamp_len_multibyte_3byte_at_boundary() {
    // U+4E16 (世) is 3 bytes in UTF-8 (0xE4 0xB8 0x96).
    // 510 ASCII bytes + '世' = 513 bytes. Byte 512 falls inside '世'.
    let s = "a".repeat(510) + "世";
    assert_eq!(s.len(), 513);

    let result = clamp_len(&s);
    assert_eq!(result.len(), 510);
}

#[test]
fn clamp_len_multibyte_4byte_at_boundary() {
    // U+1F600 (😀) is 4 bytes in UTF-8.
    // 509 ASCII bytes + '😀' = 513 bytes. Byte 512 falls inside '😀'.
    let s = "a".repeat(509) + "😀";
    assert_eq!(s.len(), 513);

    let result = clamp_len(&s);
    assert_eq!(result.len(), 509);
}
