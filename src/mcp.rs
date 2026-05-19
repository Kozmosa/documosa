use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::AppState;
use crate::db;
use crate::diff;
use crate::error::{AppError, Result};
use crate::models::{HistoryCategory, HistoryListOptions, Identity, RoleMode};
use crate::mmdash_blocks::{self, BlockType};

pub fn router() -> Router<AppState> {
    Router::new().route("/mcp", post(handle))
}

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    #[serde(default)]
    id: Value,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: &'static str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<Value>,
}

async fn handle(
    State(state): State<AppState>,
    Json(request): Json<JsonRpcRequest>,
) -> Json<JsonRpcResponse> {
    let id = request.id.clone();
    let result = dispatch(&state, request).await;
    match result {
        Ok(result) => Json(JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }),
        Err(error) => Json(JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(json!({ "code": -32000, "message": error.to_string() })),
        }),
    }
}

async fn dispatch(state: &AppState, request: JsonRpcRequest) -> Result<Value> {
    match request.method.as_str() {
        "initialize" => Ok(json!({
            "protocolVersion": "2025-03-26",
            "serverInfo": { "name": "documosa", "version": env!("CARGO_PKG_VERSION") },
            "capabilities": { "tools": {} }
        })),
        "tools/list" => Ok(json!({
            "tools": [
                tool("list_pages", "List all pages", object_schema(&[], &[])),
                tool("page_create", "Create a new page", object_schema(&["title"], &[
                    ("title", string_schema()),
                    ("content", string_schema()),
                    ("content_format", enum_schema(&["rich_text", "markdown"])),
                ])),
                tool("page_get", "Get a page snapshot", object_schema(&["page_id"], &[
                    ("page_id", string_schema()),
                ])),
                tool("page_export", "Export a page as markdown", object_schema(&["page_id"], &[
                    ("page_id", string_schema()),
                ])),
                tool("block_get", "Get a block", object_schema(&["block_id"], &[
                    ("block_id", string_schema()),
                ])),
                tool("block_list_children", "List child blocks of a page or parent block", object_schema(&["page_id"], &[
                    ("page_id", string_schema()),
                    ("parent_id", string_schema()),
                    ("cursor", integer_schema()),
                    ("page_size", integer_schema()),
                ])),
                tool("block_append", "Append blocks to a page", object_schema(&["page_id"], &[
                    ("page_id", string_schema()),
                    ("blocks", string_schema()),
                    ("content", string_schema()),
                    ("after", string_schema()),
                    ("content_format", enum_schema(&["rich_text", "markdown"])),
                ])),
                tool("block_update", "Update a block", object_schema(&["block_id"], &[
                    ("block_id", string_schema()),
                    ("block_type", string_schema()),
                    ("content", string_schema()),
                    ("properties_json", string_schema()),
                    ("content_format", enum_schema(&["rich_text", "markdown"])),
                ])),
                tool("block_delete", "Delete a block", object_schema(&["block_id"], &[
                    ("block_id", string_schema()),
                ])),
                tool("comment_create", "Create a comment on a block", object_schema(&["block_id", "body"], &[
                    ("block_id", string_schema()),
                    ("body", string_schema()),
                    ("start_column", integer_schema()),
                    ("end_column", integer_schema()),
                ])),
                tool("reply_comment", "Reply to a comment", object_schema(&["block_id", "comment_id", "body"], &[
                    ("block_id", string_schema()),
                    ("comment_id", string_schema()),
                    ("body", string_schema()),
                ])),
                tool("resolve_comment", "Resolve a comment", object_schema(&["block_id", "comment_id"], &[
                    ("block_id", string_schema()),
                    ("comment_id", string_schema()),
                ])),
                tool("suggestion_create", "Create a suggestion on a page", object_schema(&["page_id", "kind"], &[
                    ("page_id", string_schema()),
                    ("kind", enum_schema(&["insert", "replace", "delete"])),
                    ("target_block_id", string_schema()),
                    ("parent_id", string_schema()),
                    ("content", string_array_schema()),
                ])),
                tool("suggestion_accept", "Accept a suggestion", object_schema(&["page_id", "suggestion_id"], &[
                    ("page_id", string_schema()),
                    ("suggestion_id", string_schema()),
                ])),
                tool("suggestion_reject", "Reject a suggestion", object_schema(&["page_id", "suggestion_id"], &[
                    ("page_id", string_schema()),
                    ("suggestion_id", string_schema()),
                ])),
                tool("history_list", "List history events for a page", object_schema(&["page_id"], &[
                    ("page_id", string_schema()),
                    ("category", enum_schema(&["document-comment", "all", "content", "comment", "suggestion", "system"])),
                    ("from", string_schema()),
                    ("to", string_schema()),
                    ("limit", integer_schema()),
                ])),
                tool("history_diff", "Diff two history events", object_schema(&["page_id", "from", "to"], &[
                    ("page_id", string_schema()),
                    ("from", string_schema()),
                    ("to", string_schema()),
                    ("context", integer_schema()),
                ])),
                tool("set_audit_note", "Set a shared audit event note", object_schema(&["page_id", "audit_event_id", "body"], &[
                    ("page_id", string_schema()),
                    ("audit_event_id", string_schema()),
                    ("body", string_schema()),
                ])),
                tool("clear_audit_note", "Clear a shared audit event note", object_schema(&["page_id", "audit_event_id"], &[
                    ("page_id", string_schema()),
                    ("audit_event_id", string_schema()),
                ])),
            ]
        })),
        "tools/call" => {
            let name = request
                .params
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| AppError::BadRequest("tools/call requires name".into()))?;
            let args = request
                .params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            call_tool(state, name, args).await
        }
        _ => Err(AppError::BadRequest(format!(
            "unsupported MCP method {}",
            request.method
        ))),
    }
}

