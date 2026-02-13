#![allow(unsafe_code)]

use std::os::unix::io::RawFd;
use std::sync::atomic::{AtomicI32, Ordering};

/// Static storage for the signal pipe's write end, accessible from the signal handler.
///
/// Set to -1 when no handler is installed.
static SIGNAL_PIPE_WRITE: AtomicI32 = AtomicI32::new(-1);

/// Async-signal-safe SIGWINCH handler.
///
/// Writes a single byte to the signal pipe to notify the event loop.
/// Only calls `write()`, which is async-signal-safe per POSIX.
unsafe extern "C" fn sigwinch_handler(_sig: libc::c_int) {
    let fd = SIGNAL_PIPE_WRITE.load(Ordering::Relaxed);
    if fd >= 0 {
        let byte: u8 = 1;
        // write() is async-signal-safe; ignore return value
        libc::write(fd, &byte as *const u8 as *const libc::c_void, 1);
    }
}

/// SIGWINCH signal handler using the self-pipe trick.
///
/// Creates an internal pipe and registers a SIGWINCH handler that writes to it.
/// The read end of the pipe can be polled alongside stdin to detect terminal resizes.
pub struct SignalHandler {
    pipe_read: RawFd,
    pipe_write: RawFd,
    old_action: libc::sigaction,
}

impl SignalHandler {
    /// Create a new `SignalHandler` that catches SIGWINCH.
    ///
    /// Sets up a pipe and registers a signal handler using `sigaction` with `SA_RESTART`.
    /// The signal handler writes a byte to the pipe when SIGWINCH is received.
    ///
    /// # Errors
    ///
    /// Returns an error if pipe creation or signal registration fails.
    pub fn new() -> std::io::Result<Self> {
        // Create the pipe
        let mut fds = [0i32; 2];
        if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        let pipe_read = fds[0];
        let pipe_write = fds[1];

        // Set both ends to non-blocking
        for &fd in &[pipe_read, pipe_write] {
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
            if flags < 0 {
                unsafe {
                    libc::close(pipe_read);
                    libc::close(pipe_write);
                }
                return Err(std::io::Error::last_os_error());
            }
            if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } != 0 {
                unsafe {
                    libc::close(pipe_read);
                    libc::close(pipe_write);
                }
                return Err(std::io::Error::last_os_error());
            }
        }

        // Set close-on-exec for both ends
        for &fd in &[pipe_read, pipe_write] {
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
            if flags >= 0 {
                unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) };
            }
        }

        // Store the write end in the global atomic
        SIGNAL_PIPE_WRITE.store(pipe_write, Ordering::Release);

        // Register the SIGWINCH handler
        let mut new_action: libc::sigaction = unsafe { std::mem::zeroed() };
        new_action.sa_sigaction = sigwinch_handler as usize;
        new_action.sa_flags = libc::SA_RESTART;
        unsafe { libc::sigemptyset(&mut new_action.sa_mask) };

        let mut old_action: libc::sigaction = unsafe { std::mem::zeroed() };

        if unsafe { libc::sigaction(libc::SIGWINCH, &new_action, &mut old_action) } != 0 {
            SIGNAL_PIPE_WRITE.store(-1, Ordering::Release);
            unsafe {
                libc::close(pipe_read);
                libc::close(pipe_write);
            }
            return Err(std::io::Error::last_os_error());
        }

        Ok(Self {
            pipe_read,
            pipe_write,
            old_action,
        })
    }

    /// Returns the read end of the signal pipe for use with `poll()`.
    pub fn pipe_fd(&self) -> RawFd {
        self.pipe_read
    }

    /// Drain all pending bytes from the signal pipe.
    ///
    /// Call this after handling the resize event to clear the pipe.
    pub fn drain(&self) {
        let mut buf = [0u8; 64];
        loop {
            let n = unsafe {
                libc::read(
                    self.pipe_read,
                    buf.as_mut_ptr().cast(),
                    buf.len(),
                )
            };
            if n <= 0 {
                break;
            }
        }
    }
}

