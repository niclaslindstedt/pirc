use crate::encryption::EncryptionStatus;
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
    pub encryption_status: EncryptionStatus,
}

impl TabInfo {
    /// Compute the display text for this tab.
    fn display_text(&self) -> String {
        let mut parts = String::new();
        match self.encryption_status {
            EncryptionStatus::Active => parts.push_str("[E2E]"),
            EncryptionStatus::Establishing => parts.push_str("[...]"),
            EncryptionStatus::None => {}
        }
        parts.push_str(&self.label);
        if self.unread_count > 0 {
            parts.push_str(&format!(" [{}]", self.unread_count));
        }
        parts
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
        let seps = if e > s + 1 {
            (e - s - 1) * TAB_SEPARATOR.len()
        } else {
            0
        };
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
    buf.clear_region(
        region.x,
        region.y,
        region.width,
        region.height,
        STYLE_TAB_NORMAL,
    );

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
        write_bounded(
            buf,
            &mut col,
            max_col,
            row,
            OVERFLOW_LEFT,
            STYLE_TAB_SEPARATOR,
        );
    }

    // Draw visible tabs
    for (i, tab) in tabs[start..end].iter().enumerate() {
        if i > 0 {
            write_bounded(
                buf,
                &mut col,
                max_col,
                row,
                TAB_SEPARATOR,
                STYLE_TAB_SEPARATOR,
            );
        }
        let text = tab.display_text();
        let style = tab.style();
        write_bounded(buf, &mut col, max_col, row, &text, style);
    }

    // Draw right overflow indicator
    if show_right {
        write_bounded(
            buf,
            &mut col,
            max_col,
            row,
            OVERFLOW_RIGHT,
            STYLE_TAB_SEPARATOR,
        );
    }
}

/// Write text into the buffer at the current column, advancing `col`.
/// Characters beyond `max_col` are silently dropped.
fn write_bounded(
    buf: &mut Buffer,
    col: &mut usize,
    max_col: usize,
    row: u16,
    text: &str,
    style: Style,
) {
    for ch in text.chars() {
        if *col >= max_col {
            break;
        }
        buf.set(*col as u16, row, ch, style);
        *col += 1;
    }
}

#[cfg(test)]
mod tests;
