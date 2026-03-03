#![allow(unsafe_code)]

use std::io::{self, Write};
use std::os::unix::io::AsRawFd;

/// Terminal dimensions (columns, rows).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalSize {
    pub cols: u16,
    pub rows: u16,
}

/// Query the current terminal size using `ioctl(TIOCGWINSZ)`.
///
/// Returns `None` if stdout is not a terminal or the ioctl fails.
#[cfg(unix)]
pub fn terminal_size() -> Option<TerminalSize> {
    let mut winsize = libc::winsize {
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };

    let fd = io::stdout().as_raw_fd();
    let ret = unsafe { libc::ioctl(fd, libc::TIOCGWINSZ, &mut winsize) };

    if ret == 0 && winsize.ws_col > 0 && winsize.ws_row > 0 {
        Some(TerminalSize {
            cols: winsize.ws_col,
            rows: winsize.ws_row,
        })
    } else {
        None
    }
}

/// Configure a `libc::termios` for raw mode.
///
/// Raw mode disables:
/// - Canonical mode (line buffering)
/// - Echo
/// - Signal generation (Ctrl-C, Ctrl-Z, etc.)
/// - Implementation-defined input/output processing
///
/// Sets minimum read of 1 byte with no timeout.
#[cfg(unix)]
fn make_raw(termios: &mut libc::termios) {
    // Input flags: disable break signal, CR-to-NL, parity, strip, flow control
    termios.c_iflag &= !(libc::BRKINT | libc::ICRNL | libc::INPCK | libc::ISTRIP | libc::IXON);

    // Output flags: disable post-processing
    termios.c_oflag &= !libc::OPOST;

    // Control flags: set 8-bit characters
    termios.c_cflag |= libc::CS8;

    // Local flags: disable echo, canonical mode, extensions, signal chars
    termios.c_lflag &= !(libc::ECHO | libc::ICANON | libc::IEXTEN | libc::ISIG);

    // Control characters: read returns after 1 byte, no timeout
    termios.c_cc[libc::VMIN] = 1;
    termios.c_cc[libc::VTIME] = 0;
}

/// RAII guard that enables raw terminal mode and restores the original state on drop.
///
/// When created, this guard:
/// 1. Saves the current termios settings
/// 2. Switches to raw mode
/// 3. Enters the alternate screen buffer
/// 4. Hides the cursor
///
/// When dropped, it reverses all of these in the opposite order.
#[cfg(unix)]
pub struct RawModeGuard {
    original_termios: libc::termios,
    fd: i32,
}

#[cfg(unix)]
impl RawModeGuard {
    /// Enable raw terminal mode on stdout.
    ///
    /// Returns a guard that will restore the terminal state when dropped.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `tcgetattr` fails (stdout may not be a terminal)
    /// - `tcsetattr` fails
    /// - Writing escape sequences to stdout fails
    pub fn enable() -> io::Result<Self> {
        let fd = io::stdout().as_raw_fd();

        // Save original termios
        let mut original_termios = unsafe { std::mem::zeroed::<libc::termios>() };
        if unsafe { libc::tcgetattr(fd, &mut original_termios) } != 0 {
            return Err(io::Error::last_os_error());
        }

        // Configure raw mode
        let mut raw = original_termios;
        make_raw(&mut raw);

        if unsafe { libc::tcsetattr(fd, libc::TCSAFLUSH, &raw) } != 0 {
            return Err(io::Error::last_os_error());
        }

        let guard = Self {
            original_termios,
            fd,
        };

        // Enter alternate screen buffer
        let mut stdout = io::stdout().lock();
        stdout.write_all(b"\x1b[?1049h")?; // alternate screen
        stdout.flush()?;

        Ok(guard)
    }
}

