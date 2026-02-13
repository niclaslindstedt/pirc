use crate::tui::mirc_colors::StyledSpan;
use crate::tui::style::Style;

/// A single cell in the screen buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    pub style: Style,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            ch: ' ',
            style: Style::new(),
        }
    }
}

/// A 2D grid of styled cells with dirty-region tracking.
///
/// Cells are stored in row-major order: `index = row * width + col`.
/// Each cell has an associated dirty flag that is set when the cell content
/// changes, enabling efficient partial screen updates.
pub struct Buffer {
    cells: Vec<Cell>,
    dirty: Vec<bool>,
    width: u16,
    height: u16,
}

impl Buffer {
    /// Create a new buffer filled with default cells (space, default style).
    pub fn new(width: u16, height: u16) -> Self {
        let size = (width as usize) * (height as usize);
        Self {
            cells: vec![Cell::default(); size],
            dirty: vec![false; size],
            width,
            height,
        }
    }

    /// Returns the width of the buffer.
    pub fn width(&self) -> u16 {
        self.width
    }

    /// Returns the height of the buffer.
    pub fn height(&self) -> u16 {
        self.height
    }

    /// Compute the flat index for (col, row). Returns `None` if out of bounds.
    fn index(&self, col: u16, row: u16) -> Option<usize> {
        if col < self.width && row < self.height {
            Some((row as usize) * (self.width as usize) + (col as usize))
        } else {
            None
        }
    }

    /// Read a cell at the given position.
    ///
    /// Returns the default cell if the position is out of bounds.
    pub fn get(&self, col: u16, row: u16) -> &Cell {
        match self.index(col, row) {
            Some(idx) => &self.cells[idx],
            None => &DEFAULT_CELL,
        }
    }

    /// Write a cell at the given position. Marks the cell dirty only if
    /// the content actually changes. Out-of-bounds writes are silently ignored.
    pub fn set(&mut self, col: u16, row: u16, ch: char, style: Style) {
        if let Some(idx) = self.index(col, row) {
            let cell = &mut self.cells[idx];
            if cell.ch != ch || cell.style != style {
                cell.ch = ch;
                cell.style = style;
                self.dirty[idx] = true;
            }
        }
    }

    /// Write a string starting at (col, row) with the given style.
    ///
    /// Characters are written left-to-right. Characters that would extend
    /// beyond the buffer width are silently skipped. The row is not wrapped.
    pub fn write_str(&mut self, col: u16, row: u16, text: &str, style: Style) {
        if row >= self.height {
            return;
        }
        let mut c = col as usize;
        for ch in text.chars() {
            if c >= self.width as usize {
                break;
            }
            self.set(c as u16, row, ch, style);
            c += 1;
        }
    }

