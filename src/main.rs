mod fetch;
mod index;
mod inspect;

use std::sync::Arc;

use ai_catalog::AiCatalog;
use ai_catalog_discover::{Discovery, Error, Fetcher, discover_with};
use ai_catalog_validate::{Diagnostic, Options, Outcome, Verification, validate, verify};
use axum::extract::{DefaultBodyLimit, Query, Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{Html, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;

use fetch::{Evidence, Guarded, Hop, Tracing};
use index::{Index, Query as IndexQuery, Record};
use inspect::Inspection;

const INDEX: &str = include_str!("../static/index.html");
const DISCOVER: &str = include_str!("../static/discover.html");
const CATALOGS: &str = include_str!("../static/catalogs.html");
const CATALOGS_SCRIPT: &str = include_str!("../static/catalogs.js");
const SEEDS: &str = include_str!("../data/seeds.txt");
const PUBLISH: &str = include_str!("../static/publish.html");
const STYLE: &str = include_str!("../static/style.css");
const SCRIPT: &str = include_str!("../static/discover.js");
const PUBLISH_SCRIPT: &str = include_str!("../static/publish.js");
const THEME: &str = include_str!("../static/theme.js");
const LOGO: &[u8] = include_bytes!("../static/logo.png");
const MAX_INPUT: usize = 300;
const MAX_URL: usize = 2048;
const MAX_CONCURRENT: usize = 8;
const MAX_EVIDENCE_FETCHES: usize = 12;
const MAX_RAW_BYTES: usize = 512 * 1024;
const MAX_DRAFT_BYTES: usize = 256 * 1024;
const REFRESH_AFTER: u64 = 24 * 60 * 60;
const RETRY_AFTER: u64 = 60 * 60;
const CRAWL_PAUSE: std::time::Duration = std::time::Duration::from_secs(60 * 60);

struct App {
    fetcher: Guarded,
    slots: Semaphore,
    index: Index,
}

#[derive(Deserialize)]
struct Params {
    domain: String,
}

#[derive(Deserialize)]
struct InspectParams {
    url: String,
    #[serde(rename = "type", default)]
    media_type: String,
    digest: Option<String>,
}

#[derive(Serialize)]
struct Report {
    input: String,
    found: bool,
    outcome: String,
    method: Option<&'static str>,
    url: Option<String>,
    spec_version: Option<String>,
    host: Option<String>,
    entries: Vec<EntryView>,
    trust: Option<TrustView>,
    hints: Vec<String>,
    /// The document as served, cut at `MAX_RAW_BYTES`.
    raw: Option<String>,
    raw_truncated: bool,
    trace: Vec<Hop>,
}

#[derive(Serialize)]
struct EntryView {
    identifier: String,
    display_name: Option<String>,
    entry_type: String,
    url: Option<String>,
    inline: bool,
    nested: bool,
    digest: Option<String>,
    /// What an inline artifact is, summarized without a fetch.
    inspection: Option<Inspection>,
    publisher_endorsed: bool,
    manifests: Vec<ManifestView>,
    artifact: Option<Check>,
}

#[derive(Serialize)]
struct TrustView {
    level: String,
    valid: bool,
    errors: Vec<Issue>,
    warnings: Vec<Issue>,
    catalog_signature: Option<Check>,
    catalog_authorized: bool,
}

#[derive(Serialize)]
struct Issue {
    code: String,
    path: String,
    message: String,
}

#[derive(Serialize)]
struct ManifestView {
    contributor: String,
    signer: Option<String>,
    publisher: bool,
    signature: Option<Check>,
    release: Option<Check>,
}

/// `status` is `verified`, `failed` or `unverifiable`.
#[derive(Serialize)]
struct Check {
    status: &'static str,
    reason: Option<String>,
}

impl From<&Outcome> for Check {
    fn from(outcome: &Outcome) -> Self {
        let (status, reason) = match outcome {
            Outcome::Verified => ("verified", None),
            Outcome::Failed(reason) => ("failed", Some(reason.clone())),
            Outcome::Unverifiable(reason) => ("unverifiable", Some(reason.clone())),
        };
        Self { status, reason }
    }
}

impl From<&Diagnostic> for Issue {
    fn from(d: &Diagnostic) -> Self {
        Self {
            code: d.code.to_string(),
            path: d.path.clone(),
            message: d.message.clone(),
        }
    }
}

/// The blocking client must be created and dropped outside the async runtime.
fn main() {
    let app = Arc::new(App {
        fetcher: Guarded::new().expect("cannot create the HTTP client"),
        slots: Semaphore::new(MAX_CONCURRENT),
        index: Index::load(std::env::var("INDEX_FILE").unwrap_or_else(|_| "index.json".into())),
    });
    let crawler = Arc::clone(&app);
    std::thread::spawn(move || crawl(&crawler));
    tokio::runtime::Runtime::new()
        .expect("cannot start the runtime")
        .block_on(serve(Arc::clone(&app)));
}

async fn serve(app: Arc<App>) {
    let router = Router::new()
        .route("/", get(|| async { Html(INDEX) }))
        .route("/discover", get(|| async { Html(DISCOVER) }))
        .route("/publish", get(|| async { Html(PUBLISH) }))
        .route("/catalogs", get(|| async { Html(CATALOGS) }))
        .route(
            "/style.css",
            get(|| async { ([(header::CONTENT_TYPE, "text/css")], STYLE) }),
        )
        .route(
            "/discover.js",
            get(|| async { ([(header::CONTENT_TYPE, "text/javascript")], SCRIPT) }),
        )
        .route(
            "/catalogs.js",
            get(|| async { ([(header::CONTENT_TYPE, "text/javascript")], CATALOGS_SCRIPT) }),
        )
        .route(
            "/publish.js",
            get(|| async { ([(header::CONTENT_TYPE, "text/javascript")], PUBLISH_SCRIPT) }),
        )
        .route(
            "/theme.js",
            get(|| async { ([(header::CONTENT_TYPE, "text/javascript")], THEME) }),
        )
        .route(
            "/favicon.ico",
            get(|| async {
                (
                    [
                        (header::CONTENT_TYPE, "image/png"),
                        (header::CACHE_CONTROL, "no-cache"),
                    ],
                    LOGO,
                )
            }),
        )
        .route(
            "/logo.png",
            get(|| async {
                (
                    [
                        (header::CONTENT_TYPE, "image/png"),
                        (header::CACHE_CONTROL, "no-cache"),
                    ],
                    LOGO,
                )
            }),
        )
        .route("/api/discover", get(discover))
        .route("/api/inspect", get(inspect_entry))
        .route("/api/index", get(search_index))
        .route("/api/index/submit", post(submit_catalog))
        .route(
            "/api/validate",
            post(check_draft).layer(DefaultBodyLimit::max(MAX_DRAFT_BYTES)),
        )
        .layer(middleware::from_fn(security_headers))
        .with_state(app);
    let bind = std::env::var("BIND").unwrap_or_else(|_| "127.0.0.1:8080".into());
    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .unwrap_or_else(|e| panic!("cannot listen on {bind}: {e}"));
    println!("AI Catalog Explorer on http://{bind}");
    axum::serve(listener, router).await.unwrap();
}

async fn security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'none'; style-src 'self'; script-src 'self'; connect-src 'self'; img-src 'self'",
        ),
    );
    response
}