#[cfg(unix)]
impl Drop for RawModeGuard {
    fn drop(&mut self) {
        // Show cursor and leave alternate screen buffer
        let mut stdout = io::stdout().lock();
        let _ = stdout.write_all(b"\x1b[?25h"); // show cursor
        let _ = stdout.write_all(b"\x1b[?1049l"); // leave alternate screen
        let _ = stdout.flush();

        // Restore original terminal settings
        unsafe {
            libc::tcsetattr(self.fd, libc::TCSAFLUSH, &self.original_termios);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_make_raw_disables_canonical_mode() {
        let mut termios = unsafe { std::mem::zeroed::<libc::termios>() };
        termios.c_lflag = libc::ECHO | libc::ICANON | libc::IEXTEN | libc::ISIG;
        termios.c_iflag = libc::BRKINT | libc::ICRNL | libc::INPCK | libc::ISTRIP | libc::IXON;
        termios.c_oflag = libc::OPOST;

        make_raw(&mut termios);

        assert_eq!(
            termios.c_lflag & libc::ICANON,
            0,
            "ICANON should be cleared"
        );
        assert_eq!(termios.c_lflag & libc::ECHO, 0, "ECHO should be cleared");
        assert_eq!(
            termios.c_lflag & libc::IEXTEN,
            0,
            "IEXTEN should be cleared"
        );
        assert_eq!(termios.c_lflag & libc::ISIG, 0, "ISIG should be cleared");
    }

    #[test]
    fn test_make_raw_disables_input_processing() {
        let mut termios = unsafe { std::mem::zeroed::<libc::termios>() };
        termios.c_iflag = libc::BRKINT | libc::ICRNL | libc::INPCK | libc::ISTRIP | libc::IXON;

        make_raw(&mut termios);

        assert_eq!(
            termios.c_iflag & libc::BRKINT,
            0,
            "BRKINT should be cleared"
        );
        assert_eq!(termios.c_iflag & libc::ICRNL, 0, "ICRNL should be cleared");
        assert_eq!(termios.c_iflag & libc::INPCK, 0, "INPCK should be cleared");
        assert_eq!(
            termios.c_iflag & libc::ISTRIP,
            0,
            "ISTRIP should be cleared"
        );
        assert_eq!(termios.c_iflag & libc::IXON, 0, "IXON should be cleared");
    }

    #[test]
    fn test_make_raw_disables_output_processing() {
        let mut termios = unsafe { std::mem::zeroed::<libc::termios>() };
        termios.c_oflag = libc::OPOST;

        make_raw(&mut termios);

        assert_eq!(termios.c_oflag & libc::OPOST, 0, "OPOST should be cleared");
    }

    #[test]
    fn test_make_raw_sets_cs8() {
        let mut termios = unsafe { std::mem::zeroed::<libc::termios>() };

        make_raw(&mut termios);

        assert_ne!(termios.c_cflag & libc::CS8, 0, "CS8 should be set");
    }

    #[test]
    fn test_make_raw_sets_vmin_and_vtime() {
        let mut termios = unsafe { std::mem::zeroed::<libc::termios>() };

        make_raw(&mut termios);

        assert_eq!(termios.c_cc[libc::VMIN], 1, "VMIN should be 1");
        assert_eq!(termios.c_cc[libc::VTIME], 0, "VTIME should be 0");
    }

    #[test]
    fn test_make_raw_preserves_unrelated_flags() {
        let mut termios = unsafe { std::mem::zeroed::<libc::termios>() };
        // Set some flags that make_raw should not touch
        termios.c_iflag = libc::IGNBRK | libc::BRKINT;

        make_raw(&mut termios);

        assert_ne!(
            termios.c_iflag & libc::IGNBRK,
            0,
            "IGNBRK should be preserved"
        );
        assert_eq!(
            termios.c_iflag & libc::BRKINT,
            0,
            "BRKINT should be cleared"
        );
    }

    #[test]
    fn test_terminal_size_returns_some_or_none() {
        // In CI or non-terminal environments, this may return None.
        // In a real terminal, it should return Some with positive dimensions.
        // We just verify it doesn't panic.
        let size = terminal_size();
        if let Some(s) = size {
            assert!(s.cols > 0, "cols should be positive");
            assert!(s.rows > 0, "rows should be positive");
        }
    }

    #[test]
    fn test_terminal_size_struct_equality() {
        let a = TerminalSize { cols: 80, rows: 24 };
        let b = TerminalSize { cols: 80, rows: 24 };
        let c = TerminalSize {
            cols: 120,
            rows: 40,
        };

        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn test_terminal_size_struct_clone() {
        let a = TerminalSize { cols: 80, rows: 24 };
        let b = a;
        assert_eq!(a, b);
    }

    #[test]
    fn test_terminal_size_struct_debug() {
        let size = TerminalSize { cols: 80, rows: 24 };
        let debug = format!("{size:?}");
        assert!(debug.contains("80"));
        assert!(debug.contains("24"));
    }
}
