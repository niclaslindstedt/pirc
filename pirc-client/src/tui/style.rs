use std::io::{self, Write};

/// Terminal color representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    /// Use the terminal's default color.
    Default,
    /// Standard ANSI color (0–7).
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    /// Bright ANSI color (8–15).
    BrightBlack,
    BrightRed,
    BrightGreen,
    BrightYellow,
    BrightBlue,
    BrightMagenta,
    BrightCyan,
    BrightWhite,
    /// 256-color palette index (0–255).
    Palette(u8),
    /// 24-bit true color.
    Rgb(u8, u8, u8),
}

impl Color {
    /// Write the SGR parameter(s) for this color as a foreground color.
    fn write_fg_param(&self, w: &mut impl Write) -> io::Result<()> {
        match self {
            Color::Default => w.write_all(b"39"),
            Color::Black => w.write_all(b"30"),
            Color::Red => w.write_all(b"31"),
            Color::Green => w.write_all(b"32"),
            Color::Yellow => w.write_all(b"33"),
            Color::Blue => w.write_all(b"34"),
            Color::Magenta => w.write_all(b"35"),
            Color::Cyan => w.write_all(b"36"),
            Color::White => w.write_all(b"37"),
            Color::BrightBlack => w.write_all(b"90"),
            Color::BrightRed => w.write_all(b"91"),
            Color::BrightGreen => w.write_all(b"92"),
            Color::BrightYellow => w.write_all(b"93"),
            Color::BrightBlue => w.write_all(b"94"),
            Color::BrightMagenta => w.write_all(b"95"),
            Color::BrightCyan => w.write_all(b"96"),
            Color::BrightWhite => w.write_all(b"97"),
            Color::Palette(n) => write!(w, "38;5;{}", n),
            Color::Rgb(r, g, b) => write!(w, "38;2;{};{};{}", r, g, b),
        }
    }

    /// Write the SGR parameter(s) for this color as a background color.
    fn write_bg_param(&self, w: &mut impl Write) -> io::Result<()> {
        match self {
            Color::Default => w.write_all(b"49"),
            Color::Black => w.write_all(b"40"),
            Color::Red => w.write_all(b"41"),
            Color::Green => w.write_all(b"42"),
            Color::Yellow => w.write_all(b"43"),
            Color::Blue => w.write_all(b"44"),
            Color::Magenta => w.write_all(b"45"),
            Color::Cyan => w.write_all(b"46"),
            Color::White => w.write_all(b"47"),
            Color::BrightBlack => w.write_all(b"100"),
            Color::BrightRed => w.write_all(b"101"),
            Color::BrightGreen => w.write_all(b"102"),
            Color::BrightYellow => w.write_all(b"103"),
            Color::BrightBlue => w.write_all(b"104"),
            Color::BrightMagenta => w.write_all(b"105"),
            Color::BrightCyan => w.write_all(b"106"),
            Color::BrightWhite => w.write_all(b"107"),
            Color::Palette(n) => write!(w, "48;5;{}", n),
            Color::Rgb(r, g, b) => write!(w, "48;2;{};{};{}", r, g, b),
        }
    }
}

/// Text style attributes and colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Style {
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    pub bold: bool,
    pub underline: bool,
    pub italic: bool,
    pub reverse: bool,
}

impl Default for Style {
    fn default() -> Self {
        Self::new()
    }
}

impl Style {
    /// Create a new style with no attributes set.
    pub const fn new() -> Self {
        Self {
            fg: None,
            bg: None,
            bold: false,
            underline: false,
            italic: false,
            reverse: false,
        }
    }

    /// Set the foreground color.
    pub const fn fg(mut self, color: Color) -> Self {
        self.fg = Some(color);
        self
    }

    /// Set the background color.
    pub const fn bg(mut self, color: Color) -> Self {
        self.bg = Some(color);
        self
    }

    /// Set the bold attribute.
    pub const fn bold(mut self, on: bool) -> Self {
        self.bold = on;
        self
    }

    /// Set the underline attribute.
    pub const fn underline(mut self, on: bool) -> Self {
        self.underline = on;
        self
    }

    /// Set the italic attribute.
    pub const fn italic(mut self, on: bool) -> Self {
        self.italic = on;
        self
    }

    /// Set the reverse/inverse attribute.
    pub const fn reverse(mut self, on: bool) -> Self {
        self.reverse = on;
        self
    }

