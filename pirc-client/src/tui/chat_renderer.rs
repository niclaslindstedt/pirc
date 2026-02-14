use crate::tui::buffer::Buffer;
use crate::tui::layout::Rect;
use crate::tui::message_buffer::{BufferLine, LineType, MessageBuffer};
use crate::tui::mirc_colors::{parse_mirc_format, StyledSpan};
use crate::tui::style::{Color, Style};

/// Style for timestamps (dim).
const STYLE_TIMESTAMP: Style = Style::new().fg(Color::BrightBlack);

/// Style for action lines (* nick action).
const STYLE_ACTION: Style = Style::new().bold(true).italic(true);

/// Style for notice lines (-nick- notice).
const STYLE_NOTICE: Style = Style::new().bold(true);

/// Style for join lines (-->).
const STYLE_JOIN: Style = Style::new().fg(Color::Green);

/// Style for part/quit/kick lines (<--/<<<).
const STYLE_PART: Style = Style::new().fg(Color::Red);

/// Style for mode/topic lines (===).
const STYLE_MODE: Style = Style::new().fg(Color::Yellow);

/// Style for system lines (***).
const STYLE_SYSTEM: Style = Style::new().fg(Color::Cyan);

/// Style for error lines (!!!).
const STYLE_ERROR: Style = Style::new().fg(Color::Red).bold(true);

/// Style for the scroll indicator.
const STYLE_SCROLL_INDICATOR: Style = Style::new().bold(true).fg(Color::Yellow).reverse(true);

/// Nick color palette (avoiding black/white for readability).
const NICK_COLORS: [Color; 8] = [
    Color::Red,
    Color::Green,
    Color::Yellow,
    Color::Blue,
    Color::Magenta,
    Color::Cyan,
    Color::BrightRed,
    Color::BrightGreen,
];

/// Timestamp column width: "[HH:MM] " = 8 chars.
const TIMESTAMP_WIDTH: usize = 8;

/// Determine a deterministic color for a nick based on its hash.
pub fn nick_color(nick: &str) -> Color {
    let hash = nick.bytes().fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32));
    NICK_COLORS[(hash as usize) % NICK_COLORS.len()]
}

/// Render the chat area with messages from a `MessageBuffer`.
///
/// Messages are drawn bottom-aligned: the most recent visible messages fill
/// the region from the bottom up. Each `BufferLine` is formatted according
/// to its `LineType`, with timestamps, sender formatting, and mIRC color
/// code support in message content.
///
/// When the buffer is scrolled up (not at bottom), a scroll indicator is
/// shown on the last row of the region.
pub fn render_chat_area(
    buf: &mut Buffer,
    region: &Rect,
    messages: &mut MessageBuffer,
    nick_width: usize,
) {
    let default_style = Style::new();
    buf.clear_region(region.x, region.y, region.width, region.height, default_style);

    if region.width == 0 || region.height == 0 {
        return;
    }

    let is_at_bottom = messages.is_at_bottom();
    let region_width = region.width as usize;

    // Reserve one row for scroll indicator if scrolled up
    let available_rows = if is_at_bottom {
        region.height as usize
    } else {
        (region.height as usize).saturating_sub(1)
    };

    if available_rows == 0 {
        // Only room for the scroll indicator
        render_scroll_indicator(buf, region, messages);
        return;
    }

    // Get messages visible in the view. We request more than available_rows
    // because line wrapping means one BufferLine may take multiple visual rows.
    // Request a generous number and then trim from the top.
    let view = messages.messages_in_view(available_rows);

    // Compute visual lines for each message, then figure out which ones fit
    let indent = TIMESTAMP_WIDTH + nick_width + 2; // +2 for "< " and "> " around nick or prefix spacing
    let content_width = region_width.saturating_sub(indent);

    let mut visual_lines: Vec<Vec<VisualLine>> = Vec::with_capacity(view.len());
    for line in view {
        visual_lines.push(format_line(line, nick_width, region_width, content_width));
    }

    // Calculate total visual lines
    let total_visual: usize = visual_lines.iter().map(|vl| vl.len()).sum();

    // We render bottom-aligned: if total visual lines <= available_rows,
    // start from the appropriate row; otherwise, skip leading visual lines
    let skip = total_visual.saturating_sub(available_rows);
    let start_row = if total_visual < available_rows {
        region.y as usize + (available_rows - total_visual)
    } else {
        region.y as usize
    };

    let mut current_row = start_row;
    let mut skipped = 0;

    for vlines in &visual_lines {
        for vline in vlines {
            if skipped < skip {
                skipped += 1;
                continue;
            }
            if current_row >= region.y as usize + available_rows {
                break;
            }
            render_visual_line(buf, region.x, current_row as u16, region_width, vline);
            current_row += 1;
        }
    }

    // Render scroll indicator if scrolled up
    if !is_at_bottom {
        render_scroll_indicator(buf, region, messages);
    }
}