fn tool(name: &str, description: &str, input_schema: Value) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": input_schema
    })
}

async fn call_tool(state: &AppState, name: &str, args: Value) -> Result<Value> {
    let actor = actor_from_args(&args)?;
    let (value, text) = match name {
        "list_pages" => (json!(db::list_pages(&state.pool).await?), None),
        "page_create" => {
            let title = string_arg(&args, "title")?;
            let content = args
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let content_format = args
                .get("content_format")
                .and_then(Value::as_str)
                .unwrap_or("rich_text");

            let blocks_json = if content_format == "markdown" {
                let inputs = markdown_to_block_inputs(&content)?;
                serde_json::to_string(&inputs)?
            } else {
                content
            };

            let title_rt = json!([{"type":"text","text":{"content":&title},"plain_text":&title}]);
            let title_json = serde_json::to_string(&title_rt).unwrap_or_default();
            let snap = db::create_page(&state.pool, &actor, title_json, blocks_json).await?;
            state.hub.page_created(&snap.page.id);
            (json!(snap), None)
        }
        "page_get" => (
            json!(db::snapshot(&state.pool, &string_arg(&args, "page_id")?).await?),
            None,
        ),
        "page_export" => {
            let md =
                db::export_markdown(&state.pool, &string_arg(&args, "page_id")?).await?;
            (json!({ "markdown": md }), Some(md))
        }
        "block_get" => (
            json!(db::get_block(&state.pool, &string_arg(&args, "block_id")?).await?),
            None,
        ),
        "block_list_children" => {
            let page_id = string_arg(&args, "page_id")?;
            let parent_id = args.get("parent_id").and_then(Value::as_str);
            let cursor = args.get("cursor").and_then(Value::as_f64);
            let page_size = optional_i64_arg(&args, "page_size")?
                .unwrap_or(50)
                .max(1)
                .min(100);
            let (blocks, next_cursor, has_more) = db::list_children(
                &state.pool,
                parent_id,
                &page_id,
                cursor,
                page_size,
            )
            .await?;
            (
                json!({
                    "results": blocks,
                    "next_cursor": next_cursor,
                    "has_more": has_more
                }),
                None,
            )
        }
        "block_append" => {
            let page_id = string_arg(&args, "page_id")?;
            let after = args.get("after").and_then(Value::as_str);
            let content_format = args
                .get("content_format")
                .and_then(Value::as_str)
                .unwrap_or("rich_text");

            let block_inputs = if content_format == "markdown" {
                let content = string_arg(&args, "content")?;
                markdown_to_block_inputs(&content)?
            } else {
                let blocks_str = string_arg(&args, "blocks")?;
                serde_json::from_str(&blocks_str)?
            };

            let blocks = db::append_blocks(
                &state.pool,
                &actor,
                &page_id,
                block_inputs,
                after,
            )
            .await?;
            let ids: Vec<String> = blocks.iter().map(|b| b.id.clone()).collect();
            state.hub.block_inserted(&page_id, &ids, after);
            let snap = db::snapshot(&state.pool, &page_id).await?;
            (json!(snap), None)
        }
        "block_update" => {
            let block_id = string_arg(&args, "block_id")?;
            let block_type = args.get("block_type").and_then(Value::as_str);
            let properties_json = args.get("properties_json").and_then(Value::as_str);
            let content_format = args
                .get("content_format")
                .and_then(Value::as_str)
                .unwrap_or("rich_text");

            let content_json = match (args.get("content").and_then(Value::as_str), content_format) {
                (Some(content), "markdown") => {
                    Some(serde_json::to_string(&[serde_json::json!({
                        "type": "text",
                        "text": { "content": content },
                        "plain_text": content,
                    })])?)
                }
                (Some(content), _) => Some(content.to_string()),
                (None, _) => None,
            };

            let block = db::update_block(
                &state.pool,
                &actor,
                &block_id,
                block_type,
                content_json.as_deref(),
                properties_json,
            )
            .await?;
            state.hub.block_updated(&block.page_id, &block_id);
            let snap = db::snapshot(&state.pool, &block.page_id).await?;
            (json!(snap), None)
        }
        "block_delete" => {
            let block_id = string_arg(&args, "block_id")?;
            let block =
                db::delete_block(&state.pool, &actor, &block_id).await?;
            state.hub.block_deleted(&block.page_id, &[block_id]);
            let snap = db::snapshot(&state.pool, &block.page_id).await?;
            (json!(snap), None)
        }
        "comment_create" => {
            let block_id = string_arg(&args, "block_id")?;
            let page_id = db::get_block(&state.pool, &block_id).await?.page_id;
            let snap = db::create_comment(
                &state.pool,
                &actor,
                &page_id,
                db::CommentDraft {
                    target_block_id: block_id,
                    start_column: optional_i64_arg(&args, "start_column")?,
                    end_column: optional_i64_arg(&args, "end_column")?,
                    body: string_arg(&args, "body")?,
                },
            )
            .await?;
            state
                .hub
                .comment_created(&page_id, snap.comments.last().unwrap().id.as_str());
            (json!(snap), None)
        }
        "reply_comment" => {
            let block_id = string_arg(&args, "block_id")?;
            let page_id = db::get_block(&state.pool, &block_id).await?.page_id;
            let snap = db::reply_comment(
                &state.pool,
                &actor,
                &page_id,
                &string_arg(&args, "comment_id")?,
                string_arg(&args, "body")?,
            )
            .await?;
            state.hub.content_changed(&page_id);
            (json!(snap), None)
        }
        "resolve_comment" => {
            let block_id = string_arg(&args, "block_id")?;
            let page_id = db::get_block(&state.pool, &block_id).await?.page_id;
            let comment_id = string_arg(&args, "comment_id")?;
            let snap = db::resolve_comment(&state.pool, &actor, &page_id, &comment_id).await?;
            state.hub.comment_resolved(&page_id, &comment_id);
            (json!(snap), None)
        }
        "suggestion_create" => {
            let page_id = string_arg(&args, "page_id")?;
            let target_block_id = args
                .get("target_block_id")
                .and_then(Value::as_str)
                .map(str::to_string);
            let content = args
                .get("content")
                .map(|_| string_vec_arg(&args, "content"))
                .transpose()?
                .unwrap_or_default();
            let snap = db::create_suggestion(
                &state.pool,
                &actor,
                &page_id,
                string_arg(&args, "kind")?,
                target_block_id,
                content,
            )
            .await?;
            state
                .hub
                .suggestion_created(&page_id, snap.suggestions.last().unwrap().id.as_str());
            (json!(snap), None)
        }
        "suggestion_accept" | "suggestion_reject" => {
            let page_id = string_arg(&args, "page_id")?;
            let accept = name == "suggestion_accept";
            let snap = db::decide_suggestion(
                &state.pool,
                &actor,
                &page_id,
                &string_arg(&args, "suggestion_id")?,
                accept,
            )
            .await?;
            state
                .hub
                .suggestion_decided(&page_id, &string_arg(&args, "suggestion_id")?, accept);
            (json!(snap), None)
        }
        "history_list" => {
            let page_id = string_arg(&args, "page_id")?;
            (
                json!(
                    db::list_history_events(
                        &state.pool,
                        &page_id,
                        history_options_from_args(&args)?
                    )
                    .await?
                ),
                None,
            )
        }
        "history_diff" => {
            let page_id = string_arg(&args, "page_id")?;
            let history_diff = db::history_diff(
                &state.pool,
                &page_id,
                &string_arg(&args, "from")?,
                &string_arg(&args, "to")?,
            )
            .await?;
            let context = optional_i64_arg(&args, "context")?.unwrap_or(3).max(0) as usize;
            let unified_diff = diff::format_history_unified_diff(&history_diff, context);
            let mut value = serde_json::to_value(&history_diff)?;
            if let Some(object) = value.as_object_mut() {
                object.insert("unified_diff".to_string(), json!(unified_diff));
            }
            (value, Some(unified_diff))
        }
        "set_audit_note" => {
            let page_id = string_arg(&args, "page_id")?;
            let snap = db::put_audit_event_note(
                &state.pool,
                &actor,
                &page_id,
                &string_arg(&args, "audit_event_id")?,
                string_arg(&args, "body")?,
            )
            .await?;
            state.hub.content_changed(&page_id);
            (json!(snap), None)
        }
        "clear_audit_note" => {
            let page_id = string_arg(&args, "page_id")?;
            let snap = db::put_audit_event_note(
                &state.pool,
                &actor,
                &page_id,
                &string_arg(&args, "audit_event_id")?,
                String::new(),
            )
            .await?;
            state.hub.content_changed(&page_id);
            (json!(snap), None)
        }
        _ => return Err(AppError::BadRequest(format!("unknown tool {name}"))),
    };
    tool_result(value, text)
}

