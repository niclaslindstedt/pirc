#![no_main]

use libfuzzer_sys::fuzz_target;

// Fuzz the parse -> serialize -> re-parse round-trip.
// If parse(input) succeeds, serializing via Display and re-parsing
// must produce an equivalent message.
fuzz_target!(|data: &[u8]| {
    let input = match std::str::from_utf8(data) {
        Ok(s) => s,
        Err(_) => return,
    };

    // Strip the trailing \r\n or \n for the content check, since the parser
    // strips these from the end.
    let content = input
        .strip_suffix("\r\n")
        .or_else(|| input.strip_suffix('\n'))
        .unwrap_or(input);

    // Skip inputs with embedded \r or \n in the message body — these are
    // malformed per IRC protocol (CR/LF are message delimiters only) and
    // create unavoidable round-trip asymmetries.
    if content.contains('\r') || content.contains('\n') {
        return;
    }

    let msg = match pirc_protocol::parse(input) {
        Ok(m) => m,
        Err(_) => return,
    };

    // Serialize the message back to wire format
    let serialized = msg.to_string();

    // Re-parse the serialized form
    let reparsed = match pirc_protocol::parse(&serialized) {
        Ok(m) => m,
        Err(e) => {
            panic!(
                "round-trip failure: parse succeeded for input but serialized form failed to parse.\n\
                 Input:      {:?}\n\
                 Serialized: {:?}\n\
                 Error:      {e}",
                input, serialized
            );
        }
    };

    // The re-parsed message must equal the original
    assert_eq!(
        msg, reparsed,
        "round-trip mismatch:\n  Input:      {:?}\n  Serialized: {:?}\n  Original:   {:?}\n  Reparsed:   {:?}",
        input, serialized, msg, reparsed
    );
});
