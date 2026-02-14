use crate::tui::buffer::Buffer;
use crate::tui::layout::Rect;
use crate::tui::style::{Color, Style};

/// Style for the currently active tab (reverse video + bold).
pub const STYLE_TAB_ACTIVE: Style = Style::new().bold(true).reverse(true);

/// Style for tabs with unread messages (bold + yellow foreground).
pub const STYLE_TAB_UNREAD: Style = Style::new().bold(true).fg(Color::Yellow);

/// Style for tabs with activity but no unread messages (bold + cyan).
pub const STYLE_TAB_ACTIVITY: Style = Style::new().bold(true).fg(Color::Cyan);

/// Style for normal/inactive tabs (default).
pub const STYLE_TAB_NORMAL: Style = Style::new();

/// Style for the separator between tabs (dim / bright-black).
pub const STYLE_TAB_SEPARATOR: Style = Style::new().fg(Color::BrightBlack);

/// Separator string drawn between adjacent tabs.
const TAB_SEPARATOR: &str = " | ";

/// Left overflow indicator.
const OVERFLOW_LEFT: &str = "< ";

/// Right overflow indicator.
const OVERFLOW_RIGHT: &str = " >";

/// Input data for a single tab in the tab bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabInfo {
    pub label: String,
    pub is_active: bool,
    pub unread_count: usize,
    pub has_activity: bool,
}

impl TabInfo {
    /// Compute the display text for this tab.
    fn display_text(&self) -> String {
        if self.unread_count > 0 {
            format!("{} [{}]", self.label, self.unread_count)
        } else {
            self.label.clone()
        }
    }

    /// Choose the rendering style for this tab.
    fn style(&self) -> Style {
        if self.is_active {
            STYLE_TAB_ACTIVE
        } else if self.unread_count > 0 {
            STYLE_TAB_UNREAD
        } else if self.has_activity {
            STYLE_TAB_ACTIVITY
        } else {
            STYLE_TAB_NORMAL
        }
    }
}

/// Compute the display width of all tabs laid out sequentially.
/// Returns the total width including separators.
fn total_tabs_width(tabs: &[TabInfo]) -> usize {
    if tabs.is_empty() {
        return 0;
    }
    let content: usize = tabs.iter().map(|t| t.display_text().len()).sum();
    let separators = (tabs.len() - 1) * TAB_SEPARATOR.len();
    content + separators
}

/// Determine the visible range of tab indices when overflow occurs.
///
/// Returns `(start, end)` where `start..end` are the visible tab indices.
/// The active tab at `active_idx` is always included.
fn compute_visible_range(tabs: &[TabInfo], available: usize, active_idx: usize) -> (usize, usize) {
    let n = tabs.len();
    if n == 0 {
        return (0, 0);
    }

    // If everything fits, show all
    if total_tabs_width(tabs) <= available {
        return (0, n);
    }

    // Start by showing just the active tab, then expand outward
    let mut start = active_idx;
    let mut end = active_idx + 1;

    // Width of current visible range
    let width_of_range = |s: usize, e: usize| -> usize {
        let content: usize = tabs[s..e].iter().map(|t| t.display_text().len()).sum();
        let seps = if e > s + 1 { (e - s - 1) * TAB_SEPARATOR.len() } else { 0 };
        let left_indicator = if s > 0 { OVERFLOW_LEFT.len() } else { 0 };
        let right_indicator = if e < n { OVERFLOW_RIGHT.len() } else { 0 };
        content + seps + left_indicator + right_indicator
    };

    // Alternately try expanding left, then right
    loop {
        let mut expanded = false;

        // Try expanding left
        if start > 0 {
            let w = width_of_range(start - 1, end);
            if w <= available {
                start -= 1;
                expanded = true;
            }
        }

        // Try expanding right
        if end < n {
            let w = width_of_range(start, end + 1);
            if w <= available {
                end += 1;
                expanded = true;
            }
        }

        if !expanded {
            break;
        }
    }

    (start, end)
}

