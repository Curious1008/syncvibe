use ratatui::style::Color;

pub fn parse_hex_color(hex: &str) -> Color {
    let hex = hex.trim_start_matches('#');
    if hex.len() != 6 {
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
