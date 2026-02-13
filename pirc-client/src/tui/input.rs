#![allow(unsafe_code)]

use std::os::unix::io::RawFd;

use super::signal::SignalHandler;
use super::terminal::terminal_size;

/// A structured key event parsed from raw terminal input bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyEvent {
    /// A regular Unicode character (ASCII or multi-byte UTF-8).
    Char(char),
    /// Enter / Return key (CR).
    Enter,
    /// Backspace key.
    Backspace,
    /// Delete key (CSI 3~).
    Delete,
    /// Tab key.
    Tab,
    /// Arrow up.
    Up,
    /// Arrow down.
    Down,
    /// Arrow left.
    Left,
    /// Arrow right.
    Right,
    /// Home key.
    Home,
    /// End key.
    End,
    /// Page Up key.
    PageUp,
    /// Page Down key.
    PageDown,
    /// Bare Escape key (no following sequence).
    Escape,
    /// Ctrl+key combination. The char is the lowercase letter (e.g. `Ctrl('c')`).
    Ctrl(char),
    /// Terminal window resize event with new dimensions (cols, rows).
    Resize(u16, u16),
    /// An unrecognized escape sequence.
    Unknown(Vec<u8>),
}

/// Non-blocking terminal input reader that parses raw bytes into `KeyEvent`s.
///
/// Uses `libc::poll` and `libc::read` on a file descriptor (typically stdin)
/// to read raw bytes, then parses escape sequences into structured events.
///
/// When a `SignalHandler` is attached, also polls the signal pipe and emits
/// `KeyEvent::Resize` events when SIGWINCH is received.
pub struct InputReader {
    fd: RawFd,
    buf: Vec<u8>,
    signal_handler: Option<SignalHandler>,
}

impl InputReader {
    /// Create a new `InputReader` reading from the given file descriptor.
    pub fn new(fd: RawFd) -> Self {
        Self {
            fd,
            buf: Vec::with_capacity(64),
            signal_handler: None,
        }
    }

    /// Create a new `InputReader` reading from stdin (fd 0).
    pub fn from_stdin() -> Self {
        Self::new(libc::STDIN_FILENO)
    }

    /// Attach a `SignalHandler` so that SIGWINCH events produce `KeyEvent::Resize`.
    pub fn set_signal_handler(&mut self, handler: SignalHandler) {
        self.signal_handler = Some(handler);
    }

    /// Poll for a key event with the given timeout in milliseconds.
    ///
    /// Returns `Some(KeyEvent)` if a key event is available, or `None` if the
    /// timeout expires with no input. Returns `None` on read errors.
    ///
    /// When a signal handler is attached, also polls the signal pipe.
    /// Resize events take priority over key input.
    pub fn poll_event(&mut self, timeout_ms: u32) -> Option<KeyEvent> {
        match self.poll_ready(timeout_ms) {
            PollResult::None => None,
            PollResult::Resize => {
                if let Some(ref handler) = self.signal_handler {
                    handler.drain();
                }
                if let Some(size) = terminal_size() {
                    Some(KeyEvent::Resize(size.cols, size.rows))
                } else {
                    None
                }
            }
            PollResult::Input => {
                self.read_bytes();
                if self.buf.is_empty() {
                    return None;
                }
                Some(self.parse_event())
            }
        }
    }

    /// Check whether the fd or signal pipe has data available.
    fn poll_ready(&self, timeout_ms: u32) -> PollResult {
        let has_signal = self.signal_handler.is_some();
        let nfds: libc::nfds_t = if has_signal { 2 } else { 1 };

        let mut pfds = [
            libc::pollfd {
                fd: self.fd,
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: self
                    .signal_handler
                    .as_ref()
                    .map_or(-1, |h| h.pipe_fd()),
                events: libc::POLLIN,
                revents: 0,
            },
        ];

        let ret = unsafe { libc::poll(pfds.as_mut_ptr(), nfds, timeout_ms as i32) };

        if ret <= 0 {
            return PollResult::None;
        }

        // Check signal pipe first (resize takes priority)
        if has_signal && (pfds[1].revents & libc::POLLIN) != 0 {
            return PollResult::Resize;
        }

        if (pfds[0].revents & libc::POLLIN) != 0 {
            return PollResult::Input;
        }

        PollResult::None
    }