async fn discover(
    State(app): State<Arc<App>>,
    Query(params): Query<Params>,
) -> Result<Json<Report>, (StatusCode, &'static str)> {
    if params.domain.len() > MAX_INPUT {
        return Err((StatusCode::BAD_REQUEST, "that input is too long"));
    }
    let permit = app.slots.try_acquire().map_err(|_| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "busy, try again in a moment",
        )
    })?;
    let input = params.domain;
    let report = {
        let app = Arc::clone(&app);
        tokio::task::spawn_blocking(move || run(&app.fetcher, &input)).await
    };
    drop(permit);
    report
        .map(Json)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "discovery failed"))
}

async fn search_index(
    State(app): State<Arc<App>>,
    Query(query): Query<IndexQuery>,
) -> Json<index::Answer> {
    Json(app.index.query(&query))
}

#[derive(Serialize)]
struct Submitted {
    added: bool,
    message: String,
}

async fn submit_catalog(
    State(app): State<Arc<App>>,
    Query(params): Query<Params>,
) -> Result<Json<Submitted>, (StatusCode, &'static str)> {
    if params.domain.len() > MAX_INPUT {
        return Err((StatusCode::BAD_REQUEST, "that input is too long"));
    }
    let permit = app.slots.try_acquire().map_err(|_| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "busy, try again in a moment",
        )
    })?;
    let input = params.domain;
    let outcome = {
        let app = Arc::clone(&app);
        tokio::task::spawn_blocking(move || {
            let report = run(&app.fetcher, &input);
            let record = record_from(&report, &input);
            if !report.found {
                return Ok(Submitted {
                    added: false,
                    message: report.outcome,
                });
            }
            if app.index.len() >= index::MAX_RECORDS && !app.index.contains(&record) {
                return Err((StatusCode::SERVICE_UNAVAILABLE, "the index is full"));
            }
            app.index.upsert(record);
            Ok(Submitted {
                added: true,
                message: format!("Added {}.", report.url.unwrap_or(report.input)),
            })
        })
        .await
    };
    drop(permit);
    outcome
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "the check failed"))?
        .map(Json)
}

