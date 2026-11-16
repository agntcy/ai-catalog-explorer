//! Reads an entry's artifact and summarizes it by its media type.

use ai_catalog_discover::Fetcher;
use serde::Serialize;
use serde_json::Value;
use url::Url;

const MAX_PREVIEW: usize = 512 * 1024;
const MAX_INLINE_PREVIEW: usize = 8 * 1024;
const MAX_ITEMS: usize = 50;

#[derive(Serialize, Default)]
pub struct Inspection {
    /// `a2a`, `mcp`, `openapi`, `catalog`, `text` or `other`.
    pub kind: &'static str,
    pub status: Option<u16>,
    pub content_type: Option<String>,
    pub size: usize,
    /// Present when the entry states a digest.
    pub digest: Option<Check>,
    pub fields: Vec<Field>,
    pub items_title: Option<&'static str>,
    pub items: Vec<Item>,
    /// MCP servers the card says can be reached over the network, for testing in the MCP Inspector.
    pub remotes: Vec<Remote>,
    pub preview: String,
    pub truncated: bool,
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct Check {
    pub status: &'static str,
    pub reason: Option<String>,
}

#[derive(Serialize)]
pub struct Field {
    pub label: &'static str,
    pub value: String,
}

#[derive(Serialize)]
pub struct Remote {
    pub url: String,
    /// The transport name the MCP Inspector expects: `http` or `sse`.
    pub transport: &'static str,
}

#[derive(Serialize)]
pub struct Item {
    pub title: String,
    pub detail: Option<String>,
}

pub fn inspect(
    fetcher: &dyn Fetcher,
    url: &Url,
    media_type: &str,
    digest: Option<&str>,
) -> Inspection {
    match fetcher.get(url) {
        Ok(response) if response.is_success() => summarize(
            &response.body,
            media_type,
            digest,
            Some(response.status),
            response.content_type.clone(),
            MAX_PREVIEW,
        ),
        Ok(response) => Inspection {
            kind: "other",
            status: Some(response.status),
            error: Some(format!("the server answered HTTP {}", response.status)),
            ..Inspection::default()
        },
        Err(error) => Inspection {
            kind: "other",
            error: Some(error),
            ..Inspection::default()
        },
    }
}

/// Summarizes an artifact that is included in the catalog. A digest covers its canonical form.
pub fn inspect_data(media_type: &str, digest: Option<&str>, data: &Value) -> Inspection {
    match ai_catalog_trust::canonicalize(data.to_string()) {
        Ok(canonical) => summarize(
            canonical.as_bytes(),
            media_type,
            digest,
            None,
            None,
            MAX_INLINE_PREVIEW,
        ),
        Err(error) => Inspection {
            kind: "other",
            error: Some(error.to_string()),
            ..Inspection::default()
        },
    }
}

fn summarize(
    body: &[u8],
    media_type: &str,
    digest: Option<&str>,
    status: Option<u16>,
    content_type: Option<String>,
    preview_limit: usize,
) -> Inspection {
    let json: Option<Value> = serde_json::from_slice(body).ok();
    let kind = kind_of(media_type);

    let mut inspection = Inspection {
        kind,
        status,
        content_type,
        size: body.len(),
        digest: digest.map(
            |expected| match ai_catalog_trust::verify_digest(expected, body) {
                Ok(true) => Check {
                    status: "verified",
                    reason: None,
                },
                Ok(false) => Check {
                    status: "failed",
                    reason: Some("the content does not match the entry digest".into()),
                },
                Err(error) => Check {
                    status: "failed",
                    reason: Some(error.to_string()),
                },
            },
        ),
        ..Inspection::default()
    };

    match (kind, &json) {
        ("a2a", Some(card)) => a2a(&mut inspection, card),
        ("mcp", Some(card)) => mcp(&mut inspection, card),
        ("openapi", Some(spec)) => openapi(&mut inspection, spec),
        _ => {}
    }

    match (&json, std::str::from_utf8(body)) {
        (Some(value), _) => {
            let pretty = serde_json::to_string_pretty(value).unwrap_or_default();
            (inspection.preview, inspection.truncated) = clip(&pretty, preview_limit);
        }
        (None, Ok(text)) => (inspection.preview, inspection.truncated) = clip(text, preview_limit),
        (None, Err(_)) => inspection.preview = "Binary content, not shown.".into(),
    }
    inspection
}

fn kind_of(media_type: &str) -> &'static str {
    let media = media_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();