/// A single visual (screen) line, ready to be rendered.
struct VisualLine {
    spans: Vec<SpanData>,
}

/// A piece of text with a style to render.
struct SpanData {
    text: String,
    style: Style,
}

/// Format a single BufferLine into one or more visual lines.
fn format_line(
    line: &BufferLine,
    nick_width: usize,
    region_width: usize,
    content_width: usize,
) -> Vec<VisualLine> {
    let timestamp = format!("[{}] ", line.timestamp);
    let ts_len = timestamp.len();

    let (prefix, prefix_style, content_spans) = match line.line_type {
        LineType::Message => {
            let nick = line.sender.as_deref().unwrap_or("???");
            let prefix = format!("{:<width$} ", format!("<{}>", nick), width = nick_width + 2);
            let style = Style::new().fg(nick_color(nick));
            let spans = parse_mirc_format(&line.content);
            (prefix, style, to_span_data(&spans))
        }
        LineType::Action => {
            let nick = line.sender.as_deref().unwrap_or("???");
            let prefix = format!("{:<width$} ", format!("* {}", nick), width = nick_width + 2);
            let content = vec![SpanData {
                text: line.content.clone(),
                style: STYLE_ACTION,
            }];
            (prefix, STYLE_ACTION, content)
        }
        LineType::Notice => {
            let nick = line.sender.as_deref().unwrap_or("???");
            let prefix = format!("{:<width$} ", format!("-{}-", nick), width = nick_width + 2);
            let content = vec![SpanData {
                text: line.content.clone(),
                style: STYLE_NOTICE,
            }];
            (prefix, STYLE_NOTICE, content)
        }
        LineType::Join => {
            let nick = line.sender.as_deref().unwrap_or("???");
            let prefix = format!("{:<width$} ", "-->", width = nick_width + 2);
            let content = vec![SpanData {
                text: format!("{} has joined", nick),
                style: STYLE_JOIN,
            }];
            (prefix, STYLE_JOIN, content)
        }
        LineType::Part => {
            let nick = line.sender.as_deref().unwrap_or("???");
            let reason = if line.content.is_empty() {
                String::new()
            } else {
                format!(" ({})", line.content)
            };
            let prefix = format!("{:<width$} ", "<--", width = nick_width + 2);
            let content = vec![SpanData {
                text: format!("{} has left{}", nick, reason),
                style: STYLE_PART,
            }];
            (prefix, STYLE_PART, content)
        }
        LineType::Quit => {
            let nick = line.sender.as_deref().unwrap_or("???");
            let reason = if line.content.is_empty() {
                String::new()
            } else {
                format!(" ({})", line.content)
            };
            let prefix = format!("{:<width$} ", "<--", width = nick_width + 2);
            let content = vec![SpanData {
                text: format!("{} has quit{}", nick, reason),
                style: STYLE_PART,
            }];
            (prefix, STYLE_PART, content)
        }
        LineType::Kick => {
            let nick = line.sender.as_deref().unwrap_or("???");
            let reason = if line.content.is_empty() {
                String::new()
            } else {
                format!(" ({})", line.content)
            };
            let prefix = format!("{:<width$} ", "<<<", width = nick_width + 2);
            let content = vec![SpanData {
                text: format!("{} was kicked{}", nick, reason),
                style: STYLE_PART,
            }];
            (prefix, STYLE_PART, content)
        }
        LineType::Mode => {
            let prefix = format!("{:<width$} ", "===", width = nick_width + 2);
            let content = vec![SpanData {
                text: line.content.clone(),
                style: STYLE_MODE,
            }];
            (prefix, STYLE_MODE, content)
        }
        LineType::Topic => {
            let prefix = format!("{:<width$} ", "===", width = nick_width + 2);
            let content = vec![SpanData {
                text: format!("topic: {}", line.content),
                style: STYLE_MODE,
            }];
            (prefix, STYLE_MODE, content)
        }
        LineType::System => {
            let prefix = format!("{:<width$} ", "***", width = nick_width + 2);
            let content = vec![SpanData {
                text: line.content.clone(),
                style: STYLE_SYSTEM,
            }];
            (prefix, STYLE_SYSTEM, content)
        }
        LineType::Error => {
            let prefix = format!("{:<width$} ", "!!!", width = nick_width + 2);
            let content = vec![SpanData {
                text: line.content.clone(),
                style: STYLE_ERROR,
            }];
            (prefix, STYLE_ERROR, content)
        }
    };

    // Build the first visual line
    let indent_width = ts_len + prefix.len();
    let mut lines = Vec::new();

    // Flatten content spans into characters with styles for wrapping
    let mut content_chars: Vec<(char, Style)> = Vec::new();
    for span in &content_spans {
        for ch in span.text.chars() {
            content_chars.push((ch, span.style));
        }
    }

    if content_width == 0 || region_width == 0 {
        // Can't fit any content, just show what we can
        let mut spans = vec![
            SpanData {
                text: timestamp.clone(),
                style: STYLE_TIMESTAMP,
            },
            SpanData {
                text: prefix.clone(),
                style: prefix_style,
            },
        ];
        spans.extend(content_spans);
        lines.push(VisualLine { spans });
        return lines;
    }

    // First line: timestamp + prefix + content (up to content_width chars)
    let first_line_chars: Vec<(char, Style)> = content_chars
        .iter()
        .take(content_width)
        .cloned()
        .collect();

    let mut first_spans = vec![
        SpanData {
            text: timestamp,
            style: STYLE_TIMESTAMP,
        },
        SpanData {
            text: prefix,
            style: prefix_style,
        },
    ];
    first_spans.extend(chars_to_spans(&first_line_chars));
    lines.push(VisualLine { spans: first_spans });

    // Continuation lines (wrapped)
    let remaining = &content_chars[first_line_chars.len()..];
    let wrap_width = region_width.saturating_sub(indent_width);
    if wrap_width > 0 {
        for chunk in remaining.chunks(wrap_width) {
            let indent_str = " ".repeat(indent_width);
            let mut spans = vec![SpanData {
                text: indent_str,
                style: Style::new(),
            }];
            spans.extend(chars_to_spans(chunk));
            lines.push(VisualLine { spans });
        }
    }

    lines
}

