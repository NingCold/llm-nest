//! Shared attachment names and metadata presentation.

pub fn valid_file_name(name: &str) -> bool {
    !name.trim().is_empty()
        && name.len() <= 240
        && !name.ends_with([' ', '.'])
        && !name.chars().any(|c| {
            c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
        })
        && !matches!(name, "." | "..")
        && !matches!(
            name.split('.')
                .next()
                .unwrap_or("")
                .to_ascii_uppercase()
                .as_str(),
            "CON"
                | "PRN"
                | "AUX"
                | "NUL"
                | "COM1"
                | "COM2"
                | "COM3"
                | "COM4"
                | "COM5"
                | "COM6"
                | "COM7"
                | "COM8"
                | "COM9"
                | "LPT1"
                | "LPT2"
                | "LPT3"
                | "LPT4"
                | "LPT5"
                | "LPT6"
                | "LPT7"
                | "LPT8"
                | "LPT9"
        )
}
pub fn file_name(name: Option<&str>) -> String {
    name.filter(|n| valid_file_name(n))
        .unwrap_or("attachment.bin")
        .into()
}
pub fn placeholder(name: Option<&str>, mime: &str, size: usize) -> String {
    format!(
        "[文件 · {} · {} · {} 字节；文件内容尚未读取]",
        file_name(name),
        mime,
        size
    )
}