fn tool_result(value: Value, text: Option<String>) -> Result<Value> {
    let text = match text {
        Some(text) => text,
        None => serde_json::to_string_pretty(&value)?,
    };
    Ok(json!({
        "content": [{ "type": "text", "text": text }],
        "structuredContent": value
    }))
}

fn object_schema(required: &[&str], properties: &[(&str, Value)]) -> Value {
    let mut property_map = Map::new();
    property_map.insert("client_id".to_string(), string_schema());
    property_map.insert("nickname".to_string(), string_schema());
    property_map.insert(
        "role_mode".to_string(),
        enum_schema(&["reviewer", "writer"]),
    );
    for (name, schema) in properties {
        property_map.insert((*name).to_string(), schema.clone());
    }
    json!({
        "type": "object",
        "properties": property_map,
        "required": required,
        "additionalProperties": false
    })
}

fn string_schema() -> Value {
    json!({ "type": "string" })
}

fn integer_schema() -> Value {
    json!({ "type": "integer" })
}

fn string_array_schema() -> Value {
    json!({ "type": "array", "items": { "type": "string" } })
}

fn enum_schema(values: &[&str]) -> Value {
    json!({ "type": "string", "enum": values })
}

fn actor_from_args(args: &Value) -> Result<Identity> {
    let actor_kind = match args
        .get("actor_kind")
        .and_then(Value::as_str)
        .unwrap_or("human")
    {
        "agent" => {
            let agent_id = args
                .get("agent_id")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string();
            let session_ref = args
                .get("session_ref")
                .and_then(Value::as_str)
                .map(str::to_string);
            let task_ref = args
                .get("task_ref")
                .and_then(Value::as_str)
                .map(str::to_string);
            documosa_core::identity::ActorKind::Agent {
                agent_id,
                session_ref,
                task_ref,
            }
        }
        _ => documosa_core::identity::ActorKind::Human,
    };
    Ok(Identity {
        client_id: args
            .get("client_id")
            .and_then(Value::as_str)
            .unwrap_or("mcp-client")
            .to_string(),
        nickname: args
            .get("nickname")
            .and_then(Value::as_str)
            .unwrap_or("MCP")
            .to_string(),
        role_mode: RoleMode::parse(
            args.get("role_mode")
                .and_then(Value::as_str)
                .unwrap_or("reviewer"),
        )?,
        actor_kind,
    })
}

