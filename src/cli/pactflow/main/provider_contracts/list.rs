use clap::ArgMatches;
use comfy_table::{Table, presets::UTF8_FULL};
use serde_json::Value;

use super::capability::{list_url, provider_contracts_template, unsupported_server_message};
use crate::cli::pact_broker::main::{
    HALClient, PactBrokerError,
    utils::{get_auth, get_broker_url, get_custom_headers, get_retries, get_ssl_options},
};

/// Fetches the list resource of a provider version.
async fn fetch_contracts(
    hal_client: HALClient,
    provider: &str,
    version: &str,
) -> Result<Value, PactBrokerError> {
    let index = hal_client.clone().fetch("").await?;
    let template = provider_contracts_template(&index).ok_or_else(|| {
        PactBrokerError::ContentError(unsupported_server_message("list-provider-contracts"))
    })?;
    let url = list_url(&template, provider, version)?;
    match hal_client.fetch(&url).await {
        Err(PactBrokerError::NotFound(_)) => Err(PactBrokerError::NotFound(format!(
            "No provider version {provider} {version} was found"
        ))),
        other => other,
    }
}

/// The text of a JSON string field, empty when it is null or missing.
fn text<'a>(contract: &'a Value, field: &str) -> &'a str {
    contract.get(field).and_then(Value::as_str).unwrap_or("")
}

/// Renders the list as a table, or the empty-list message when it has no entries.
/// `PUBLISHED` is `updatedAt`, which a republish bumps.
fn render_table(body: &Value, provider: &str, version: &str) -> String {
    let contracts = body
        .pointer("/_embedded/contracts")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if contracts.is_empty() {
        return format!("No provider contracts are published for {provider} version {version}");
    }
    let mut table = Table::new();
    table.load_style(UTF8_FULL).set_header(vec![
        "KEY",
        "SPECIFICATION",
        "NAME",
        "SELF VERIFICATION",
        "PUBLISHED",
    ]);
    for contract in contracts {
        let self_verification = match contract
            .pointer("/selfVerification/success")
            .and_then(Value::as_bool)
        {
            Some(true) => "true",
            Some(false) => "false",
            None => "unknown",
        };
        table.add_row(vec![
            text(contract, "key"),
            text(contract, "specification"),
            text(contract, "name"),
            self_verification,
            text(contract, "updatedAt"),
        ]);
    }
    table.to_string()
}

