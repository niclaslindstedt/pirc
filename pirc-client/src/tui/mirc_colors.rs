use crate::tui::style::{Color, Style};

/// mIRC control characters.
const MIRC_COLOR: char = '\x03';
const MIRC_BOLD: char = '\x02';
const MIRC_UNDERLINE: char = '\x1f';
const MIRC_REVERSE: char = '\x16';
const MIRC_RESET: char = '\x0f';
const MIRC_ITALIC: char = '\x1d';

/// A segment of text with an associated style.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyledSpan<'a> {
    pub text: &'a str,
    pub style: Style,
}

/// Map a mIRC color index (0–15) to the corresponding `Color`.
pub fn mirc_to_color(index: u8) -> Option<Color> {
    match index {
        0 => Some(Color::White),
        1 => Some(Color::Black),
        2 => Some(Color::Blue),
        3 => Some(Color::Green),
        4 => Some(Color::Red),
        5 => Some(Color::BrightRed),      // brown/dark red → BrightRed (maroon)
        6 => Some(Color::Magenta),
        7 => Some(Color::Yellow),          // orange → Yellow
        8 => Some(Color::BrightYellow),
        9 => Some(Color::BrightGreen),
        10 => Some(Color::Cyan),
        11 => Some(Color::BrightCyan),
        12 => Some(Color::BrightBlue),
        13 => Some(Color::BrightMagenta),
        14 => Some(Color::BrightBlack),    // dark grey
        15 => Some(Color::BrightWhite),    // light grey
        _ => None,
    }
}

/// Parse a string containing mIRC formatting codes into styled spans.
///
/// Supports:
/// - `\x03FG` and `\x03FG,BG` color codes (FG/BG are 0–15, one or two digits)
/// - `\x03` alone resets colors
/// - `\x02` toggles bold
/// - `\x1d` toggles italic
/// - `\x1f` toggles underline
/// - `\x16` toggles reverse
/// - `\x0f` resets all formatting
pub fn parse_mirc_format(input: &str) -> Vec<StyledSpan<'_>> {
    let mut spans = Vec::new();
    let mut style = Style::new();
    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    let mut span_start = 0;

    while i < len {
        match bytes[i] as char {
            MIRC_BOLD | MIRC_ITALIC | MIRC_UNDERLINE | MIRC_REVERSE | MIRC_RESET | MIRC_COLOR => {
                // Flush any accumulated text before this control character.
                if span_start < i {
                    spans.push(StyledSpan {
                        text: &input[span_start..i],
                        style,
                    });
                }

                match bytes[i] as char {
                    MIRC_BOLD => {
                        style.bold = !style.bold;
                        i += 1;
                    }
                    MIRC_ITALIC => {
                        style.italic = !style.italic;
                        i += 1;
                    }
                    MIRC_UNDERLINE => {
                        style.underline = !style.underline;
                        i += 1;
                    }
                    MIRC_REVERSE => {
                        style.reverse = !style.reverse;
                        i += 1;
                    }
                    MIRC_RESET => {
                        style = Style::new();
                        i += 1;
                    }
                    MIRC_COLOR => {
                        i += 1; // skip \x03
                        let fg = parse_color_number(bytes, &mut i);
                        match fg {
                            Some(fg_idx) => {
                                style.fg = mirc_to_color(fg_idx);
                                // Check for ,BG
                                if i < len && bytes[i] == b',' {
                                    let comma_pos = i;
                                    i += 1;
                                    let bg = parse_color_number(bytes, &mut i);
                                    match bg {
                                        Some(bg_idx) => {
                                            style.bg = mirc_to_color(bg_idx);
                                        }
                                        None => {
                                            // Comma was not followed by a valid number;
                                            // rewind to include the comma in text.
                                            i = comma_pos;
                                        }
                                    }
                                }
                            }
                            None => {
                                // Bare \x03 with no number resets colors.
                                style.fg = None;
                                style.bg = None;
                            }
                        }
                    }
                    _ => unreachable!(),
                }

                span_start = i;
            }
            _ => {
                i += 1;
            }
        }
    }

    // Flush trailing text.
    if span_start < len {
        spans.push(StyledSpan {
            text: &input[span_start..],
            style,
        });
    }

    spans
}

