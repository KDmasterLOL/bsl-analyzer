use std::collections::HashSet;

/// Extract language keys from NStr string content.
/// Looks for patterns like: `ru='text'`, `en = "text"`, etc.
pub fn extract_language_keys(text: &str) -> HashSet<String> {
    let mut keys = HashSet::new();
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        // Look for start of identifier (letter or _)
        if chars[i].is_alphabetic() || chars[i] == '_' {
            let start = i;
            // Collect entire identifier
            while i < len && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let ident: String = chars[start..i].iter().collect();

            // Skip whitespace
            while i < len && chars[i].is_whitespace() {
                i += 1;
            }

            // Check for =
            if i < len && chars[i] == '=' {
                i += 1;
                // Skip whitespace
                while i < len && chars[i].is_whitespace() {
                    i += 1;
                }
                // Check for quote (single or double)
                if i < len && (chars[i] == '\'' || chars[i] == '"') {
                    keys.insert(ident.to_lowercase());
                }
            }
        } else {
            i += 1;
        }
    }

    keys
}
