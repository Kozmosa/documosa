use std::collections::BTreeMap;
use std::env;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use criterion::{Criterion, Throughput};
use futures_util::StreamExt;
use reqwest::{Client, RequestBuilder};
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio_tungstenite::connect_async;

#[derive(Clone, Debug)]
struct BenchConfig {
    initial_lines: usize,
    ws_clients: usize,
    concurrency: usize,
    ops_per_iter: usize,
}

impl BenchConfig {
    fn from_env() -> Self {
        Self {
            initial_lines: env_usize("DOCUMOSA_BENCH_INITIAL_LINES", 100),
            ws_clients: env_usize("DOCUMOSA_BENCH_WS_CLIENTS", 8),
            concurrency: env_usize("DOCUMOSA_BENCH_CONCURRENCY", 16),
            ops_per_iter: env_usize("DOCUMOSA_BENCH_OPS_PER_ITER", 100),
        }
    }
}

struct BenchServer {
    _temp_dir: TempDir,
    base_url: String,
    document_id: String,
    diff_from_event_id: String,
    diff_to_event_id: String,
    note_event_id: String,
    client: Client,
    ws_events: Arc<AtomicU64>,
    shutdown: Option<oneshot::Sender<()>>,
    server_handle: JoinHandle<()>,
    ws_handles: Vec<JoinHandle<()>>,
}

#[derive(Clone)]
struct OperationContext {
    base_url: String,
    document_id: String,
    diff_from_event_id: String,
    diff_to_event_id: String,
    note_event_id: String,
    client: Client,
}

#[derive(Default)]
struct WorkerReport {
    successes: usize,
    failures: usize,
    latencies: Vec<Duration>,
    operations: BTreeMap<&'static str, OperationStats>,
}

struct IterationReport {
    configured_ops: usize,
    successes: usize,
    failures: usize,
    latencies: Vec<Duration>,
    ws_events: u64,
    operations: BTreeMap<&'static str, OperationStats>,
}

#[derive(Clone, Debug, Default)]
struct OperationStats {
    successes: usize,
    failures: usize,
    http_statuses: BTreeMap<String, usize>,
    mcp_errors: BTreeMap<String, usize>,
    error_samples: Vec<String>,
}

struct OperationOutcome {
    operation: &'static str,
    success: bool,
    http_status: Option<String>,
    mcp_error: Option<String>,
    error_sample: Option<String>,
}

struct SeedData {
    document_id: String,
    created_event_id: String,
    inserted_event_id: String,
}

fn main() {
    let config = BenchConfig::from_env();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to build Tokio runtime");
    let server = runtime
        .block_on(BenchServer::start(&config))
        .expect("failed to start benchmark server");

    let mut criterion = Criterion::default().configure_from_args();
    {
        let mut group = criterion.benchmark_group("documosa_stress");
        group.throughput(Throughput::Elements(config.ops_per_iter as u64));
        group.bench_function("mixed_rest_mcp_ws", |bencher| {
            bencher.iter_custom(|iterations| {
                runtime.block_on(async {
                    let mut total = Duration::ZERO;
                    for _ in 0..iterations {
                        let started = Instant::now();
                        let report = run_iteration(&server, &config).await;
                        let elapsed = started.elapsed();
                        total += elapsed;
                        report.print(elapsed);
                    }
                    total
                })
            });
        });
        group.finish();
    }
    runtime.block_on(server.shutdown());
    criterion.final_summary();
}

impl BenchServer {
    async fn start(config: &BenchConfig) -> anyhow::Result<Self> {
        let temp_dir = tempfile::tempdir()?;
        let pool = documosa::db::connect(&temp_dir.path().join("documosa.sqlite")).await?;
        documosa::db::migrate(&pool).await?;

        let app = documosa::build_app(pool, PathBuf::from("missing")).await;
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let addr = listener.local_addr()?;
        let base_url = format!("http://{addr}");
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let server_handle = tokio::spawn(async move {
            if let Err(error) = axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
            {
                eprintln!("benchmark server error: {error:#}");
            }
        });

        let client = Client::builder().pool_max_idle_per_host(128).build()?;
        wait_for_health(&client, &base_url).await?;
        let seed = seed_document(&client, &base_url, config.initial_lines).await?;

        let ws_events = Arc::new(AtomicU64::new(0));
        let mut ws_handles = Vec::with_capacity(config.ws_clients);
        for index in 0..config.ws_clients {
            let url = format!(
                "ws://{addr}/api/documents/{}/ws?client_id=bench-ws-{index}&nickname=BenchWs{index}&role_mode=reviewer",
                seed.document_id
            );
            let (stream, _) = connect_async(&url).await?;
            let events = Arc::clone(&ws_events);
            ws_handles.push(tokio::spawn(async move {
                let (_, mut reader) = stream.split();
                while let Some(message) = reader.next().await {
                    match message {
                        Ok(message) if message.is_text() => {
                            events.fetch_add(1, Ordering::Relaxed);
                        }
                        Ok(message) if message.is_close() => break,
                        Ok(_) => {}
                        Err(_) => break,
                    }
                }
            }));
        }

        Ok(Self {
            _temp_dir: temp_dir,
            base_url,
            document_id: seed.document_id,
            diff_from_event_id: seed.created_event_id,
            diff_to_event_id: seed.inserted_event_id.clone(),
            note_event_id: seed.inserted_event_id,
            client,
            ws_events,
            shutdown: Some(shutdown_tx),
            server_handle,
            ws_handles,
        })
    }

