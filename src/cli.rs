use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use reqwest::Client;
use serde::Serialize;
use serde_json::json;

use crate::diff;
use crate::models::HistoryDiff;
use crate::notion::NotionBackend;

#[derive(Parser)]
#[command(name = "documosa")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    Serve(ServeArgs),
    Document {
        #[command(subcommand)]
        command: DocumentCommand,
    },
    Line {
        #[command(subcommand)]
        command: LineCommand,
    },
    Comment {
        #[command(subcommand)]
        command: CommentCommand,
    },
    Suggestion {
        #[command(subcommand)]
        command: SuggestionCommand,
    },
    History {
        #[command(subcommand)]
        command: HistoryCommand,
    },
}

#[derive(Args)]
pub struct ServeArgs {
    #[arg(long, default_value = "0.0.0.0")]
    pub addr: String,
    #[arg(long, hide = true)]
    pub host: Option<String>,
    #[arg(long, default_value_t = 4317)]
    pub port: u16,
    #[arg(long)]
    pub data_dir: Option<PathBuf>,
}

#[derive(Clone, Args)]
pub struct ClientArgs {
    #[arg(long, default_value = "http://127.0.0.1:4317")]
    pub server: String,
    #[arg(long)]
    pub notion_token: Option<String>,
    #[arg(long, hide = true, default_value = "https://api.notion.com")]
    pub notion_api_base_url: String,
    #[arg(long, default_value = "cli-client")]
    pub client_id: String,
    #[arg(long, default_value = "CLI")]
    pub nickname: String,
    #[arg(long, value_enum, default_value_t = RoleArg::Reviewer)]
    pub role_mode: RoleArg,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum RoleArg {
    Reviewer,
    Writer,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum HistoryCategoryArg {
    DocumentComment,
    All,
    Content,
    Comment,
    Suggestion,
    System,
}

impl HistoryCategoryArg {
    fn as_str(self) -> &'static str {
        match self {
            Self::DocumentComment => "document-comment",
            Self::All => "all",
            Self::Content => "content",
            Self::Comment => "comment",
            Self::Suggestion => "suggestion",
            Self::System => "system",
        }
    }
}

impl RoleArg {
    fn as_str(self) -> &'static str {
        match self {
            RoleArg::Reviewer => "reviewer",
            RoleArg::Writer => "writer",
        }
    }
}

#[derive(Subcommand)]
pub enum DocumentCommand {
    List(ListDocumentsArgs),
    Create(CreateDocumentArgs),
    Import(ImportDocumentArgs),
    Get(DocumentIdArgs),
    Export(DocumentIdArgs),
}

#[derive(Args)]
pub struct ListDocumentsArgs {
    #[command(flatten)]
    pub client: ClientArgs,
    #[arg(long)]
    pub notion_parent_page_id: Option<String>,
}

#[derive(Args)]
pub struct CreateDocumentArgs {
    #[command(flatten)]
    pub client: ClientArgs,
    #[arg(long)]
    pub title: String,
    #[arg(long, default_value = "")]
    pub content: String,
    #[arg(long)]
    pub notion_parent_page_id: Option<String>,
}

#[derive(Args)]
pub struct ImportDocumentArgs {
    #[command(flatten)]
    pub client: ClientArgs,
    #[arg(long)]
    pub title: String,
    #[arg(long)]
    pub file: PathBuf,
    #[arg(long)]
    pub notion_parent_page_id: Option<String>,
}

#[derive(Args)]
pub struct DocumentIdArgs {
    #[command(flatten)]
    pub client: ClientArgs,
    pub document_id: String,
}

#[derive(Subcommand)]
pub enum LineCommand {
    Insert(InsertLineArgs),
    Replace(ReplaceLineArgs),
    Delete(DeleteLineArgs),
}