/// Checks the seed list and everything submitted, again once a record is a day old.
fn crawl(app: &App) {
    loop {
        let mut inputs: Vec<String> = SEEDS
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(String::from)
            .collect();
        for known in app.index.inputs() {
            if !inputs.iter().any(|i| i.eq_ignore_ascii_case(&known)) {
                inputs.push(known);
            }
        }
        for input in inputs {
            let fresh = app.index.checked(&input).is_some_and(|(at, found)| {
                let limit = if found { REFRESH_AFTER } else { RETRY_AFTER };
                index::now().saturating_sub(at) < limit
            });
            if !fresh {
                let mut report = run(&app.fetcher, &input);
                if !report.found {
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    report = run(&app.fetcher, &input);
                }
                app.index.upsert(record_from(&report, &input));
            }
        }
        std::thread::sleep(CRAWL_PAUSE);
    }
}

/// The media type without parameters, so `text/markdown; profile=…` counts as `text/markdown`.
fn base_type(media_type: &str) -> String {
    media_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
}

fn record_from(report: &Report, input: &str) -> Record {
    let mut record = Record::new(input);
    if !report.found {
        let hops: Vec<String> = report
            .trace
            .iter()
            .map(|h| {
                let result = h.error.clone().unwrap_or_else(|| {
                    h.status
                        .map_or_else(|| "no response".into(), |s| s.to_string())
                });
                format!("{} {result}", h.url)
            })
            .collect();
        record.reason = Some(format!("{} ({})", report.outcome, hops.join("; ")));
        return record;
    }
    record.status = "ok".into();
    record.catalog_url = report.url.clone();
    record.method = report.method.map(String::from);
    record.host = report.host.clone();
    if let Some(trust) = &report.trust {
        record.level = Some(trust.level.clone());
        record.valid = trust.valid;
    }
    record.items = report
        .entries
        .iter()
        .map(|e| index::Item {
            identifier: e.identifier.clone(),
            name: e.display_name.clone(),
            entry_type: base_type(&e.entry_type),
            endorsed: e.publisher_endorsed,
        })
        .collect();
    record
}

#[derive(Serialize)]
struct Draft {
    valid: bool,
    level: String,
    errors: Vec<Issue>,
    warnings: Vec<Issue>,
}

/// Offline check of a catalog being written: nothing is fetched.
async fn check_draft(body: String) -> Json<Draft> {
    let issue = |message: String| Issue {
        code: "parse".into(),
        path: String::new(),
        message,
    };
    Json(match ai_catalog::parse_str(&body) {
        Err(error) => Draft {
            valid: false,
            level: String::new(),
            errors: vec![issue(error.to_string())],
            warnings: Vec::new(),
        },
        Ok(catalog) => {
            let result = validate(&catalog);
            Draft {
                valid: result.is_valid,
                level: result.conformance_level.to_string(),
                errors: result.errors.iter().map(Issue::from).collect(),
                warnings: result.warnings.iter().map(Issue::from).collect(),
            }
        }
    })
}

async fn inspect_entry(
    State(app): State<Arc<App>>,
    Query(params): Query<InspectParams>,
) -> Result<Json<Inspection>, (StatusCode, &'static str)> {
    if params.url.len() > MAX_URL {
        return Err((StatusCode::BAD_REQUEST, "that address is too long"));
    }
    let url = url::Url::parse(&params.url)
        .map_err(|_| (StatusCode::BAD_REQUEST, "that is not a valid address"))?;
    let permit = app.slots.try_acquire().map_err(|_| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "busy, try again in a moment",
        )
    })?;
    let result = {
        let app = Arc::clone(&app);
        tokio::task::spawn_blocking(move || {
            inspect::inspect(
                &app.fetcher,
                &url,
                &params.media_type,
                params.digest.as_deref(),
            )
        })
        .await
    };
    drop(permit);
    result
        .map(Json)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "inspection failed"))
}