    async fn shutdown(mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        for handle in &self.ws_handles {
            handle.abort();
        }
        for handle in self.ws_handles {
            let _ = handle.await;
        }
        let _ = self.server_handle.await;
    }

    fn operation_context(&self) -> OperationContext {
        OperationContext {
            base_url: self.base_url.clone(),
            document_id: self.document_id.clone(),
            diff_from_event_id: self.diff_from_event_id.clone(),
            diff_to_event_id: self.diff_to_event_id.clone(),
            note_event_id: self.note_event_id.clone(),
            client: self.client.clone(),
        }
    }
}

impl IterationReport {
    fn print(mut self, elapsed: Duration) {
        self.latencies.sort_unstable();
        let total = self.successes + self.failures;
        let error_rate = if total == 0 {
            0.0
        } else {
            self.failures as f64 * 100.0 / total as f64
        };
        println!(
            "documosa_stress: ops={} completed={} success={} failed={} error_rate={:.2}% elapsed={:?} p50={:?} p95={:?} p99={:?} max={:?} ws_events={}",
            self.configured_ops,
            total,
            self.successes,
            self.failures,
            error_rate,
            elapsed,
            percentile(&self.latencies, 50),
            percentile(&self.latencies, 95),
            percentile(&self.latencies, 99),
            self.latencies.last().copied().unwrap_or_default(),
            self.ws_events,
        );
        for (operation, stats) in &self.operations {
            if stats.failures == 0 {
                continue;
            }
            println!(
                "  op={operation} success={} failed={} http_status={:?} mcp_errors={:?} samples={:?}",
                stats.successes,
                stats.failures,
                stats.http_statuses,
                stats.mcp_errors,
                stats.error_samples,
            );
        }
    }
}

impl OperationStats {
    fn record(&mut self, outcome: OperationOutcome) {
        if outcome.success {
            self.successes += 1;
            return;
        }
        self.failures += 1;
        if let Some(status) = outcome.http_status {
            *self.http_statuses.entry(status).or_default() += 1;
        }
        if let Some(error) = outcome.mcp_error {
            *self.mcp_errors.entry(error).or_default() += 1;
        }
        if let Some(sample) = outcome.error_sample
            && self.error_samples.len() < 3
        {
            self.error_samples.push(sample);
        }
    }

    fn merge(&mut self, other: OperationStats) {
        self.successes += other.successes;
        self.failures += other.failures;
        for (status, count) in other.http_statuses {
            *self.http_statuses.entry(status).or_default() += count;
        }
        for (error, count) in other.mcp_errors {
            *self.mcp_errors.entry(error).or_default() += count;
        }
        for sample in other.error_samples {
            if self.error_samples.len() >= 3 {
                break;
            }
            self.error_samples.push(sample);
        }
    }
}

impl OperationOutcome {
    fn success(operation: &'static str) -> Self {
        Self {
            operation,
            success: true,
            http_status: None,
            mcp_error: None,
            error_sample: None,
        }
    }

    fn failure(
        operation: &'static str,
        http_status: Option<String>,
        mcp_error: Option<String>,
        error_sample: Option<String>,
    ) -> Self {
        Self {
            operation,
            success: false,
            http_status,
            mcp_error,
            error_sample,
        }
    }
}

