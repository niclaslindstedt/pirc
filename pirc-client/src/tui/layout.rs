/// Minimum terminal width for a usable layout.
pub const MIN_WIDTH: u16 = 40;
/// Minimum terminal height for a usable layout.
pub const MIN_HEIGHT: u16 = 10;
/// Default width of the user list panel in columns.
pub const DEFAULT_USER_LIST_WIDTH: u16 = 20;

/// A rectangle representing a screen region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

impl Rect {
    /// Create a new rectangle.
    pub const fn new(x: u16, y: u16, width: u16, height: u16) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Returns the column one past the right edge (exclusive).
    pub const fn right(&self) -> u16 {
        self.x + self.width
    }

    /// Returns the row one past the bottom edge (exclusive).
    pub const fn bottom(&self) -> u16 {
        self.y + self.height
    }

    /// Returns true if the given (col, row) is inside this rectangle.
    pub const fn contains(&self, col: u16, row: u16) -> bool {
        col >= self.x && col < self.x + self.width && row >= self.y && row < self.y + self.height
    }
}

/// The computed layout of all IRC UI regions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    /// Top row — channel/tab bar.
    pub channel_tabs: Rect,
    /// Main chat message area.
    pub chat_area: Rect,
    /// Right-side user list (zero-width when hidden).
    pub user_list: Rect,
    /// Second-to-last row — status information.
    pub status_bar: Rect,
    /// Bottom row — text input.
    pub input_line: Rect,
    /// Column position of the vertical separator between chat and user list.
    /// Equal to `chat_area.right()`. When user list is hidden this equals the
    /// terminal width (i.e. no separator is drawn).
    pub separator: u16,
}

