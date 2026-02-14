use crate::tui::buffer::Buffer;
use crate::tui::buffer_manager::BufferId;
use crate::tui::layout::Rect;
use crate::tui::style::{Color, Style};

/// Style for the status bar background (reverse video).
pub const STYLE_STATUS_BG: Style = Style::new().reverse(true);

/// Style for the nick display (bold on reverse background).
pub const STYLE_STATUS_NICK: Style = Style::new().bold(true).reverse(true);

/// Style for the channel/buffer name (reverse background).
pub const STYLE_STATUS_CHANNEL: Style = Style::new().reverse(true);

/// Style for the away indicator (red + bold on reverse background).
pub const STYLE_STATUS_AWAY: Style = Style::new().fg(Color::Red).bold(true).reverse(true);

/// Input data for rendering the status bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusBarInfo {
    /// Current user nick.
    pub nick: String,
    /// Display label for the active buffer (e.g., "#general", "Status").
    pub buffer_label: String,
    /// Active buffer identifier for type-specific rendering.
    pub buffer_id: BufferId,
    /// Channel topic, if applicable.
    pub topic: Option<String>,
    /// Number of users in the channel, if applicable.
    pub user_count: Option<usize>,
    /// Server lag in milliseconds, if available.
    pub lag: Option<u32>,
    /// Whether the user is marked as away.
    pub away: bool,
    /// Scroll position info, e.g. "Scrolled: +42".
    pub scroll_info: Option<String>,
}

/// Render the status bar into the given buffer region.
///
/// Layout:
/// - Left-aligned:  `[nick] #channel (+topic) [3 users]`
/// - Right-aligned: `[away] [lag: 42ms] [Scrolled: +42]`
///
/// The entire row is filled with a reverse-video background.
pub fn render_status_bar(buf: &mut Buffer, region: &Rect, info: &StatusBarInfo) {
    if region.width == 0 || region.height == 0 {
        return;
    }

    let row = region.y;
    let width = region.width as usize;

    // Clear the entire row with the status bar background.
    buf.clear_region(region.x, row, region.width, 1, STYLE_STATUS_BG);

    // Build left-side segments.
    let left = build_left_text(info, width);

    // Build right-side segments.
    let right = build_right_text(info);

    // Calculate lengths (in chars).
    let left_len: usize = left.iter().map(|(t, _)| t.chars().count()).sum();
    let right_len: usize = right.iter().map(|(t, _)| t.chars().count()).sum();

    // Write left-side content.
    let mut col = region.x as usize;
    let max_col = (region.x as usize) + width;
    for (text, style) in &left {
        for ch in text.chars() {
            if col >= max_col {
                break;
            }
            buf.set(col as u16, row, ch, *style);
            col += 1;
        }
    }

    // Write right-side content if it fits without overlapping left.
    if right_len > 0 {
        let right_start = if left_len + 1 + right_len <= width {
            (region.x as usize) + width - right_len
        } else {
            // Not enough room — skip right-side content.
            return;
        };

        let mut col = right_start;
        for (text, style) in &right {
            for ch in text.chars() {
                if col >= max_col {
                    break;
                }
                buf.set(col as u16, row, ch, *style);
                col += 1;
            }
        }
    }
}