async fn run_iteration(server: &BenchServer, config: &BenchConfig) -> IterationReport {
    let next_op = Arc::new(AtomicUsize::new(0));
    let mut handles = Vec::with_capacity(config.concurrency);
    for worker_id in 0..config.concurrency {
        let next_op = Arc::clone(&next_op);
        let ctx = server.operation_context();
        let ops_per_iter = config.ops_per_iter;
        handles.push(tokio::spawn(async move {
            let mut report = WorkerReport::default();
            loop {
                let op_index = next_op.fetch_add(1, Ordering::Relaxed);
                if op_index >= ops_per_iter {
                    break;
                }
                let started = Instant::now();
                let outcome = run_operation(&ctx, op_index, worker_id).await;
                report.latencies.push(started.elapsed());
                if outcome.success {
                    report.successes += 1;
                } else {
                    report.failures += 1;
                }
                report
                    .operations
                    .entry(outcome.operation)
                    .or_insert_with(OperationStats::default)
                    .record(outcome);
            }
            report
        }));
    }

    let mut successes = 0;
    let mut failures = 0;
    let mut latencies = Vec::with_capacity(config.ops_per_iter);
    let mut operations = BTreeMap::new();
    for handle in handles {
        match handle.await {
            Ok(report) => {
                successes += report.successes;
                failures += report.failures;
                latencies.extend(report.latencies);
                for (operation, stats) in report.operations {
                    operations
                        .entry(operation)
                        .or_insert_with(OperationStats::default)
                        .merge(stats);
                }
            }
            Err(_) => failures += 1,
        }
    }

    IterationReport {
        configured_ops: config.ops_per_iter,
        successes,
        failures,
        latencies,
        ws_events: server.ws_events.load(Ordering::Relaxed),
        operations,
    }
}

async fn run_operation(
    ctx: &OperationContext,
    op_index: usize,
    worker_id: usize,
) -> OperationOutcome {
    match op_index % 8 {
        0 => {
            rest_outcome(
                "get_document",
                ctx.client.get(format!(
                    "{}/api/documents/{}",
                    ctx.base_url, ctx.document_id
                )),
            )
            .await
        }
        1 => {
            rest_outcome(
                "line_insert",
                identity_headers(
                    ctx.client
                        .post(format!(
                            "{}/api/documents/{}/lines/insert",
                            ctx.base_url, ctx.document_id
                        ))
                        .json(&json!({
                            "after_line_id": null,
                            "content": [format!("bench line {op_index} from worker {worker_id}")]
                        })),
                    &format!("bench-writer-{worker_id}"),
                    "Bench Writer",
                    "writer",
                ),
            )
            .await
        }
        2 => {
            rest_outcome(
                "history_list",
                ctx.client.get(format!(
                    "{}/api/documents/{}/history?category=all&limit=50",
                    ctx.base_url, ctx.document_id
                )),
            )
            .await
        }
        3 => {
            rest_outcome(
                "history_diff",
                ctx.client.get(format!(
                    "{}/api/documents/{}/history-diff?from={}&to={}",
                    ctx.base_url, ctx.document_id, ctx.diff_from_event_id, ctx.diff_to_event_id
                )),
            )
            .await
        }
        4 => {
            rest_outcome(
                "note_set",
                identity_headers(
                    ctx.client
                        .put(format!(
                            "{}/api/documents/{}/audit-events/{}/note",
                            ctx.base_url, ctx.document_id, ctx.note_event_id
                        ))
                        .json(&json!({ "body": format!("bench note {op_index}") })),
                    &format!("bench-reviewer-{worker_id}"),
                    "Bench Reviewer",
                    "reviewer",
                ),
            )
            .await
        }
        5 => {
            rest_outcome(
                "note_clear",
                identity_headers(
                    ctx.client
                        .put(format!(
                            "{}/api/documents/{}/audit-events/{}/note",
                            ctx.base_url, ctx.document_id, ctx.note_event_id
                        ))
                        .json(&json!({ "body": "" })),
                    &format!("bench-reviewer-{worker_id}"),
                    "Bench Reviewer",
                    "reviewer",
                ),
            )
            .await
        }
        6 => {
            mcp_tool_outcome(
                ctx,
                op_index,
                "mcp_get_document",
                "get_document",
                json!({ "document_id": ctx.document_id }),
            )
            .await
        }
        _ => {
            mcp_tool_outcome(
                ctx,
                op_index,
                "mcp_history_list",
                "list_history_events",
                json!({ "document_id": ctx.document_id, "category": "all", "limit": 50 }),
            )
            .await
        }
    }
}