#[derive(Args)]
pub struct InsertLineArgs {
    #[command(flatten)]
    pub client: ClientArgs,
    pub document_id: String,
    #[arg(long)]
    pub after_line_id: Option<String>,
    #[arg(long = "line", required = true)]
    pub lines: Vec<String>,
}

#[derive(Args)]
pub struct ReplaceLineArgs {
    #[command(flatten)]
    pub client: ClientArgs,
    pub document_id: String,
    #[arg(long = "line-id", required = true)]
    pub line_ids: Vec<String>,
    #[arg(long = "line", required = true)]
    pub lines: Vec<String>,
}

#[derive(Args)]
pub struct DeleteLineArgs {
    #[command(flatten)]
    pub client: ClientArgs,
    pub document_id: String,
    #[arg(long = "line-id", required = true)]
    pub line_ids: Vec<String>,
}

#[derive(Subcommand)]
pub enum CommentCommand {
    Create(CreateCommentArgs),
    Reply(ReplyCommentArgs),
    Resolve(ResolveCommentArgs),
}

#[derive(Args)]
pub struct CreateCommentArgs {
    #[command(flatten)]
    pub client: ClientArgs,
    pub document_id: String,
    #[arg(long)]
    pub start_line_id: String,
    #[arg(long)]
    pub end_line_id: String,
    #[arg(long)]
    pub body: String,
}

#[derive(Args)]
pub struct ReplyCommentArgs {
    #[command(flatten)]
    pub client: ClientArgs,
    pub document_id: String,
    pub comment_id: String,
    #[arg(long)]
    pub body: String,
}

#[derive(Args)]
pub struct ResolveCommentArgs {
    #[command(flatten)]
    pub client: ClientArgs,
    pub document_id: String,
    pub comment_id: String,
}

#[derive(Subcommand)]
pub enum SuggestionCommand {
    Create(CreateSuggestionArgs),
    Accept(DecideSuggestionArgs),
    Reject(DecideSuggestionArgs),
}

#[derive(Subcommand)]
pub enum HistoryCommand {
    List(HistoryListArgs),
    Diff(HistoryDiffArgs),
    Note {
        #[command(subcommand)]
        command: HistoryNoteCommand,
    },
}

#[derive(Args)]
pub struct HistoryListArgs {
    #[command(flatten)]
    pub client: ClientArgs,
    pub document_id: String,
    #[arg(long, value_enum, default_value_t = HistoryCategoryArg::DocumentComment)]
    pub category: HistoryCategoryArg,
    #[arg(long)]
    pub from: Option<String>,
    #[arg(long)]
    pub to: Option<String>,
    #[arg(long)]
    pub limit: Option<i64>,
}

#[derive(Args)]
pub struct HistoryDiffArgs {
    #[command(flatten)]
    pub client: ClientArgs,
    pub document_id: String,
    #[arg(long)]
    pub from: String,
    #[arg(long)]
    pub to: String,
    #[arg(long, default_value_t = 3)]
    pub context: usize,
}

#[derive(Subcommand)]
pub enum HistoryNoteCommand {
    Set(HistoryNoteSetArgs),
    Clear(HistoryNoteClearArgs),
}

#[derive(Args)]
pub struct HistoryNoteSetArgs {
    #[command(flatten)]
    pub client: ClientArgs,
    pub document_id: String,
    pub audit_event_id: String,
    #[arg(long)]
    pub body: String,
}

#[derive(Args)]
pub struct HistoryNoteClearArgs {
    #[command(flatten)]
    pub client: ClientArgs,
    pub document_id: String,
    pub audit_event_id: String,
}

#[derive(Args)]
pub struct CreateSuggestionArgs {
    #[command(flatten)]
    pub client: ClientArgs,
    pub document_id: String,
    #[arg(long)]
    pub kind: String,
    #[arg(long)]
    pub anchor_line_id: Option<String>,
    #[arg(long)]
    pub start_line_id: Option<String>,
    #[arg(long)]
    pub end_line_id: Option<String>,
    #[arg(long = "line")]
    pub content: Vec<String>,
}

