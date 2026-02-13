use std::io::{self, Write};

use crate::tui::ansi;
use crate::tui::buffer::{Buffer, Cell};
use crate::tui::style::Style;

/// Double-buffered terminal renderer with diff-based output.
///
/// Maintains a front buffer (what is currently displayed) and a back buffer
/// (what we want to display next). On flush, only the cells that differ
/// between front and back are emitted as ANSI escape sequences, producing
/// flicker-free updates.
pub struct Renderer<W: Write> {
    front: Buffer,
    back: Buffer,
    output: W,
    width: u16,
    height: u16,
}

impl<W: Write> Renderer<W> {
    /// Create a new renderer with the given dimensions and output writer.
    pub fn new(width: u16, height: u16, output: W) -> Self {
        let mut back = Buffer::new(width, height);
        back.mark_all_dirty();
        Self {
            front: Buffer::new(width, height),
            back,
            output,
            width,
            height,
        }
    }

    /// Get mutable access to the back buffer for drawing.
    pub fn back_buffer(&mut self) -> &mut Buffer {
        &mut self.back
    }

    /// Get the current width.
    pub fn width(&self) -> u16 {
        self.width
    }

    /// Get the current height.
    pub fn height(&self) -> u16 {
        self.height
    }

    /// Compare front and back buffers, emit ANSI sequences for changed cells,
    /// then update the front buffer to match.
    #[allow(clippy::cast_possible_truncation)]
    pub fn flush(&mut self) -> io::Result<()> {
        if self.width == 0 || self.height == 0 {
            return Ok(());
        }

        let mut current_style: Option<Style> = None;

        for row in 0..self.height {
            let mut col: u16 = 0;
            while col < self.width {
                let back_cell = *self.back.get(col, row);
                let front_cell = *self.front.get(col, row);

                if back_cell == front_cell {
                    col += 1;
                    continue;
                }

                // Found a changed cell — start a run of consecutive changes
                let run_start = col;

                // Position cursor (ANSI is 1-based)
                ansi::move_to(&mut self.output, col + 1, row + 1)?;

                // Process this cell and consecutive changed cells
                while col < self.width {
                    let bc = *self.back.get(col, row);
                    let fc = *self.front.get(col, row);

                    if bc == fc && col != run_start {
                        // End of changed run
                        break;
                    }

                    // Emit style if changed
                    emit_style_change(&mut self.output, &mut current_style, bc.style)?;

                    // Emit character
                    write_char(&mut self.output, bc.ch)?;

                    col += 1;
                }

                // Check if rest of row is default spaces — use clear_to_end_of_line
                if self.rest_of_row_is_default(&self.back, col, row) {
                    // Reset style for the clear operation
                    if current_style.is_some() && current_style != Some(Style::new()) {
                        Style::write_reset(&mut self.output)?;
                        current_style = Some(Style::new());
                    }
                    ansi::clear_to_end_of_line(&mut self.output)?;
                }
            }
        }

        // Reset style at end of frame
        if current_style.is_some() && current_style != Some(Style::new()) {
            Style::write_reset(&mut self.output)?;
        }

        self.output.flush()?;

        // Sync: copy back buffer content to front buffer
        self.sync_front_from_back();

        Ok(())
    }

    /// Mark all cells dirty and trigger a full re-render on next flush.
    pub fn force_redraw(&mut self) {
        // Reset front buffer so all cells will differ from back buffer
        self.front = Buffer::new(self.width, self.height);
        self.back.mark_all_dirty();
    }

    /// Handle terminal resize: resize both buffers, clear screen, and prepare
    /// for a full redraw.
    pub fn resize(&mut self, new_width: u16, new_height: u16) -> io::Result<()> {
        self.width = new_width;
        self.height = new_height;
        self.front.resize(new_width, new_height);
        self.back.resize(new_width, new_height);
        ansi::clear_screen(&mut self.output)?;
        ansi::move_to(&mut self.output, 1, 1)?;
        self.output.flush()?;
        Ok(())
    }