impl Drop for SignalHandler {
    fn drop(&mut self) {
        // Restore the original signal handler
        unsafe {
            libc::sigaction(libc::SIGWINCH, &self.old_action, std::ptr::null_mut());
        }

        // Clear the global pipe write fd
        SIGNAL_PIPE_WRITE.store(-1, Ordering::Release);

        // Close the pipe
        unsafe {
            libc::close(self.pipe_read);
            libc::close(self.pipe_write);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signal_handler_creation() {
        let handler = SignalHandler::new().expect("should create signal handler");
        assert!(handler.pipe_fd() >= 0, "pipe_fd should be non-negative");
        // The global write fd should be set
        assert!(
            SIGNAL_PIPE_WRITE.load(Ordering::Acquire) >= 0,
            "global write fd should be set"
        );
        drop(handler);
        // After drop, global write fd should be cleared
        assert_eq!(
            SIGNAL_PIPE_WRITE.load(Ordering::Acquire),
            -1,
            "global write fd should be -1 after drop"
        );
    }

    #[test]
    fn test_signal_handler_pipe_fd_is_readable() {
        let handler = SignalHandler::new().expect("should create signal handler");
        let fd = handler.pipe_fd();

        // Poll the pipe with a zero timeout — should return no data (not readable)
        let mut pfd = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let ret = unsafe { libc::poll(&mut pfd, 1, 0) };
        // No signal has been sent, so the pipe should be empty
        assert_eq!(ret, 0, "pipe should not be readable initially");
    }

    #[test]
    fn test_signal_handler_drain_on_empty_pipe() {
        let handler = SignalHandler::new().expect("should create signal handler");
        // Draining an empty pipe should not block or panic
        handler.drain();
    }

    #[test]
    fn test_signal_handler_write_and_drain() {
        let handler = SignalHandler::new().expect("should create signal handler");
        let write_fd = SIGNAL_PIPE_WRITE.load(Ordering::Acquire);

        // Simulate what the signal handler does: write a byte
        let byte: u8 = 1;
        let written =
            unsafe { libc::write(write_fd, &byte as *const u8 as *const libc::c_void, 1) };
        assert_eq!(written, 1, "should write 1 byte");

        // The pipe should now be readable
        let mut pfd = libc::pollfd {
            fd: handler.pipe_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let ret = unsafe { libc::poll(&mut pfd, 1, 0) };
        assert!(ret > 0, "pipe should be readable after write");

        // Drain should consume the byte
        handler.drain();

        // Pipe should be empty again
        let ret = unsafe { libc::poll(&mut pfd, 1, 0) };
        assert_eq!(ret, 0, "pipe should be empty after drain");
    }

    #[test]
    fn test_signal_handler_drain_multiple_bytes() {
        let handler = SignalHandler::new().expect("should create signal handler");
        let write_fd = SIGNAL_PIPE_WRITE.load(Ordering::Acquire);

        // Write multiple bytes (simulating multiple rapid SIGWINCH signals)
        for _ in 0..5 {
            let byte: u8 = 1;
            unsafe { libc::write(write_fd, &byte as *const u8 as *const libc::c_void, 1) };
        }

        handler.drain();

        // Pipe should be empty
        let mut pfd = libc::pollfd {
            fd: handler.pipe_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let ret = unsafe { libc::poll(&mut pfd, 1, 0) };
        assert_eq!(ret, 0, "pipe should be empty after draining multiple bytes");
    }

    #[test]
    fn test_signal_handler_drop_restores_state() {
        let handler = SignalHandler::new().expect("should create signal handler");
        let pipe_read = handler.pipe_fd();

        drop(handler);

        // Global should be cleared
        assert_eq!(SIGNAL_PIPE_WRITE.load(Ordering::Acquire), -1);

        // The pipe fds should be closed — reading from closed fd returns error
        let mut buf = [0u8; 1];
        let ret = unsafe { libc::read(pipe_read, buf.as_mut_ptr().cast(), 1) };
        assert!(ret < 0, "read from closed pipe should fail");
    }

    #[test]
    fn test_signal_handler_pipe_nonblocking() {
        let handler = SignalHandler::new().expect("should create signal handler");
        let fd = handler.pipe_fd();

        // Verify the read end is non-blocking by trying to read from empty pipe
        let mut buf = [0u8; 1];
        let ret = unsafe { libc::read(fd, buf.as_mut_ptr().cast(), 1) };
        // Should return -1 with EAGAIN/EWOULDBLOCK
        assert_eq!(ret, -1, "non-blocking read from empty pipe should return -1");
        let err = std::io::Error::last_os_error();
        assert!(
            err.kind() == std::io::ErrorKind::WouldBlock,
            "error should be WouldBlock, got: {err}"
        );
    }

    #[test]
    fn test_sigwinch_delivery() {
        let handler = SignalHandler::new().expect("should create signal handler");

        // Send SIGWINCH to ourselves
        unsafe { libc::raise(libc::SIGWINCH) };

        // Give the signal a moment to be delivered
        // (signals are typically delivered very quickly on the same thread)
        let mut pfd = libc::pollfd {
            fd: handler.pipe_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // Use a short timeout to allow for delivery
        let ret = unsafe { libc::poll(&mut pfd, 1, 100) };
        assert!(ret > 0, "pipe should be readable after SIGWINCH");

        handler.drain();

        // Pipe should be empty after drain
        let ret = unsafe { libc::poll(&mut pfd, 1, 0) };
        assert_eq!(ret, 0, "pipe should be empty after drain");
    }

    #[test]
    fn test_sigwinch_handler_is_async_signal_safe() {
        // This test verifies the handler function can be called directly
        // (simulating what happens when the OS delivers a signal).
        let handler = SignalHandler::new().expect("should create signal handler");

        // Call the handler function directly
        unsafe { sigwinch_handler(libc::SIGWINCH) };

        // Verify a byte was written to the pipe
        let mut pfd = libc::pollfd {
            fd: handler.pipe_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let ret = unsafe { libc::poll(&mut pfd, 1, 0) };
        assert!(ret > 0, "pipe should be readable after direct handler call");

        handler.drain();
    }

    #[test]
    fn test_sigwinch_handler_with_invalid_fd() {
        // When no handler is installed, the global fd is -1
        // Calling the handler should not crash
        let prev = SIGNAL_PIPE_WRITE.load(Ordering::Acquire);
        SIGNAL_PIPE_WRITE.store(-1, Ordering::Release);

        unsafe { sigwinch_handler(libc::SIGWINCH) };
        // Should not crash or hang

        SIGNAL_PIPE_WRITE.store(prev, Ordering::Release);
    }
}