#[derive(Args)]
pub struct DecideSuggestionArgs {
    #[command(flatten)]
    pub client: ClientArgs,
    pub document_id: String,
    pub suggestion_id: String,
}

pub async fn run(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        Command::Serve(args) => {
            let data_dir = args.data_dir.unwrap_or_else(default_data_dir);
            crate::serve(args.host.unwrap_or(args.addr), args.port, data_dir).await
        }
        Command::Document { command } => run_document(command).await,
        Command::Line { command } => run_line(command).await,
        Command::Comment { command } => run_comment(command).await,
        Command::Suggestion { command } => run_suggestion(command).await,
        Command::History { command } => run_history(command).await,
    }
}

fn default_data_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".documosa")
}

async fn run_document(command: DocumentCommand) -> anyhow::Result<()> {
    match command {
        DocumentCommand::List(args) => {
            if let Some(notion) = notion_backend(&args.client) {
                let parent_page_id = require_parent_page_id(args.notion_parent_page_id.as_deref())?;
                print_json(notion.list_documents(parent_page_id).await?)
            } else {
                print_json(request(&args.client, reqwest::Method::GET, "/api/documents", ()).await?)
            }
        }
        DocumentCommand::Create(args) => {
            if let Some(notion) = notion_backend(&args.client) {
                let parent_page_id = require_parent_page_id(args.notion_parent_page_id.as_deref())?;
                print_json(
                    notion
                        .create_document(parent_page_id, &args.title, &args.content)
                        .await?,
                )
            } else {
                print_json(
                    request(
                        &args.client,
                        reqwest::Method::POST,
                        "/api/documents",
                        json!({ "title": args.title, "content": args.content }),
                    )
                    .await?,
                )
            }
        }
        DocumentCommand::Import(args) => {
            let content = tokio::fs::read_to_string(args.file).await?;
            if let Some(notion) = notion_backend(&args.client) {
                let parent_page_id = require_parent_page_id(args.notion_parent_page_id.as_deref())?;
                print_json(
                    notion
                        .create_document(parent_page_id, &args.title, &content)
                        .await?,
                )
            } else {
                print_json(
                    request(
                        &args.client,
                        reqwest::Method::POST,
                        "/api/documents",
                        json!({ "title": args.title, "content": content }),
                    )
                    .await?,
                )
            }
        }
        DocumentCommand::Get(args) => {
            if let Some(notion) = notion_backend(&args.client) {
                print_json(notion.get_document(&args.document_id).await?)
            } else {
                print_json(
                    request(
                        &args.client,
                        reqwest::Method::GET,
                        &format!("/api/documents/{}", args.document_id),
                        (),
                    )
                    .await?,
                )
            }
        }
        DocumentCommand::Export(args) => {
            if let Some(notion) = notion_backend(&args.client) {
                let text = notion.export_document(&args.document_id).await?;
                println!("{text}");
                Ok(())
            } else {
                let text = request_text(
                    &args.client,
                    reqwest::Method::GET,
                    &format!("/api/documents/{}/export", args.document_id),
                    (),
                )
                .await?;
                println!("{text}");
                Ok(())
            }
        }
    }
}

