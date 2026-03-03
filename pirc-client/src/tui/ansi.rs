use std::io::{self, Write};

/// Move the cursor to an absolute position (1-based row and column).
pub fn move_to(w: &mut impl Write, col: u16, row: u16) -> io::Result<()> {
    write!(w, "\x1b[{};{}H", row, col)
}

/// Move the cursor up by `n` rows.
pub fn move_up(w: &mut impl Write, n: u16) -> io::Result<()> {
    write!(w, "\x1b[{}A", n)
}

/// Move the cursor down by `n` rows.
pub fn move_down(w: &mut impl Write, n: u16) -> io::Result<()> {
    write!(w, "\x1b[{}B", n)
}

/// Move the cursor right by `n` columns.
pub fn move_right(w: &mut impl Write, n: u16) -> io::Result<()> {
    write!(w, "\x1b[{}C", n)
}

/// Move the cursor left by `n` columns.
pub fn move_left(w: &mut impl Write, n: u16) -> io::Result<()> {
    write!(w, "\x1b[{}D", n)
}

/// Save the current cursor position.
pub fn save_cursor(w: &mut impl Write) -> io::Result<()> {
    w.write_all(b"\x1b[s")
}

/// Restore a previously saved cursor position.
pub fn restore_cursor(w: &mut impl Write) -> io::Result<()> {
    w.write_all(b"\x1b[u")
}

/// Clear the entire screen.
pub fn clear_screen(w: &mut impl Write) -> io::Result<()> {
    w.write_all(b"\x1b[2J")
}

/// Clear the entire current line.
pub fn clear_line(w: &mut impl Write) -> io::Result<()> {
    w.write_all(b"\x1b[2K")
}

/// Clear from the cursor to the end of the current line.
pub fn clear_to_end_of_line(w: &mut impl Write) -> io::Result<()> {
    w.write_all(b"\x1b[K")
}

/// Clear from the cursor to the end of the screen.
pub fn clear_to_end_of_screen(w: &mut impl Write) -> io::Result<()> {
    w.write_all(b"\x1b[J")
}

/// Set the scrolling region to rows `top` through `bottom` (1-based, inclusive).
pub fn set_scroll_region(w: &mut impl Write, top: u16, bottom: u16) -> io::Result<()> {
    write!(w, "\x1b[{};{}r", top, bottom)
}

/// Scroll the contents of the scroll region up by `n` lines.
///
/// New blank lines appear at the bottom of the region.
pub fn scroll_up(w: &mut impl Write, n: u16) -> io::Result<()> {
    write!(w, "\x1b[{}S", n)
}

/// Scroll the contents of the scroll region down by `n` lines.
///
/// New blank lines appear at the top of the region.
pub fn scroll_down(w: &mut impl Write, n: u16) -> io::Result<()> {
    write!(w, "\x1b[{}T", n)
}

/// Reset the scroll region to the full screen.
pub fn reset_scroll_region(w: &mut impl Write) -> io::Result<()> {
    w.write_all(b"\x1b[r")
}

/// Hide the terminal cursor.
pub fn hide_cursor(w: &mut impl Write) -> io::Result<()> {
    w.write_all(b"\x1b[?25l")
}