    /// Returns true if this style has no attributes or colors set.
    pub fn is_empty(&self) -> bool {
        self.fg.is_none()
            && self.bg.is_none()
            && !self.bold
            && !self.underline
            && !self.italic
            && !self.reverse
    }

    /// Write the SGR escape sequence for this style.
    ///
    /// If the style is empty (no attributes set), nothing is written.
    /// Otherwise writes `\x1b[<params>m` with the appropriate SGR parameters.
    pub fn write_sgr(&self, w: &mut impl Write) -> io::Result<()> {
        if self.is_empty() {
            return Ok(());
        }

        w.write_all(b"\x1b[")?;
        let mut need_sep = false;

        if self.bold {
            w.write_all(b"1")?;
            need_sep = true;
        }
        if self.italic {
            if need_sep {
                w.write_all(b";")?;
            }
            w.write_all(b"3")?;
            need_sep = true;
        }
        if self.underline {
            if need_sep {
                w.write_all(b";")?;
            }
            w.write_all(b"4")?;
            need_sep = true;
        }
        if self.reverse {
            if need_sep {
                w.write_all(b";")?;
            }
            w.write_all(b"7")?;
            need_sep = true;
        }
        if let Some(ref fg) = self.fg {
            if need_sep {
                w.write_all(b";")?;
            }
            fg.write_fg_param(w)?;
            need_sep = true;
        }
        if let Some(ref bg) = self.bg {
            if need_sep {
                w.write_all(b";")?;
            }
            bg.write_bg_param(w)?;
        }

        w.write_all(b"m")
    }