    /// Read available bytes from the fd into the internal buffer.
    fn read_bytes(&mut self) {
        self.buf.clear();
        let mut tmp = [0u8; 64];
        let n = unsafe { libc::read(self.fd, tmp.as_mut_ptr().cast(), tmp.len()) };
        if n > 0 {
            self.buf.extend_from_slice(&tmp[..n as usize]);
        }
    }

    /// Parse one key event from the internal buffer, consuming the bytes used.
    fn parse_event(&mut self) -> KeyEvent {
        let event = parse_key_event(&self.buf);
        self.buf.clear();
        event
    }
}

/// Internal result of polling for readiness.
enum PollResult {
    /// Nothing ready within the timeout.
    None,
    /// The signal pipe is readable (SIGWINCH received).
    Resize,
    /// The input fd is readable (key data available).
    Input,
}

/// Parse a key event from a raw byte slice.
///
/// This is the core parsing logic, separated from I/O for testability.
pub(crate) fn parse_key_event(buf: &[u8]) -> KeyEvent {
    if buf.is_empty() {
        return KeyEvent::Unknown(Vec::new());
    }

    match buf[0] {
        // Ctrl+letter: 0x01..=0x1a (except special cases)
        0x01..=0x07 | 0x0b..=0x0c | 0x0e..=0x1a => {
            let ch = (buf[0] + b'a' - 1) as char;
            KeyEvent::Ctrl(ch)
        }
        // Ctrl+H / Backspace
        0x08 => KeyEvent::Backspace,
        // Tab
        0x09 => KeyEvent::Tab,
        // Line feed (Ctrl+J) — treat as Ctrl('j') since Enter is CR in raw mode
        0x0a => KeyEvent::Ctrl('j'),
        // Enter (CR)
        0x0d => KeyEvent::Enter,
        // Escape
        0x1b => parse_escape_sequence(buf),
        // Backspace (DEL)
        0x7f => KeyEvent::Backspace,
        // Regular ASCII
        0x20..=0x7e => KeyEvent::Char(buf[0] as char),
        // UTF-8 multi-byte
        _ => parse_utf8(buf),
    }
}

/// Parse an escape sequence starting with 0x1b.
fn parse_escape_sequence(buf: &[u8]) -> KeyEvent {
    debug_assert!(buf[0] == 0x1b);

    // Bare escape — no more bytes
    if buf.len() == 1 {
        return KeyEvent::Escape;
    }

    match buf[1] {
        // CSI sequence: ESC [
        b'[' => parse_csi_sequence(buf),
        // SS3 sequence: ESC O (some terminals send arrows this way)
        b'O' => parse_ss3_sequence(buf),
        // Alt+letter or unknown — for now, treat ESC+letter as Unknown
        _ => KeyEvent::Unknown(buf.to_vec()),
    }
}

/// Parse a CSI sequence: ESC [ ... (final byte)
fn parse_csi_sequence(buf: &[u8]) -> KeyEvent {
    // Minimum: ESC [ X  (3 bytes)
    if buf.len() < 3 {
        return KeyEvent::Unknown(buf.to_vec());
    }

    match buf[2] {
        // Arrow keys: ESC [ A/B/C/D
        b'A' => KeyEvent::Up,
        b'B' => KeyEvent::Down,
        b'C' => KeyEvent::Right,
        b'D' => KeyEvent::Left,
        // Home/End: ESC [ H / ESC [ F
        b'H' => KeyEvent::Home,
        b'F' => KeyEvent::End,
        // Numbered sequences: ESC [ <number> ~
        b'1'..=b'9' => parse_csi_numbered(buf),
        _ => KeyEvent::Unknown(buf.to_vec()),
    }
}