/// Build the left-aligned text segments: `[nick] #channel (+topic) [3 users]`.
fn build_left_text(info: &StatusBarInfo, max_width: usize) -> Vec<(String, Style)> {
    let mut segments: Vec<(String, Style)> = Vec::new();

    // Nick
    segments.push((format!("[{}]", info.nick), STYLE_STATUS_NICK));

    // Buffer/channel name
    segments.push((format!(" {}", info.buffer_label), STYLE_STATUS_CHANNEL));

    // Topic (only for channels with a topic)
    if let Some(ref topic) = info.topic {
        if !topic.is_empty() {
            // Calculate space used so far + " (+" prefix + ")" suffix.
            let used: usize = segments.iter().map(|(t, _)| t.chars().count()).sum();
            let prefix = " (+";
            let suffix = ")";
            let overhead = prefix.len() + suffix.len();
            let available = max_width.saturating_sub(used + overhead);

            if available > 0 {
                let topic_chars: Vec<char> = topic.chars().collect();
                let displayed = if topic_chars.len() > available {
                    // Truncate with ellipsis.
                    let trunc_len = available.saturating_sub(3);
                    let truncated: String = topic_chars[..trunc_len].iter().collect();
                    format!("{prefix}{truncated}...{suffix}")
                } else {
                    format!("{prefix}{}{suffix}", topic)
                };
                segments.push((displayed, STYLE_STATUS_BG));
            }
        }
    }

    // User count (only for channel buffers)
    if let Some(count) = info.user_count {
        if matches!(info.buffer_id, BufferId::Channel(_)) {
            segments.push((format!(" [{} users]", count), STYLE_STATUS_BG));
        }
    }

    segments
}