pub fn list(args: &ArgMatches) -> Result<Value, PactBrokerError> {
    let provider = args
        .get_one::<String>("provider")
        .expect("PROVIDER is required");
    let version = args
        .get_one::<String>("provider-app-version")
        .expect("PROVIDER_APP_VERSION is required");
    let output = args
        .get_one::<String>("output")
        .map(String::as_str)
        .unwrap_or("table");

    let hal_client = HALClient::with_url(
        get_broker_url(args).trim_end_matches('/'),
        Some(get_auth(args)),
        get_ssl_options(args),
        get_custom_headers(args),
    )
    .with_retry_count(get_retries(args));

    let body = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(fetch_contracts(hal_client, provider, version))?;
    if output == "json" {
        println!("{}", serde_json::to_string(&body).unwrap());
    } else {
        println!("{}", render_table(&body, provider, version));
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use axum::{
        Router,
        http::{Method, StatusCode, Uri},
    };
    use serde_json::json;
    use tokio::net::TcpListener;

    use super::*;
    use crate::cli::pact_broker::main::types::SslOptions;

    const TEMPLATE: &str = "/provider-contracts/provider/{provider}/version/{version}";

    /// A server whose index (at `/`) carries `index_links` and which answers
    /// every other request with `status` and `body`. Returns its URL and the
    /// requests seen.
    async fn spawn_server(
        index_links: Value,
        status: u16,
        body: Value,
    ) -> (String, Arc<Mutex<Vec<String>>>) {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = requests.clone();
        let router = Router::new().fallback(move |method: Method, uri: Uri| {
            let recorded = recorded.clone();
            let index_links = index_links.clone();
            let body = body.clone();
            async move {
                let target = uri.path_and_query().map(|pq| pq.as_str()).unwrap_or("");
                recorded.lock().unwrap().push(format!("{method} {target}"));
                let (status, response) = if uri.path() == "/" {
                    (200, json!({"_links": index_links}))
                } else {
                    (status, body)
                };
                (
                    StatusCode::from_u16(status).unwrap(),
                    [("content-type", "application/hal+json")],
                    response.to_string(),
                )
            }
        });
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        (format!("http://{addr}"), requests)
    }

    fn links() -> Value {
        json!({"pf:provider-contracts": {"href": TEMPLATE, "templated": true}})
    }

    fn client(base: &str) -> HALClient {
        HALClient::with_url(base, None, SslOptions::default(), None)
    }

    fn body() -> Value {
        json!({"_embedded": {"contracts": [
            {
                "key": "oas",
                "specification": "oas",
                "name": null,
                "contentType": "application/yaml",
                "createdAt": "2026-01-01T00:00:00Z",
                "updatedAt": "2026-01-02T00:00:00Z",
                "buildUrl": null,
                "selfVerification": {"success": true}
            },
            {
                "key": "asyncapi/events",
                "specification": "asyncapi",
                "name": "events",
                "contentType": "application/json",
                "createdAt": "2026-01-01T00:00:00Z",
                "updatedAt": "2026-01-03T00:00:00Z",
                "buildUrl": null,
                "selfVerification": {"success": false}
            }
        ]}})
    }

    #[tokio::test]
    async fn fetches_the_expanded_list_url() {
        let (base, requests) = spawn_server(links(), 200, body()).await;
        let fetched = fetch_contracts(client(&base), "Foo Bar", "1/2")
            .await
            .unwrap();
        assert_eq!(fetched, body());
        assert_eq!(
            requests.lock().unwrap().last().unwrap(),
            "GET /provider-contracts/provider/Foo%20Bar/version/1%2F2"
        );
    }

    #[tokio::test]
    async fn reports_a_missing_version() {
        let (base, _) = spawn_server(links(), 404, json!({})).await;
        let err = fetch_contracts(client(&base), "Foo", "1.0")
            .await
            .unwrap_err();
        assert_eq!(err, "No provider version Foo 1.0 was found");
    }

    #[tokio::test]
    async fn refuses_without_the_relation_and_sends_nothing_else() {
        for index_links in [json!({}), Value::Null] {
            let (base, requests) = spawn_server(index_links, 200, body()).await;
            let err = fetch_contracts(client(&base), "Foo", "1.0")
                .await
                .unwrap_err();
            assert_eq!(
                err,
                "This PactFlow server keeps one provider contract per provider version, so list-provider-contracts cannot be honoured. Upgrade PactFlow, or publish one document per provider version without a name."
            );
            assert_eq!(requests.lock().unwrap().as_slice(), ["GET /"]);
        }
    }

    #[test]
    fn table_has_one_row_per_contract() {
        let table = render_table(&body(), "Foo", "1.0");
        let lines: Vec<&str> = table.lines().collect();
        assert!(lines[1].contains("KEY"));
        assert!(lines[1].contains("SPECIFICATION"));
        assert!(lines[1].contains("NAME"));
        assert!(lines[1].contains("SELF VERIFICATION"));
        assert!(lines[1].contains("PUBLISHED"));
        assert_eq!(lines.len(), 7, "{table}");
        assert!(lines[3].contains("oas") && lines[3].contains("2026-01-02T00:00:00Z"));
        assert!(lines[3].contains("true"));
        assert!(lines[5].contains("asyncapi/events") && lines[5].contains("events"));
        assert!(lines[5].contains("false"));
    }

    #[test]
    fn an_unnamed_document_has_an_empty_name() {
        let table = render_table(&body(), "Foo", "1.0");
        let row: Vec<&str> = table
            .lines()
            .nth(3)
            .unwrap()
            .split('┆')
            .map(str::trim)
            .collect();
        // KEY, SPECIFICATION, NAME, SELF VERIFICATION, PUBLISHED
        assert_eq!(row[0].trim_start_matches('│').trim(), "oas");
        assert_eq!(row[2], "");
    }

    #[test]
    fn self_verification_is_unknown_without_a_boolean_success() {
        let body = json!({"_embedded": {"contracts": [
            {"key": "oas"},
            {"key": "asyncapi", "selfVerification": null},
            {"key": "graphql", "selfVerification": {"success": "true"}}
        ]}});
        let table = render_table(&body, "Foo", "1.0");
        for line in [3, 5, 7] {
            let row: Vec<&str> = table
                .lines()
                .nth(line)
                .unwrap()
                .split('┆')
                .map(str::trim)
                .collect();
            // KEY, SPECIFICATION, NAME, SELF VERIFICATION, PUBLISHED
            assert_eq!(row[3], "unknown", "{table}");
        }
    }

    #[test]
    fn an_empty_list_prints_the_message() {
        let empty = json!({"_embedded": {"contracts": []}});
        assert_eq!(
            render_table(&empty, "Foo", "1.0"),
            "No provider contracts are published for Foo version 1.0"
        );
        assert_eq!(
            render_table(&json!({}), "Foo", "1.0"),
            "No provider contracts are published for Foo version 1.0"
        );
    }
}
