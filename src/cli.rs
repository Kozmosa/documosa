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
    #[arg(long)]
    pub token: Option<String>,
    #[arg(long)]
    pub jwt: Option<String>,
}

impl ClientArgs {
    fn bearer_token(&self) -> Option<String> {
        self.jwt
            .clone()
            .or_else(|| self.token.clone())
            .filter(|t| !t.is_empty())
    }
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
    #[allow(dead_code)]
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
    pub block_id: String,
    #[arg(long)]
    pub start_column: Option<i64>,
    #[arg(long)]
    pub end_column: Option<i64>,
    #[arg(long)]
    pub body: String,
}

#[derive(Args)]
pub struct ReplyCommentArgs {
    #[command(flatten)]
    pub client: ClientArgs,
    pub block_id: String,
    pub comment_id: String,
    #[arg(long)]
    pub body: String,
}

#[derive(Args)]
pub struct ResolveCommentArgs {
    #[command(flatten)]
    pub client: ClientArgs,
    pub block_id: String,
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
    pub target_block_id: Option<String>,
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
                let parent_page_id =
                    require_parent_page_id(args.notion_parent_page_id.as_deref())?;
                print_json(notion.list_documents(parent_page_id).await?)
            } else {
                print_json(request(&args.client, reqwest::Method::GET, "/pages", ()).await?)
            }
        }
        DocumentCommand::Create(args) => {
            if let Some(notion) = notion_backend(&args.client) {
                let parent_page_id =
                    require_parent_page_id(args.notion_parent_page_id.as_deref())?;
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
                        "/pages",
                        json!({ "title": args.title, "content_json": args.content }),
                    )
                    .await?,
                )
            }
        }
        DocumentCommand::Import(args) => {
            let content = tokio::fs::read_to_string(args.file).await?;
            if let Some(notion) = notion_backend(&args.client) {
                let parent_page_id =
                    require_parent_page_id(args.notion_parent_page_id.as_deref())?;
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
                        "/pages",
                        json!({ "title": args.title, "content_json": content }),
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
                        &format!("/pages/{}/snapshot", args.document_id),
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
                let value: serde_json::Value = request(
                    &args.client,
                    reqwest::Method::GET,
                    &format!("/pages/{}/export/md", args.document_id),
                    (),
                )
                .await?;
                if let Some(md) = value.get("markdown").and_then(|v| v.as_str()) {
                    println!("{md}");
                }
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
                let children: Vec<serde_json::Value> = args
                    .lines
                    .iter()
                    .map(|line| {
                        let content_json = serde_json::json!([{
                            "type": "text",
                            "text": { "content": line },
                            "plain_text": line,
                        }])
                        .to_string();
                        json!({
                            "block_type": "paragraph",
                            "content_json": content_json,
                        })
                    })
                    .collect();
                print_json(
                    request(
                        &args.client,
                        reqwest::Method::PATCH,
                        &format!("/pages/{}/children", args.document_id),
                        json!({ "children": children, "after": args.after_line_id }),
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
                let text = args.lines.join("\n");
                let content_json = serde_json::json!([{
                    "type": "text",
                    "text": { "content": text },
                    "plain_text": text,
                }])
                .to_string();
                // Use first line_id; existing behavior for multiple is best-effort
                if let Some(block_id) = args.line_ids.first() {
                    print_json(
                        request(
                            &args.client,
                            reqwest::Method::PATCH,
                            &format!("/blocks/{}", block_id),
                            json!({ "content_json": content_json }),
                        )
                        .await?,
                    )
                } else {
                    anyhow::bail!("at least one line-id is required");
                }
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
                if let Some(block_id) = args.line_ids.first() {
                    let snap: serde_json::Value = request(
                        &args.client,
                        reqwest::Method::DELETE,
                        &format!("/blocks/{}", block_id),
                        json!({}),
                    )
                    .await?;
                    print_json(snap)
                } else {
                    anyhow::bail!("at least one line-id is required");
                }
            }
        }
    }
}

async fn run_comment(command: CommentCommand) -> anyhow::Result<()> {
    if comment_client_args(&command).notion_token.is_some() {
        unsupported_notion_backend()?;
    }
    match command {
        CommentCommand::Create(args) => print_json(
            request(
                &args.client,
                reqwest::Method::POST,
                &format!("/blocks/{}/comments", args.block_id),
                json!({
                    "body": args.body,
                    "start_column": args.start_column,
                    "end_column": args.end_column,
                }),
            )
            .await?,
        ),
        CommentCommand::Reply(args) => print_json(
            request(
                &args.client,
                reqwest::Method::POST,
                &format!("/blocks/{}/comments/{}/replies", args.block_id, args.comment_id),
                json!({ "body": args.body }),
            )
            .await?,
        ),
        CommentCommand::Resolve(args) => print_json(
            request(
                &args.client,
                reqwest::Method::POST,
                &format!("/blocks/{}/comments/{}/resolve", args.block_id, args.comment_id),
                json!({}),
            )
            .await?,
        ),
    }
}

async fn run_suggestion(command: SuggestionCommand) -> anyhow::Result<()> {
    if suggestion_client_args(&command).notion_token.is_some() {
        unsupported_notion_backend()?;
    }
    match command {
        SuggestionCommand::Create(args) => print_json(
            request(
                &args.client,
                reqwest::Method::POST,
                &format!("/pages/{}/suggestions", args.document_id),
                json!({
                    "kind": args.kind,
                    "target_block_id": args.target_block_id,
                    "content": args.content,
                }),
            )
            .await?,
        ),
        SuggestionCommand::Accept(args) => print_json(
            request(
                &args.client,
                reqwest::Method::POST,
                &format!("/pages/{}/suggestions/{}/accept", args.document_id, args.suggestion_id),
                json!({}),
            )
            .await?,
        ),
        SuggestionCommand::Reject(args) => print_json(
            request(
                &args.client,
                reqwest::Method::POST,
                &format!("/pages/{}/suggestions/{}/reject", args.document_id, args.suggestion_id),
                json!({}),
            )
            .await?,
        ),
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
                    "/pages/{}/history-diff?from={}&to={}",
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
                    reqwest::Method::POST,
                    &format!(
                        "/pages/{}/audit/{}/note",
                        args.document_id, args.audit_event_id
                    ),
                    json!({ "body": args.body }),
                )
                .await?,
            ),
            HistoryNoteCommand::Clear(args) => print_json(
                request(
                    &args.client,
                    reqwest::Method::POST,
                    &format!(
                        "/pages/{}/audit/{}/note",
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
        "/pages/{}/history?{}",
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

fn build_request(
    client_args: &ClientArgs,
    method: reqwest::Method,
    path: &str,
) -> reqwest::RequestBuilder {
    let url = format!("{}{}", client_args.server.trim_end_matches('/'), path);
    let builder = Client::new().request(method, url);
    if let Some(token) = client_args.bearer_token() {
        builder.bearer_auth(token)
    } else {
        builder
    }
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
