use std::io::{self, BufRead, Write};

const MAX_NAME_LEN: usize = 32;

/// Prompt the user for input, returning their trimmed response.
pub fn prompt(msg: &str) -> String {
    print!("{}", msg);
    io::stdout().flush().unwrap();
    let mut buf = String::new();
    io::stdin().lock().read_line(&mut buf).unwrap();
    buf.trim().to_string()
}

/// Prompt with a default value shown in brackets. Empty input returns the default.
pub fn prompt_with_default(msg: &str, default: &str) -> String {
    print!("{} [{}]: ", msg, default);
    io::stdout().flush().unwrap();
    let mut buf = String::new();
    io::stdin().lock().read_line(&mut buf).unwrap();
    let input = buf.trim();
    if input.is_empty() {
        default.to_string()
    } else {
        input.to_string()
    }
}

/// Ask a yes/no question. Empty input = yes.
#[allow(dead_code)]
pub fn confirm(msg: &str) -> bool {
    print!("{} [Y/n]: ", msg);
    io::stdout().flush().unwrap();
    let mut buf = String::new();
    io::stdin().lock().read_line(&mut buf).unwrap();
    let input = buf.trim().to_lowercase();
    input.is_empty() || input == "y" || input == "yes"
}

/// Sanitize a display name: strip control chars, trim, enforce max length.
pub fn sanitize_name(name: &str) -> String {
    let clean: String = name
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .to_string();
    if clean.len() > MAX_NAME_LEN {
        clean.chars().take(MAX_NAME_LEN).collect()
    } else {
        clean
    }
}

/// Validate a hex color string. Returns true for #RRGGBB format.
pub fn is_valid_color(color: &str) -> bool {
    color.len() == 7
        && color.starts_with('#')
        && color[1..].chars().all(|c| c.is_ascii_hexdigit())
}
