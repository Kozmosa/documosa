// src/db/text.rs — pure text utilities, no SQL dependencies

pub(crate) fn split_lines_preserve_trailing(content: &str) -> Vec<&str> {
    if content.is_empty() {
        return vec![];
    }
    let mut parts: Vec<&str> = content.split('\n').collect();
    for line in &mut parts {
        if let Some(stripped) = line.strip_suffix('\r') {
            *line = stripped;
        }
    }
    if content.chars().all(|c| c == '\n' || c == '\r') {
        parts.pop();
    }
    parts
}

pub(crate) fn text_summary(value: &str) -> String {
    value.chars().take(120).collect()
}

pub(crate) fn text_len(value: &str) -> usize {
    value.chars().count()
}

pub(crate) fn line_count(value: &str) -> usize {
    split_lines_preserve_trailing(value).len()
}
