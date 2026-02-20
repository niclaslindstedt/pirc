#![no_main]

use libfuzzer_sys::fuzz_target;

// Fuzz the parse() function with arbitrary byte sequences.
// The parser must never panic on any input — it should always return
// Ok(Message) or Err(ProtocolError).
fuzz_target!(|data: &[u8]| {
    // The parser takes &str, so only feed valid UTF-8
    if let Ok(input) = std::str::from_utf8(data) {
        // Must not panic — errors are fine
        let _ = pirc_protocol::parse(input);
    }
});
