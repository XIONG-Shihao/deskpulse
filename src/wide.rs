//! NUL-terminated UTF-16 strings for the Win32 APIs.

/// Encodes `text` as UTF-16 with a trailing NUL.
pub fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}