/// Convert parsed mIRC StyledSpans to owned SpanData.
fn to_span_data(spans: &[StyledSpan<'_>]) -> Vec<SpanData> {
    spans
        .iter()
        .map(|s| SpanData {
            text: s.text.to_string(),
            style: s.style,
        })
        .collect()
}

/// Convert a slice of (char, Style) into coalesced SpanData.
fn chars_to_spans(chars: &[(char, Style)]) -> Vec<SpanData> {
    if chars.is_empty() {
        return Vec::new();
    }

    let mut spans = Vec::new();
    let mut current_style = chars[0].1;
    let mut current_text = String::new();
    current_text.push(chars[0].0);

    for &(ch, style) in &chars[1..] {
        if style == current_style {
            current_text.push(ch);
        } else {
            spans.push(SpanData {
                text: current_text.clone(),
                style: current_style,
            });
            current_text.clear();
            current_text.push(ch);
            current_style = style;
        }
    }

    spans.push(SpanData {
        text: current_text,
        style: current_style,
    });

    spans
}

/// Render a single visual line into the buffer.
fn render_visual_line(buf: &mut Buffer, start_col: u16, row: u16, max_width: usize, vline: &VisualLine) {
    let mut col = start_col as usize;
    let max_col = start_col as usize + max_width;

    for span in &vline.spans {
        for ch in span.text.chars() {
            if col >= max_col {
                return;
            }
            buf.set(col as u16, row, ch, span.style);
            col += 1;
        }
    }
}

/// Render the scroll indicator at the bottom of the region.
fn render_scroll_indicator(buf: &mut Buffer, region: &Rect, messages: &MessageBuffer) {
    let row = region.y + region.height - 1;
    let total = messages.len();
    let indicator = format!("-- {} more --", total);
    let width = region.width as usize;

    // Center the indicator
    let text_len = indicator.len().min(width);
    let start = region.x as usize + (width.saturating_sub(text_len)) / 2;

    buf.write_str(start as u16, row, &indicator, STYLE_SCROLL_INDICATOR);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::layout::Rect;

    fn make_line(sender: &str, content: &str, line_type: LineType) -> BufferLine {
        BufferLine {
            timestamp: "12:00".to_string(),
            sender: Some(sender.to_string()),
            content: content.to_string(),
            line_type,
        }
    }

    fn make_system_line(content: &str, line_type: LineType) -> BufferLine {
        BufferLine {
            timestamp: "12:00".to_string(),
            sender: None,
            content: content.to_string(),
            line_type,
        }
    }

    fn row_text(buf: &Buffer, row: u16, start: u16, width: u16) -> String {
        (start..start + width).map(|col| buf.get(col, row).ch).collect()
    }

    fn cell_style(buf: &Buffer, col: u16, row: u16) -> Style {
        buf.get(col, row).style
    }

    // --- nick_color ---

    #[test]
    fn nick_color_deterministic() {
        let c1 = nick_color("alice");
        let c2 = nick_color("alice");
        assert_eq!(c1, c2);
    }

    #[test]
    fn nick_color_different_nicks_vary() {
        // Different nicks should generally produce different colors
        // (not guaranteed but we check several)
        let colors: Vec<Color> = ["alice", "bob", "charlie", "dave", "eve", "frank", "grace", "heidi"]
            .iter()
            .map(|n| nick_color(n))
            .collect();
        // At least 3 distinct colors from 8 nicks
        let mut unique = colors.clone();
        unique.dedup();
        assert!(unique.len() >= 3, "Expected at least 3 distinct colors, got {:?}", colors);
    }

    #[test]
    fn nick_color_returns_palette_color() {
        let c = nick_color("test");
        assert!(NICK_COLORS.contains(&c));
    }

    // --- format_line for each LineType ---

    #[test]
    fn format_message_line() {
        let line = make_line("alice", "hello world", LineType::Message);
        let vlines = format_line(&line, 8, 80, 60);
        assert_eq!(vlines.len(), 1);
        // First span is timestamp
        assert_eq!(vlines[0].spans[0].text, "[12:00] ");
        assert_eq!(vlines[0].spans[0].style, STYLE_TIMESTAMP);
        // Second span is nick
        assert!(vlines[0].spans[1].text.contains("<alice>"));
    }

    #[test]
    fn format_action_line() {
        let line = make_line("alice", "waves", LineType::Action);
        let vlines = format_line(&line, 8, 80, 60);
        assert_eq!(vlines.len(), 1);
        assert!(vlines[0].spans[1].text.contains("* alice"));
        assert_eq!(vlines[0].spans[1].style, STYLE_ACTION);
    }

    #[test]
    fn format_notice_line() {
        let line = make_line("alice", "hey!", LineType::Notice);
        let vlines = format_line(&line, 8, 80, 60);
        assert_eq!(vlines.len(), 1);
        assert!(vlines[0].spans[1].text.contains("-alice-"));
        assert_eq!(vlines[0].spans[1].style, STYLE_NOTICE);
    }

    #[test]
    fn format_join_line() {
        let line = make_line("alice", "", LineType::Join);
        let vlines = format_line(&line, 8, 80, 60);
        assert_eq!(vlines.len(), 1);
        assert!(vlines[0].spans[1].text.contains("-->"));
        // Content should say "alice has joined"
        let content_text: String = vlines[0].spans[2..].iter().map(|s| s.text.clone()).collect();
        assert!(content_text.contains("alice has joined"), "Got: {}", content_text);
    }

    #[test]
    fn format_part_line_with_reason() {
        let line = make_line("alice", "bye!", LineType::Part);
        let vlines = format_line(&line, 8, 80, 60);
        let content_text: String = vlines[0].spans[2..].iter().map(|s| s.text.clone()).collect();
        assert!(content_text.contains("alice has left"), "Got: {}", content_text);
        assert!(content_text.contains("(bye!)"), "Got: {}", content_text);
    }

    #[test]
    fn format_part_line_no_reason() {
        let line = make_line("alice", "", LineType::Part);
        let vlines = format_line(&line, 8, 80, 60);
        let content_text: String = vlines[0].spans[2..].iter().map(|s| s.text.clone()).collect();
        assert!(content_text.contains("alice has left"), "Got: {}", content_text);
        assert!(!content_text.contains("("), "Should not have parens: {}", content_text);
    }

    #[test]
    fn format_quit_line() {
        let line = make_line("bob", "connection reset", LineType::Quit);
        let vlines = format_line(&line, 8, 80, 60);
        let content_text: String = vlines[0].spans[2..].iter().map(|s| s.text.clone()).collect();
        assert!(content_text.contains("bob has quit"), "Got: {}", content_text);
        assert!(content_text.contains("(connection reset)"), "Got: {}", content_text);
    }

    #[test]
    fn format_kick_line() {
        let line = make_line("bob", "spam", LineType::Kick);
        let vlines = format_line(&line, 8, 80, 60);
        assert!(vlines[0].spans[1].text.contains("<<<"));
        let content_text: String = vlines[0].spans[2..].iter().map(|s| s.text.clone()).collect();
        assert!(content_text.contains("bob was kicked"), "Got: {}", content_text);
        assert!(content_text.contains("(spam)"), "Got: {}", content_text);
    }

    #[test]
    fn format_mode_line() {
        let line = make_system_line("+o alice", LineType::Mode);
        let vlines = format_line(&line, 8, 80, 60);
        assert!(vlines[0].spans[1].text.contains("==="));
        let content_text: String = vlines[0].spans[2..].iter().map(|s| s.text.clone()).collect();
        assert!(content_text.contains("+o alice"), "Got: {}", content_text);
        assert_eq!(vlines[0].spans[2].style, STYLE_MODE);
    }

    #[test]
    fn format_topic_line() {
        let line = make_system_line("Welcome to #rust!", LineType::Topic);
        let vlines = format_line(&line, 8, 80, 60);
        let content_text: String = vlines[0].spans[2..].iter().map(|s| s.text.clone()).collect();
        assert!(content_text.contains("topic: Welcome to #rust!"), "Got: {}", content_text);
    }

    #[test]
    fn format_system_line() {
        let line = make_system_line("Server notice", LineType::System);
        let vlines = format_line(&line, 8, 80, 60);
        assert!(vlines[0].spans[1].text.contains("***"));
        assert_eq!(vlines[0].spans[2].style, STYLE_SYSTEM);
    }

    #[test]
    fn format_error_line() {
        let line = make_system_line("Connection failed", LineType::Error);
        let vlines = format_line(&line, 8, 80, 60);
        assert!(vlines[0].spans[1].text.contains("!!!"));
        assert_eq!(vlines[0].spans[2].style, STYLE_ERROR);
    }

    // --- Line wrapping ---

    #[test]
    fn wrapping_long_message() {
        // Region width 40, timestamp=8, nick_width=8 => prefix ~18, content_width ~22
        let long_content = "a".repeat(50);
        let line = make_line("alice", &long_content, LineType::Message);
        let vlines = format_line(&line, 8, 40, 22);
        assert!(vlines.len() > 1, "Expected wrapping, got {} lines", vlines.len());
        // First line has content_width=22 chars, rest wraps
    }

    #[test]
    fn wrapping_short_message_no_wrap() {
        let line = make_line("alice", "hi", LineType::Message);
        let vlines = format_line(&line, 8, 80, 60);
        assert_eq!(vlines.len(), 1);
    }

    #[test]
    fn wrapping_continuation_has_indent() {
        let long_content = "a".repeat(50);
        let line = make_line("alice", &long_content, LineType::Message);
        let vlines = format_line(&line, 8, 40, 22);
        // Second line should start with spaces (indent)
        if vlines.len() > 1 {
            let first_span = &vlines[1].spans[0];
            assert!(first_span.text.chars().all(|c| c == ' '), "Continuation indent should be spaces");
        }
    }

    // --- render_chat_area ---

    #[test]
    fn render_empty_buffer() {
        let mut screen = Buffer::new(80, 24);
        let region = Rect::new(0, 1, 60, 20);
        let mut mb = MessageBuffer::new(100);
        render_chat_area(&mut screen, &region, &mut mb, 8);
        // Should just be spaces
        let text = row_text(&screen, 1, 0, 60);
        assert_eq!(text.trim(), "");
    }

    #[test]
    fn render_single_message() {
        let mut screen = Buffer::new(80, 24);
        let region = Rect::new(0, 1, 60, 5);
        let mut mb = MessageBuffer::new(100);
        mb.push_message(make_line("alice", "hello", LineType::Message));
        render_chat_area(&mut screen, &region, &mut mb, 8);
        // Message should be on the last row (bottom-aligned)
        let last_row = 1 + 5 - 1; // row 5
        let text = row_text(&screen, last_row, 0, 60);
        assert!(text.contains("[12:00]"), "Should have timestamp: '{}'", text);
        assert!(text.contains("alice"), "Should have nick: '{}'", text);
        assert!(text.contains("hello"), "Should have content: '{}'", text);
    }

    #[test]
    fn render_multiple_messages_fill_region() {
        let mut screen = Buffer::new(80, 24);
        let region = Rect::new(0, 0, 60, 3);
        let mut mb = MessageBuffer::new(100);
        mb.push_message(make_line("alice", "msg1", LineType::Message));
        mb.push_message(make_line("bob", "msg2", LineType::Message));
        mb.push_message(make_line("charlie", "msg3", LineType::Message));
        render_chat_area(&mut screen, &region, &mut mb, 8);

        let r0 = row_text(&screen, 0, 0, 60);
        let r1 = row_text(&screen, 1, 0, 60);
        let r2 = row_text(&screen, 2, 0, 60);
        assert!(r0.contains("msg1"), "Row 0: '{}'", r0);
        assert!(r1.contains("msg2"), "Row 1: '{}'", r1);
        assert!(r2.contains("msg3"), "Row 2: '{}'", r2);
    }

    #[test]
    fn render_scroll_indicator_when_not_at_bottom() {
        let mut screen = Buffer::new(80, 24);
        let region = Rect::new(0, 0, 60, 5);
        let mut mb = MessageBuffer::new(100);
        for i in 0..20 {
            mb.push_message(make_line("alice", &format!("msg{}", i), LineType::Message));
        }
        mb.scroll_up(5);
        render_chat_area(&mut screen, &region, &mut mb, 8);

        // Bottom row should have the scroll indicator
        let bottom_row = 4;
        let text = row_text(&screen, bottom_row, 0, 60);
        assert!(text.contains("more"), "Scroll indicator expected: '{}'", text);
    }

    #[test]
    fn render_no_scroll_indicator_at_bottom() {
        let mut screen = Buffer::new(80, 24);
        let region = Rect::new(0, 0, 60, 5);
        let mut mb = MessageBuffer::new(100);
        for i in 0..3 {
            mb.push_message(make_line("alice", &format!("msg{}", i), LineType::Message));
        }
        render_chat_area(&mut screen, &region, &mut mb, 8);

        // No scroll indicator
        let bottom_row = 4;
        let text = row_text(&screen, bottom_row, 0, 60);
        assert!(!text.contains("more"), "No scroll indicator expected: '{}'", text);
    }

    #[test]
    fn render_zero_width_region() {
        let mut screen = Buffer::new(80, 24);
        let region = Rect::new(0, 0, 0, 5);
        let mut mb = MessageBuffer::new(100);
        mb.push_message(make_line("alice", "hello", LineType::Message));
        // Should not panic
        render_chat_area(&mut screen, &region, &mut mb, 8);
    }

    #[test]
    fn render_zero_height_region() {
        let mut screen = Buffer::new(80, 24);
        let region = Rect::new(0, 0, 60, 0);
        let mut mb = MessageBuffer::new(100);
        mb.push_message(make_line("alice", "hello", LineType::Message));
        // Should not panic
        render_chat_area(&mut screen, &region, &mut mb, 8);
    }

    #[test]
    fn render_with_region_offset() {
        let mut screen = Buffer::new(80, 24);
        let region = Rect::new(5, 3, 40, 2);
        let mut mb = MessageBuffer::new(100);
        mb.push_message(make_line("alice", "hi", LineType::Message));
        render_chat_area(&mut screen, &region, &mut mb, 8);

        // Message should render within the region, not at (0,0)
        let text = row_text(&screen, 4, 5, 40);
        assert!(text.contains("[12:00]"), "Should render in region: '{}'", text);
        // Row 0 should be empty
        let r0 = row_text(&screen, 0, 0, 80);
        assert_eq!(r0.trim(), "");
    }

    #[test]
    fn render_timestamp_style() {
        let mut screen = Buffer::new(80, 24);
        let region = Rect::new(0, 0, 60, 1);
        let mut mb = MessageBuffer::new(100);
        mb.push_message(make_line("alice", "hello", LineType::Message));
        render_chat_area(&mut screen, &region, &mut mb, 8);

        // First character '[' should have STYLE_TIMESTAMP
        assert_eq!(cell_style(&screen, 0, 0), STYLE_TIMESTAMP);
    }

    #[test]
    fn render_join_style() {
        let mut screen = Buffer::new(80, 24);
        let region = Rect::new(0, 0, 60, 1);
        let mut mb = MessageBuffer::new(100);
        mb.push_message(make_line("alice", "", LineType::Join));
        render_chat_area(&mut screen, &region, &mut mb, 8);

        // The prefix "-->" should be in STYLE_JOIN (green)
        // Find the "-->" chars (after timestamp)
        let text = row_text(&screen, 0, 0, 60);
        let arrow_pos = text.find("-->").expect("Should find -->");
        assert_eq!(cell_style(&screen, arrow_pos as u16, 0), STYLE_JOIN);
    }

    #[test]
    fn render_mirc_colors_in_content() {
        let mut screen = Buffer::new(80, 24);
        let region = Rect::new(0, 0, 60, 1);
        let mut mb = MessageBuffer::new(100);
        // Message with mIRC red color code
        mb.push_message(BufferLine {
            timestamp: "12:00".to_string(),
            sender: Some("alice".to_string()),
            content: "\x034red text".to_string(),
            line_type: LineType::Message,
        });
        render_chat_area(&mut screen, &region, &mut mb, 8);

        // Find "red text" in the output and check its color
        let text = row_text(&screen, 0, 0, 60);
        let pos = text.find("red text").expect("Should find 'red text'");
        let style = cell_style(&screen, pos as u16, 0);
        assert_eq!(style.fg, Some(Color::Red), "mIRC red should apply");
    }

    #[test]
    fn render_different_line_types_distinct_prefixes() {
        let types_and_prefixes = [
            (LineType::Join, "-->"),
            (LineType::Part, "<--"),
            (LineType::Quit, "<--"),
            (LineType::Kick, "<<<"),
            (LineType::Mode, "==="),
            (LineType::System, "***"),
            (LineType::Error, "!!!"),
        ];

        for (lt, expected_prefix) in types_and_prefixes {
            let mut screen = Buffer::new(80, 24);
            let region = Rect::new(0, 0, 60, 1);
            let mut mb = MessageBuffer::new(100);
            mb.push_message(make_line("nick", "content", lt));
            render_chat_area(&mut screen, &region, &mut mb, 8);

            let text = row_text(&screen, 0, 0, 60);
            assert!(
                text.contains(expected_prefix),
                "LineType {:?} should have prefix '{}', got: '{}'",
                lt, expected_prefix, text
            );
        }
    }

    // --- chars_to_spans ---

    #[test]
    fn chars_to_spans_empty() {
        let spans = chars_to_spans(&[]);
        assert!(spans.is_empty());
    }

    #[test]
    fn chars_to_spans_single_style() {
        let style = Style::new().fg(Color::Red);
        let chars: Vec<(char, Style)> = "hello".chars().map(|c| (c, style)).collect();
        let spans = chars_to_spans(&chars);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text, "hello");
        assert_eq!(spans[0].style, style);
    }

    #[test]
    fn chars_to_spans_two_styles() {
        let red = Style::new().fg(Color::Red);
        let blue = Style::new().fg(Color::Blue);
        let chars = vec![
            ('a', red), ('b', red),
            ('c', blue), ('d', blue),
        ];
        let spans = chars_to_spans(&chars);
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].text, "ab");
        assert_eq!(spans[0].style, red);
        assert_eq!(spans[1].text, "cd");
        assert_eq!(spans[1].style, blue);
    }

    // --- to_span_data ---

    #[test]
    fn to_span_data_empty() {
        let spans = to_span_data(&[]);
        assert!(spans.is_empty());
    }

    #[test]
    fn to_span_data_converts() {
        let styled = vec![StyledSpan {
            text: "hello",
            style: Style::new().bold(true),
        }];
        let data = to_span_data(&styled);
        assert_eq!(data.len(), 1);
        assert_eq!(data[0].text, "hello");
        assert!(data[0].style.bold);
    }
}