/// Build the right-aligned text segments: `[away] [lag: 42ms] [Scrolled: +42]`.
fn build_right_text(info: &StatusBarInfo) -> Vec<(String, Style)> {
    let mut segments: Vec<(String, Style)> = Vec::new();

    if info.away {
        segments.push(("[away] ".to_string(), STYLE_STATUS_AWAY));
    }

    if let Some(lag) = info.lag {
        segments.push((format!("[lag: {}ms] ", lag), STYLE_STATUS_BG));
    }

    if let Some(ref scroll) = info.scroll_info {
        segments.push((format!("[{}]", scroll), STYLE_STATUS_BG));
    }

    segments
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::buffer::Buffer;
    use crate::tui::layout::Rect;

    /// Helper: render status bar and extract the text from the given row.
    fn render_to_string(width: u16, info: &StatusBarInfo) -> String {
        let mut buf = Buffer::new(width, 1);
        let region = Rect::new(0, 0, width, 1);
        render_status_bar(&mut buf, &region, info);
        (0..width)
            .map(|col| buf.get(col, 0).ch)
            .collect::<String>()
            .trim_end()
            .to_string()
    }

    fn basic_info() -> StatusBarInfo {
        StatusBarInfo {
            nick: "user".to_string(),
            buffer_label: "#general".to_string(),
            buffer_id: BufferId::Channel("#general".to_string()),
            topic: None,
            user_count: None,
            lag: None,
            away: false,
            scroll_info: None,
        }
    }

    #[test]
    fn test_zero_width_region() {
        let mut buf = Buffer::new(10, 1);
        let region = Rect::new(0, 0, 0, 1);
        render_status_bar(&mut buf, &region, &basic_info());
        // Buffer should be untouched.
        assert_eq!(buf.get(0, 0).ch, ' ');
    }

    #[test]
    fn test_zero_height_region() {
        let mut buf = Buffer::new(10, 1);
        let region = Rect::new(0, 0, 10, 0);
        render_status_bar(&mut buf, &region, &basic_info());
        assert_eq!(buf.get(0, 0).ch, ' ');
    }

    #[test]
    fn test_basic_nick_and_channel() {
        let text = render_to_string(60, &basic_info());
        assert!(text.contains("[user]"), "should contain nick: {text}");
        assert!(text.contains("#general"), "should contain channel: {text}");
    }

    #[test]
    fn test_nick_style_is_bold() {
        let mut buf = Buffer::new(60, 1);
        let region = Rect::new(0, 0, 60, 1);
        render_status_bar(&mut buf, &region, &basic_info());
        // '[' at col 0 has nick style (bold + reverse).
        let cell = buf.get(0, 0);
        assert_eq!(cell.ch, '[');
        assert!(cell.style.bold, "nick should be bold");
        assert!(cell.style.reverse, "nick should be reverse");
    }

    #[test]
    fn test_channel_style_is_reverse() {
        let mut buf = Buffer::new(60, 1);
        let region = Rect::new(0, 0, 60, 1);
        render_status_bar(&mut buf, &region, &basic_info());
        // After "[user] ", channel starts. Find '#'.
        let mut found = false;
        for col in 0..60 {
            if buf.get(col, 0).ch == '#' {
                let cell = buf.get(col, 0);
                assert!(cell.style.reverse, "channel should be reverse");
                assert!(!cell.style.bold, "channel should not be bold");
                found = true;
                break;
            }
        }
        assert!(found, "should find '#' character");
    }

    #[test]
    fn test_with_topic() {
        let mut info = basic_info();
        info.topic = Some("Welcome to general".to_string());
        let text = render_to_string(80, &info);
        assert!(
            text.contains("(+Welcome to general)"),
            "should contain topic: {text}"
        );
    }

    #[test]
    fn test_topic_truncation() {
        let mut info = basic_info();
        info.topic = Some("This is a very long topic that should be truncated".to_string());
        let text = render_to_string(40, &info);
        assert!(
            text.contains("..."),
            "long topic should be truncated with ellipsis: {text}"
        );
    }

    #[test]
    fn test_user_count_for_channel() {
        let mut info = basic_info();
        info.user_count = Some(42);
        let text = render_to_string(60, &info);
        assert!(
            text.contains("[42 users]"),
            "should show user count: {text}"
        );
    }

    #[test]
    fn test_user_count_not_shown_for_status() {
        let mut info = basic_info();
        info.buffer_id = BufferId::Status;
        info.buffer_label = "Status".to_string();
        info.user_count = Some(10);
        let text = render_to_string(60, &info);
        assert!(
            !text.contains("users"),
            "status buffer should not show user count: {text}"
        );
    }

    #[test]
    fn test_user_count_not_shown_for_query() {
        let mut info = basic_info();
        info.buffer_id = BufferId::Query("bob".to_string());
        info.buffer_label = "bob".to_string();
        info.user_count = Some(2);
        let text = render_to_string(60, &info);
        assert!(
            !text.contains("users"),
            "query buffer should not show user count: {text}"
        );
    }

    #[test]
    fn test_away_indicator() {
        let mut info = basic_info();
        info.away = true;
        let text = render_to_string(60, &info);
        assert!(
            text.contains("[away]"),
            "should show away indicator: {text}"
        );
    }

    #[test]
    fn test_away_style_is_red_bold() {
        let mut info = basic_info();
        info.away = true;
        let mut buf = Buffer::new(60, 1);
        let region = Rect::new(0, 0, 60, 1);
        render_status_bar(&mut buf, &region, &info);
        // Find the 'a' in "[away]".
        for col in 0..60 {
            if buf.get(col, 0).ch == 'a' && col + 1 < 60 && buf.get(col + 1, 0).ch == 'w' {
                let cell = buf.get(col, 0);
                assert!(cell.style.bold, "away should be bold");
                assert_eq!(cell.style.fg, Some(Color::Red), "away should be red");
                break;
            }
        }
    }

    #[test]
    fn test_not_away_hides_indicator() {
        let info = basic_info(); // away = false
        let text = render_to_string(60, &info);
        assert!(
            !text.contains("[away]"),
            "should not show away when not away: {text}"
        );
    }

    #[test]
    fn test_lag_indicator() {
        let mut info = basic_info();
        info.lag = Some(42);
        let text = render_to_string(60, &info);
        assert!(text.contains("[lag: 42ms]"), "should show lag: {text}");
    }

    #[test]
    fn test_no_lag_hides_indicator() {
        let info = basic_info(); // lag = None
        let text = render_to_string(60, &info);
        assert!(
            !text.contains("lag"),
            "should not show lag when None: {text}"
        );
    }

    #[test]
    fn test_scroll_info() {
        let mut info = basic_info();
        info.scroll_info = Some("Scrolled: +42".to_string());
        let text = render_to_string(60, &info);
        assert!(
            text.contains("[Scrolled: +42]"),
            "should show scroll info: {text}"
        );
    }

    #[test]
    fn test_no_scroll_hides_indicator() {
        let info = basic_info(); // scroll_info = None
        let text = render_to_string(60, &info);
        assert!(
            !text.contains("Scrolled"),
            "should not show scroll info: {text}"
        );
    }

    #[test]
    fn test_all_right_side_elements() {
        let mut info = basic_info();
        info.away = true;
        info.lag = Some(100);
        info.scroll_info = Some("Scrolled: +10".to_string());
        let text = render_to_string(80, &info);
        assert!(text.contains("[away]"), "should contain away: {text}");
        assert!(text.contains("[lag: 100ms]"), "should contain lag: {text}");
        assert!(
            text.contains("[Scrolled: +10]"),
            "should contain scroll: {text}"
        );
    }

    #[test]
    fn test_right_aligned_position() {
        let mut info = basic_info();
        info.lag = Some(5);
        let width: u16 = 60;
        let mut buf = Buffer::new(width, 1);
        let region = Rect::new(0, 0, width, 1);
        render_status_bar(&mut buf, &region, &info);

        // Right side text should end near the right edge.
        let text = render_to_string(width, &info);
        // The right side should NOT be adjacent to the left side.
        let left_end = text.find('#').unwrap() + "#general".len();
        let right_start = text.find("[lag:").unwrap();
        assert!(
            right_start > left_end,
            "right should be separated from left"
        );
    }

    #[test]
    fn test_right_side_hidden_when_no_room() {
        let mut info = basic_info();
        info.away = true;
        info.lag = Some(999);
        info.scroll_info = Some("Scrolled: +999".to_string());
        // Very narrow — left side takes up most space.
        let text = render_to_string(20, &info);
        // Should still have nick and channel, but right-side may be hidden.
        assert!(text.contains("[user]"), "nick should still show: {text}");
    }

    #[test]
    fn test_region_offset() {
        let mut buf = Buffer::new(80, 5);
        let region = Rect::new(5, 3, 40, 1);
        render_status_bar(&mut buf, &region, &basic_info());
        // Row 3 at col 5 should have '['.
        assert_eq!(buf.get(5, 3).ch, '[');
        // Row 0 at col 0 should be untouched.
        assert_eq!(buf.get(0, 0).ch, ' ');
    }

    #[test]
    fn test_status_buffer_rendering() {
        let info = StatusBarInfo {
            nick: "me".to_string(),
            buffer_label: "Status".to_string(),
            buffer_id: BufferId::Status,
            topic: None,
            user_count: None,
            lag: None,
            away: false,
            scroll_info: None,
        };
        let text = render_to_string(40, &info);
        assert!(text.contains("[me]"), "should show nick: {text}");
        assert!(text.contains("Status"), "should show Status label: {text}");
    }

    #[test]
    fn test_query_buffer_rendering() {
        let info = StatusBarInfo {
            nick: "alice".to_string(),
            buffer_label: "bob".to_string(),
            buffer_id: BufferId::Query("bob".to_string()),
            topic: None,
            user_count: None,
            lag: None,
            away: false,
            scroll_info: None,
        };
        let text = render_to_string(40, &info);
        assert!(text.contains("[alice]"), "should show nick: {text}");
        assert!(text.contains("bob"), "should show query target: {text}");
    }

    #[test]
    fn test_empty_topic_not_shown() {
        let mut info = basic_info();
        info.topic = Some(String::new());
        let text = render_to_string(60, &info);
        assert!(
            !text.contains("(+"),
            "empty topic should not be shown: {text}"
        );
    }

    #[test]
    fn test_full_row_background() {
        let width: u16 = 40;
        let mut buf = Buffer::new(width, 1);
        let region = Rect::new(0, 0, width, 1);
        render_status_bar(&mut buf, &region, &basic_info());
        // Every cell in the row should have reverse set.
        for col in 0..width {
            let cell = buf.get(col, 0);
            assert!(
                cell.style.reverse,
                "col {col} should have reverse background, got {:?}",
                cell.style
            );
        }
    }
}
