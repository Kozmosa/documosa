use reqwest::{Client, Method, StatusCode};
use serde::Serialize;
use serde_json::{Value, json};

const NOTION_VERSION: &str = "2026-03-11";
const RICH_TEXT_CHUNK_LIMIT: usize = 2000;

#[derive(Clone)]
pub struct NotionBackend {
    client: Client,
    token: String,
    base_url: String,
}

impl NotionBackend {
    pub fn new(token: String, base_url: String) -> Self {
        Self {
            client: Client::new(),
            token,
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    pub async fn list_documents(&self, parent_page_id: &str) -> anyhow::Result<Value> {
        let children = self.block_children(parent_page_id).await?;
        let documents: Vec<Value> = children
            .into_iter()
            .filter(|block| block["type"] == "child_page")
            .map(|block| {
                json!({
                    "id": string_field(&block, "id"),
                    "title": block["child_page"]["title"].as_str().unwrap_or("").to_string(),
                    "created_at": string_field(&block, "created_time"),
                    "updated_at": string_field(&block, "last_edited_time"),
                })
            })
            .collect();
        Ok(Value::Array(documents))
    }

    pub async fn create_document(
        &self,
        parent_page_id: &str,
        title: &str,
        content: &str,
    ) -> anyhow::Result<Value> {
        let page = self.create_page(parent_page_id, title).await?;
        let page_id = string_field(&page, "id");
        let lines: Vec<String> = content.lines().map(ToString::to_string).collect();
        self.append_paragraphs(&page_id, lines, None).await?;
        self.snapshot(&page_id).await
    }

    pub async fn get_document(&self, page_id: &str) -> anyhow::Result<Value> {
        self.snapshot(page_id).await
    }

    pub async fn export_document(&self, page_id: &str) -> anyhow::Result<String> {
        let lines = self.paragraph_lines(page_id).await?;
        Ok(lines
            .into_iter()
            .map(|line| line.content)
            .collect::<Vec<_>>()
            .join("\n"))
    }

    pub async fn insert_lines(
        &self,
        page_id: &str,
        after_line_id: Option<String>,
        lines: Vec<String>,
    ) -> anyhow::Result<Value> {
        if lines.is_empty() {
            anyhow::bail!("at least one line is required");
        }
        if let Some(anchor) = &after_line_id {
            let paragraphs = self.paragraph_lines(page_id).await?;
            ensure_line_belongs_to_page(&paragraphs, page_id, anchor)?;
        }
        let position = after_line_id
            .as_ref()
            .map(|anchor| json!({ "type": "after_block", "after_block": { "id": anchor } }))
            .unwrap_or_else(|| json!({ "type": "start" }));
        self.insert_paragraphs(page_id, lines, position).await?;
        self.snapshot(page_id).await
    }

    pub async fn replace_lines(
        &self,
        page_id: &str,
        line_ids: Vec<String>,
        lines: Vec<String>,
    ) -> anyhow::Result<Value> {
        if line_ids.is_empty() || line_ids.len() != lines.len() {
            anyhow::bail!("line_ids and content must be non-empty and equal length");
        }
        let paragraphs = self.paragraph_lines(page_id).await?;
        for line_id in &line_ids {
            ensure_line_belongs_to_page(&paragraphs, page_id, line_id)?;
        }
        for (line_id, content) in line_ids.iter().zip(lines.iter()) {
            self.request(
                Method::PATCH,
                &format!("/v1/blocks/{line_id}"),
                Some(json!({ "paragraph": { "rich_text": rich_text(content) } })),
            )
            .await?;
        }
        self.snapshot(page_id).await
    }

    pub async fn delete_lines(
        &self,
        page_id: &str,
        line_ids: Vec<String>,
    ) -> anyhow::Result<Value> {
        if line_ids.is_empty() {
            anyhow::bail!("at least one line id is required");
        }
        let paragraphs = self.paragraph_lines(page_id).await?;
        for line_id in &line_ids {
            ensure_line_belongs_to_page(&paragraphs, page_id, line_id)?;
        }
        for line_id in line_ids {
            self.request(
                Method::DELETE,
                &format!("/v1/blocks/{line_id}"),
                Option::<Value>::None,
            )
            .await?;
        }
        self.snapshot(page_id).await
    }

    async fn snapshot(&self, page_id: &str) -> anyhow::Result<Value> {
        let page = self
            .request(
                Method::GET,
                &format!("/v1/pages/{page_id}"),
                Option::<Value>::None,
            )
            .await?;
        let document = json!({
            "id": string_field(&page, "id"),
            "title": page_title(&page),
            "created_at": string_field(&page, "created_time"),
            "updated_at": string_field(&page, "last_edited_time"),
        });
        let lines = self
            .paragraph_lines(page_id)
            .await?
            .into_iter()
            .map(|line| line.to_json())
            .collect::<Vec<_>>();
        Ok(json!({
            "document": document,
            "lines": lines,
            "comments": [],
            "replies": [],
            "suggestions": [],
            "locks": [],
            "audit_events": [],
        }))
    }

    async fn create_page(&self, parent_page_id: &str, title: &str) -> anyhow::Result<Value> {
        self.request(
            Method::POST,
            "/v1/pages",
            Some(json!({
                "parent": { "type": "page_id", "page_id": parent_page_id },
                "properties": {
                    "title": {
                        "title": rich_text(title)
                    }
                }
            })),
        )
        .await
    }

    async fn append_paragraphs(
        &self,
        block_id: &str,
        lines: Vec<String>,
        position: Option<Value>,
    ) -> anyhow::Result<()> {
        for chunk in lines.chunks(100) {
            self.append_paragraph_chunk(block_id, chunk, position.as_ref())
                .await?;
        }
        Ok(())
    }

    async fn insert_paragraphs(
        &self,
        block_id: &str,
        lines: Vec<String>,
        position: Value,
    ) -> anyhow::Result<()> {
        if position["type"].as_str() == Some("start") {
            for chunk in lines.chunks(100).rev() {
                self.append_paragraph_chunk(block_id, chunk, Some(&position))
                    .await?;
            }
            return Ok(());
        }

        let mut position = position;
        for chunk in lines.chunks(100) {
            let response = self
                .append_paragraph_chunk(block_id, chunk, Some(&position))
                .await?;
            if let Some(last_id) = response["results"]
                .as_array()
                .and_then(|results| results.last())
                .and_then(|block| block["id"].as_str())
            {
                position = json!({ "type": "after_block", "after_block": { "id": last_id } });
            }
        }
        Ok(())
    }

    async fn append_paragraph_chunk(
        &self,
        block_id: &str,
        lines: &[String],
        position: Option<&Value>,
    ) -> anyhow::Result<Value> {
        let mut body = json!({
            "children": lines
                .iter()
                .map(|line| json!({
                    "type": "paragraph",
                    "paragraph": { "rich_text": rich_text(line) }
                }))
                .collect::<Vec<_>>()
        });
        if let Some(position) = position {
            body["position"] = position.clone();
        }
        self.request(
            Method::PATCH,
            &format!("/v1/blocks/{block_id}/children"),
            Some(body),
        )
        .await
    }

    async fn paragraph_lines(&self, page_id: &str) -> anyhow::Result<Vec<NotionLine>> {
        let blocks = self.block_children(page_id).await?;
        Ok(blocks
            .into_iter()
            .filter(|block| block["type"] == "paragraph")
            .enumerate()
            .map(|(index, block)| NotionLine {
                id: string_field(&block, "id"),
                document_id: page_id.to_string(),
                order_index: (index as i64 + 1) * 1000,
                content: rich_text_plain_text(&block["paragraph"]["rich_text"]),
                created_at: string_field(&block, "created_time"),
                updated_at: string_field(&block, "last_edited_time"),
            })
            .collect())
    }

    async fn block_children(&self, block_id: &str) -> anyhow::Result<Vec<Value>> {
        let mut cursor: Option<String> = None;
        let mut blocks = Vec::new();
        loop {
            let path = match &cursor {
                Some(cursor) => format!(
                    "/v1/blocks/{block_id}/children?page_size=100&start_cursor={}",
                    query_component(cursor)
                ),
                None => format!("/v1/blocks/{block_id}/children?page_size=100"),
            };
            let response = self
                .request(Method::GET, &path, Option::<Value>::None)
                .await?;
            blocks.extend(response["results"].as_array().cloned().unwrap_or_default());
            if !response["has_more"].as_bool().unwrap_or(false) {
                break;
            }
            cursor = response["next_cursor"].as_str().map(ToString::to_string);
            if cursor.is_none() {
                break;
            }
        }
        Ok(blocks)
    }

    async fn request<T: Serialize>(
        &self,
        method: Method,
        path: &str,
        body: Option<T>,
    ) -> anyhow::Result<Value> {
        let mut request = self
            .client
            .request(method, format!("{}{}", self.base_url, path))
            .bearer_auth(&self.token)
            .header("Notion-Version", NOTION_VERSION)
            .header(reqwest::header::CONTENT_TYPE, "application/json");
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await?;
        let status = response.status();
        let text = response.text().await?;
        if !status.is_success() {
            anyhow::bail!("Notion API {status}: {text}");
        }
        if status == StatusCode::NO_CONTENT || text.trim().is_empty() {
            return Ok(Value::Null);
        }
        Ok(serde_json::from_str(&text)?)
    }
}

struct NotionLine {
    id: String,
    document_id: String,
    order_index: i64,
    content: String,
    created_at: String,
    updated_at: String,
}

impl NotionLine {
    fn to_json(self) -> Value {
        json!({
            "id": self.id,
            "document_id": self.document_id,
            "order_index": self.order_index,
            "content": self.content,
            "revision": 1,
            "deleted": false,
            "created_at": self.created_at,
            "updated_at": self.updated_at,
        })
    }
}

fn ensure_line_belongs_to_page(
    lines: &[NotionLine],
    page_id: &str,
    line_id: &str,
) -> anyhow::Result<()> {
    if lines.iter().any(|line| line.id == line_id) {
        Ok(())
    } else {
        anyhow::bail!("line_id {line_id} is not a paragraph block in document {page_id}")
    }
}

fn rich_text(value: &str) -> Vec<Value> {
    if value.is_empty() {
        return Vec::new();
    }
    chunk_text(value, RICH_TEXT_CHUNK_LIMIT)
        .into_iter()
        .map(|content| {
            json!({
                "type": "text",
                "text": { "content": content },
            })
        })
        .collect()
}

fn rich_text_plain_text(value: &Value) -> String {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    item["plain_text"]
                        .as_str()
                        .or_else(|| item["text"]["content"].as_str())
                })
                .collect::<String>()
        })
        .unwrap_or_default()
}

fn page_title(page: &Value) -> String {
    if let Some(title) = page["child_page"]["title"].as_str() {
        return title.to_string();
    }
    if let Some(properties) = page["properties"].as_object() {
        for property in properties.values() {
            if property["type"] == "title" {
                return rich_text_plain_text(&property["title"]);
            }
        }
    }
    String::new()
}

fn string_field(value: &Value, field: &str) -> String {
    value[field].as_str().unwrap_or("").to_string()
}

fn chunk_text(value: &str, max_chars: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    for ch in value.chars() {
        if current.chars().count() == max_chars {
            chunks.push(current);
            current = String::new();
        }
        current.push(ch);
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

fn query_component(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(byte as char);
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}