    if media.contains("gzip") || media.contains("zip") {
        "other"
    } else if media.contains("a2a-agent-card") {
        "a2a"
    } else if media.contains("mcp-server") {
        "mcp"
    } else if media.contains("openapi") {
        "openapi"
    } else if media.contains("ai-catalog") {
        "catalog"
    } else if media.contains("skill") || media.contains("markdown") || media == "text/plain" {
        "text"
    } else {
        "other"
    }
}

fn clip(text: &str, limit: usize) -> (String, bool) {
    if text.len() <= limit {
        return (text.to_owned(), false);
    }
    let mut end = limit;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (text[..end].to_owned(), true)
}

fn text(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

fn field(inspection: &mut Inspection, label: &'static str, value: Option<String>) {
    if let Some(value) = value {
        inspection.fields.push(Field {
            label,
            value: brief(&value),
        });
    }
}

/// A long value cut for display; the preview has the full document.
fn brief(value: &str) -> String {
    const MAX: usize = 280;
    match value.char_indices().nth(MAX) {
        Some((end, _)) => format!("{}…", value[..end].trim_end()),
        None => value.to_owned(),
    }
}

/// The names of the members of `value[key]` whose value is `true` or, for other values, all of them.
fn enabled(value: &Value, key: &str) -> Option<String> {
    let members = value.get(key)?.as_object()?;
    let names: Vec<&str> = members
        .iter()
        .filter(|(_, v)| v.as_bool() != Some(false))
        .map(|(name, _)| name.as_str())
        .collect();

    (!names.is_empty()).then(|| names.join(", "))
}

fn list(value: &Value, key: &str) -> Option<String> {
    let names: Vec<&str> = value
        .get(key)?
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .collect();

    (!names.is_empty()).then(|| names.join(", "))
}

fn a2a(inspection: &mut Inspection, card: &Value) {
    field(inspection, "Name", text(card, "name"));
    field(inspection, "Description", text(card, "description"));
    field(inspection, "Version", text(card, "version"));
    field(inspection, "Protocol", text(card, "protocolVersion"));
    field(inspection, "Endpoint", text(card, "url"));
    field(inspection, "Transport", text(card, "preferredTransport"));
    field(inspection, "Capabilities", enabled(card, "capabilities"));
    field(
        inspection,
        "Authentication",
        Some(enabled(card, "securitySchemes").unwrap_or_else(|| "none declared".into())),
    );
    field(inspection, "Accepts", list(card, "defaultInputModes"));
    field(inspection, "Returns", list(card, "defaultOutputModes"));

    inspection.items_title = Some("Skills");
    inspection.items = card
        .get("skills")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(MAX_ITEMS)
        .map(|skill| Item {
            title: text(skill, "name")
                .or_else(|| text(skill, "id"))
                .unwrap_or_else(|| "unnamed skill".into()),
            detail: text(skill, "description"),
        })
        .collect();
}

fn mcp(inspection: &mut Inspection, card: &Value) {
    let info = card.get("serverInfo").unwrap_or(card);

    field(
        inspection,
        "Name",
        text(info, "title").or_else(|| text(info, "name")),
    );
    field(inspection, "Description", text(info, "description"));
    field(inspection, "Version", text(info, "version"));
    field(inspection, "Website", text(info, "websiteUrl"));
    field(
        inspection,
        "Repository",
        card.get("repository").and_then(|repo| text(repo, "url")),
    );
    field(inspection, "Protocol", text(card, "protocolVersion"));
    field(
        inspection,
        "Endpoint",
        text(card, "endpoint").or_else(|| text(card, "url")),
    );
    field(inspection, "Capabilities", enabled(card, "capabilities"));
    field(inspection, "Schema", text(card, "$schema"));

    let tools = card.get("tools").and_then(Value::as_array);
    let remotes = card.get("remotes").and_then(Value::as_array);

    if let Some(tools) = tools {
        inspection.items_title = Some("Tools");
        inspection.items = tools
            .iter()
            .take(MAX_ITEMS)
            .map(|tool| Item {
                title: text(tool, "name").unwrap_or_else(|| "unnamed tool".into()),
                detail: text(tool, "description"),
            })
            .collect();
    }
    inspection.remotes = remotes
        .into_iter()
        .flatten()
        .filter_map(|remote| {
            let transport = match text(remote, "type")?.as_str() {
                "streamable-http" => "http",
                "sse" => "sse",
                _ => return None,
            };
            Some(Remote {
                url: text(remote, "url").filter(|url| url.starts_with("https://"))?,
                transport,
            })
        })
        .collect();
    if tools.is_none()
        && let Some(remotes) = remotes
    {
        inspection.items_title = Some("Remotes");
        inspection.items = remotes
            .iter()
            .take(MAX_ITEMS)
            .map(|remote| Item {
                title: text(remote, "url").unwrap_or_else(|| "unnamed remote".into()),
                detail: text(remote, "type").or_else(|| text(remote, "transport")),
            })
            .collect();
    }
}

fn openapi(inspection: &mut Inspection, spec: &Value) {
    let info = spec.get("info").unwrap_or(&Value::Null);

    field(inspection, "Title", text(info, "title"));
    field(inspection, "Description", text(info, "description"));
    field(inspection, "Version", text(info, "version"));
    field(
        inspection,
        "OpenAPI",
        text(spec, "openapi").or_else(|| text(spec, "swagger")),
    );

    let servers: Vec<String> = spec
        .get("servers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|server| text(server, "url"))
        .collect();
    field(
        inspection,
        "Servers",
        (!servers.is_empty()).then(|| servers.join(", ")),
    );

    inspection.items_title = Some("Operations");
    inspection.items = spec
        .get("paths")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .take(MAX_ITEMS)
        .map(|(path, methods)| {
            let verbs: Vec<String> = methods
                .as_object()
                .into_iter()
                .flatten()
                .filter(|(name, _)| {
                    ["get", "put", "post", "delete", "patch", "head", "options"]
                        .contains(&name.as_str())
                })
                .map(|(name, _)| name.to_ascii_uppercase())
                .collect();
            Item {
                title: path.clone(),
                detail: (!verbs.is_empty()).then(|| verbs.join(", ")),
            }
        })
        .collect();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_types_map_to_a_kind() {
        for (media, kind) in [
            ("application/a2a-agent-card+json", "a2a"),
            ("application/mcp-server-card+json; charset=utf-8", "mcp"),
            ("application/vnd.oai.openapi+json", "openapi"),
            ("application/ai-skill", "text"),
            ("application/octet-stream", "other"),
            ("application/agent-skills+gzip", "other"),
        ] {
            assert_eq!(kind_of(media), kind, "{media}");
        }
    }

    #[test]
    fn a_registry_style_mcp_card_is_summarized() {
        let card = serde_json::json!({
            "$schema": "https://example.com/server.schema.json",
            "name": "io.example/server",
            "title": "Example",
            "version": "1.0.0",
            "websiteUrl": "https://example.com",
            "repository": {"url": "https://github.com/example/server"},
            "remotes": [{"type": "streamable-http", "url": "https://example.com/mcp"}],
        });
        let mut inspection = Inspection::default();

        mcp(&mut inspection, &card);

        let value = |label| {
            inspection
                .fields
                .iter()
                .find(|f| f.label == label)
                .map(|f| f.value.as_str())
        };
        assert_eq!(value("Name"), Some("Example"));
        assert_eq!(value("Website"), Some("https://example.com"));
        assert_eq!(
            value("Repository"),
            Some("https://github.com/example/server")
        );
        assert_eq!(
            inspection.items[0].detail.as_deref(),
            Some("streamable-http")
        );
        assert_eq!(inspection.remotes.len(), 1);
        assert_eq!(inspection.remotes[0].url, "https://example.com/mcp");
        assert_eq!(inspection.remotes[0].transport, "http");
    }

    #[test]
    fn an_a2a_card_is_summarized() {
        let card = serde_json::json!({
            "name": "Billing",
            "capabilities": {"streaming": true, "pushNotifications": false},
            "skills": [{"id": "refund", "name": "Refund", "description": "Refunds an order"}],
        });
        let mut inspection = Inspection::default();

        a2a(&mut inspection, &card);

        let value = |label| {
            inspection
                .fields
                .iter()
                .find(|f| f.label == label)
                .map(|f| f.value.as_str())
        };
        assert_eq!(value("Name"), Some("Billing"));
        assert_eq!(value("Capabilities"), Some("streaming"));
        assert_eq!(value("Authentication"), Some("none declared"));
        assert_eq!(inspection.items[0].title, "Refund");
    }
}