async fn wait_for_health(client: &Client, base_url: &str) -> anyhow::Result<()> {
    for _ in 0..100 {
        if let Ok(response) = client.get(format!("{base_url}/api/health")).send().await
            && response.status().is_success()
        {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    anyhow::bail!("benchmark server did not become healthy");
}

async fn seed_document(
    client: &Client,
    base_url: &str,
    initial_lines: usize,
) -> anyhow::Result<SeedData> {
    let content = (1..=initial_lines)
        .map(|line| format!("Initial benchmark line {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    let created = identity_headers(
        client
            .post(format!("{base_url}/api/documents"))
            .json(&json!({ "title": "Benchmark Document", "content": content })),
        "bench-seed",
        "Bench Seed",
        "writer",
    )
    .send()
    .await?
    .error_for_status()?
    .json::<Value>()
    .await?;

    let document_id = json_string(&created, &["document", "id"])?;
    let created_event_id = audit_event_id(&created, "document.created")?;
    let inserted = identity_headers(
        client
            .post(format!(
                "{base_url}/api/documents/{document_id}/lines/insert"
            ))
            .json(&json!({
                "after_line_id": null,
                "content": ["stable benchmark diff line"]
            })),
        "bench-seed",
        "Bench Seed",
        "writer",
    )
    .send()
    .await?
    .error_for_status()?
    .json::<Value>()
    .await?;
    let inserted_event_id = audit_event_id(&inserted, "lines.inserted")?;

    Ok(SeedData {
        document_id,
        created_event_id,
        inserted_event_id,
    })
}

async fn rest_outcome(operation: &'static str, request: RequestBuilder) -> OperationOutcome {
    match request.send().await {
        Ok(response) if response.status().is_success() => match response.bytes().await {
            Ok(_) => OperationOutcome::success(operation),
            Err(error) => OperationOutcome::failure(
                operation,
                None,
                None,
                Some(format!("failed to read response body: {error}")),
            ),
        },
        Ok(response) => {
            let status = response.status().as_u16().to_string();
            let body = response
                .text()
                .await
                .unwrap_or_else(|error| format!("failed to read error body: {error}"));
            OperationOutcome::failure(operation, Some(status), None, Some(sample_body(&body)))
        }
        Err(error) => OperationOutcome::failure(operation, None, None, Some(error.to_string())),
    }
}

async fn mcp_tool_outcome(
    ctx: &OperationContext,
    id: usize,
    operation: &'static str,
    name: &str,
    arguments: Value,
) -> OperationOutcome {
    let response = ctx
        .client
        .post(format!("{}/mcp", ctx.base_url))
        .json(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": {
                "name": name,
                "arguments": arguments
            }
        }))
        .send()
        .await;
    let response = match response {
        Ok(response) => response,
        Err(error) => {
            return OperationOutcome::failure(operation, None, None, Some(error.to_string()));
        }
    };
    if !response.status().is_success() {
        let status = response.status().as_u16().to_string();
        let body = response
            .text()
            .await
            .unwrap_or_else(|error| format!("failed to read error body: {error}"));
        return OperationOutcome::failure(operation, Some(status), None, Some(sample_body(&body)));
    }
    match response.json::<Value>().await {
        Ok(value) if value.get("error").is_none() => OperationOutcome::success(operation),
        Ok(value) => {
            let error = value
                .get("error")
                .and_then(|error| error.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("unknown MCP error")
                .to_string();
            OperationOutcome::failure(
                operation,
                None,
                Some(error),
                Some(sample_body(&value.to_string())),
            )
        }
        Err(error) => OperationOutcome::failure(
            operation,
            None,
            None,
            Some(format!("failed to decode MCP response: {error}")),
        ),
    }
}

fn sample_body(body: &str) -> String {
    const MAX_SAMPLE_CHARS: usize = 240;
    body.chars().take(MAX_SAMPLE_CHARS).collect()
}

fn identity_headers(
    request: RequestBuilder,
    client_id: &str,
    nickname: &str,
    role_mode: &str,
) -> RequestBuilder {
    request
        .header("x-documosa-client-id", client_id)
        .header("x-documosa-nickname", nickname)
        .header("x-documosa-role-mode", role_mode)
}

fn audit_event_id(value: &Value, event_type: &str) -> anyhow::Result<String> {
    value
        .get("audit_events")
        .and_then(Value::as_array)
        .and_then(|events| {
            events
                .iter()
                .find(|event| event.get("event_type").and_then(Value::as_str) == Some(event_type))
        })
        .and_then(|event| event.get("id"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("missing seeded audit event {event_type}"))
}

fn json_string(value: &Value, path: &[&str]) -> anyhow::Result<String> {
    let mut cursor = value;
    for key in path {
        cursor = cursor
            .get(*key)
            .ok_or_else(|| anyhow::anyhow!("missing JSON key {key}"))?;
    }
    cursor
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("JSON path {path:?} was not a string"))
}

fn percentile(latencies: &[Duration], percentile: usize) -> Duration {
    if latencies.is_empty() {
        return Duration::ZERO;
    }
    let rank = (latencies.len() * percentile).div_ceil(100);
    latencies[rank.saturating_sub(1).min(latencies.len() - 1)]
}

fn env_usize(name: &str, default: usize) -> usize {
    env::var(name)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}