    /// Write the SGR reset sequence (`\x1b[0m`).
    pub fn write_reset(w: &mut impl Write) -> io::Result<()> {
        w.write_all(b"\x1b[0m")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect(f: impl FnOnce(&mut Vec<u8>) -> io::Result<()>) -> String {
        let mut buf = Vec::new();
        f(&mut buf).unwrap();
        String::from_utf8(buf).unwrap()
    }

    // --- Style builder ---

    #[test]
    fn test_new_style_is_empty() {
        let s = Style::new();
        assert!(s.is_empty());
    }

    #[test]
    fn test_default_style_is_empty() {
        let s = Style::default();
        assert!(s.is_empty());
    }

    #[test]
    fn test_builder_fg() {
        let s = Style::new().fg(Color::Red);
        assert_eq!(s.fg, Some(Color::Red));
        assert!(!s.is_empty());
    }

    #[test]
    fn test_builder_bg() {
        let s = Style::new().bg(Color::Blue);
        assert_eq!(s.bg, Some(Color::Blue));
    }

    #[test]
    fn test_builder_chaining() {
        let s = Style::new()
            .fg(Color::Green)
            .bg(Color::Black)
            .bold(true)
            .underline(true)
            .italic(true)
            .reverse(true);
        assert_eq!(s.fg, Some(Color::Green));
        assert_eq!(s.bg, Some(Color::Black));
        assert!(s.bold);
        assert!(s.underline);
        assert!(s.italic);
        assert!(s.reverse);
    }

    // --- SGR generation ---

    #[test]
    fn test_empty_style_writes_nothing() {
        let output = collect(|w| Style::new().write_sgr(w));
        assert_eq!(output, "");
    }

    #[test]
    fn test_reset() {
        let output = collect(|w| Style::write_reset(w));
        assert_eq!(output, "\x1b[0m");
    }

    #[test]
    fn test_bold_only() {
        let output = collect(|w| Style::new().bold(true).write_sgr(w));
        assert_eq!(output, "\x1b[1m");
    }

    #[test]
    fn test_italic_only() {
        let output = collect(|w| Style::new().italic(true).write_sgr(w));
        assert_eq!(output, "\x1b[3m");
    }

    #[test]
    fn test_underline_only() {
        let output = collect(|w| Style::new().underline(true).write_sgr(w));
        assert_eq!(output, "\x1b[4m");
    }

    #[test]
    fn test_reverse_only() {
        let output = collect(|w| Style::new().reverse(true).write_sgr(w));
        assert_eq!(output, "\x1b[7m");
    }

    #[test]
    fn test_fg_standard_colors() {
        let cases = [
            (Color::Black, "30"),
            (Color::Red, "31"),
            (Color::Green, "32"),
            (Color::Yellow, "33"),
            (Color::Blue, "34"),
            (Color::Magenta, "35"),
            (Color::Cyan, "36"),
            (Color::White, "37"),
        ];
        for (color, code) in cases {
            let output = collect(|w| Style::new().fg(color).write_sgr(w));
            assert_eq!(output, format!("\x1b[{}m", code), "fg {:?}", color);
        }
    }

    #[test]
    fn test_fg_bright_colors() {
        let cases = [
            (Color::BrightBlack, "90"),
            (Color::BrightRed, "91"),
            (Color::BrightGreen, "92"),
            (Color::BrightYellow, "93"),
            (Color::BrightBlue, "94"),
            (Color::BrightMagenta, "95"),
            (Color::BrightCyan, "96"),
            (Color::BrightWhite, "97"),
        ];
        for (color, code) in cases {
            let output = collect(|w| Style::new().fg(color).write_sgr(w));
            assert_eq!(output, format!("\x1b[{}m", code), "fg {:?}", color);
        }
    }

    #[test]
    fn test_bg_standard_colors() {
        let cases = [
            (Color::Black, "40"),
            (Color::Red, "41"),
            (Color::Green, "42"),
            (Color::Yellow, "43"),
            (Color::Blue, "44"),
            (Color::Magenta, "45"),
            (Color::Cyan, "46"),
            (Color::White, "47"),
        ];
        for (color, code) in cases {
            let output = collect(|w| Style::new().bg(color).write_sgr(w));
            assert_eq!(output, format!("\x1b[{}m", code), "bg {:?}", color);
        }
    }

    #[test]
    fn test_bg_bright_colors() {
        let cases = [
            (Color::BrightBlack, "100"),
            (Color::BrightRed, "101"),
            (Color::BrightGreen, "102"),
            (Color::BrightYellow, "103"),
            (Color::BrightBlue, "104"),
            (Color::BrightMagenta, "105"),
            (Color::BrightCyan, "106"),
            (Color::BrightWhite, "107"),
        ];
        for (color, code) in cases {
            let output = collect(|w| Style::new().bg(color).write_sgr(w));
            assert_eq!(output, format!("\x1b[{}m", code), "bg {:?}", color);
        }
    }

    #[test]
    fn test_fg_default() {
        let output = collect(|w| Style::new().fg(Color::Default).write_sgr(w));
        assert_eq!(output, "\x1b[39m");
    }

    #[test]
    fn test_bg_default() {
        let output = collect(|w| Style::new().bg(Color::Default).write_sgr(w));
        assert_eq!(output, "\x1b[49m");
    }

    #[test]
    fn test_palette_fg() {
        let output = collect(|w| Style::new().fg(Color::Palette(196)).write_sgr(w));
        assert_eq!(output, "\x1b[38;5;196m");
    }

    #[test]
    fn test_palette_bg() {
        let output = collect(|w| Style::new().bg(Color::Palette(42)).write_sgr(w));
        assert_eq!(output, "\x1b[48;5;42m");
    }

    #[test]
    fn test_rgb_fg() {
        let output = collect(|w| Style::new().fg(Color::Rgb(255, 128, 0)).write_sgr(w));
        assert_eq!(output, "\x1b[38;2;255;128;0m");
    }

    #[test]
    fn test_rgb_bg() {
        let output = collect(|w| Style::new().bg(Color::Rgb(0, 0, 0)).write_sgr(w));
        assert_eq!(output, "\x1b[48;2;0;0;0m");
    }

    #[test]
    fn test_combined_fg_bg() {
        let output = collect(|w| Style::new().fg(Color::Red).bg(Color::White).write_sgr(w));
        assert_eq!(output, "\x1b[31;47m");
    }

    #[test]
    fn test_bold_with_fg() {
        let output = collect(|w| Style::new().bold(true).fg(Color::Cyan).write_sgr(w));
        assert_eq!(output, "\x1b[1;36m");
    }

    #[test]
    fn test_all_attributes() {
        let output = collect(|w| {
            Style::new()
                .bold(true)
                .italic(true)
                .underline(true)
                .reverse(true)
                .fg(Color::Yellow)
                .bg(Color::Blue)
                .write_sgr(w)
        });
        assert_eq!(output, "\x1b[1;3;4;7;33;44m");
    }

    #[test]
    fn test_sgr_then_reset() {
        let mut buf = Vec::new();
        Style::new().fg(Color::Red).write_sgr(&mut buf).unwrap();
        buf.extend_from_slice(b"hello");
        Style::write_reset(&mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert_eq!(output, "\x1b[31mhello\x1b[0m");
    }
}
