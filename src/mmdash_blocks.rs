use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum BlockType {
    #[serde(rename = "heading_1")]
    Heading1,
    #[serde(rename = "heading_2")]
    Heading2,
    #[serde(rename = "heading_3")]
    Heading3,
    #[serde(rename = "code")]
    Code,
    #[serde(rename = "equation")]
    Equation,
    #[serde(rename = "bulleted_list_item")]
    BulletedListItem,
    #[serde(rename = "numbered_list_item")]
    NumberedListItem,
    #[serde(rename = "quote")]
    Quote,
    #[serde(rename = "divider")]
    Divider,
    #[serde(rename = "paragraph")]
    Paragraph,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Block {
    #[serde(rename = "type")]
    pub block_type: BlockType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
}

impl Block {
    pub fn heading_1(content: impl Into<String>) -> Self {
        Self {
            block_type: BlockType::Heading1,
            content: Some(content.into()),
            language: None,
        }
    }

    pub fn heading_2(content: impl Into<String>) -> Self {
        Self {
            block_type: BlockType::Heading2,
            content: Some(content.into()),
            language: None,
        }
    }

    pub fn heading_3(content: impl Into<String>) -> Self {
        Self {
            block_type: BlockType::Heading3,
            content: Some(content.into()),
            language: None,
        }
    }

    pub fn code(content: impl Into<String>, language: impl Into<String>) -> Self {
        Self {
            block_type: BlockType::Code,
            content: Some(content.into()),
            language: Some(language.into()),
        }
    }

    pub fn equation(content: impl Into<String>) -> Self {
        Self {
            block_type: BlockType::Equation,
            content: Some(content.into()),
            language: None,
        }
    }

    pub fn bulleted_list_item(content: impl Into<String>) -> Self {
        Self {
            block_type: BlockType::BulletedListItem,
            content: Some(content.into()),
            language: None,
        }
    }

    pub fn numbered_list_item(content: impl Into<String>) -> Self {
        Self {
            block_type: BlockType::NumberedListItem,
            content: Some(content.into()),
            language: None,
        }
    }

    pub fn quote(content: impl Into<String>) -> Self {
        Self {
            block_type: BlockType::Quote,
            content: Some(content.into()),
            language: None,
        }
    }

    pub fn divider() -> Self {
        Self {
            block_type: BlockType::Divider,
            content: None,
            language: None,
        }
    }

    pub fn paragraph(content: impl Into<String>) -> Self {
        Self {
            block_type: BlockType::Paragraph,
            content: Some(content.into()),
            language: None,
        }
    }
}

pub fn markdown_to_blocks(markdown: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let lines: Vec<&str> = markdown.split('\n').collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];

        if line.is_empty() {
            i += 1;
            continue;
        }

        // Code block: ```lang
        if line.starts_with("```") {
            let lang = line.strip_prefix("```").unwrap_or("").trim().to_string();
            i += 1;
            let mut code_lines = Vec::new();
            while i < lines.len() && !lines[i].starts_with("```") {
                code_lines.push(lines[i]);
                i += 1;
            }
            if i < lines.len() {
                i += 1; // skip closing ```
            }
            blocks.push(Block::code(code_lines.join("\n"), lang));
            continue;
        }

        // Equation: $$ ... $$ (single line or multi-line)
        if line.starts_with("$$") {
            // Single-line: $$ content $$
            if line.len() > 4 && line.ends_with("$$") {
                let content = line[2..line.len() - 2].trim().to_string();
                blocks.push(Block::equation(content));
                i += 1;
                continue;
            }
            // Multi-line: $$\n...\n$$
            i += 1;
            let mut eq_lines = Vec::new();
            while i < lines.len() && !lines[i].starts_with("$$") {
                eq_lines.push(lines[i]);
                i += 1;
            }
            if i < lines.len() {
                i += 1; // skip closing $$
            }
            blocks.push(Block::equation(eq_lines.join("\n")));
            continue;
        }

        // Heading 1
        if let Some(content) = line.strip_prefix("# ") {
            blocks.push(Block::heading_1(content.trim()));
            i += 1;
            continue;
        }

        // Heading 2
        if let Some(content) = line.strip_prefix("## ") {
            blocks.push(Block::heading_2(content.trim()));
            i += 1;
            continue;
        }

        // Heading 3
        if let Some(content) = line.strip_prefix("### ") {
            blocks.push(Block::heading_3(content.trim()));
            i += 1;
            continue;
        }

        // Divider
        if line == "---" || line == "***" {
            blocks.push(Block::divider());
            i += 1;
            continue;
        }

        // Bulleted list
        if let Some(content) = line.strip_prefix("- ") {
            blocks.push(Block::bulleted_list_item(content.trim()));
            i += 1;
            continue;
        }

        // Numbered list
        if let Some(rest) = line.strip_prefix("1. ") {
            blocks.push(Block::numbered_list_item(rest.trim()));
            i += 1;
            continue;
        }

        // Quote
        if let Some(content) = line.strip_prefix("> ") {
            blocks.push(Block::quote(content.trim()));
            i += 1;
            continue;
        }

        // Paragraph (default)
        blocks.push(Block::paragraph(line.trim()));
        i += 1;
    }

    blocks
}

