use ratatui::style::Color;
use unicode_width::UnicodeWidthChar;

/// Truncate a string to fit within `max_width` display columns, appending "..." if truncated.
/// Uses Unicode display width (CJK chars = 2 columns, ASCII = 1).
pub fn truncate_str(s: &str, max_width: usize) -> String {
    let mut width = 0;
    let mut truncated = String::new();
    for c in s.chars() {
        let cw = c.width().unwrap_or(0);
        if width + cw > max_width.saturating_sub(3) {
            truncated.push_str("...");
            return truncated;
        }
        width += cw;
        truncated.push(c);
    }
    // Didn't need truncation
    s.to_string()
}

pub fn parse_hex_color(hex: &str) -> Color {
    let hex = hex.trim_start_matches('#');
    if hex.len() != 6 || !hex.is_ascii() {
        return Color::Cyan;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(255);
    let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(255);
    let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(255);
    Color::Rgb(r, g, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_hex_with_hash() {
        assert_eq!(parse_hex_color("#ff0000"), Color::Rgb(255, 0, 0));
        assert_eq!(parse_hex_color("#00ff00"), Color::Rgb(0, 255, 0));
        assert_eq!(parse_hex_color("#0000ff"), Color::Rgb(0, 0, 255));
    }

    #[test]
    fn parse_valid_hex_without_hash() {
        assert_eq!(parse_hex_color("ff8800"), Color::Rgb(255, 136, 0));
    }

    #[test]
    fn parse_black_and_white() {
        assert_eq!(parse_hex_color("#000000"), Color::Rgb(0, 0, 0));
        assert_eq!(parse_hex_color("#ffffff"), Color::Rgb(255, 255, 255));
    }

    #[test]
    fn fallback_on_empty() {
        assert_eq!(parse_hex_color(""), Color::Cyan);
    }

    #[test]
    fn fallback_on_short_hex() {
        assert_eq!(parse_hex_color("#fff"), Color::Cyan);
    }

    #[test]
    fn fallback_on_long_hex() {
        assert_eq!(parse_hex_color("#ff00ff00"), Color::Cyan);
    }

    #[test]
    fn fallback_on_garbage() {
        assert_eq!(parse_hex_color("not-a-color"), Color::Cyan);
    }

    #[test]
    fn handles_invalid_hex_chars() {
        // "gggggg" is 6 chars but not valid hex → unwrap_or(255) for each
        assert_eq!(parse_hex_color("#gggggg"), Color::Rgb(255, 255, 255));
    }

    #[test]
    fn handles_mixed_case() {
        assert_eq!(parse_hex_color("#FF8800"), Color::Rgb(255, 136, 0));
        assert_eq!(parse_hex_color("#Ff8800"), Color::Rgb(255, 136, 0));
    }
}
