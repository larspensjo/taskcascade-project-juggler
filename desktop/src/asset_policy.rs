/// Check the built entry point for sources forbidden by the desktop CSP.
pub fn violations(index_html: &str) -> Vec<String> {
    let html = index_html.to_ascii_lowercase();
    let mut result = Vec::new();
    if html.contains("data:") {
        result.push("data: URI found; use served assets (Vite assetsInlineLimit: 0)".into());
    }
    let mut remaining = html.as_str();
    while let Some(start) = remaining.find("<script") {
        remaining = &remaining[start + "<script".len()..];
        if !remaining.starts_with(|c: char| c.is_ascii_whitespace() || c == '>' || c == '/') {
            continue;
        }
        // A quoted '>' belongs to the attribute, not the end of the tag.
        let mut quote = None;
        let end = remaining.char_indices().find_map(|(index, c)| {
            match (quote, c) {
                (Some(q), c) if q == c => quote = None,
                (None, '\'' | '"') => quote = Some(c),
                (None, '>') => return Some(index),
                _ => {}
            }
            None
        });
        let Some(end) = end else {
            result.push("unterminated script tag".into());
            break;
        };
        if !has_source(&remaining[..end]) {
            result.push("inline script found; scripts must have a non-empty src attribute".into());
        }
        remaining = &remaining[end + 1..];
    }
    result
}

fn has_source(mut attributes: &str) -> bool {
    while !attributes.is_empty() {
        attributes = attributes.trim_start_matches(|c: char| c.is_ascii_whitespace() || c == '/');
        let end = attributes
            .find(|c: char| c.is_ascii_whitespace() || c == '=')
            .unwrap_or(attributes.len());
        let name = &attributes[..end];
        attributes = attributes[end..].trim_start();
        let Some(rest) = attributes.strip_prefix('=') else {
            continue;
        };
        attributes = rest.trim_start();
        let value;
        if let Some(quote @ ('\'' | '"')) = attributes.chars().next() {
            attributes = &attributes[1..];
            let end = attributes.find(quote).unwrap_or(attributes.len());
            value = &attributes[..end];
            attributes = &attributes[(end + 1).min(attributes.len())..];
        } else {
            let end = attributes
                .find(char::is_whitespace)
                .unwrap_or(attributes.len());
            value = &attributes[..end];
            attributes = &attributes[end..];
        }
        if name == "src" && !value.trim().is_empty() {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_inline_scripts_and_data_uris() {
        assert!(!violations("<script>inline()</script>").is_empty());
        assert!(!violations("<link href=\"data:image/svg+xml,...\">").is_empty());
        assert!(!violations("<SCRIPT type='module'>inline()</SCRIPT>").is_empty());
        assert!(!violations("<script data-src='external.js'>inline()</script>").is_empty());
        assert!(!violations("<script src=''>inline()</script>").is_empty());
    }

    #[test]
    fn accepts_external_module_script() {
        assert!(violations(
            r#"<script type="module" crossorigin src="/assets/index-abc.js"></script>"#
        )
        .is_empty());
    }
}