pub fn blocks_to_markdown(blocks: &[Block]) -> String {
    let mut lines = Vec::new();

    for block in blocks {
        match block.block_type {
            BlockType::Heading1 => {
                if let Some(content) = &block.content {
                    lines.push(format!("# {}", content));
                }
            }
            BlockType::Heading2 => {
                if let Some(content) = &block.content {
                    lines.push(format!("## {}", content));
                }
            }
            BlockType::Heading3 => {
                if let Some(content) = &block.content {
                    lines.push(format!("### {}", content));
                }
            }
            BlockType::Code => {
                let lang = block.language.as_deref().unwrap_or("");
                lines.push(format!("```{}", lang));
                if let Some(content) = &block.content {
                    lines.push(content.clone());
                }
                lines.push("```".to_string());
            }
            BlockType::Equation => {
                if let Some(content) = &block.content {
                    if content.contains('\n') {
                        lines.push("$$".to_string());
                        lines.push(content.clone());
                        lines.push("$$".to_string());
                    } else {
                        lines.push(format!("$$ {} $$", content));
                    }
                }
            }
            BlockType::BulletedListItem => {
                if let Some(content) = &block.content {
                    lines.push(format!("- {}", content));
                }
            }
            BlockType::NumberedListItem => {
                if let Some(content) = &block.content {
                    lines.push(format!("1. {}", content));
                }
            }
            BlockType::Quote => {
                if let Some(content) = &block.content {
                    lines.push(format!("> {}", content));
                }
            }
            BlockType::Divider => {
                lines.push("---".to_string());
            }
            BlockType::Paragraph => {
                if let Some(content) = &block.content {
                    lines.push(content.clone());
                }
            }
        }
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heading_1() {
        let markdown = "# Hello World";
        let blocks = markdown_to_blocks(markdown);
        assert_eq!(blocks, vec![Block::heading_1("Hello World")]);
    }

    #[test]
    fn test_heading_2() {
        let markdown = "## Subheading";
        let blocks = markdown_to_blocks(markdown);
        assert_eq!(blocks, vec![Block::heading_2("Subheading")]);
    }

    #[test]
    fn test_heading_3() {
        let markdown = "### Detail";
        let blocks = markdown_to_blocks(markdown);
        assert_eq!(blocks, vec![Block::heading_3("Detail")]);
    }

    #[test]
    fn test_code_block() {
        let markdown = "```python\nimport numpy as np\nprint(1)\n```";
        let blocks = markdown_to_blocks(markdown);
        assert_eq!(
            blocks,
            vec![Block::code("import numpy as np\nprint(1)", "python")]
        );
    }

    #[test]
    fn test_code_block_no_lang() {
        let markdown = "```\nhello\nworld\n```";
        let blocks = markdown_to_blocks(markdown);
        assert_eq!(blocks, vec![Block::code("hello\nworld", "")]);
    }

    #[test]
    fn test_equation_single_line() {
        let markdown = "$$ E = mc^2 $$";
        let blocks = markdown_to_blocks(markdown);
        assert_eq!(blocks, vec![Block::equation("E = mc^2")]);
    }

    #[test]
    fn test_equation_multi_line() {
        let markdown = "$$\na + b = c\nx^2 + y^2 = z^2\n$$";
        let blocks = markdown_to_blocks(markdown);
        assert_eq!(
            blocks,
            vec![Block::equation("a + b = c\nx^2 + y^2 = z^2")]
        );
    }

    #[test]
    fn test_bulleted_list() {
        let markdown = "- Item 1";
        let blocks = markdown_to_blocks(markdown);
        assert_eq!(blocks, vec![Block::bulleted_list_item("Item 1")]);
    }

    #[test]
    fn test_numbered_list() {
        let markdown = "1. First item";
        let blocks = markdown_to_blocks(markdown);
        assert_eq!(blocks, vec![Block::numbered_list_item("First item")]);
    }

    #[test]
    fn test_quote() {
        let markdown = "> A quote";
        let blocks = markdown_to_blocks(markdown);
        assert_eq!(blocks, vec![Block::quote("A quote")]);
    }

    #[test]
    fn test_divider() {
        let blocks_dash = markdown_to_blocks("---");
        let blocks_star = markdown_to_blocks("***");
        assert_eq!(blocks_dash, vec![Block::divider()]);
        assert_eq!(blocks_star, vec![Block::divider()]);
    }

    #[test]
    fn test_paragraph() {
        let markdown = "Plain text paragraph.";
        let blocks = markdown_to_blocks(markdown);
        assert_eq!(blocks, vec![Block::paragraph("Plain text paragraph.")]);
    }

    #[test]
    fn test_full_document() {
        let markdown = r#"# Heading Text

## Subheading

Plain text paragraph.
```python
import numpy as np
```
$$
a + b = c
d + e = f
$$
- List item 1
1. Numbered item 1

> A quote.

---"#;

        let blocks = markdown_to_blocks(markdown);
        assert_eq!(blocks.len(), 9);
        assert_eq!(blocks[0], Block::heading_1("Heading Text"));
        assert_eq!(blocks[1], Block::heading_2("Subheading"));
        assert_eq!(blocks[2], Block::paragraph("Plain text paragraph."));
        assert_eq!(blocks[3], Block::code("import numpy as np", "python"));
        assert_eq!(blocks[4], Block::equation("a + b = c\nd + e = f"));
        assert_eq!(blocks[5], Block::bulleted_list_item("List item 1"));
        assert_eq!(blocks[6], Block::numbered_list_item("Numbered item 1"));
        assert_eq!(blocks[7], Block::quote("A quote."));
        assert_eq!(blocks[8], Block::divider());
    }

    #[test]
    fn test_blocks_to_markdown_heading() {
        let blocks = vec![
            Block::heading_1("Title"),
            Block::heading_2("Subtitle"),
            Block::heading_3("Detail"),
        ];
        let md = blocks_to_markdown(&blocks);
        assert_eq!(md, "# Title\n## Subtitle\n### Detail");
    }

    #[test]
    fn test_blocks_to_markdown_code() {
        let blocks = vec![Block::code("print(1)", "python")];
        let md = blocks_to_markdown(&blocks);
        assert_eq!(md, "```python\nprint(1)\n```");
    }

    #[test]
    fn test_blocks_to_markdown_equation_single_line() {
        let blocks = vec![Block::equation("E = mc^2")];
        let md = blocks_to_markdown(&blocks);
        assert_eq!(md, "$$ E = mc^2 $$");
    }

    #[test]
    fn test_blocks_to_markdown_equation_multi_line() {
        let blocks = vec![Block::equation("a + b = c\nx^2 + y^2 = z^2")];
        let md = blocks_to_markdown(&blocks);
        assert_eq!(md, "$$\na + b = c\nx^2 + y^2 = z^2\n$$");
    }

    #[test]
    fn test_round_trip() {
        let original = r#"# Title

Paragraph text.
```rust
fn main() {}
```
$$ x^2 $$
$$
a + b = c
d + e = f
$$
- Bullet
1. Number
> Quote
---"#;
        let blocks = markdown_to_blocks(original);
        let reconstructed = blocks_to_markdown(&blocks);
        let blocks2 = markdown_to_blocks(&reconstructed);
        assert_eq!(blocks, blocks2);
    }

    #[test]
    fn test_multi_line_equation_in_document() {
        let markdown = "# Math\n\n$$\na + b = c\nd + e = f\n$$\n\nParagraph.";
        let blocks = markdown_to_blocks(markdown);
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[0], Block::heading_1("Math"));
        assert_eq!(blocks[1], Block::equation("a + b = c\nd + e = f"));
        assert_eq!(blocks[2], Block::paragraph("Paragraph."));
    }
}