/// Show the terminal cursor.
pub fn show_cursor(w: &mut impl Write) -> io::Result<()> {
    w.write_all(b"\x1b[?25h")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect(f: impl FnOnce(&mut Vec<u8>) -> io::Result<()>) -> String {
        let mut buf = Vec::new();
        f(&mut buf).unwrap();
        String::from_utf8(buf).unwrap()
    }

    // --- Cursor movement ---

    #[test]
    fn test_move_to_origin() {
        assert_eq!(collect(|w| move_to(w, 1, 1)), "\x1b[1;1H");
    }

    #[test]
    fn test_move_to_arbitrary() {
        assert_eq!(collect(|w| move_to(w, 42, 10)), "\x1b[10;42H");
    }

    #[test]
    fn test_move_to_large_values() {
        assert_eq!(collect(|w| move_to(w, 999, 500)), "\x1b[500;999H");
    }

    #[test]
    fn test_move_up() {
        assert_eq!(collect(|w| move_up(w, 1)), "\x1b[1A");
    }

    #[test]
    fn test_move_up_multiple() {
        assert_eq!(collect(|w| move_up(w, 5)), "\x1b[5A");
    }

    #[test]
    fn test_move_down() {
        assert_eq!(collect(|w| move_down(w, 1)), "\x1b[1B");
    }

    #[test]
    fn test_move_down_multiple() {
        assert_eq!(collect(|w| move_down(w, 12)), "\x1b[12B");
    }

    #[test]
    fn test_move_right() {
        assert_eq!(collect(|w| move_right(w, 1)), "\x1b[1C");
    }

    #[test]
    fn test_move_right_multiple() {
        assert_eq!(collect(|w| move_right(w, 80)), "\x1b[80C");
    }

    #[test]
    fn test_move_left() {
        assert_eq!(collect(|w| move_left(w, 1)), "\x1b[1D");
    }

    #[test]
    fn test_move_left_multiple() {
        assert_eq!(collect(|w| move_left(w, 3)), "\x1b[3D");
    }

    #[test]
    fn test_save_cursor() {
        assert_eq!(collect(|w| save_cursor(w)), "\x1b[s");
    }

    #[test]
    fn test_restore_cursor() {
        assert_eq!(collect(|w| restore_cursor(w)), "\x1b[u");
    }

    // --- Screen clearing ---

    #[test]
    fn test_clear_screen() {
        assert_eq!(collect(|w| clear_screen(w)), "\x1b[2J");
    }

    #[test]
    fn test_clear_line() {
        assert_eq!(collect(|w| clear_line(w)), "\x1b[2K");
    }

    #[test]
    fn test_clear_to_end_of_line() {
        assert_eq!(collect(|w| clear_to_end_of_line(w)), "\x1b[K");
    }

    #[test]
    fn test_clear_to_end_of_screen() {
        assert_eq!(collect(|w| clear_to_end_of_screen(w)), "\x1b[J");
    }

    // --- Scrolling ---

    #[test]
    fn test_set_scroll_region() {
        assert_eq!(collect(|w| set_scroll_region(w, 1, 24)), "\x1b[1;24r");
    }

    #[test]
    fn test_set_scroll_region_partial() {
        assert_eq!(collect(|w| set_scroll_region(w, 5, 20)), "\x1b[5;20r");
    }

    #[test]
    fn test_scroll_up() {
        assert_eq!(collect(|w| scroll_up(w, 1)), "\x1b[1S");
    }

    #[test]
    fn test_scroll_up_multiple() {
        assert_eq!(collect(|w| scroll_up(w, 10)), "\x1b[10S");
    }

    #[test]
    fn test_scroll_down() {
        assert_eq!(collect(|w| scroll_down(w, 1)), "\x1b[1T");
    }

    #[test]
    fn test_scroll_down_multiple() {
        assert_eq!(collect(|w| scroll_down(w, 7)), "\x1b[7T");
    }

    #[test]
    fn test_reset_scroll_region() {
        assert_eq!(collect(|w| reset_scroll_region(w)), "\x1b[r");
    }

    // --- Composition / batching ---

    #[test]
    fn test_multiple_operations_batched() {
        let mut buf = Vec::new();
        move_to(&mut buf, 1, 1).unwrap();
        clear_screen(&mut buf).unwrap();
        move_to(&mut buf, 10, 5).unwrap();

        let output = String::from_utf8(buf).unwrap();
        assert_eq!(output, "\x1b[1;1H\x1b[2J\x1b[5;10H");
    }

    #[test]
    fn test_save_restore_cursor_roundtrip() {
        let mut buf = Vec::new();
        save_cursor(&mut buf).unwrap();
        move_to(&mut buf, 50, 25).unwrap();
        restore_cursor(&mut buf).unwrap();

        let output = String::from_utf8(buf).unwrap();
        assert_eq!(output, "\x1b[s\x1b[25;50H\x1b[u");
    }

    #[test]
    fn test_scroll_region_workflow() {
        let mut buf = Vec::new();
        set_scroll_region(&mut buf, 2, 23).unwrap();
        scroll_up(&mut buf, 3).unwrap();
        reset_scroll_region(&mut buf).unwrap();

        let output = String::from_utf8(buf).unwrap();
        assert_eq!(output, "\x1b[2;23r\x1b[3S\x1b[r");
    }
}