fn run(fetcher: &dyn Fetcher, input: &str) -> Report {
    let tracing = Tracing::new(fetcher);
    let result = discover_with(&tracing, input);
    let mut trace = tracing.into_hops();
    let input = input.trim().to_string();
    match result {
        Ok(found) => {
            if let Some(last) = trace.last_mut() {
                last.note = Some(format!("catalog found ({})", found.method));
            }
            found_report(input, found, trace, fetcher)
        }
        Err(Error::NotFound { attempts, .. }) => {
            for hop in &mut trace {
                let reached = hop.final_url.as_ref().unwrap_or(&hop.url);
                if let Some(attempt) = attempts.iter().find(|a| &a.url == reached) {
                    hop.note = Some(format!("{}: {}", attempt.method, attempt.reason));
                }
            }
            Report {
                outcome: format!("No AI Catalog found for {input}."),
                ..empty(input, trace)
            }
        }
        Err(error) => Report {
            outcome: error.to_string(),
            ..empty(input, trace)
        },
    }
}

fn empty(input: String, trace: Vec<Hop>) -> Report {
    Report {
        input,
        found: false,
        outcome: String::new(),
        method: None,
        url: None,
        spec_version: None,
        host: None,
        entries: Vec::new(),
        trust: None,
        hints: Vec::new(),
        raw: None,
        raw_truncated: false,
        trace,
    }
}

fn found_report(input: String, found: Discovery, trace: Vec<Hop>, fetcher: &dyn Fetcher) -> Report {
    let evidence = Evidence::new(fetcher, MAX_EVIDENCE_FETCHES);
    let options = Options::default().with_base_url(found.url.clone());
    let served = trace
        .iter()
        .rev()
        .find(|hop| hop.final_url.as_ref().unwrap_or(&hop.url) == &found.url)
        .and_then(|hop| hop.content_type.as_deref());
    let hints = hints(&found.catalog, &found.url, served);
    let (raw, raw_truncated) = excerpt(&found.bytes);
    let verification = verify(&found.bytes, &options, &evidence).ok();
    let mut entries: Vec<EntryView> = found
        .catalog
        .entries
        .iter()
        .map(|e| EntryView {
            identifier: e.identifier.clone(),
            display_name: e.display_name.clone(),
            entry_type: e.entry_type.clone(),
            url: e.url.clone(),
            inline: e.data.is_some(),
            nested: e.is_nested_catalog(),
            digest: e.digest.clone(),
            inspection: e
                .data
                .as_ref()
                .map(|data| inspect::inspect_data(&e.entry_type, e.digest.as_deref(), data)),
            publisher_endorsed: false,
            manifests: Vec::new(),
            artifact: None,
        })
        .collect();
    if let Some(verification) = &verification {
        for trust in &verification.entries {
            if let Some(view) = entries.get_mut(trust.index) {
                view.publisher_endorsed = trust.publisher_endorsed;
                view.artifact = trust.artifact.as_ref().map(Check::from);
                view.manifests = trust
                    .manifests
                    .iter()
                    .map(|m| ManifestView {
                        contributor: m.contributor.clone(),
                        signer: m.signer.clone(),
                        publisher: m.publisher,
                        signature: m.signature.as_ref().map(Check::from),
                        release: m.release.as_ref().map(Check::from),
                    })
                    .collect();
            }
        }
    }
    Report {
        found: true,
        outcome: format!(
            "Found via {} at {}: {} {}.",
            found.method,
            found.url,
            entries.len(),
            if entries.len() == 1 {
                "entry"
            } else {
                "entries"
            },
        ),
        method: Some(found.method.as_str()),
        url: Some(found.url),
        spec_version: Some(found.catalog.spec_version),
        host: found.catalog.host.map(|h| h.display_name),
        entries,
        trust: verification.as_ref().map(trust_view),
        hints,
        raw: Some(raw),
        raw_truncated,
        ..empty(input, trace)
    }
}

fn trust_view(v: &Verification) -> TrustView {
    TrustView {
        level: v.conformance_level.to_string(),
        valid: v.validation.is_valid,
        errors: v.validation.errors.iter().map(Issue::from).collect(),
        warnings: v.validation.warnings.iter().map(Issue::from).collect(),
        catalog_signature: v.catalog_signature.as_ref().map(Check::from),
        catalog_authorized: v.catalog_authorized,
    }
}

/// Legal but probably unintended things in the catalog.
fn hints(catalog: &AiCatalog, url: &str, media_type: Option<&str>) -> Vec<String> {
    let mut hints = Vec::new();
    if catalog.host.is_none() {
        hints.push("No host, so the catalog stays at Minimal.".into());
    }
    if catalog.entries.is_empty() {
        hints.push("The catalog lists no entries.".into());
    }
    if !catalog.extra_fields.is_empty() {
        let names: Vec<&str> = catalog.extra_fields.keys().map(String::as_str).collect();
        hints.push(format!(
            "Members the specification does not define: {}. Consumers ignore them.",
            names.join(", ")
        ));
    }
    hints.extend(trust_hints(catalog, url, media_type));
    hints
}

