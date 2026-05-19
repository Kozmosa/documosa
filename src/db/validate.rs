use documosa_core::rich_text::RichTextToken;
use crate::error::{AppError, Result};

const VALID_BLOCK_TYPES: &[&str] = &[
    "paragraph", "heading_1", "heading_2", "heading_3",
    "bulleted_list_item", "numbered_list_item", "to_do", "toggle",
    "code", "equation", "quote", "callout", "divider",
    "image", "table", "table_row", "column_list", "column",
    "child_page",
];

pub(crate) fn validate_block_input(input: &super::block::BlockInput) -> Result<()> {
    if !VALID_BLOCK_TYPES.contains(&input.block_type.as_str()) {
        return Err(AppError::BadRequest(format!(
            "invalid block_type: '{}'. Must be one of: {}",
            input.block_type,
            VALID_BLOCK_TYPES.join(", "),
        )));
    }
    let tokens: Vec<RichTextToken> = serde_json::from_str(&input.content_json)
        .map_err(|e| AppError::BadRequest(format!("invalid content_json: not valid RichText array: {e}")))?;
    if tokens.is_empty() && !matches!(input.block_type.as_str(), "divider" | "table" | "column_list" | "image" | "child_page") {
        return Err(AppError::BadRequest("content_json must be a non-empty rich text array for this block type".into()));
    }
    Ok(())
}