    /// Write pre-styled spans (e.g. from the mIRC parser) starting at (col, row).
    ///
    /// Each span's text is written with its associated style. Characters
    /// beyond the buffer width are silently skipped. The row is not wrapped.
    pub fn write_styled_spans(&mut self, col: u16, row: u16, spans: &[StyledSpan<'_>]) {
        if row >= self.height {
            return;
        }
        let mut c = col as usize;
        for span in spans {
            for ch in span.text.chars() {
                if c >= self.width as usize {
                    return;
                }
                self.set(c as u16, row, ch, span.style);
                c += 1;
            }
        }
    }

    /// Fill a rectangular region with spaces in the given style.
    ///
    /// The region is clamped to the buffer bounds. Out-of-bounds portions
    /// are silently ignored.
    pub fn clear_region(&mut self, col: u16, row: u16, width: u16, height: u16, style: Style) {
        let col_end = (col as usize + width as usize).min(self.width as usize);
        let row_end = (row as usize + height as usize).min(self.height as usize);
        let col_start = (col as usize).min(self.width as usize);
        let row_start = (row as usize).min(self.height as usize);

        for r in row_start..row_end {
            for c in col_start..col_end {
                self.set(c as u16, r as u16, ' ', style);
            }
        }
    }

    /// Resize the buffer to new dimensions. All cells are reset to defaults
    /// and all cells are marked dirty.
    pub fn resize(&mut self, new_width: u16, new_height: u16) {
        let size = (new_width as usize) * (new_height as usize);
        self.width = new_width;
        self.height = new_height;
        self.cells = vec![Cell::default(); size];
        self.dirty = vec![true; size];
    }

    /// Iterate over only the dirty cells, yielding `(col, row, &Cell)`.
    pub fn dirty_cells(&self) -> impl Iterator<Item = (u16, u16, &Cell)> {
        let width = self.width as usize;
        self.dirty
            .iter()
            .enumerate()
            .filter(|(_, &d)| d)
            .map(move |(idx, _)| {
                let col = (idx % width) as u16;
                let row = (idx / width) as u16;
                (col, row, &self.cells[idx])
            })
    }

    /// Mark all cells as dirty (for full redraws).
    pub fn mark_all_dirty(&mut self) {
        self.dirty.fill(true);
    }

    /// Reset all dirty flags after a render pass.
    pub fn clear_dirty(&mut self) {
        self.dirty.fill(false);
    }
}

/// Static default cell for out-of-bounds `get()` calls.
static DEFAULT_CELL: Cell = Cell {
    ch: ' ',
    style: Style::new(),
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::style::Color;

    // --- Cell default ---

    #[test]
    fn test_cell_default() {
        let cell = Cell::default();
        assert_eq!(cell.ch, ' ');
        assert_eq!(cell.style, Style::new());
    }

    // --- Buffer::new ---

    #[test]
    fn test_new_creates_correct_size() {
        let buf = Buffer::new(80, 24);
        assert_eq!(buf.width(), 80);
        assert_eq!(buf.height(), 24);
    }

    #[test]
    fn test_new_all_cells_default() {
        let buf = Buffer::new(10, 5);
        for row in 0..5 {
            for col in 0..10 {
                let cell = buf.get(col, row);
                assert_eq!(cell.ch, ' ');
                assert_eq!(cell.style, Style::new());
            }
        }
    }

    #[test]
    fn test_new_no_dirty_cells() {
        let buf = Buffer::new(10, 5);
        assert_eq!(buf.dirty_cells().count(), 0);
    }

    #[test]
    fn test_new_zero_width() {
        let buf = Buffer::new(0, 10);
        assert_eq!(buf.width(), 0);
        assert_eq!(buf.height(), 10);
        assert_eq!(buf.dirty_cells().count(), 0);
    }

    #[test]
    fn test_new_zero_height() {
        let buf = Buffer::new(10, 0);
        assert_eq!(buf.width(), 10);
        assert_eq!(buf.height(), 0);
        assert_eq!(buf.dirty_cells().count(), 0);
    }

    #[test]
    fn test_new_zero_dimensions() {
        let buf = Buffer::new(0, 0);
        assert_eq!(buf.width(), 0);
        assert_eq!(buf.height(), 0);
        assert_eq!(buf.dirty_cells().count(), 0);
    }

    // --- Buffer::get ---

    #[test]
    fn test_get_out_of_bounds_returns_default() {
        let buf = Buffer::new(10, 5);
        let cell = buf.get(10, 0);
        assert_eq!(cell.ch, ' ');
        assert_eq!(cell.style, Style::new());

        let cell = buf.get(0, 5);
        assert_eq!(cell.ch, ' ');

        let cell = buf.get(100, 100);
        assert_eq!(cell.ch, ' ');
    }

    // --- Buffer::set ---

    #[test]
    fn test_set_basic() {
        let mut buf = Buffer::new(10, 5);
        let style = Style::new().fg(Color::Red);
        buf.set(3, 2, 'X', style);

        let cell = buf.get(3, 2);
        assert_eq!(cell.ch, 'X');
        assert_eq!(cell.style, style);
    }

    #[test]
    fn test_set_marks_dirty() {
        let mut buf = Buffer::new(10, 5);
        buf.set(3, 2, 'X', Style::new());

        let dirty: Vec<_> = buf.dirty_cells().collect();
        assert_eq!(dirty.len(), 1);
        assert_eq!(dirty[0].0, 3); // col
        assert_eq!(dirty[0].1, 2); // row
        assert_eq!(dirty[0].2.ch, 'X');
    }

    #[test]
    fn test_set_same_value_not_dirty() {
        let mut buf = Buffer::new(10, 5);
        // Setting default cell to default values should not mark dirty
        buf.set(0, 0, ' ', Style::new());
        assert_eq!(buf.dirty_cells().count(), 0);
    }

    #[test]
    fn test_set_different_char_marks_dirty() {
        let mut buf = Buffer::new(10, 5);
        buf.set(0, 0, 'A', Style::new());
        assert_eq!(buf.dirty_cells().count(), 1);
    }

    #[test]
    fn test_set_different_style_marks_dirty() {
        let mut buf = Buffer::new(10, 5);
        buf.set(0, 0, ' ', Style::new().bold(true));
        assert_eq!(buf.dirty_cells().count(), 1);
    }

    #[test]
    fn test_set_out_of_bounds_ignored() {
        let mut buf = Buffer::new(10, 5);
        buf.set(10, 0, 'X', Style::new());
        buf.set(0, 5, 'X', Style::new());
        buf.set(100, 100, 'X', Style::new());
        assert_eq!(buf.dirty_cells().count(), 0);
    }

    // --- Buffer::write_str ---

    #[test]
    fn test_write_str_basic() {
        let mut buf = Buffer::new(20, 5);
        let style = Style::new().fg(Color::Green);
        buf.write_str(2, 1, "hello", style);

        assert_eq!(buf.get(2, 1).ch, 'h');
        assert_eq!(buf.get(3, 1).ch, 'e');
        assert_eq!(buf.get(4, 1).ch, 'l');
        assert_eq!(buf.get(5, 1).ch, 'l');
        assert_eq!(buf.get(6, 1).ch, 'o');

        for col in 2..7 {
            assert_eq!(buf.get(col, 1).style, style);
        }
    }

    #[test]
    fn test_write_str_truncates_at_width() {
        let mut buf = Buffer::new(5, 1);
        buf.write_str(3, 0, "abcdef", Style::new());

        assert_eq!(buf.get(3, 0).ch, 'a');
        assert_eq!(buf.get(4, 0).ch, 'b');
        // 'c', 'd', 'e', 'f' should be dropped
        assert_eq!(buf.dirty_cells().count(), 2);
    }

    #[test]
    fn test_write_str_at_width_boundary() {
        let mut buf = Buffer::new(5, 1);
        buf.write_str(5, 0, "abc", Style::new());
        // Starting at col 5 which is out of bounds for width 5
        assert_eq!(buf.dirty_cells().count(), 0);
    }

    #[test]
    fn test_write_str_out_of_bounds_row() {
        let mut buf = Buffer::new(10, 5);
        buf.write_str(0, 5, "hello", Style::new());
        assert_eq!(buf.dirty_cells().count(), 0);
    }

    #[test]
    fn test_write_str_empty() {
        let mut buf = Buffer::new(10, 5);
        buf.write_str(0, 0, "", Style::new());
        assert_eq!(buf.dirty_cells().count(), 0);
    }

    #[test]
    fn test_write_str_multibyte_chars() {
        let mut buf = Buffer::new(10, 1);
        buf.write_str(0, 0, "aéb", Style::new());
        assert_eq!(buf.get(0, 0).ch, 'a');
        assert_eq!(buf.get(1, 0).ch, 'é');
        assert_eq!(buf.get(2, 0).ch, 'b');
        assert_eq!(buf.dirty_cells().count(), 3);
    }

    // --- Buffer::write_styled_spans ---

    #[test]
    fn test_write_styled_spans_basic() {
        let mut buf = Buffer::new(20, 1);
        let red = Style::new().fg(Color::Red);
        let blue = Style::new().fg(Color::Blue);
        let spans = vec![
            StyledSpan {
                text: "red",
                style: red,
            },
            StyledSpan {
                text: "blue",
                style: blue,
            },
        ];
        buf.write_styled_spans(0, 0, &spans);

        assert_eq!(buf.get(0, 0).ch, 'r');
        assert_eq!(buf.get(0, 0).style, red);
        assert_eq!(buf.get(1, 0).ch, 'e');
        assert_eq!(buf.get(2, 0).ch, 'd');
        assert_eq!(buf.get(2, 0).style, red);
        assert_eq!(buf.get(3, 0).ch, 'b');
        assert_eq!(buf.get(3, 0).style, blue);
        assert_eq!(buf.get(6, 0).ch, 'e');
        assert_eq!(buf.get(6, 0).style, blue);
    }

    #[test]
    fn test_write_styled_spans_truncates() {
        let mut buf = Buffer::new(4, 1);
        let spans = vec![StyledSpan {
            text: "abcdef",
            style: Style::new(),
        }];
        buf.write_styled_spans(0, 0, &spans);

        assert_eq!(buf.get(0, 0).ch, 'a');
        assert_eq!(buf.get(3, 0).ch, 'd');
        assert_eq!(buf.dirty_cells().count(), 4);
    }

    #[test]
    fn test_write_styled_spans_with_offset() {
        let mut buf = Buffer::new(10, 1);
        let spans = vec![StyledSpan {
            text: "hi",
            style: Style::new().bold(true),
        }];
        buf.write_styled_spans(5, 0, &spans);

        assert_eq!(buf.get(5, 0).ch, 'h');
        assert_eq!(buf.get(6, 0).ch, 'i');
        assert!(buf.get(5, 0).style.bold);
    }

    #[test]
    fn test_write_styled_spans_out_of_bounds_row() {
        let mut buf = Buffer::new(10, 5);
        let spans = vec![StyledSpan {
            text: "hello",
            style: Style::new(),
        }];
        buf.write_styled_spans(0, 5, &spans);
        assert_eq!(buf.dirty_cells().count(), 0);
    }

    #[test]
    fn test_write_styled_spans_empty() {
        let mut buf = Buffer::new(10, 1);
        buf.write_styled_spans(0, 0, &[]);
        assert_eq!(buf.dirty_cells().count(), 0);
    }

    // --- Buffer::clear_region ---

    #[test]
    fn test_clear_region_basic() {
        let mut buf = Buffer::new(10, 5);
        let style = Style::new().fg(Color::Red);
        // First fill some cells
        buf.write_str(0, 0, "XXXXXXXXXX", style);
        buf.clear_dirty();

        // Now clear a region
        let clear_style = Style::new().bg(Color::Blue);
        buf.clear_region(2, 0, 3, 1, clear_style);

        assert_eq!(buf.get(2, 0).ch, ' ');
        assert_eq!(buf.get(2, 0).style, clear_style);
        assert_eq!(buf.get(3, 0).ch, ' ');
        assert_eq!(buf.get(4, 0).ch, ' ');
        // Adjacent cells unchanged
        assert_eq!(buf.get(1, 0).ch, 'X');
        assert_eq!(buf.get(5, 0).ch, 'X');
    }

    #[test]
    fn test_clear_region_clamps_to_bounds() {
        let mut buf = Buffer::new(5, 3);
        // Clear region that extends beyond buffer
        buf.clear_region(3, 1, 10, 10, Style::new().bold(true));

        // Should only affect cells within bounds
        assert!(buf.get(3, 1).style.bold);
        assert!(buf.get(4, 1).style.bold);
        assert!(buf.get(3, 2).style.bold);
        assert!(buf.get(4, 2).style.bold);
        // Cells outside region unaffected
        assert!(!buf.get(2, 1).style.bold);
    }

    #[test]
    fn test_clear_region_entirely_out_of_bounds() {
        let mut buf = Buffer::new(5, 3);
        buf.clear_region(10, 10, 5, 5, Style::new());
        assert_eq!(buf.dirty_cells().count(), 0);
    }

    #[test]
    fn test_clear_region_zero_size() {
        let mut buf = Buffer::new(10, 5);
        buf.clear_region(0, 0, 0, 0, Style::new());
        assert_eq!(buf.dirty_cells().count(), 0);
    }

    #[test]
    fn test_clear_region_full_buffer() {
        let mut buf = Buffer::new(5, 3);
        let style = Style::new().fg(Color::Red);
        buf.write_str(0, 0, "hello", style);
        buf.clear_dirty();

        buf.clear_region(0, 0, 5, 3, Style::new());
        // All cells that had content should now be dirty
        for row in 0..3 {
            for col in 0..5 {
                assert_eq!(buf.get(col, row).ch, ' ');
                assert_eq!(buf.get(col, row).style, Style::new());
            }
        }
    }

    // --- Buffer::resize ---

    #[test]
    fn test_resize_changes_dimensions() {
        let mut buf = Buffer::new(10, 5);
        buf.resize(20, 10);
        assert_eq!(buf.width(), 20);
        assert_eq!(buf.height(), 10);
    }

    #[test]
    fn test_resize_resets_cells() {
        let mut buf = Buffer::new(10, 5);
        buf.set(0, 0, 'X', Style::new().bold(true));
        buf.resize(10, 5);

        // After resize, all cells should be default
        assert_eq!(buf.get(0, 0).ch, ' ');
        assert_eq!(buf.get(0, 0).style, Style::new());
    }

    #[test]
    fn test_resize_marks_all_dirty() {
        let mut buf = Buffer::new(10, 5);
        buf.resize(3, 2);
        assert_eq!(buf.dirty_cells().count(), 6); // 3 * 2
    }

    #[test]
    fn test_resize_to_zero() {
        let mut buf = Buffer::new(10, 5);
        buf.resize(0, 0);
        assert_eq!(buf.width(), 0);
        assert_eq!(buf.height(), 0);
        assert_eq!(buf.dirty_cells().count(), 0);
    }

    #[test]
    fn test_resize_grow() {
        let mut buf = Buffer::new(5, 3);
        buf.resize(10, 8);
        assert_eq!(buf.width(), 10);
        assert_eq!(buf.height(), 8);
        assert_eq!(buf.dirty_cells().count(), 80);
    }

    #[test]
    fn test_resize_shrink() {
        let mut buf = Buffer::new(10, 8);
        buf.resize(5, 3);
        assert_eq!(buf.width(), 5);
        assert_eq!(buf.height(), 3);
        assert_eq!(buf.dirty_cells().count(), 15);
    }

    // --- dirty_cells ---

    #[test]
    fn test_dirty_cells_returns_correct_positions() {
        let mut buf = Buffer::new(10, 5);
        buf.set(3, 2, 'A', Style::new());
        buf.set(7, 4, 'B', Style::new());

        let dirty: Vec<_> = buf.dirty_cells().collect();
        assert_eq!(dirty.len(), 2);

        // Check that positions are correct
        assert!(dirty.iter().any(|&(c, r, cell)| c == 3 && r == 2 && cell.ch == 'A'));
        assert!(dirty.iter().any(|&(c, r, cell)| c == 7 && r == 4 && cell.ch == 'B'));
    }

    #[test]
    fn test_dirty_cells_empty_after_clear() {
        let mut buf = Buffer::new(10, 5);
        buf.set(0, 0, 'X', Style::new());
        buf.clear_dirty();
        assert_eq!(buf.dirty_cells().count(), 0);
    }

    // --- mark_all_dirty ---

    #[test]
    fn test_mark_all_dirty() {
        let mut buf = Buffer::new(5, 3);
        buf.mark_all_dirty();
        assert_eq!(buf.dirty_cells().count(), 15); // 5 * 3
    }

    #[test]
    fn test_mark_all_dirty_after_clear() {
        let mut buf = Buffer::new(5, 3);
        buf.set(0, 0, 'X', Style::new());
        buf.clear_dirty();
        buf.mark_all_dirty();
        assert_eq!(buf.dirty_cells().count(), 15);
    }

    // --- clear_dirty ---

    #[test]
    fn test_clear_dirty() {
        let mut buf = Buffer::new(10, 5);
        buf.set(0, 0, 'A', Style::new());
        buf.set(5, 3, 'B', Style::new());
        assert_eq!(buf.dirty_cells().count(), 2);

        buf.clear_dirty();
        assert_eq!(buf.dirty_cells().count(), 0);
    }

    #[test]
    fn test_clear_dirty_then_modify() {
        let mut buf = Buffer::new(10, 5);
        buf.set(0, 0, 'A', Style::new());
        buf.clear_dirty();
        buf.set(1, 1, 'B', Style::new());

        let dirty: Vec<_> = buf.dirty_cells().collect();
        assert_eq!(dirty.len(), 1);
        assert_eq!(dirty[0].0, 1);
        assert_eq!(dirty[0].1, 1);
    }

    // --- Overwrite tracking ---

    #[test]
    fn test_overwrite_same_content_not_dirty() {
        let mut buf = Buffer::new(10, 5);
        let style = Style::new().fg(Color::Red);
        buf.set(0, 0, 'A', style);
        buf.clear_dirty();

        // Set same content again
        buf.set(0, 0, 'A', style);
        assert_eq!(buf.dirty_cells().count(), 0);
    }

    #[test]
    fn test_overwrite_different_char_dirty() {
        let mut buf = Buffer::new(10, 5);
        let style = Style::new().fg(Color::Red);
        buf.set(0, 0, 'A', style);
        buf.clear_dirty();

        buf.set(0, 0, 'B', style);
        assert_eq!(buf.dirty_cells().count(), 1);
    }

    #[test]
    fn test_overwrite_different_style_dirty() {
        let mut buf = Buffer::new(10, 5);
        buf.set(0, 0, 'A', Style::new().fg(Color::Red));
        buf.clear_dirty();

        buf.set(0, 0, 'A', Style::new().fg(Color::Blue));
        assert_eq!(buf.dirty_cells().count(), 1);
    }

    // --- Edge cases ---

    #[test]
    fn test_write_str_unicode_emoji() {
        let mut buf = Buffer::new(10, 1);
        // Each emoji is a single char for this test
        buf.write_str(0, 0, "a☺b", Style::new());
        assert_eq!(buf.get(0, 0).ch, 'a');
        assert_eq!(buf.get(1, 0).ch, '☺');
        assert_eq!(buf.get(2, 0).ch, 'b');
    }

    #[test]
    fn test_large_buffer() {
        let buf = Buffer::new(200, 100);
        assert_eq!(buf.width(), 200);
        assert_eq!(buf.height(), 100);
        assert_eq!(buf.dirty_cells().count(), 0);
    }

    #[test]
    fn test_single_cell_buffer() {
        let mut buf = Buffer::new(1, 1);
        buf.set(0, 0, 'X', Style::new().bold(true));

        let cell = buf.get(0, 0);
        assert_eq!(cell.ch, 'X');
        assert!(cell.style.bold);

        let dirty: Vec<_> = buf.dirty_cells().collect();
        assert_eq!(dirty.len(), 1);
        assert_eq!(dirty[0].0, 0);
        assert_eq!(dirty[0].1, 0);
    }

    #[test]
    fn test_write_str_at_last_column() {
        let mut buf = Buffer::new(5, 1);
        buf.write_str(4, 0, "ab", Style::new());
        assert_eq!(buf.get(4, 0).ch, 'a');
        // 'b' should be dropped (beyond width)
        assert_eq!(buf.dirty_cells().count(), 1);
    }

    #[test]
    fn test_clear_region_does_not_dirty_unchanged_cells() {
        let mut buf = Buffer::new(5, 1);
        // Buffer already contains default cells (space, default style)
        buf.clear_region(0, 0, 5, 1, Style::new());
        // Clearing with default style on default cells => no change
        assert_eq!(buf.dirty_cells().count(), 0);
    }

    #[test]
    fn test_dirty_cells_after_write_str() {
        let mut buf = Buffer::new(10, 1);
        buf.write_str(2, 0, "abc", Style::new().fg(Color::Red));

        let dirty: Vec<_> = buf.dirty_cells().collect();
        assert_eq!(dirty.len(), 3);

        // Verify all dirty cells are in the correct column range
        let cols: Vec<u16> = dirty.iter().map(|&(c, _, _)| c).collect();
        assert!(cols.contains(&2));
        assert!(cols.contains(&3));
        assert!(cols.contains(&4));
    }

    #[test]
    fn test_multiple_operations_accumulate_dirty() {
        let mut buf = Buffer::new(10, 5);
        buf.set(0, 0, 'A', Style::new());
        buf.set(5, 3, 'B', Style::new());
        buf.write_str(0, 4, "hi", Style::new().bold(true));

        // 1 + 1 + 2 = 4 dirty cells
        assert_eq!(buf.dirty_cells().count(), 4);
    }
}