/// Parse CSI numbered sequences like ESC [ 1 ~ or ESC [ 1 ; 5 A
fn parse_csi_numbered(buf: &[u8]) -> KeyEvent {
    // Collect the numeric parameter(s)
    // Common patterns:
    //   ESC [ <n> ~          — special keys
    //   ESC [ 1 ; <mod> A    — modified arrow keys (we ignore modifier)

    // Find the final byte (first byte in 0x40..=0x7e after the number)
    let params = &buf[2..];

    // Find the terminating character
    let mut i = 0;
    while i < params.len() && (params[i] == b';' || params[i].is_ascii_digit()) {
        i += 1;
    }

    if i >= params.len() {
        return KeyEvent::Unknown(buf.to_vec());
    }

    let final_byte = params[i];

    // Extract the first numeric parameter
    let num_str: Vec<u8> = params[..i]
        .iter()
        .copied()
        .take_while(|&b| b.is_ascii_digit())
        .collect();
    let num: u16 = num_str
        .iter()
        .fold(0u16, |acc, &b| acc.saturating_mul(10).saturating_add((b - b'0') as u16));

    match final_byte {
        b'~' => match num {
            1 => KeyEvent::Home,
            2 => KeyEvent::Unknown(buf.to_vec()), // Insert — not mapped
            3 => KeyEvent::Delete,
            4 => KeyEvent::End,
            5 => KeyEvent::PageUp,
            6 => KeyEvent::PageDown,
            7 => KeyEvent::Home,  // rxvt Home
            8 => KeyEvent::End,   // rxvt End
            _ => KeyEvent::Unknown(buf.to_vec()),
        },
        // Modified arrow/navigation keys: ESC [ 1 ; <mod> A/B/C/D/H/F
        b'A' => KeyEvent::Up,
        b'B' => KeyEvent::Down,
        b'C' => KeyEvent::Right,
        b'D' => KeyEvent::Left,
        b'H' => KeyEvent::Home,
        b'F' => KeyEvent::End,
        _ => KeyEvent::Unknown(buf.to_vec()),
    }
}

/// Parse an SS3 sequence: ESC O <letter>
fn parse_ss3_sequence(buf: &[u8]) -> KeyEvent {
    if buf.len() < 3 {
        return KeyEvent::Unknown(buf.to_vec());
    }

    match buf[2] {
        b'A' => KeyEvent::Up,
        b'B' => KeyEvent::Down,
        b'C' => KeyEvent::Right,
        b'D' => KeyEvent::Left,
        b'H' => KeyEvent::Home,
        b'F' => KeyEvent::End,
        _ => KeyEvent::Unknown(buf.to_vec()),
    }
}