    /// Check if the rest of a row (from col to end) is all default cells.
    fn rest_of_row_is_default(&self, buf: &Buffer, col: u16, row: u16) -> bool {
        let default = Cell::default();
        for c in col..self.width {
            if *buf.get(c, row) != default {
                return false;
            }
        }
        // Only optimize if there are actually remaining columns
        col < self.width
    }

    /// Copy back buffer contents to front buffer cell by cell.
    fn sync_front_from_back(&mut self) {
        for row in 0..self.height {
            for col in 0..self.width {
                let cell = *self.back.get(col, row);
                // Use set to copy content; front's dirty flags don't matter
                // since we never read them for rendering.
                self.front.set(col, row, cell.ch, cell.style);
            }
        }
        self.front.clear_dirty();
        self.back.clear_dirty();
    }
}

/// Emit an SGR style change only when the new style differs from the current.
fn emit_style_change(w: &mut impl Write, current: &mut Option<Style>, new_style: Style) -> io::Result<()> {
    if *current == Some(new_style) {
        return Ok(());
    }

    // We always reset and re-apply because diffing individual style attributes
    // and emitting minimal SGR changes would be complex and error-prone.
    // Reset + apply is simple, correct, and the overhead is minimal since
    // we only do it when the style actually changes.
    if new_style.is_empty() {
        Style::write_reset(w)?;
    } else {
        Style::write_reset(w)?;
        new_style.write_sgr(w)?;
    }

    *current = Some(new_style);
    Ok(())
}

