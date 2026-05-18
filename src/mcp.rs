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
                tool("list_documents", "List documents", object_schema(&[], &[])),
                tool("create_document", "Create a document", object_schema(&["title"], &[("title", string_schema()), ("content", string_schema())])),
                tool("get_document", "Get a document snapshot", object_schema(&["document_id"], &[("document_id", string_schema())])),
                tool("export_document", "Export active text", object_schema(&["document_id"], &[("document_id", string_schema())])),
                tool("list_history_events", "List document history events", object_schema(&["document_id"], &[("document_id", string_schema()), ("category", enum_schema(&["document-comment", "all", "content", "comment", "suggestion", "system"])), ("from", string_schema()), ("to", string_schema()), ("limit", integer_schema())])),
                tool("diff_history_events", "Diff two document history events", object_schema(&["document_id", "from", "to"], &[("document_id", string_schema()), ("from", string_schema()), ("to", string_schema()), ("context", integer_schema())])),
                tool("set_audit_event_note", "Set a shared audit event note", object_schema(&["document_id", "audit_event_id", "body"], &[("document_id", string_schema()), ("audit_event_id", string_schema()), ("body", string_schema())])),
                tool("clear_audit_event_note", "Clear a shared audit event note", object_schema(&["document_id", "audit_event_id"], &[("document_id", string_schema()), ("audit_event_id", string_schema())])),
                tool("insert_lines", "Insert lines atomically", object_schema(&["document_id", "content"], &[("document_id", string_schema()), ("after_line_id", string_schema()), ("content", string_array_schema())])),
                tool("replace_lines", "Replace lines atomically", object_schema(&["document_id", "line_ids", "content"], &[("document_id", string_schema()), ("line_ids", string_array_schema()), ("content", string_array_schema())])),
                tool("delete_lines", "Delete lines atomically", object_schema(&["document_id", "line_ids"], &[("document_id", string_schema()), ("line_ids", string_array_schema())])),
                tool("comment_on_range", "Comment on a line range", object_schema(&["document_id", "start_line_id", "end_line_id", "body"], &[("document_id", string_schema()), ("start_line_id", string_schema()), ("end_line_id", string_schema()), ("body", string_schema())])),
                tool("reply_comment", "Reply to a comment", object_schema(&["document_id", "comment_id", "body"], &[("document_id", string_schema()), ("comment_id", string_schema()), ("body", string_schema())])),
                tool("resolve_comment", "Resolve a comment", object_schema(&["document_id", "comment_id"], &[("document_id", string_schema()), ("comment_id", string_schema())])),
                tool("suggest_change", "Create a structured suggestion", object_schema(&["document_id", "kind"], &[("document_id", string_schema()), ("kind", enum_schema(&["insert", "replace", "delete"])), ("anchor_line_id", string_schema()), ("start_line_id", string_schema()), ("end_line_id", string_schema()), ("content", string_array_schema())])),
                tool("accept_suggestion", "Accept a suggestion", object_schema(&["document_id", "suggestion_id"], &[("document_id", string_schema()), ("suggestion_id", string_schema())])),
                tool("reject_suggestion", "Reject a suggestion", object_schema(&["document_id", "suggestion_id"], &[("document_id", string_schema()), ("suggestion_id", string_schema())]))
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
        "list_documents" => (json!(db::list_documents(&state.pool).await?), None),
        "create_document" => {
            let title = string_arg(&args, "title")?;
            let content = args
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let snap = db::create_document(&state.pool, &actor, title, content).await?;
            (json!(snap), None)
        }
        "get_document" => (
            json!(db::snapshot(&state.pool, &string_arg(&args, "document_id")?).await?),
            None,
        ),
        "export_document" => (
            json!({
                "content": db::export_document(&state.pool, &string_arg(&args, "document_id")?).await?
            }),
            None,
        ),
        "list_history_events" => {
            let document_id = string_arg(&args, "document_id")?;
            (
                json!(
                    db::list_history_events(
                        &state.pool,
                        &document_id,
                        history_options_from_args(&args)?
                    )
                    .await?
                ),
                None,
            )
        }
        "diff_history_events" => {
            let document_id = string_arg(&args, "document_id")?;
            let history_diff = db::history_diff(
                &state.pool,
                &document_id,
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
        "set_audit_event_note" => {
            let document_id = string_arg(&args, "document_id")?;
            let snap = db::put_audit_event_note(
                &state.pool,
                &actor,
                &document_id,
                &string_arg(&args, "audit_event_id")?,
                string_arg(&args, "body")?,
            )
            .await?;
            state.hub.content_changed(&document_id);
            (json!(snap), None)
        }
        "clear_audit_event_note" => {
            let document_id = string_arg(&args, "document_id")?;
            let snap = db::put_audit_event_note(
                &state.pool,
                &actor,
                &document_id,
                &string_arg(&args, "audit_event_id")?,
                String::new(),
            )
            .await?;
            state.hub.content_changed(&document_id);
            (json!(snap), None)
        }
        "insert_lines" => {
            let document_id = string_arg(&args, "document_id")?;
            let content = string_vec_arg(&args, "content")?;
            let after = args
                .get("after_line_id")
                .and_then(Value::as_str)
                .map(str::to_string);
            let snap = db::insert_lines(&state.pool, &actor, &document_id, after, content).await?;
            state.hub.content_changed(&document_id);
            (json!(snap), None)
        }
        "replace_lines" => {
            let document_id = string_arg(&args, "document_id")?;
            let line_ids = string_vec_arg(&args, "line_ids")?;
            let snap = db::replace_lines(
                &state.pool,
                &actor,
                &document_id,
                line_ids.clone(),
                string_vec_arg(&args, "content")?,
            ).await?;
            state.hub.lines_replaced(&document_id, &line_ids);
            (json!(snap), None)
        }
        "delete_lines" => {
            let document_id = string_arg(&args, "document_id")?;
            let line_ids = string_vec_arg(&args, "line_ids")?;
            let snap = db::delete_lines(
                &state.pool,
                &actor,
                &document_id,
                line_ids.clone(),
            ).await?;
            state.hub.lines_deleted(&document_id, &line_ids);
            (json!(snap), None)
        }
        "comment_on_range" => {
            let document_id = string_arg(&args, "document_id")?;
            let snap = db::create_comment(
                &state.pool,
                &actor,
                &document_id,
                db::CommentDraft {
                    start_line_id: string_arg(&args, "start_line_id")?,
                    end_line_id: string_arg(&args, "end_line_id")?,
                    start_column: None,
                    end_column: None,
                    body: string_arg(&args, "body")?,
                },
            )
            .await?;
            state.hub.comment_created(&document_id, snap.comments.last().unwrap().id.as_str());
            (json!(snap), None)
        }
        "reply_comment" => {
            let document_id = string_arg(&args, "document_id")?;
            let snap = db::reply_comment(
                &state.pool,
                &actor,
                &document_id,
                &string_arg(&args, "comment_id")?,
                string_arg(&args, "body")?,
            )
            .await?;
            state.hub.content_changed(&document_id);
            (json!(snap), None)
        }
        "resolve_comment" => {
            let document_id = string_arg(&args, "document_id")?;
            let snap = db::resolve_comment(
                &state.pool,
                &actor,
                &document_id,
                &string_arg(&args, "comment_id")?,
            )
            .await?;
            state.hub.comment_resolved(&document_id, &string_arg(&args, "comment_id")?);
            (json!(snap), None)
        }
        "suggest_change" => {
            let document_id = string_arg(&args, "document_id")?;
            let snap = db::create_suggestion(
                &state.pool,
                &actor,
                &document_id,
                string_arg(&args, "kind")?,
                args.get("anchor_line_id")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                args.get("start_line_id")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                args.get("end_line_id")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                args.get("content")
                    .map(|_| string_vec_arg(&args, "content"))
                    .transpose()?
                    .unwrap_or_default(),
            )
            .await?;
            state.hub.suggestion_created(&document_id, snap.suggestions.last().unwrap().id.as_str());
            (json!(snap), None)
        }
        "accept_suggestion" | "reject_suggestion" => {
            let document_id = string_arg(&args, "document_id")?;
            let accept = name == "accept_suggestion";
            let snap = db::decide_suggestion(
                &state.pool,
                &actor,
                &document_id,
                &string_arg(&args, "suggestion_id")?,
                accept,
            )
            .await?;
            state.hub.suggestion_decided(&document_id, &string_arg(&args, "suggestion_id")?, accept);
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
