use similar::TextDiff;

use crate::models::HistoryDiff;

pub fn format_history_unified_diff(diff: &HistoryDiff, context: usize) -> String {
    let from_label = format!("{} ({})", diff.from_event.created_at, diff.from_event.id);
    let to_label = format!("{} ({})", diff.to_event.created_at, diff.to_event.id);
    unified_diff(
        &from_label,
        &to_label,
        &diff.from_content,
        &diff.to_content,
        context,
    )
}

pub fn unified_diff(
    from_label: &str,
    to_label: &str,
    from_content: &str,
    to_content: &str,
    context: usize,
) -> String {
    TextDiff::from_lines(from_content, to_content)
        .unified_diff()
        .header(from_label, to_label)
        .context_radius(context)
        .to_string()
}