fn string_arg(args: &Value, key: &str) -> Result<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| AppError::BadRequest(format!("missing string argument {key}")))
}

fn string_vec_arg(args: &Value, key: &str) -> Result<Vec<String>> {
    args.get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .ok_or_else(|| AppError::BadRequest(format!("missing string array argument {key}")))
}

fn optional_i64_arg(args: &Value, key: &str) -> Result<Option<i64>> {
    match args.get(key) {
        Some(value) => value
            .as_i64()
            .map(Some)
            .ok_or_else(|| AppError::BadRequest(format!("{key} must be an integer"))),
        None => Ok(None),
    }
}

fn history_options_from_args(args: &Value) -> Result<HistoryListOptions> {
    Ok(HistoryListOptions {
        category: HistoryCategory::parse(
            args.get("category")
                .and_then(Value::as_str)
                .unwrap_or("document-comment"),
        )?,
        from: args
            .get("from")
            .and_then(Value::as_str)
            .map(|value| normalize_history_timestamp("from", value))
            .transpose()?,
        to: args
            .get("to")
            .and_then(Value::as_str)
            .map(|value| normalize_history_timestamp("to", value))
            .transpose()?,
        limit: optional_i64_arg(args, "limit")?.unwrap_or(db::HISTORY_DEFAULT_LIMIT),
    })
}