impl Layout {
    /// Compute the layout for the given terminal dimensions.
    ///
    /// `user_list_width` controls the width of the user list panel. Pass `0`
    /// to hide the user list entirely (the chat area expands to fill the space).
    ///
    /// Returns `None` if the terminal is too small for a usable layout
    /// (below `MIN_WIDTH` x `MIN_HEIGHT`).
    pub fn compute(width: u16, height: u16, user_list_width: u16) -> Option<Self> {
        if width < MIN_WIDTH || height < MIN_HEIGHT {
            return None;
        }

        // Fixed rows: channel_tabs (1) + status_bar (1) + input_line (1) = 3
        let middle_height = height - 3;

        // Clamp user_list_width so the chat area always gets at least 1 column
        // (plus 1 for the separator when the user list is visible).
        let effective_ul_width = if user_list_width == 0 {
            0
        } else {
            // separator takes 1 col, user list takes user_list_width cols
            let max_ul = width.saturating_sub(2); // at least 1 col for chat + 1 for sep
            user_list_width.min(max_ul)
        };

        let show_user_list = effective_ul_width > 0;

        // Separator column and widths
        let (chat_width, separator_col) = if show_user_list {
            let sep = width - effective_ul_width - 1;
            (sep, sep)
        } else {
            (width, width)
        };

        let channel_tabs = Rect::new(0, 0, width, 1);
        let input_line = Rect::new(0, height - 1, width, 1);
        let status_bar = Rect::new(0, height - 2, width, 1);
        let chat_area = Rect::new(0, 1, chat_width, middle_height);
        let user_list = if show_user_list {
            Rect::new(separator_col + 1, 1, effective_ul_width, middle_height)
        } else {
            Rect::new(width, 1, 0, middle_height)
        };

        Some(Layout {
            channel_tabs,
            chat_area,
            user_list,
            status_bar,
            input_line,
            separator: separator_col,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Rect basics ---

    #[test]
    fn test_rect_new() {
        let r = Rect::new(5, 10, 20, 8);
        assert_eq!(r.x, 5);
        assert_eq!(r.y, 10);
        assert_eq!(r.width, 20);
        assert_eq!(r.height, 8);
    }

    #[test]
    fn test_rect_right() {
        let r = Rect::new(5, 0, 20, 1);
        assert_eq!(r.right(), 25);
    }

    #[test]
    fn test_rect_bottom() {
        let r = Rect::new(0, 3, 10, 7);
        assert_eq!(r.bottom(), 10);
    }

    #[test]
    fn test_rect_contains_inside() {
        let r = Rect::new(5, 5, 10, 10);
        assert!(r.contains(5, 5)); // top-left corner
        assert!(r.contains(14, 14)); // bottom-right corner (inclusive)
        assert!(r.contains(10, 10)); // middle
    }

    #[test]
    fn test_rect_contains_outside() {
        let r = Rect::new(5, 5, 10, 10);
        assert!(!r.contains(4, 5)); // left of
        assert!(!r.contains(5, 4)); // above
        assert!(!r.contains(15, 5)); // right edge (exclusive)
        assert!(!r.contains(5, 15)); // bottom edge (exclusive)
    }

    #[test]
    fn test_rect_contains_zero_size() {
        let r = Rect::new(5, 5, 0, 0);
        assert!(!r.contains(5, 5));
    }

    #[test]
    fn test_rect_clone_copy() {
        let a = Rect::new(1, 2, 3, 4);
        let b = a;
        assert_eq!(a, b);
    }

    #[test]
    fn test_rect_debug() {
        let r = Rect::new(1, 2, 3, 4);
        let debug = format!("{r:?}");
        assert!(debug.contains("Rect"));
    }

    // --- Layout::compute too small ---

    #[test]
    fn test_compute_too_narrow() {
        assert!(Layout::compute(39, 24, DEFAULT_USER_LIST_WIDTH).is_none());
    }

    #[test]
    fn test_compute_too_short() {
        assert!(Layout::compute(80, 9, DEFAULT_USER_LIST_WIDTH).is_none());
    }

    #[test]
    fn test_compute_both_too_small() {
        assert!(Layout::compute(10, 5, DEFAULT_USER_LIST_WIDTH).is_none());
    }

    #[test]
    fn test_compute_at_minimum() {
        assert!(Layout::compute(MIN_WIDTH, MIN_HEIGHT, DEFAULT_USER_LIST_WIDTH).is_some());
    }

    // --- Standard 80x24 layout with user list ---

    #[test]
    fn test_standard_layout_channel_tabs() {
        let layout = Layout::compute(80, 24, 20).unwrap();
        assert_eq!(layout.channel_tabs, Rect::new(0, 0, 80, 1));
    }

    #[test]
    fn test_standard_layout_input_line() {
        let layout = Layout::compute(80, 24, 20).unwrap();
        assert_eq!(layout.input_line, Rect::new(0, 23, 80, 1));
    }

    #[test]
    fn test_standard_layout_status_bar() {
        let layout = Layout::compute(80, 24, 20).unwrap();
        assert_eq!(layout.status_bar, Rect::new(0, 22, 80, 1));
    }

    #[test]
    fn test_standard_layout_chat_area() {
        let layout = Layout::compute(80, 24, 20).unwrap();
        // chat: cols 0..59, rows 1..22 (height=21)
        assert_eq!(layout.chat_area, Rect::new(0, 1, 59, 21));
    }

    #[test]
    fn test_standard_layout_user_list() {
        let layout = Layout::compute(80, 24, 20).unwrap();
        // user_list: cols 60..80, rows 1..22 (height=21)
        assert_eq!(layout.user_list, Rect::new(60, 1, 20, 21));
    }

    #[test]
    fn test_standard_layout_separator() {
        let layout = Layout::compute(80, 24, 20).unwrap();
        assert_eq!(layout.separator, 59);
    }

    // --- Layout without user list ---

    #[test]
    fn test_layout_no_user_list() {
        let layout = Layout::compute(80, 24, 0).unwrap();
        assert_eq!(layout.chat_area, Rect::new(0, 1, 80, 21));
        assert_eq!(layout.user_list.width, 0);
        assert_eq!(layout.separator, 80);
    }

    #[test]
    fn test_layout_no_user_list_chat_full_width() {
        let layout = Layout::compute(120, 40, 0).unwrap();
        assert_eq!(layout.chat_area.width, 120);
    }

    // --- Regions don't overlap and tile the terminal ---

    fn regions_cover_and_dont_overlap(width: u16, height: u16, ul_width: u16) {
        let layout = match Layout::compute(width, height, ul_width) {
            Some(l) => l,
            None => return, // too small, nothing to check
        };

        // Build a grid and mark each cell as belonging to exactly one region
        let mut grid = vec![0u8; (width as usize) * (height as usize)];
        let regions = [
            layout.channel_tabs,
            layout.chat_area,
            layout.user_list,
            layout.status_bar,
            layout.input_line,
        ];

        for region in &regions {
            for row in region.y..region.bottom() {
                for col in region.x..region.right() {
                    let idx = (row as usize) * (width as usize) + (col as usize);
                    assert!(
                        idx < grid.len(),
                        "region {:?} out of terminal bounds ({width}x{height})",
                        region
                    );
                    grid[idx] += 1;
                }
            }
        }

        // Account for the separator column (if user list is visible)
        let show_ul = layout.user_list.width > 0;
        if show_ul {
            let sep_col = layout.separator;
            for row in 1..(height - 2) {
                let idx = (row as usize) * (width as usize) + (sep_col as usize);
                grid[idx] += 1; // separator column
            }
        }

        // Every cell should be covered exactly once
        for row in 0..height {
            for col in 0..width {
                let idx = (row as usize) * (width as usize) + (col as usize);
                assert_eq!(
                    grid[idx], 1,
                    "cell ({col}, {row}) covered {} times (expected 1) in {width}x{height} layout (ul_width={ul_width})",
                    grid[idx]
                );
            }
        }
    }

    #[test]
    fn test_no_overlap_80x24_with_user_list() {
        regions_cover_and_dont_overlap(80, 24, 20);
    }

    #[test]
    fn test_no_overlap_80x24_without_user_list() {
        regions_cover_and_dont_overlap(80, 24, 0);
    }

    #[test]
    fn test_no_overlap_120x40_with_user_list() {
        regions_cover_and_dont_overlap(120, 40, 25);
    }

    #[test]
    fn test_no_overlap_minimum_size() {
        regions_cover_and_dont_overlap(MIN_WIDTH, MIN_HEIGHT, DEFAULT_USER_LIST_WIDTH);
    }

    #[test]
    fn test_no_overlap_minimum_without_user_list() {
        regions_cover_and_dont_overlap(MIN_WIDTH, MIN_HEIGHT, 0);
    }

    // --- Resize adaptation ---

    #[test]
    fn test_layout_adapts_to_larger_terminal() {
        let small = Layout::compute(80, 24, 20).unwrap();
        let large = Layout::compute(160, 48, 20).unwrap();

        assert!(large.chat_area.width > small.chat_area.width);
        assert!(large.chat_area.height > small.chat_area.height);
    }

    #[test]
    fn test_layout_adapts_to_smaller_terminal() {
        let large = Layout::compute(80, 24, 20).unwrap();
        let small = Layout::compute(50, 12, 20).unwrap();

        assert!(small.chat_area.width < large.chat_area.width);
        assert!(small.chat_area.height < large.chat_area.height);
    }

    // --- User list width clamping ---

    #[test]
    fn test_user_list_width_clamped() {
        // Request a user list wider than the terminal can support
        let layout = Layout::compute(50, 24, 60).unwrap();
        // Chat area must have at least 1 column
        assert!(layout.chat_area.width >= 1);
        assert!(layout.user_list.width > 0);
        assert!(layout.user_list.width <= 48); // 50 - 2
    }

    #[test]
    fn test_user_list_exact_max() {
        // user_list_width = width - 2 (max allowed)
        let layout = Layout::compute(42, 12, 40).unwrap();
        assert_eq!(layout.user_list.width, 40);
        assert_eq!(layout.chat_area.width, 1); // 42 - 40 - 1 separator
    }

    // --- Edge case: minimum size with user list ---

    #[test]
    fn test_minimum_with_default_user_list() {
        let layout = Layout::compute(40, 10, 20).unwrap();
        // middle_height = 10 - 3 = 7
        assert_eq!(layout.chat_area.height, 7);
        assert_eq!(layout.user_list.height, 7);
        // chat_width = 40 - 20 - 1 = 19
        assert_eq!(layout.chat_area.width, 19);
        assert_eq!(layout.user_list.width, 20);
    }

    // --- Layout fields consistency ---

    #[test]
    fn test_separator_equals_chat_right() {
        let layout = Layout::compute(80, 24, 20).unwrap();
        assert_eq!(layout.separator, layout.chat_area.right());
    }

    #[test]
    fn test_user_list_starts_after_separator() {
        let layout = Layout::compute(80, 24, 20).unwrap();
        assert_eq!(layout.user_list.x, layout.separator + 1);
    }

    #[test]
    fn test_status_bar_below_chat() {
        let layout = Layout::compute(80, 24, 20).unwrap();
        assert_eq!(layout.status_bar.y, layout.chat_area.bottom());
    }

    #[test]
    fn test_input_below_status() {
        let layout = Layout::compute(80, 24, 20).unwrap();
        assert_eq!(layout.input_line.y, layout.status_bar.bottom());
    }

    #[test]
    fn test_chat_below_tabs() {
        let layout = Layout::compute(80, 24, 20).unwrap();
        assert_eq!(layout.chat_area.y, layout.channel_tabs.bottom());
    }

    // --- Various sizes tiling ---

    #[test]
    fn test_no_overlap_various_sizes() {
        let sizes = [
            (40, 10),
            (60, 15),
            (80, 24),
            (100, 30),
            (120, 40),
            (200, 60),
        ];
        for (w, h) in sizes {
            regions_cover_and_dont_overlap(w, h, DEFAULT_USER_LIST_WIDTH);
            regions_cover_and_dont_overlap(w, h, 0);
            regions_cover_and_dont_overlap(w, h, 10);
        }
    }
}