/// Parse up to two digits as a color number (0–15) from `bytes` starting at `*pos`.
/// Advances `*pos` past the consumed digits.
fn parse_color_number(bytes: &[u8], pos: &mut usize) -> Option<u8> {
    let start = *pos;
    if start >= bytes.len() || !bytes[start].is_ascii_digit() {
        return None;
    }

    let first = bytes[start] - b'0';
    *pos += 1;

    // Check for a second digit.
    if *pos < bytes.len() && bytes[*pos].is_ascii_digit() {
        let two_digit = first * 10 + (bytes[*pos] - b'0');
        if two_digit <= 15 {
            *pos += 1;
            return Some(two_digit);
        }
        // Single digit if two-digit value > 15
    }

    Some(first)
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- mirc_to_color mapping ---

    #[test]
    fn test_mirc_color_mapping_all_16() {
        let expected = [
            (0, Color::White),
            (1, Color::Black),
            (2, Color::Blue),
            (3, Color::Green),
            (4, Color::Red),
            (5, Color::BrightRed),
            (6, Color::Magenta),
            (7, Color::Yellow),
            (8, Color::BrightYellow),
            (9, Color::BrightGreen),
            (10, Color::Cyan),
            (11, Color::BrightCyan),
            (12, Color::BrightBlue),
            (13, Color::BrightMagenta),
            (14, Color::BrightBlack),
            (15, Color::BrightWhite),
        ];
        for (idx, color) in expected {
            assert_eq!(mirc_to_color(idx), Some(color), "mIRC color {}", idx);
        }
    }

    #[test]
    fn test_mirc_color_out_of_range() {
        assert_eq!(mirc_to_color(16), None);
        assert_eq!(mirc_to_color(99), None);
        assert_eq!(mirc_to_color(255), None);
    }

    // --- Plain text (no formatting) ---

    #[test]
    fn test_plain_text() {
        let spans = parse_mirc_format("hello world");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text, "hello world");
        assert_eq!(spans[0].style, Style::new());
    }

    #[test]
    fn test_empty_string() {
        let spans = parse_mirc_format("");
        assert_eq!(spans.len(), 0);
    }

    // --- Bold ---

    #[test]
    fn test_bold_text() {
        let input = "\x02bold\x02";
        let spans = parse_mirc_format(input);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text, "bold");
        assert!(spans[0].style.bold);
    }

    #[test]
    fn test_bold_toggle() {
        let input = "\x02on\x02off";
        let spans = parse_mirc_format(input);
        assert_eq!(spans.len(), 2);
        assert!(spans[0].style.bold);
        assert_eq!(spans[0].text, "on");
        assert!(!spans[1].style.bold);
        assert_eq!(spans[1].text, "off");
    }

    // --- Italic ---

    #[test]
    fn test_italic_text() {
        let input = "\x1ditalic\x1d";
        let spans = parse_mirc_format(input);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text, "italic");
        assert!(spans[0].style.italic);
    }

    // --- Underline ---

    #[test]
    fn test_underline_text() {
        let input = "\x1funderlined\x1f";
        let spans = parse_mirc_format(input);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text, "underlined");
        assert!(spans[0].style.underline);
    }

    // --- Reverse ---

    #[test]
    fn test_reverse_text() {
        let input = "\x16reversed\x16";
        let spans = parse_mirc_format(input);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text, "reversed");
        assert!(spans[0].style.reverse);
    }

    // --- Reset ---

    #[test]
    fn test_reset_clears_all() {
        let input = "\x02\x1fbold+underline\x0fplain";
        let spans = parse_mirc_format(input);
        assert_eq!(spans.len(), 2);
        assert!(spans[0].style.bold);
        assert!(spans[0].style.underline);
        assert_eq!(spans[0].text, "bold+underline");
        assert_eq!(spans[1].style, Style::new());
        assert_eq!(spans[1].text, "plain");
    }

    // --- Foreground color only ---

    #[test]
    fn test_fg_color_single_digit() {
        let input = "\x034red text";
        let spans = parse_mirc_format(input);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text, "red text");
        assert_eq!(spans[0].style.fg, Some(Color::Red));
        assert_eq!(spans[0].style.bg, None);
    }

    #[test]
    fn test_fg_color_two_digits() {
        let input = "\x0312blue text";
        let spans = parse_mirc_format(input);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text, "blue text");
        assert_eq!(spans[0].style.fg, Some(Color::BrightBlue));
    }

    // --- Foreground + background ---

    #[test]
    fn test_fg_and_bg() {
        let input = "\x034,2red on blue";
        let spans = parse_mirc_format(input);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text, "red on blue");
        assert_eq!(spans[0].style.fg, Some(Color::Red));
        assert_eq!(spans[0].style.bg, Some(Color::Blue));
    }

    #[test]
    fn test_fg_and_bg_two_digit() {
        let input = "\x0310,12cyan on brightblue";
        let spans = parse_mirc_format(input);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text, "cyan on brightblue");
        assert_eq!(spans[0].style.fg, Some(Color::Cyan));
        assert_eq!(spans[0].style.bg, Some(Color::BrightBlue));
    }

    // --- Bare \x03 resets colors ---

    #[test]
    fn test_bare_color_code_resets() {
        let input = "\x034red\x03plain";
        let spans = parse_mirc_format(input);
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].style.fg, Some(Color::Red));
        assert_eq!(spans[0].text, "red");
        assert_eq!(spans[1].style.fg, None);
        assert_eq!(spans[1].text, "plain");
    }

    // --- Mixed formatting ---

    #[test]
    fn test_bold_with_color() {
        let input = "\x02\x034bold red\x0f";
        let spans = parse_mirc_format(input);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text, "bold red");
        assert!(spans[0].style.bold);
        assert_eq!(spans[0].style.fg, Some(Color::Red));
    }

    #[test]
    fn test_multiple_color_changes() {
        let input = "\x034red\x039green\x0312blue";
        let spans = parse_mirc_format(input);
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0].text, "red");
        assert_eq!(spans[0].style.fg, Some(Color::Red));
        assert_eq!(spans[1].text, "green");
        assert_eq!(spans[1].style.fg, Some(Color::BrightGreen));
        assert_eq!(spans[2].text, "blue");
        assert_eq!(spans[2].style.fg, Some(Color::BrightBlue));
    }

    // --- Edge cases ---

    #[test]
    fn test_color_code_at_end_of_string() {
        let input = "text\x03";
        let spans = parse_mirc_format(input);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text, "text");
    }

    #[test]
    fn test_consecutive_control_codes() {
        let input = "\x02\x1f\x16styled\x0f";
        let spans = parse_mirc_format(input);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text, "styled");
        assert!(spans[0].style.bold);
        assert!(spans[0].style.underline);
        assert!(spans[0].style.reverse);
    }

    #[test]
    fn test_text_before_and_after_formatting() {
        let input = "before\x02bold\x02after";
        let spans = parse_mirc_format(input);
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0].text, "before");
        assert!(!spans[0].style.bold);
        assert_eq!(spans[1].text, "bold");
        assert!(spans[1].style.bold);
        assert_eq!(spans[2].text, "after");
        assert!(!spans[2].style.bold);
    }

    #[test]
    fn test_color_zero() {
        // mIRC color 0 = White
        let input = "\x030white text";
        let spans = parse_mirc_format(input);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text, "white text");
        assert_eq!(spans[0].style.fg, Some(Color::White));
    }

    #[test]
    fn test_color_15() {
        let input = "\x0315light grey";
        let spans = parse_mirc_format(input);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text, "light grey");
        assert_eq!(spans[0].style.fg, Some(Color::BrightWhite));
    }

    #[test]
    fn test_color_preserves_attributes() {
        let input = "\x02\x034red bold\x039green bold";
        let spans = parse_mirc_format(input);
        assert_eq!(spans.len(), 2);
        assert!(spans[0].style.bold);
        assert_eq!(spans[0].style.fg, Some(Color::Red));
        assert!(spans[1].style.bold);
        assert_eq!(spans[1].style.fg, Some(Color::BrightGreen));
    }

    #[test]
    fn test_only_control_codes_no_text() {
        let input = "\x02\x1f\x0f";
        let spans = parse_mirc_format(input);
        assert_eq!(spans.len(), 0);
    }

    #[test]
    fn test_comma_without_bg_number() {
        // \x034, followed by non-digit: comma should be part of text
        let input = "\x034,hello";
        let spans = parse_mirc_format(input);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text, ",hello");
        assert_eq!(spans[0].style.fg, Some(Color::Red));
        assert_eq!(spans[0].style.bg, None);
    }

    #[test]
    fn test_fg_bg_with_zero() {
        let input = "\x030,1white on black";
        let spans = parse_mirc_format(input);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text, "white on black");
        assert_eq!(spans[0].style.fg, Some(Color::White));
        assert_eq!(spans[0].style.bg, Some(Color::Black));
    }
}