/// Write a single character to the output.
fn write_char(w: &mut impl Write, ch: char) -> io::Result<()> {
    let mut buf = [0u8; 4];
    let s = ch.encode_utf8(&mut buf);
    w.write_all(s.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::style::Color;

    /// Helper: create a renderer backed by a Vec<u8> for testing.
    fn test_renderer(width: u16, height: u16) -> Renderer<Vec<u8>> {
        Renderer::new(width, height, Vec::new())
    }

    /// Extract the output bytes from a renderer.
    fn output(r: &Renderer<Vec<u8>>) -> &[u8] {
        &r.output
    }

    /// Extract the output as a string.
    fn output_str(r: &Renderer<Vec<u8>>) -> String {
        String::from_utf8_lossy(&r.output).into_owned()
    }

    // --- Construction ---

    #[test]
    fn test_new_creates_correct_dimensions() {
        let r = test_renderer(80, 24);
        assert_eq!(r.width(), 80);
        assert_eq!(r.height(), 24);
    }

    #[test]
    fn test_new_zero_dimensions() {
        let r = test_renderer(0, 0);
        assert_eq!(r.width(), 0);
        assert_eq!(r.height(), 0);
    }

    #[test]
    fn test_back_buffer_accessible() {
        let mut r = test_renderer(10, 5);
        let buf = r.back_buffer();
        assert_eq!(buf.width(), 10);
        assert_eq!(buf.height(), 5);
    }

    // --- First flush (full draw) ---

    #[test]
    fn test_first_flush_empty_buffer_no_output() {
        // A completely default buffer (all spaces, default style) produces
        // output on first flush because front and back differ (back is marked
        // all-dirty on construction, but content is default in both).
        // Actually: back buffer is initialized with mark_all_dirty but front
        // and back have the same content (defaults). Since we diff by cell
        // content equality, not by dirty flags, no output should be produced.
        let mut r = test_renderer(5, 3);
        // Front and back are both default — no diff
        // But wait: back has mark_all_dirty. Our flush uses cell comparison,
        // not dirty flags. So identical content = no output.
        r.flush().unwrap();
        // Since both buffers start with the same default content,
        // and our diff is content-based, the output should be empty.
        // But the constructor marks back dirty AND resets front to default.
        // Both have default cells so diff finds no changes.
        assert!(output(&r).is_empty());
    }

    #[test]
    fn test_first_flush_with_content() {
        let mut r = test_renderer(10, 3);
        r.back_buffer().write_str(0, 0, "hello", Style::new());
        r.flush().unwrap();

        let out = output_str(&r);
        // Should contain cursor movement and text
        assert!(out.contains("hello"));
        // Should position cursor at row 1, col 1 (1-based)
        assert!(out.contains("\x1b[1;1H"));
    }

    #[test]
    fn test_flush_zero_dimensions_no_output() {
        let mut r = test_renderer(0, 0);
        r.flush().unwrap();
        assert!(output(&r).is_empty());
    }

    // --- Diff-based output ---

    #[test]
    fn test_no_changes_no_output() {
        let mut r = test_renderer(10, 3);
        r.back_buffer().write_str(0, 0, "hello", Style::new());
        r.flush().unwrap();

        // Clear captured output
        r.output.clear();

        // Flush again with no changes
        r.flush().unwrap();
        assert!(output(&r).is_empty());
    }

    #[test]
    fn test_single_cell_change() {
        let mut r = test_renderer(10, 3);
        r.back_buffer().write_str(0, 0, "hello", Style::new());
        r.flush().unwrap();
        r.output.clear();

        // Change one cell
        r.back_buffer().set(2, 0, 'L', Style::new());
        r.flush().unwrap();

        let out = output_str(&r);
        // Should move cursor to col 3, row 1 (1-based) and emit 'L'
        assert!(out.contains("\x1b[1;3H"));
        assert!(out.contains('L'));
    }

    #[test]
    fn test_consecutive_changed_cells_batched() {
        let mut r = test_renderer(10, 1);
        r.flush().unwrap();
        r.output.clear();

        // Write a run of characters
        r.back_buffer().write_str(2, 0, "abc", Style::new());
        r.flush().unwrap();

        let out = output_str(&r);
        // Should have one move_to and then the text together
        assert!(out.contains("\x1b[1;3H")); // move to col 3 row 1 (1-based)
        assert!(out.contains("abc"));
    }

    #[test]
    fn test_non_consecutive_changes_get_separate_moves() {
        let mut r = test_renderer(10, 1);
        r.flush().unwrap();
        r.output.clear();

        // Change two non-adjacent cells
        r.back_buffer().set(1, 0, 'A', Style::new());
        r.back_buffer().set(5, 0, 'B', Style::new());
        r.flush().unwrap();

        let out = output_str(&r);
        // Should have two move_to sequences
        assert!(out.contains("\x1b[1;2H")); // col 2 (1-based)
        assert!(out.contains("\x1b[1;6H")); // col 6 (1-based)
        assert!(out.contains('A'));
        assert!(out.contains('B'));
    }

    // --- Style handling ---

    #[test]
    fn test_styled_text_emits_sgr() {
        let mut r = test_renderer(10, 1);
        let style = Style::new().fg(Color::Red);
        r.back_buffer().write_str(0, 0, "hi", style);
        r.flush().unwrap();

        let out = output_str(&r);
        // Should contain SGR for red foreground (31)
        assert!(out.contains("\x1b[31m") || out.contains(";31m") || out.contains("31;"));
    }

    #[test]
    fn test_style_change_within_row() {
        let mut r = test_renderer(10, 1);
        let red = Style::new().fg(Color::Red);
        let blue = Style::new().fg(Color::Blue);
        r.back_buffer().write_str(0, 0, "rr", red);
        r.back_buffer().write_str(2, 0, "bb", blue);
        r.flush().unwrap();

        let out = output_str(&r);
        // Should contain both red (31) and blue (34) SGR codes
        assert!(out.contains("31"));
        assert!(out.contains("34"));
    }

    #[test]
    fn test_same_style_no_redundant_sgr() {
        let mut r = test_renderer(10, 1);
        let style = Style::new().fg(Color::Green);
        r.back_buffer().write_str(0, 0, "abcde", style);
        r.flush().unwrap();

        let out = output_str(&r);
        // Count occurrences of the green SGR code — should appear only once
        let green_count = out.matches("\x1b[0m\x1b[32m").count();
        assert_eq!(green_count, 1, "SGR should be emitted only once for a uniform-style run");
    }

    #[test]
    fn test_reset_emitted_at_end_of_frame() {
        let mut r = test_renderer(10, 1);
        let style = Style::new().fg(Color::Red);
        r.back_buffer().write_str(0, 0, "hi", style);
        r.flush().unwrap();

        let out = output_str(&r);
        // Should end with a reset (\x1b[0m) before the flush
        assert!(out.contains("\x1b[0m"));
    }

    #[test]
    fn test_default_style_no_sgr_emitted() {
        let mut r = test_renderer(10, 1);
        r.back_buffer().write_str(0, 0, "abc", Style::new());
        r.flush().unwrap();

        let out = output_str(&r);
        // With default style, we should not emit SGR sequences.
        // The reset at start is \x1b[0m for the style transition.
        // Actually, the very first style emission: current is None,
        // new is Style::new() (empty). emit_style_change writes \x1b[0m.
        // That's expected — it's the initial reset.
        // But there should be no color SGR codes.
        assert!(!out.contains("\x1b[31m")); // no red
        assert!(!out.contains("\x1b[34m")); // no blue
    }

    // --- force_redraw ---

    #[test]
    fn test_force_redraw_causes_full_output() {
        let mut r = test_renderer(5, 1);
        let style = Style::new().fg(Color::Red);
        r.back_buffer().write_str(0, 0, "hello", style);
        r.flush().unwrap();
        r.output.clear();

        // No changes to back buffer, but force a redraw
        r.force_redraw();
        r.flush().unwrap();

        let out = output_str(&r);
        // Should re-emit everything
        assert!(out.contains("hello"));
    }

    #[test]
    fn test_force_redraw_after_no_changes() {
        let mut r = test_renderer(5, 1);
        r.back_buffer().write_str(0, 0, "test", Style::new());
        r.flush().unwrap();
        r.output.clear();

        // Second flush with no changes
        r.flush().unwrap();
        assert!(output(&r).is_empty());

        // Force redraw
        r.force_redraw();
        r.flush().unwrap();
        let out = output_str(&r);
        assert!(out.contains("test"));
    }

    // --- resize ---

    #[test]
    fn test_resize_updates_dimensions() {
        let mut r = test_renderer(80, 24);
        r.resize(120, 40).unwrap();
        assert_eq!(r.width(), 120);
        assert_eq!(r.height(), 40);
    }

    #[test]
    fn test_resize_clears_screen() {
        let mut r = test_renderer(10, 5);
        r.output.clear();
        r.resize(20, 10).unwrap();

        let out = output_str(&r);
        assert!(out.contains("\x1b[2J")); // clear screen
        assert!(out.contains("\x1b[1;1H")); // home cursor
    }

    #[test]
    fn test_resize_resets_buffers() {
        let mut r = test_renderer(10, 5);
        r.back_buffer().write_str(0, 0, "hello", Style::new());
        r.flush().unwrap();

        r.resize(20, 10).unwrap();
        assert_eq!(r.back_buffer().width(), 20);
        assert_eq!(r.back_buffer().height(), 10);

        // After resize, both buffers should be reset, so the old content is gone
        assert_eq!(r.back_buffer().get(0, 0).ch, ' ');
    }

    #[test]
    fn test_resize_to_zero() {
        let mut r = test_renderer(10, 5);
        r.resize(0, 0).unwrap();
        assert_eq!(r.width(), 0);
        assert_eq!(r.height(), 0);
    }

    #[test]
    fn test_resize_then_draw_and_flush() {
        let mut r = test_renderer(5, 1);
        r.resize(10, 2).unwrap();
        r.output.clear();

        r.back_buffer().write_str(0, 0, "resized", Style::new());
        r.flush().unwrap();

        let out = output_str(&r);
        assert!(out.contains("resized"));
    }

    // --- Multi-row rendering ---

    #[test]
    fn test_multiple_rows() {
        let mut r = test_renderer(10, 3);
        r.back_buffer().write_str(0, 0, "row0", Style::new());
        r.back_buffer().write_str(0, 1, "row1", Style::new());
        r.back_buffer().write_str(0, 2, "row2", Style::new());
        r.flush().unwrap();

        let out = output_str(&r);
        assert!(out.contains("row0"));
        assert!(out.contains("row1"));
        assert!(out.contains("row2"));
        // Each row should have its own cursor positioning
        assert!(out.contains("\x1b[1;1H")); // row 1
        assert!(out.contains("\x1b[2;1H")); // row 2
        assert!(out.contains("\x1b[3;1H")); // row 3
    }

    #[test]
    fn test_change_in_middle_row_only() {
        let mut r = test_renderer(10, 3);
        r.back_buffer().write_str(0, 0, "row0", Style::new());
        r.back_buffer().write_str(0, 1, "row1", Style::new());
        r.back_buffer().write_str(0, 2, "row2", Style::new());
        r.flush().unwrap();
        r.output.clear();

        // Only change middle row
        r.back_buffer().write_str(0, 1, "XXXX", Style::new());
        r.flush().unwrap();

        let out = output_str(&r);
        // Should only contain row 2 positioning and content
        assert!(out.contains("\x1b[2;1H"));
        assert!(out.contains("XXXX"));
        // Should NOT re-emit row0 or row2
        assert!(!out.contains("row0"));
        assert!(!out.contains("row2"));
    }

    // --- Front/back buffer sync ---

    #[test]
    fn test_front_buffer_syncs_after_flush() {
        let mut r = test_renderer(10, 1);
        r.back_buffer().write_str(0, 0, "sync", Style::new());
        r.flush().unwrap();
        r.output.clear();

        // Without changing back buffer, flush should produce no output
        r.flush().unwrap();
        assert!(output(&r).is_empty());
    }

    #[test]
    fn test_partial_overwrite() {
        let mut r = test_renderer(10, 1);
        r.back_buffer().write_str(0, 0, "abcde", Style::new());
        r.flush().unwrap();
        r.output.clear();

        // Overwrite part of the string
        r.back_buffer().write_str(2, 0, "XY", Style::new());
        r.flush().unwrap();

        let out = output_str(&r);
        assert!(out.contains("\x1b[1;3H")); // col 3 (1-based)
        assert!(out.contains("XY"));
        // Should not re-emit unchanged chars
        assert!(!out.contains('a'));
        assert!(!out.contains('b'));
        assert!(!out.contains('e'));
    }

    // --- Unicode ---

    #[test]
    fn test_unicode_characters() {
        let mut r = test_renderer(10, 1);
        r.back_buffer().write_str(0, 0, "héllo", Style::new());
        r.flush().unwrap();

        let out = output_str(&r);
        assert!(out.contains("héllo"));
    }

    // --- Style transitions ---

    #[test]
    fn test_bold_style() {
        let mut r = test_renderer(10, 1);
        let bold = Style::new().bold(true);
        r.back_buffer().write_str(0, 0, "bold", bold);
        r.flush().unwrap();

        let out = output_str(&r);
        assert!(out.contains("\x1b[1m")); // bold SGR
        assert!(out.contains("bold"));
    }

    #[test]
    fn test_combined_attributes() {
        let mut r = test_renderer(10, 1);
        let style = Style::new()
            .bold(true)
            .underline(true)
            .fg(Color::Cyan);
        r.back_buffer().write_str(0, 0, "hi", style);
        r.flush().unwrap();

        let out = output_str(&r);
        // Should contain bold (1), underline (4), and cyan fg (36)
        assert!(out.contains("1;"));
        assert!(out.contains(";4;") || out.contains("4;"));
        assert!(out.contains("36"));
    }

    // --- Edge cases ---

    #[test]
    fn test_single_cell_buffer() {
        let mut r = test_renderer(1, 1);
        r.back_buffer().set(0, 0, 'X', Style::new());
        r.flush().unwrap();

        let out = output_str(&r);
        assert!(out.contains('X'));
    }

    #[test]
    fn test_full_buffer_write() {
        let mut r = test_renderer(3, 2);
        r.back_buffer().write_str(0, 0, "abc", Style::new());
        r.back_buffer().write_str(0, 1, "def", Style::new());
        r.flush().unwrap();

        let out = output_str(&r);
        assert!(out.contains("abc"));
        assert!(out.contains("def"));
    }

    #[test]
    fn test_emit_style_change_same_style_no_output() {
        let mut buf = Vec::new();
        let mut current = Some(Style::new().fg(Color::Red));
        let style = Style::new().fg(Color::Red);
        emit_style_change(&mut buf, &mut current, style).unwrap();
        assert!(buf.is_empty());
    }

    #[test]
    fn test_emit_style_change_different_style() {
        let mut buf = Vec::new();
        let mut current = Some(Style::new().fg(Color::Red));
        let style = Style::new().fg(Color::Blue);
        emit_style_change(&mut buf, &mut current, style).unwrap();
        assert!(!buf.is_empty());
        let out = String::from_utf8(buf).unwrap();
        assert!(out.contains("34")); // blue fg
    }

    #[test]
    fn test_emit_style_change_from_none() {
        let mut buf = Vec::new();
        let mut current: Option<Style> = None;
        let style = Style::new().fg(Color::Green);
        emit_style_change(&mut buf, &mut current, style).unwrap();
        let out = String::from_utf8(buf).unwrap();
        assert!(out.contains("32")); // green fg
        assert_eq!(current, Some(style));
    }

    #[test]
    fn test_emit_style_change_to_default() {
        let mut buf = Vec::new();
        let mut current = Some(Style::new().fg(Color::Red));
        emit_style_change(&mut buf, &mut current, Style::new()).unwrap();
        let out = String::from_utf8(buf).unwrap();
        assert!(out.contains("\x1b[0m")); // reset
        assert_eq!(current, Some(Style::new()));
    }

    #[test]
    fn test_write_char_ascii() {
        let mut buf = Vec::new();
        write_char(&mut buf, 'A').unwrap();
        assert_eq!(buf, b"A");
    }

    #[test]
    fn test_write_char_multibyte() {
        let mut buf = Vec::new();
        write_char(&mut buf, 'é').unwrap();
        assert_eq!(buf, "é".as_bytes());
    }

    // --- Repeated flush cycles ---

    #[test]
    fn test_multiple_flush_cycles() {
        let mut r = test_renderer(10, 1);

        // Cycle 1
        r.back_buffer().write_str(0, 0, "one", Style::new());
        r.flush().unwrap();
        r.output.clear();

        // Cycle 2
        r.back_buffer().write_str(0, 0, "two", Style::new());
        r.flush().unwrap();
        let out = output_str(&r);
        assert!(out.contains("two"));
        r.output.clear();

        // Cycle 3 — no change
        r.flush().unwrap();
        assert!(output(&r).is_empty());

        // Cycle 4 — change back
        r.back_buffer().write_str(0, 0, "one", Style::new());
        r.flush().unwrap();
        let out = output_str(&r);
        assert!(out.contains("one"));
    }

    #[test]
    fn test_clear_and_redraw() {
        let mut r = test_renderer(10, 1);
        let red = Style::new().fg(Color::Red);
        r.back_buffer().write_str(0, 0, "hello", red);
        r.flush().unwrap();
        r.output.clear();

        // Clear the back buffer
        r.back_buffer().clear_region(0, 0, 10, 1, Style::new());
        r.flush().unwrap();

        let out = output_str(&r);
        // Should emit something to clear the previously drawn cells
        assert!(!out.is_empty());
    }
}