async fn run_line(command: LineCommand) -> anyhow::Result<()> {
    match command {
        LineCommand::Insert(args) => {
            if let Some(notion) = notion_backend(&args.client) {
                print_json(
                    notion
                        .insert_lines(&args.document_id, args.after_line_id, args.lines)
                        .await?,
                )
            } else {
                print_json(
                    request(
                        &args.client,
                        reqwest::Method::POST,
                        &format!("/api/documents/{}/lines/insert", args.document_id),
                        json!({ "after_line_id": args.after_line_id, "content": args.lines }),
                    )
                    .await?,
                )
            }
        }
        LineCommand::Replace(args) => {
            if let Some(notion) = notion_backend(&args.client) {
                print_json(
                    notion
                        .replace_lines(&args.document_id, args.line_ids, args.lines)
                        .await?,
                )
            } else {
                print_json(
                    request(
                        &args.client,
                        reqwest::Method::POST,
                        &format!("/api/documents/{}/lines/replace", args.document_id),
                        json!({ "line_ids": args.line_ids, "content": args.lines }),
                    )
                    .await?,
                )
            }
        }
        LineCommand::Delete(args) => {
            if let Some(notion) = notion_backend(&args.client) {
                print_json(
                    notion
                        .delete_lines(&args.document_id, args.line_ids)
                        .await?,
                )
            } else {
                print_json(
                    request(
                        &args.client,
                        reqwest::Method::POST,
                        &format!("/api/documents/{}/lines/delete", args.document_id),
                        json!({ "line_ids": args.line_ids }),
                    )
                    .await?,
                )
            }
        }
    }
}

async fn run_comment(command: CommentCommand) -> anyhow::Result<()> {
    if comment_client_args(&command).notion_token.is_some() {
        unsupported_notion_backend()?;
    }
    match command {
        CommentCommand::Create(args) => print_json(request(&args.client, reqwest::Method::POST, &format!("/api/documents/{}/comments", args.document_id), json!({ "start_line_id": args.start_line_id, "end_line_id": args.end_line_id, "body": args.body })).await?),
        CommentCommand::Reply(args) => print_json(request(&args.client, reqwest::Method::POST, &format!("/api/documents/{}/comments/{}/reply", args.document_id, args.comment_id), json!({ "body": args.body })).await?),
        CommentCommand::Resolve(args) => print_json(request(&args.client, reqwest::Method::POST, &format!("/api/documents/{}/comments/{}/resolve", args.document_id, args.comment_id), json!({})).await?),
    }
}

async fn run_suggestion(command: SuggestionCommand) -> anyhow::Result<()> {
    if suggestion_client_args(&command).notion_token.is_some() {
        unsupported_notion_backend()?;
    }
    match command {
        SuggestionCommand::Create(args) => print_json(request(&args.client, reqwest::Method::POST, &format!("/api/documents/{}/suggestions", args.document_id), json!({ "kind": args.kind, "anchor_line_id": args.anchor_line_id, "start_line_id": args.start_line_id, "end_line_id": args.end_line_id, "content": args.content })).await?),
        SuggestionCommand::Accept(args) => print_json(request(&args.client, reqwest::Method::POST, &format!("/api/documents/{}/suggestions/{}/accept", args.document_id, args.suggestion_id), json!({})).await?),
        SuggestionCommand::Reject(args) => print_json(request(&args.client, reqwest::Method::POST, &format!("/api/documents/{}/suggestions/{}/reject", args.document_id, args.suggestion_id), json!({})).await?),
    }
}

async fn run_history(command: HistoryCommand) -> anyhow::Result<()> {
    if history_client_args(&command).notion_token.is_some() {
        unsupported_notion_backend()?;
    }
    match command {
        HistoryCommand::List(args) => print_json(
            request(
                &args.client,
                reqwest::Method::GET,
                &history_list_path(&args),
                (),
            )
            .await?,
        ),
        HistoryCommand::Diff(args) => {
            let value = request(
                &args.client,
                reqwest::Method::GET,
                &format!(
                    "/api/documents/{}/history-diff?from={}&to={}",
                    args.document_id,
                    query_component(&args.from),
                    query_component(&args.to)
                ),
                (),
            )
            .await?;
            let history_diff: HistoryDiff = serde_json::from_value(value)?;
            print!(
                "{}",
                diff::format_history_unified_diff(&history_diff, args.context)
            );
            Ok(())
        }
        HistoryCommand::Note { command } => match command {
            HistoryNoteCommand::Set(args) => print_json(
                request(
                    &args.client,
                    reqwest::Method::PUT,
                    &format!(
                        "/api/documents/{}/audit-events/{}/note",
                        args.document_id, args.audit_event_id
                    ),
                    json!({ "body": args.body }),
                )
                .await?,
            ),
            HistoryNoteCommand::Clear(args) => print_json(
                request(
                    &args.client,
                    reqwest::Method::PUT,
                    &format!(
                        "/api/documents/{}/audit-events/{}/note",
                        args.document_id, args.audit_event_id
                    ),
                    json!({ "body": "" }),
                )
                .await?,
            ),
        },
    }
}