/// Render a tab bar into the given buffer region.
///
/// `tabs` should be built from `BufferManager::buffer_list()` output.
/// The active tab is always kept visible, with `<` and `>` overflow
/// indicators when tabs are hidden.
pub fn render_tab_bar(buf: &mut Buffer, region: &Rect, tabs: &[TabInfo]) {
    // Clear the entire region first
    buf.clear_region(region.x, region.y, region.width, region.height, STYLE_TAB_NORMAL);

    if tabs.is_empty() || region.width == 0 || region.height == 0 {
        return;
    }

    let available = region.width as usize;

    // Find active tab index (default to 0 if none marked active)
    let active_idx = tabs.iter().position(|t| t.is_active).unwrap_or(0);

    let (start, end) = compute_visible_range(tabs, available, active_idx);
    let show_left = start > 0;
    let show_right = end < tabs.len();

    let mut col = region.x as usize;
    let max_col = (region.x + region.width) as usize;
    let row = region.y;

    // Draw left overflow indicator
    if show_left {
        write_bounded(buf, &mut col, max_col, row, OVERFLOW_LEFT, STYLE_TAB_SEPARATOR);
    }

    // Draw visible tabs
    for (i, tab) in tabs[start..end].iter().enumerate() {
        if i > 0 {
            write_bounded(buf, &mut col, max_col, row, TAB_SEPARATOR, STYLE_TAB_SEPARATOR);
        }
        let text = tab.display_text();
        let style = tab.style();
        write_bounded(buf, &mut col, max_col, row, &text, style);
    }

    // Draw right overflow indicator
    if show_right {
        write_bounded(buf, &mut col, max_col, row, OVERFLOW_RIGHT, STYLE_TAB_SEPARATOR);
    }
}