/// What stands between the catalog and the Trusted level, for things the specification allows.
fn trust_hints(catalog: &AiCatalog, url: &str, media_type: Option<&str>) -> Vec<String> {
    let mut hints = Vec::new();
    let domain = url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_ascii_lowercase))
        .unwrap_or_default();
    let total = catalog.entries.len();
    let mut foreign: Vec<&str> = Vec::new();
    let mut not_urn: Vec<&str> = Vec::new();
    for entry in &catalog.entries {
        match entry.identifier.strip_prefix("urn:air:") {
            Some(rest) => {
                let publisher = rest.split(':').next().unwrap_or_default();
                if publisher != domain {
                    foreign.push(&entry.identifier);
                }
            }
            None => not_urn.push(&entry.identifier),
        }
    }
    if let Some(example) = foreign.first() {
        hints.push(format!(
            "{} of {total} entries name a publisher other than {domain} (for example {example}). That is allowed, but to be Trusted each publisher must sign its own entries.",
            foreign.len()
        ));
    }
    if let Some(example) = not_urn.first() {
        hints.push(format!(
            "{} of {total} entries are not in the urn:air format (for example {example}), so a publisher cannot sign for them.",
            not_urn.len()
        ));
    }
    let undigested = catalog
        .entries
        .iter()
        .filter(|e| e.digest.is_none())
        .count();
    if undigested > 0 {
        hints.push(format!(
            "{undigested} of {total} entries have no digest, so they cannot be endorsed or checked."
        ));
    }
    if let Some(served) = media_type.map(base_type)
        && served != "application/ai-catalog+json"
    {
        hints.push(format!(
            "Served as {served}. The preferred type is application/ai-catalog+json."
        ));
    }
    hints
}

/// The served bytes as text, cut at a character boundary when over the limit.
fn excerpt(bytes: &[u8]) -> (String, bool) {
    let text = String::from_utf8_lossy(bytes);
    if text.len() <= MAX_RAW_BYTES {
        return (text.into_owned(), false);
    }
    let mut end = MAX_RAW_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (text[..end].to_owned(), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(json: &str) -> AiCatalog {
        ai_catalog::parse_str(json).expect("valid catalog")
    }

    #[test]
    fn trust_hints_name_what_blocks_the_trusted_level() {
        let catalog = parsed(
            r#"{"specVersion":"1.0","host":{"displayName":"Dir"},"entries":[
                {"identifier":"urn:air:org.agntcy:catalog:a2a","type":"application/ai-catalog+json","url":"https://dir.example/a"},
                {"identifier":"https://dir.example/card","type":"application/a2a-agent-card+json","url":"https://dir.example/card"}]}"#,
        );
        let hints = hints(
            &catalog,
            "https://dir.example/.well-known/ai-catalog.json",
            Some("application/json; charset=utf-8"),
        );

        assert_eq!(hints.len(), 4, "{hints:?}");
        assert!(hints[0].contains("1 of 2 entries name a publisher other than dir.example"));
        assert!(hints[1].contains("not in the urn:air format"));
        assert!(hints[2].contains("2 of 2 entries have no digest"));
        assert!(hints[3].contains("Served as application/json"));
    }

    #[test]
    fn a_catalog_that_can_be_trusted_gets_no_trust_hints() {
        let catalog = parsed(
            r#"{"specVersion":"1.0","host":{"displayName":"Example","identifier":"did:web:example.com"},"entries":[
                {"identifier":"urn:air:example.com:mcp:x","type":"application/mcp-server-card+json","url":"https://example.com/x","digest":"sha256:00"}]}"#,
        );

        assert!(
            hints(
                &catalog,
                "https://example.com/.well-known/ai-catalog.json",
                Some("application/ai-catalog+json")
            )
            .is_empty()
        );
    }

    #[test]
    fn the_excerpt_is_cut_on_a_character_boundary() {
        let bytes = "é".repeat(MAX_RAW_BYTES);
        let (text, truncated) = excerpt(bytes.as_bytes());

        assert!(truncated);
        assert!(text.len() <= MAX_RAW_BYTES);
        assert_eq!(excerpt(b"{}"), ("{}".to_owned(), false));
    }
}