fn normalize_history_timestamp(name: &str, value: &str) -> Result<String> {
    DateTime::parse_from_rfc3339(value)
        .map(|parsed| parsed.with_timezone(&Utc).to_rfc3339())
        .map_err(|_| AppError::BadRequest(format!("{name} must be an RFC3339 timestamp")))
}

fn markdown_to_block_inputs(markdown: &str) -> Result<Vec<db::BlockInput>> {
    let mmdash_blocks = mmdash_blocks::markdown_to_blocks(markdown);
    mmdash_blocks
        .into_iter()
        .map(|b| {
            let block_type = match b.block_type {
                BlockType::Heading1 => "heading_1",
                BlockType::Heading2 => "heading_2",
                BlockType::Heading3 => "heading_3",
                BlockType::Code => "code",
                BlockType::Equation => "equation",
                BlockType::BulletedListItem => "bulleted_list_item",
                BlockType::NumberedListItem => "numbered_list_item",
                BlockType::Quote => "quote",
                BlockType::Divider => "divider",
                BlockType::Paragraph => "paragraph",
            }
            .to_string();

            let content = b.content.unwrap_or_default();
            let content_json = if matches!(b.block_type, BlockType::Divider) {
                "[]".to_string()
            } else {
                serde_json::to_string(&[serde_json::json!({
                    "type": "text",
                    "text": { "content": content },
                    "plain_text": content,
                })])?
            };

            let properties_json = match b.block_type {
                BlockType::Code => {
                    Some(serde_json::json!({ "language": b.language.unwrap_or_default() }).to_string())
                }
                _ => None,
            };

            Ok(db::BlockInput {
                block_type,
                content_json,
                properties_json,
            })
        })
        .collect()
}