fn notion_backend(client_args: &ClientArgs) -> Option<NotionBackend> {
    client_args
        .notion_token
        .as_ref()
        .map(|token| NotionBackend::new(token.clone(), client_args.notion_api_base_url.clone()))
}

fn require_parent_page_id(value: Option<&str>) -> anyhow::Result<&str> {
    value
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("--notion-parent-page-id is required in Notion mode"))
}

fn unsupported_notion_backend() -> anyhow::Result<()> {
    anyhow::bail!("Notion backend supports only document and line commands")
}

fn comment_client_args(command: &CommentCommand) -> &ClientArgs {
    match command {
        CommentCommand::Create(args) => &args.client,
        CommentCommand::Reply(args) => &args.client,
        CommentCommand::Resolve(args) => &args.client,
    }
}

fn suggestion_client_args(command: &SuggestionCommand) -> &ClientArgs {
    match command {
        SuggestionCommand::Create(args) => &args.client,
        SuggestionCommand::Accept(args) => &args.client,
        SuggestionCommand::Reject(args) => &args.client,
    }
}

fn history_client_args(command: &HistoryCommand) -> &ClientArgs {
    match command {
        HistoryCommand::List(args) => &args.client,
        HistoryCommand::Diff(args) => &args.client,
        HistoryCommand::Note { command } => match command {
            HistoryNoteCommand::Set(args) => &args.client,
            HistoryNoteCommand::Clear(args) => &args.client,
        },
    }
}

fn history_list_path(args: &HistoryListArgs) -> String {
    let mut params = vec![format!("category={}", args.category.as_str())];
    if let Some(from) = &args.from {
        params.push(format!("from={}", query_component(from)));
    }
    if let Some(to) = &args.to {
        params.push(format!("to={}", query_component(to)));
    }
    if let Some(limit) = args.limit {
        params.push(format!("limit={limit}"));
    }
    format!(
        "/api/documents/{}/history?{}",
        args.document_id,
        params.join("&")
    )
}

async fn request<T: Serialize>(
    client_args: &ClientArgs,
    method: reqwest::Method,
    path: &str,
    body: T,
) -> anyhow::Result<serde_json::Value> {
    let response = build_request(client_args, method, path)
        .json(&body)
        .send()
        .await?;
    let status = response.status();
    let text = response.text().await?;
    if !status.is_success() {
        anyhow::bail!("{status}: {text}");
    }
    Ok(serde_json::from_str(&text)?)
}

async fn request_text<T: Serialize>(
    client_args: &ClientArgs,
    method: reqwest::Method,
    path: &str,
    body: T,
) -> anyhow::Result<String> {
    let response = build_request(client_args, method, path)
        .json(&body)
        .send()
        .await?;
    let status = response.status();
    let text = response.text().await?;
    if !status.is_success() {
        anyhow::bail!("{status}: {text}");
    }
    Ok(text)
}

fn build_request(
    client_args: &ClientArgs,
    method: reqwest::Method,
    path: &str,
) -> reqwest::RequestBuilder {
    let url = format!("{}{}", client_args.server.trim_end_matches('/'), path);
    Client::new()
        .request(method, url)
        .header("x-documosa-client-id", &client_args.client_id)
        .header("x-documosa-nickname", &client_args.nickname)
        .header("x-documosa-role-mode", client_args.role_mode.as_str())
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

fn print_json(value: serde_json::Value) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}