/// Attempt to parse a UTF-8 character from the leading bytes.
fn parse_utf8(buf: &[u8]) -> KeyEvent {
    // Determine expected length from the leading byte
    let expected_len = match buf[0] {
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF7 => 4,
        _ => return KeyEvent::Unknown(buf.to_vec()),
    };

    if buf.len() < expected_len {
        return KeyEvent::Unknown(buf.to_vec());
    }

    match std::str::from_utf8(&buf[..expected_len]) {
        Ok(s) => {
            let ch = s.chars().next().unwrap();
            KeyEvent::Char(ch)
        }
        Err(_) => KeyEvent::Unknown(buf[..expected_len].to_vec()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Regular ASCII characters
    // -----------------------------------------------------------------------

    #[test]
    fn test_char_lowercase() {
        assert_eq!(parse_key_event(b"a"), KeyEvent::Char('a'));
        assert_eq!(parse_key_event(b"z"), KeyEvent::Char('z'));
    }

    #[test]
    fn test_char_uppercase() {
        assert_eq!(parse_key_event(b"A"), KeyEvent::Char('A'));
        assert_eq!(parse_key_event(b"Z"), KeyEvent::Char('Z'));
    }

    #[test]
    fn test_char_digit() {
        assert_eq!(parse_key_event(b"0"), KeyEvent::Char('0'));
        assert_eq!(parse_key_event(b"9"), KeyEvent::Char('9'));
    }

    #[test]
    fn test_char_symbols() {
        assert_eq!(parse_key_event(b"!"), KeyEvent::Char('!'));
        assert_eq!(parse_key_event(b"@"), KeyEvent::Char('@'));
        assert_eq!(parse_key_event(b" "), KeyEvent::Char(' '));
        assert_eq!(parse_key_event(b"~"), KeyEvent::Char('~'));
    }

    // -----------------------------------------------------------------------
    // Special keys
    // -----------------------------------------------------------------------

    #[test]
    fn test_enter() {
        assert_eq!(parse_key_event(b"\r"), KeyEvent::Enter);
    }

    #[test]
    fn test_tab() {
        assert_eq!(parse_key_event(b"\t"), KeyEvent::Tab);
    }

    #[test]
    fn test_backspace_0x7f() {
        assert_eq!(parse_key_event(b"\x7f"), KeyEvent::Backspace);
    }

    #[test]
    fn test_backspace_0x08() {
        assert_eq!(parse_key_event(b"\x08"), KeyEvent::Backspace);
    }

    #[test]
    fn test_escape_bare() {
        assert_eq!(parse_key_event(b"\x1b"), KeyEvent::Escape);
    }

    // -----------------------------------------------------------------------
    // Ctrl key combinations
    // -----------------------------------------------------------------------

    #[test]
    fn test_ctrl_a() {
        assert_eq!(parse_key_event(b"\x01"), KeyEvent::Ctrl('a'));
    }

    #[test]
    fn test_ctrl_c() {
        assert_eq!(parse_key_event(b"\x03"), KeyEvent::Ctrl('c'));
    }

    #[test]
    fn test_ctrl_d() {
        assert_eq!(parse_key_event(b"\x04"), KeyEvent::Ctrl('d'));
    }

    #[test]
    fn test_ctrl_z() {
        assert_eq!(parse_key_event(b"\x1a"), KeyEvent::Ctrl('z'));
    }

    #[test]
    fn test_ctrl_e() {
        assert_eq!(parse_key_event(b"\x05"), KeyEvent::Ctrl('e'));
    }

    #[test]
    fn test_ctrl_k() {
        assert_eq!(parse_key_event(b"\x0b"), KeyEvent::Ctrl('k'));
    }

    #[test]
    fn test_ctrl_l() {
        assert_eq!(parse_key_event(b"\x0c"), KeyEvent::Ctrl('l'));
    }

    #[test]
    fn test_ctrl_n() {
        assert_eq!(parse_key_event(b"\x0e"), KeyEvent::Ctrl('n'));
    }

    #[test]
    fn test_ctrl_j_is_linefeed() {
        // 0x0a is Ctrl+J / LF — we report it as Ctrl('j')
        assert_eq!(parse_key_event(b"\x0a"), KeyEvent::Ctrl('j'));
    }

    // -----------------------------------------------------------------------
    // Arrow keys (CSI)
    // -----------------------------------------------------------------------

    #[test]
    fn test_arrow_up() {
        assert_eq!(parse_key_event(b"\x1b[A"), KeyEvent::Up);
    }

    #[test]
    fn test_arrow_down() {
        assert_eq!(parse_key_event(b"\x1b[B"), KeyEvent::Down);
    }

    #[test]
    fn test_arrow_right() {
        assert_eq!(parse_key_event(b"\x1b[C"), KeyEvent::Right);
    }

    #[test]
    fn test_arrow_left() {
        assert_eq!(parse_key_event(b"\x1b[D"), KeyEvent::Left);
    }

    // -----------------------------------------------------------------------
    // Arrow keys (SS3)
    // -----------------------------------------------------------------------

    #[test]
    fn test_ss3_arrow_up() {
        assert_eq!(parse_key_event(b"\x1bOA"), KeyEvent::Up);
    }

    #[test]
    fn test_ss3_arrow_down() {
        assert_eq!(parse_key_event(b"\x1bOB"), KeyEvent::Down);
    }

    #[test]
    fn test_ss3_arrow_right() {
        assert_eq!(parse_key_event(b"\x1bOC"), KeyEvent::Right);
    }

    #[test]
    fn test_ss3_arrow_left() {
        assert_eq!(parse_key_event(b"\x1bOD"), KeyEvent::Left);
    }

    // -----------------------------------------------------------------------
    // Home / End
    // -----------------------------------------------------------------------

    #[test]
    fn test_home_csi_h() {
        assert_eq!(parse_key_event(b"\x1b[H"), KeyEvent::Home);
    }

    #[test]
    fn test_end_csi_f() {
        assert_eq!(parse_key_event(b"\x1b[F"), KeyEvent::End);
    }

    #[test]
    fn test_home_csi_1_tilde() {
        assert_eq!(parse_key_event(b"\x1b[1~"), KeyEvent::Home);
    }

    #[test]
    fn test_end_csi_4_tilde() {
        assert_eq!(parse_key_event(b"\x1b[4~"), KeyEvent::End);
    }

    #[test]
    fn test_home_rxvt_csi_7_tilde() {
        assert_eq!(parse_key_event(b"\x1b[7~"), KeyEvent::Home);
    }

    #[test]
    fn test_end_rxvt_csi_8_tilde() {
        assert_eq!(parse_key_event(b"\x1b[8~"), KeyEvent::End);
    }

    #[test]
    fn test_home_ss3() {
        assert_eq!(parse_key_event(b"\x1bOH"), KeyEvent::Home);
    }

    #[test]
    fn test_end_ss3() {
        assert_eq!(parse_key_event(b"\x1bOF"), KeyEvent::End);
    }

    // -----------------------------------------------------------------------
    // Delete / PageUp / PageDown
    // -----------------------------------------------------------------------

    #[test]
    fn test_delete() {
        assert_eq!(parse_key_event(b"\x1b[3~"), KeyEvent::Delete);
    }

    #[test]
    fn test_page_up() {
        assert_eq!(parse_key_event(b"\x1b[5~"), KeyEvent::PageUp);
    }

    #[test]
    fn test_page_down() {
        assert_eq!(parse_key_event(b"\x1b[6~"), KeyEvent::PageDown);
    }

    // -----------------------------------------------------------------------
    // Modified arrow keys (ESC [ 1 ; <mod> X)
    // -----------------------------------------------------------------------

    #[test]
    fn test_modified_arrow_up() {
        // Shift+Up: ESC [ 1 ; 2 A
        assert_eq!(parse_key_event(b"\x1b[1;2A"), KeyEvent::Up);
    }

    #[test]
    fn test_modified_arrow_down_ctrl() {
        // Ctrl+Down: ESC [ 1 ; 5 B
        assert_eq!(parse_key_event(b"\x1b[1;5B"), KeyEvent::Down);
    }

    #[test]
    fn test_modified_arrow_right_alt() {
        // Alt+Right: ESC [ 1 ; 3 C
        assert_eq!(parse_key_event(b"\x1b[1;3C"), KeyEvent::Right);
    }

    #[test]
    fn test_modified_arrow_left_ctrl_shift() {
        // Ctrl+Shift+Left: ESC [ 1 ; 6 D
        assert_eq!(parse_key_event(b"\x1b[1;6D"), KeyEvent::Left);
    }

    #[test]
    fn test_modified_home() {
        // Ctrl+Home: ESC [ 1 ; 5 H
        assert_eq!(parse_key_event(b"\x1b[1;5H"), KeyEvent::Home);
    }

    #[test]
    fn test_modified_end() {
        // Ctrl+End: ESC [ 1 ; 5 F
        assert_eq!(parse_key_event(b"\x1b[1;5F"), KeyEvent::End);
    }

    // -----------------------------------------------------------------------
    // UTF-8 multi-byte characters
    // -----------------------------------------------------------------------

    #[test]
    fn test_utf8_2_byte() {
        // é = 0xC3 0xA9
        assert_eq!(parse_key_event(&[0xC3, 0xA9]), KeyEvent::Char('é'));
    }

    #[test]
    fn test_utf8_3_byte() {
        // ☺ = 0xE2 0x98 0xBA
        assert_eq!(
            parse_key_event(&[0xE2, 0x98, 0xBA]),
            KeyEvent::Char('☺')
        );
    }

    #[test]
    fn test_utf8_4_byte() {
        // 🎉 = 0xF0 0x9F 0x8E 0x89
        assert_eq!(
            parse_key_event(&[0xF0, 0x9F, 0x8E, 0x89]),
            KeyEvent::Char('🎉')
        );
    }

    #[test]
    fn test_utf8_incomplete_2_byte() {
        // Only first byte of a 2-byte sequence
        assert_eq!(
            parse_key_event(&[0xC3]),
            KeyEvent::Unknown(vec![0xC3])
        );
    }

    #[test]
    fn test_utf8_incomplete_3_byte() {
        // Only first 2 bytes of a 3-byte sequence
        assert_eq!(
            parse_key_event(&[0xE2, 0x98]),
            KeyEvent::Unknown(vec![0xE2, 0x98])
        );
    }

    #[test]
    fn test_utf8_invalid_continuation() {
        // 0xC3 followed by a non-continuation byte
        assert_eq!(
            parse_key_event(&[0xC3, 0x20]),
            KeyEvent::Unknown(vec![0xC3, 0x20])
        );
    }

    // -----------------------------------------------------------------------
    // Unknown / edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_unknown_escape_sequence() {
        assert_eq!(
            parse_key_event(b"\x1b[99z"),
            KeyEvent::Unknown(b"\x1b[99z".to_vec())
        );
    }

    #[test]
    fn test_unknown_csi_number() {
        // Insert key (ESC [ 2 ~) — not mapped
        assert_eq!(
            parse_key_event(b"\x1b[2~"),
            KeyEvent::Unknown(b"\x1b[2~".to_vec())
        );
    }

    #[test]
    fn test_unknown_ss3_sequence() {
        assert_eq!(
            parse_key_event(b"\x1bOZ"),
            KeyEvent::Unknown(b"\x1bOZ".to_vec())
        );
    }

    #[test]
    fn test_empty_buffer() {
        assert_eq!(parse_key_event(b""), KeyEvent::Unknown(Vec::new()));
    }

    #[test]
    fn test_bare_escape_no_extra_bytes() {
        assert_eq!(parse_key_event(b"\x1b"), KeyEvent::Escape);
    }

    #[test]
    fn test_escape_with_unknown_second_byte() {
        assert_eq!(
            parse_key_event(b"\x1bX"),
            KeyEvent::Unknown(b"\x1bX".to_vec())
        );
    }

    #[test]
    fn test_short_csi_sequence() {
        // ESC [ with no final byte
        assert_eq!(
            parse_key_event(b"\x1b["),
            KeyEvent::Unknown(b"\x1b[".to_vec())
        );
    }

    #[test]
    fn test_short_ss3_sequence() {
        // ESC O with no final byte
        assert_eq!(
            parse_key_event(b"\x1bO"),
            KeyEvent::Unknown(b"\x1bO".to_vec())
        );
    }

    #[test]
    fn test_invalid_high_byte() {
        // 0xFF is not a valid UTF-8 start byte
        assert_eq!(
            parse_key_event(&[0xFF]),
            KeyEvent::Unknown(vec![0xFF])
        );
    }

    #[test]
    fn test_ctrl_all_mapped() {
        // Verify all Ctrl combinations A-Z are handled without panicking
        for b in 0x01..=0x1au8 {
            let event = parse_key_event(&[b]);
            match event {
                KeyEvent::Ctrl(_) | KeyEvent::Backspace | KeyEvent::Tab | KeyEvent::Enter => {}
                _ => panic!("Unexpected event for byte 0x{b:02x}: {event:?}"),
            }
        }
    }

    #[test]
    fn test_all_printable_ascii() {
        for b in 0x20..=0x7eu8 {
            assert_eq!(
                parse_key_event(&[b]),
                KeyEvent::Char(b as char),
                "Failed for byte 0x{b:02x}"
            );
        }
    }

    // -----------------------------------------------------------------------
    // InputReader construction
    // -----------------------------------------------------------------------

    #[test]
    fn test_input_reader_new() {
        let reader = InputReader::new(0);
        assert_eq!(reader.fd, 0);
    }

    #[test]
    fn test_input_reader_from_stdin() {
        let reader = InputReader::from_stdin();
        assert_eq!(reader.fd, libc::STDIN_FILENO);
    }

    // -----------------------------------------------------------------------
    // KeyEvent properties
    // -----------------------------------------------------------------------

    #[test]
    fn test_key_event_clone() {
        let event = KeyEvent::Char('a');
        let cloned = event.clone();
        assert_eq!(event, cloned);
    }

    #[test]
    fn test_key_event_debug() {
        let event = KeyEvent::Ctrl('c');
        let debug = format!("{event:?}");
        assert!(debug.contains("Ctrl"));
    }

    #[test]
    fn test_key_event_equality() {
        assert_eq!(KeyEvent::Up, KeyEvent::Up);
        assert_ne!(KeyEvent::Up, KeyEvent::Down);
        assert_ne!(KeyEvent::Char('a'), KeyEvent::Char('b'));
        assert_eq!(KeyEvent::Ctrl('c'), KeyEvent::Ctrl('c'));
        assert_ne!(KeyEvent::Ctrl('c'), KeyEvent::Ctrl('d'));
    }

    #[test]
    fn test_unknown_event_preserves_bytes() {
        let bytes = vec![0x1b, 0x5b, 0x39, 0x39, 0x7a];
        let event = KeyEvent::Unknown(bytes.clone());
        if let KeyEvent::Unknown(inner) = event {
            assert_eq!(inner, bytes);
        } else {
            panic!("Expected Unknown variant");
        }
    }

    // -----------------------------------------------------------------------
    // Resize event
    // -----------------------------------------------------------------------

    #[test]
    fn test_resize_event() {
        let event = KeyEvent::Resize(120, 40);
        assert_eq!(event, KeyEvent::Resize(120, 40));
        assert_ne!(event, KeyEvent::Resize(80, 24));
    }

    #[test]
    fn test_resize_event_clone() {
        let event = KeyEvent::Resize(80, 24);
        let cloned = event.clone();
        assert_eq!(event, cloned);
    }

    #[test]
    fn test_resize_event_debug() {
        let event = KeyEvent::Resize(120, 40);
        let debug = format!("{event:?}");
        assert!(debug.contains("Resize"));
        assert!(debug.contains("120"));
        assert!(debug.contains("40"));
    }

    // -----------------------------------------------------------------------
    // InputReader with signal handler
    // -----------------------------------------------------------------------

    #[test]
    fn test_input_reader_set_signal_handler() {
        let mut reader = InputReader::new(0);
        assert!(reader.signal_handler.is_none());
        let handler = SignalHandler::new().expect("should create signal handler");
        reader.set_signal_handler(handler);
        assert!(reader.signal_handler.is_some());
    }

    #[test]
    fn test_input_reader_poll_no_signal_handler() {
        // Without a signal handler, poll should just check stdin
        let mut reader = InputReader::new(0);
        // With a zero timeout, should return None (no input ready)
        let event = reader.poll_event(0);
        assert!(event.is_none());
    }

    #[test]
    fn test_input_reader_poll_resize_on_sigwinch() {
        let mut reader = InputReader::new(0);
        let handler = SignalHandler::new().expect("should create signal handler");
        reader.set_signal_handler(handler);

        // Send SIGWINCH to ourselves
        unsafe { libc::raise(libc::SIGWINCH) };

        // Poll should return a Resize event
        let event = reader.poll_event(100);
        match event {
            Some(KeyEvent::Resize(cols, rows)) => {
                // In CI the terminal_size() may return None causing no event,
                // but in a real terminal we should get dimensions.
                // Just verify we got positive values if we get Resize.
                assert!(cols > 0, "cols should be positive");
                assert!(rows > 0, "rows should be positive");
            }
            None => {
                // Acceptable in CI where terminal_size() returns None
            }
            other => panic!("Expected Resize or None, got {other:?}"),
        }
    }
}