/// Write text into the buffer at the current column, advancing `col`.
/// Characters beyond `max_col` are silently dropped.
fn write_bounded(buf: &mut Buffer, col: &mut usize, max_col: usize, row: u16, text: &str, style: Style) {
    for ch in text.chars() {
        if *col >= max_col {
            break;
        }
        buf.set(*col as u16, row, ch, style);
        *col += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: extract a row of characters from the buffer as a String.
    fn row_text(buf: &Buffer, row: u16, start: u16, width: u16) -> String {
        (start..start + width)
            .map(|col| buf.get(col, row).ch)
            .collect()
    }

    /// Helper: extract the style at a specific column.
    fn cell_style(buf: &Buffer, col: u16, row: u16) -> Style {
        buf.get(col, row).style
    }

    // --- TabInfo ---

    #[test]
    fn tab_info_display_text_no_unread() {
        let tab = TabInfo {
            label: "#general".into(),
            is_active: false,
            unread_count: 0,
            has_activity: false,
        };
        assert_eq!(tab.display_text(), "#general");
    }

    #[test]
    fn tab_info_display_text_with_unread() {
        let tab = TabInfo {
            label: "#general".into(),
            is_active: false,
            unread_count: 5,
            has_activity: false,
        };
        assert_eq!(tab.display_text(), "#general [5]");
    }

    #[test]
    fn tab_info_style_active() {
        let tab = TabInfo {
            label: "test".into(),
            is_active: true,
            unread_count: 0,
            has_activity: false,
        };
        assert_eq!(tab.style(), STYLE_TAB_ACTIVE);
    }

    #[test]
    fn tab_info_style_active_overrides_unread() {
        let tab = TabInfo {
            label: "test".into(),
            is_active: true,
            unread_count: 3,
            has_activity: true,
        };
        assert_eq!(tab.style(), STYLE_TAB_ACTIVE);
    }

    #[test]
    fn tab_info_style_unread() {
        let tab = TabInfo {
            label: "test".into(),
            is_active: false,
            unread_count: 1,
            has_activity: false,
        };
        assert_eq!(tab.style(), STYLE_TAB_UNREAD);
    }

    #[test]
    fn tab_info_style_activity() {
        let tab = TabInfo {
            label: "test".into(),
            is_active: false,
            unread_count: 0,
            has_activity: true,
        };
        assert_eq!(tab.style(), STYLE_TAB_ACTIVITY);
    }

    #[test]
    fn tab_info_style_normal() {
        let tab = TabInfo {
            label: "test".into(),
            is_active: false,
            unread_count: 0,
            has_activity: false,
        };
        assert_eq!(tab.style(), STYLE_TAB_NORMAL);
    }

    // --- total_tabs_width ---

    #[test]
    fn total_width_empty() {
        assert_eq!(total_tabs_width(&[]), 0);
    }

    #[test]
    fn total_width_single_tab() {
        let tabs = [TabInfo {
            label: "Status".into(),
            is_active: true,
            unread_count: 0,
            has_activity: false,
        }];
        // "Status" = 6 chars, no separators
        assert_eq!(total_tabs_width(&tabs), 6);
    }

    #[test]
    fn total_width_two_tabs() {
        let tabs = [
            TabInfo { label: "Status".into(), is_active: true, unread_count: 0, has_activity: false },
            TabInfo { label: "#a".into(), is_active: false, unread_count: 0, has_activity: false },
        ];
        // "Status" + " | " + "#a" = 6 + 3 + 2 = 11
        assert_eq!(total_tabs_width(&tabs), 11);
    }

    #[test]
    fn total_width_with_unread() {
        let tabs = [
            TabInfo { label: "#a".into(), is_active: false, unread_count: 3, has_activity: false },
        ];
        // "#a [3]" = 6
        assert_eq!(total_tabs_width(&tabs), 6);
    }

    // --- compute_visible_range ---

    #[test]
    fn visible_range_all_fit() {
        let tabs = vec![
            TabInfo { label: "Status".into(), is_active: true, unread_count: 0, has_activity: false },
            TabInfo { label: "#a".into(), is_active: false, unread_count: 0, has_activity: false },
        ];
        let (start, end) = compute_visible_range(&tabs, 80, 0);
        assert_eq!((start, end), (0, 2));
    }

    #[test]
    fn visible_range_empty() {
        let (start, end) = compute_visible_range(&[], 80, 0);
        assert_eq!((start, end), (0, 0));
    }

    #[test]
    fn visible_range_overflow_keeps_active() {
        // Many tabs that won't fit in 20 columns
        let tabs: Vec<TabInfo> = (0..10)
            .map(|i| TabInfo {
                label: format!("#{}", i),
                is_active: i == 5,
                unread_count: 0,
                has_activity: false,
            })
            .collect();
        let (start, end) = compute_visible_range(&tabs, 20, 5);
        assert!(start <= 5);
        assert!(end > 5);
    }

    #[test]
    fn visible_range_active_at_start() {
        let tabs: Vec<TabInfo> = (0..10)
            .map(|i| TabInfo {
                label: format!("chan{}", i),
                is_active: i == 0,
                unread_count: 0,
                has_activity: false,
            })
            .collect();
        let (start, _end) = compute_visible_range(&tabs, 25, 0);
        assert_eq!(start, 0);
    }

    #[test]
    fn visible_range_active_at_end() {
        let tabs: Vec<TabInfo> = (0..10)
            .map(|i| TabInfo {
                label: format!("chan{}", i),
                is_active: i == 9,
                unread_count: 0,
                has_activity: false,
            })
            .collect();
        let (_start, end) = compute_visible_range(&tabs, 25, 9);
        assert_eq!(end, 10);
    }

    // --- render_tab_bar ---

    #[test]
    fn render_empty_tabs() {
        let mut buf = Buffer::new(80, 1);
        let region = Rect::new(0, 0, 80, 1);
        render_tab_bar(&mut buf, &region, &[]);
        // Entire row should be spaces
        let text = row_text(&buf, 0, 0, 80);
        assert_eq!(text.trim(), "");
    }

    #[test]
    fn render_single_active_tab() {
        let mut buf = Buffer::new(80, 1);
        let region = Rect::new(0, 0, 80, 1);
        let tabs = [TabInfo {
            label: "Status".into(),
            is_active: true,
            unread_count: 0,
            has_activity: false,
        }];
        render_tab_bar(&mut buf, &region, &tabs);

        let text = row_text(&buf, 0, 0, 6);
        assert_eq!(text, "Status");
        assert_eq!(cell_style(&buf, 0, 0), STYLE_TAB_ACTIVE);
        assert_eq!(cell_style(&buf, 5, 0), STYLE_TAB_ACTIVE);
        // Rest should be cleared to normal
        assert_eq!(cell_style(&buf, 6, 0), STYLE_TAB_NORMAL);
    }

    #[test]
    fn render_two_tabs_with_separator() {
        let mut buf = Buffer::new(80, 1);
        let region = Rect::new(0, 0, 80, 1);
        let tabs = [
            TabInfo { label: "Status".into(), is_active: true, unread_count: 0, has_activity: false },
            TabInfo { label: "#rust".into(), is_active: false, unread_count: 0, has_activity: false },
        ];
        render_tab_bar(&mut buf, &region, &tabs);

        // "Status | #rust"
        let text = row_text(&buf, 0, 0, 14);
        assert_eq!(text, "Status | #rust");

        // Separator style
        assert_eq!(cell_style(&buf, 7, 0), STYLE_TAB_SEPARATOR); // "|"

        // Second tab uses normal style
        assert_eq!(cell_style(&buf, 9, 0), STYLE_TAB_NORMAL); // "#"
    }

    #[test]
    fn render_tab_with_unread() {
        let mut buf = Buffer::new(80, 1);
        let region = Rect::new(0, 0, 80, 1);
        let tabs = [
            TabInfo { label: "Status".into(), is_active: true, unread_count: 0, has_activity: false },
            TabInfo { label: "#chat".into(), is_active: false, unread_count: 3, has_activity: false },
        ];
        render_tab_bar(&mut buf, &region, &tabs);

        // "Status | #chat [3]"
        let text = row_text(&buf, 0, 0, 18);
        assert_eq!(text, "Status | #chat [3]");

        // Unread tab should have STYLE_TAB_UNREAD
        assert_eq!(cell_style(&buf, 9, 0), STYLE_TAB_UNREAD);
    }

    #[test]
    fn render_tab_with_activity() {
        let mut buf = Buffer::new(80, 1);
        let region = Rect::new(0, 0, 80, 1);
        let tabs = [
            TabInfo { label: "Status".into(), is_active: true, unread_count: 0, has_activity: false },
            TabInfo { label: "#chat".into(), is_active: false, unread_count: 0, has_activity: true },
        ];
        render_tab_bar(&mut buf, &region, &tabs);

        assert_eq!(cell_style(&buf, 9, 0), STYLE_TAB_ACTIVITY);
    }

    #[test]
    fn render_with_region_offset() {
        let mut buf = Buffer::new(80, 5);
        let region = Rect::new(5, 2, 30, 1);
        let tabs = [TabInfo {
            label: "Status".into(),
            is_active: true,
            unread_count: 0,
            has_activity: false,
        }];
        render_tab_bar(&mut buf, &region, &tabs);

        // Tab text should start at col 5, row 2
        assert_eq!(buf.get(5, 2).ch, 'S');
        assert_eq!(buf.get(10, 2).ch, 's'); // last char of "Status"
        assert_eq!(buf.get(11, 2).ch, ' '); // after "Status"
        // Nothing should be written to row 0
        assert_eq!(buf.get(5, 0).ch, ' ');
        assert_eq!(cell_style(&buf, 5, 0), Style::new());
    }

    #[test]
    fn render_overflow_shows_indicators() {
        // 20 cols, try to render many tabs
        let mut buf = Buffer::new(20, 1);
        let region = Rect::new(0, 0, 20, 1);
        let tabs: Vec<TabInfo> = (0..6)
            .map(|i| TabInfo {
                label: format!("ch{}", i),
                is_active: i == 3,
                unread_count: 0,
                has_activity: false,
            })
            .collect();
        render_tab_bar(&mut buf, &region, &tabs);

        let text: String = row_text(&buf, 0, 0, 20);
        // Active tab (ch3) should be visible
        assert!(text.contains("ch3"), "Active tab should be visible: '{}'", text);
        // Should have overflow indicator(s)
        let has_left = text.starts_with("< ");
        let has_right = text.trim_end().ends_with(">");
        assert!(has_left || has_right, "Should have overflow indicators: '{}'", text);
    }

    #[test]
    fn render_overflow_active_at_beginning() {
        let mut buf = Buffer::new(20, 1);
        let region = Rect::new(0, 0, 20, 1);
        let tabs: Vec<TabInfo> = (0..10)
            .map(|i| TabInfo {
                label: format!("chan{}", i),
                is_active: i == 0,
                unread_count: 0,
                has_activity: false,
            })
            .collect();
        render_tab_bar(&mut buf, &region, &tabs);

        let text = row_text(&buf, 0, 0, 20);
        // Active tab chan0 should be at the beginning, no left indicator
        assert!(text.starts_with("chan0"), "Should start with active tab: '{}'", text);
        assert!(text.trim_end().ends_with(">"), "Should have right overflow: '{}'", text);
    }

    #[test]
    fn render_overflow_active_at_end() {
        let mut buf = Buffer::new(20, 1);
        let region = Rect::new(0, 0, 20, 1);
        let tabs: Vec<TabInfo> = (0..10)
            .map(|i| TabInfo {
                label: format!("chan{}", i),
                is_active: i == 9,
                unread_count: 0,
                has_activity: false,
            })
            .collect();
        render_tab_bar(&mut buf, &region, &tabs);

        let text = row_text(&buf, 0, 0, 20);
        assert!(text.contains("chan9"), "Active tab should be visible: '{}'", text);
        assert!(text.starts_with("< "), "Should have left overflow: '{}'", text);
    }

    #[test]
    fn render_clears_entire_region() {
        let mut buf = Buffer::new(80, 1);
        // Pre-fill with content
        let fill_style = Style::new().fg(Color::Red);
        buf.write_str(0, 0, "XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX", fill_style);
        buf.clear_dirty();

        let region = Rect::new(0, 0, 80, 1);
        let tabs = [TabInfo {
            label: "A".into(),
            is_active: true,
            unread_count: 0,
            has_activity: false,
        }];
        render_tab_bar(&mut buf, &region, &tabs);

        // First char is 'A'
        assert_eq!(buf.get(0, 0).ch, 'A');
        // The old 'X' at col 1 should now be space
        assert_eq!(buf.get(1, 0).ch, ' ');
        assert_eq!(cell_style(&buf, 1, 0), STYLE_TAB_NORMAL);
    }

    #[test]
    fn render_zero_width_region() {
        let mut buf = Buffer::new(80, 1);
        let region = Rect::new(0, 0, 0, 1);
        // Should not panic
        render_tab_bar(&mut buf, &region, &[TabInfo {
            label: "Status".into(),
            is_active: true,
            unread_count: 0,
            has_activity: false,
        }]);
    }

    #[test]
    fn render_zero_height_region() {
        let mut buf = Buffer::new(80, 1);
        let region = Rect::new(0, 0, 80, 0);
        // Should not panic
        render_tab_bar(&mut buf, &region, &[TabInfo {
            label: "Status".into(),
            is_active: true,
            unread_count: 0,
            has_activity: false,
        }]);
    }

    #[test]
    fn render_narrow_truncates_gracefully() {
        // Region only 3 columns wide
        let mut buf = Buffer::new(3, 1);
        let region = Rect::new(0, 0, 3, 1);
        let tabs = [TabInfo {
            label: "Status".into(),
            is_active: true,
            unread_count: 0,
            has_activity: false,
        }];
        render_tab_bar(&mut buf, &region, &tabs);

        // Should show "Sta" truncated
        let text = row_text(&buf, 0, 0, 3);
        assert_eq!(text, "Sta");
    }

    #[test]
    fn render_three_tabs_all_fit() {
        let mut buf = Buffer::new(80, 1);
        let region = Rect::new(0, 0, 80, 1);
        let tabs = [
            TabInfo { label: "Status".into(), is_active: false, unread_count: 0, has_activity: false },
            TabInfo { label: "#general".into(), is_active: true, unread_count: 0, has_activity: false },
            TabInfo { label: "#rust".into(), is_active: false, unread_count: 2, has_activity: false },
        ];
        render_tab_bar(&mut buf, &region, &tabs);

        // "Status | #general | #rust [2]"
        let expected = "Status | #general | #rust [2]";
        let text = row_text(&buf, 0, 0, expected.len() as u16);
        assert_eq!(text, expected);
    }

    #[test]
    fn render_no_active_tab_defaults_to_first() {
        let mut buf = Buffer::new(80, 1);
        let region = Rect::new(0, 0, 80, 1);
        let tabs = [
            TabInfo { label: "Status".into(), is_active: false, unread_count: 0, has_activity: false },
            TabInfo { label: "#a".into(), is_active: false, unread_count: 0, has_activity: false },
        ];
        render_tab_bar(&mut buf, &region, &tabs);

        // Both tabs should be rendered (no overflow for 80 cols)
        let text = row_text(&buf, 0, 0, 11);
        assert_eq!(text, "Status | #a");
    }

    #[test]
    fn render_many_tabs_exact_fit() {
        // Calculate exact width needed for 3 tabs
        // "ab | cd | ef" = 2 + 3 + 2 + 3 + 2 = 12
        let mut buf = Buffer::new(12, 1);
        let region = Rect::new(0, 0, 12, 1);
        let tabs = [
            TabInfo { label: "ab".into(), is_active: true, unread_count: 0, has_activity: false },
            TabInfo { label: "cd".into(), is_active: false, unread_count: 0, has_activity: false },
            TabInfo { label: "ef".into(), is_active: false, unread_count: 0, has_activity: false },
        ];
        render_tab_bar(&mut buf, &region, &tabs);

        let text = row_text(&buf, 0, 0, 12);
        assert_eq!(text, "ab | cd | ef");
    }

    #[test]
    fn style_constants_are_correct() {
        assert!(STYLE_TAB_ACTIVE.bold);
        assert!(STYLE_TAB_ACTIVE.reverse);
        assert!(STYLE_TAB_UNREAD.bold);
        assert_eq!(STYLE_TAB_UNREAD.fg, Some(Color::Yellow));
        assert!(STYLE_TAB_ACTIVITY.bold);
        assert_eq!(STYLE_TAB_ACTIVITY.fg, Some(Color::Cyan));
        assert!(STYLE_TAB_NORMAL.is_empty());
        assert_eq!(STYLE_TAB_SEPARATOR.fg, Some(Color::BrightBlack));
    }
}
