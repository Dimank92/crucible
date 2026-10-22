#![allow(dead_code)]

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

/// Sanitizes extracted docstring comments against HTML injection and Stored XSS.
pub fn sanitize_docstring(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let mut out = String::with_capacity(trimmed.len());
    for c in trimmed.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            _ => out.push(c),
        }
    }
    out
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParamDoc {
    pub name: String,
    pub type_name: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDoc {
    pub name: String,
    pub description: String,
    pub params: Vec<ParamDoc>,
    pub returns: String,
    pub visibility: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventDoc {
    pub name: String,
    pub description: String,
    pub fields: Vec<ParamDoc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractDoc {
    pub contract_name: String,
    pub version: String,
    pub description: String,
    pub functions: Vec<FunctionDoc>,
    pub events: Vec<EventDoc>,
    pub generated_at: chrono::DateTime<Utc>,
}

pub struct DocGeneratorService {
    #[allow(dead_code)]
    db: PgPool,
}

impl DocGeneratorService {
    pub fn new(db: PgPool) -> Self {
        Self { db }
    }

    pub async fn generate(
        &self,
        contract_name: &str,
        source_code: &str,
    ) -> Result<ContractDoc, sqlx::Error> {
        let mut functions = Vec::new();
        let mut events = Vec::new();
        let mut description = String::new();
        let mut pending_doc = String::new();

        for line in source_code.lines() {
            let trimmed = line.trim();
            if let Some(doc) = trimmed.strip_prefix("///") {
                pending_doc.push_str(doc.trim());
                pending_doc.push(' ');
            } else if trimmed.starts_with("pub fn ") || trimmed.starts_with("fn ") {
                let fn_name = trimmed
                    .trim_start_matches("pub ")
                    .trim_start_matches("fn ")
                    .split('(')
                    .next()
                    .unwrap_or("")
                    .to_string();
                if !fn_name.is_empty() {
                    let clean_doc = sanitize_docstring(&pending_doc);
                    functions.push(FunctionDoc {
                        name: fn_name,
                        description: clean_doc,
                        params: vec![],
                        returns: "unknown".to_string(),
                        visibility: if trimmed.starts_with("pub") {
                            "public".to_string()
                        } else {
                            "private".to_string()
                        },
                    });
                }
                pending_doc.clear();
            } else if trimmed.contains("events().publish") {
                let event_name = trimmed
                    .split("publish(")
                    .nth(1)
                    .or_else(|| trimmed.split('(').nth(2))
                    .and_then(|s| s.split(',').next())
                    .unwrap_or("unknown")
                    .trim_matches(|c: char| !c.is_alphanumeric() && c != '_')
                    .to_string();
                if !event_name.is_empty() {
                    let clean_doc = sanitize_docstring(&pending_doc);
                    events.push(EventDoc {
                        name: event_name,
                        description: clean_doc,
                        fields: vec![],
                    });
                }
                pending_doc.clear();
            } else if !trimmed.is_empty() {
                if description.is_empty() && !pending_doc.is_empty() {
                    description = sanitize_docstring(&pending_doc);
                }
                pending_doc.clear();
            }
        }

        Ok(ContractDoc {
            contract_name: contract_name.to_string(),
            version: "0.1.0".to_string(),
            description,
            functions,
            events,
            generated_at: Utc::now(),
        })
    }

    pub fn generate_markdown(&self, doc: &ContractDoc) -> String {
        let mut md = format!("# {}\n\n", doc.contract_name);
        if !doc.description.is_empty() {
            md.push_str(&format!("{}", doc.description));
            md.push_str("\n\n");
        }
        if !doc.functions.is_empty() {
            md.push_str("## Functions\n\n");
            for f in &doc.functions {
                md.push_str(&format!("### `{}`\n\n{}", f.name, f.description));
                md.push_str("\n\n");
            }
        }
        if !doc.events.is_empty() {
            md.push_str("## Events\n\n");
            for e in &doc.events {
                md.push_str(&format!("### `{}`\n\n{}", e.name, e.description));
                md.push_str("\n\n");
            }
        }
        md
    }

    pub fn generate_json(&self, doc: &ContractDoc) -> serde_json::Value {
        serde_json::to_value(doc).unwrap_or(serde_json::Value::Null)
    }

    pub fn generate_openapi_spec(&self, doc: &ContractDoc) -> serde_json::Value {
        let mut paths = serde_json::Map::new();

        for f in &doc.functions {
            let mut operation = serde_json::Map::new();
            operation.insert("summary".to_string(), serde_json::json!(f.name));
            operation.insert("description".to_string(), serde_json::json!(f.description));
            operation.insert(
                "responses".to_string(),
                serde_json::json!({
                    "200": { "description": "Successful execution" }
                }),
            );

            let mut method_map = serde_json::Map::new();
            method_map.insert("post".to_string(), serde_json::Value::Object(operation));
            paths.insert(format!("/{}", f.name), serde_json::Value::Object(method_map));
        }

        serde_json::json!({
            "openapi": "3.0.0",
            "info": {
                "title": doc.contract_name,
                "version": doc.version,
                "description": doc.description
            },
            "paths": paths
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_xss_payload_in_docstrings_sanitized() {
        let pool = sqlx::PgPool::connect_lazy("postgres://localhost/test").unwrap();
        let service = DocGeneratorService::new(pool);
        let malicious_source = r#"
/// Contract top-level description <script>alert("XSS")</script>
pub struct Contract;

/// Transfers tokens safely <img src=x onerror=alert(1)>
pub fn transfer(to: String, amount: u64) {}

/// Emits when tokens transferred <iframe src="javascript:alert('owned')">
events().publish((transfer, to, amount));
"#;

        let doc = service.generate("TokenContract", malicious_source).await.unwrap();

        // Check contract description
        assert!(!doc.description.contains("<script>"));
        assert!(doc.description.contains("&lt;script&gt;alert(&quot;XSS&quot;)&lt;/script&gt;"));

        // Check function description
        let transfer_fn = doc.functions.iter().find(|f| f.name == "transfer").unwrap();
        assert!(!transfer_fn.description.contains("<img"));
        assert!(transfer_fn.description.contains("&lt;img src=x onerror=alert(1)&gt;"));

        // Check event description
        let transfer_event = doc.events.iter().find(|e| e.name == "transfer").unwrap();
        assert!(!transfer_event.description.contains("<iframe"));
        assert!(transfer_event.description.contains("&lt;iframe src=&quot;javascript:alert(&#x27;owned&#x27;)&quot;&gt;"));

        // Check Markdown output
        let md = service.generate_markdown(&doc);
        assert!(!md.contains("<script>"));
        assert!(!md.contains("<img"));
        assert!(!md.contains("<iframe"));

        // Check OpenAPI spec output
        let openapi = service.generate_openapi_spec(&doc);
        let openapi_str = openapi.to_string();
        assert!(!openapi_str.contains("<script>"));
        assert!(!openapi_str.contains("<img"));
        assert!(!openapi_str.contains("<iframe"));
    }
}
