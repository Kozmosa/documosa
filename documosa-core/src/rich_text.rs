use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct RichTextToken {
    pub r#type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<TextContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Annotations>,
    pub plain_text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub href: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TextContent {
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Annotations {
    pub bold: bool,
    pub italic: bool,
    pub strikethrough: bool,
    pub underline: bool,
    pub code: bool,
    pub color: String,
}

impl Default for Annotations {
    fn default() -> Self {
        Self {
            bold: false,
            italic: false,
            strikethrough: false,
            underline: false,
            code: false,
            color: "default".into(),
        }
    }
}

impl Default for RichTextToken {
    fn default() -> Self {
        Self {
            r#type: "text".into(),
            text: Some(TextContent {
                content: String::new(),
                link: None,
            }),
            annotations: Some(Annotations::default()),
            plain_text: String::new(),
            href: None,
        }
    }
}
