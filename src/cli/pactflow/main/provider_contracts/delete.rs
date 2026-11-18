use std::collections::HashMap;

use clap::ArgMatches;
use serde_json::{Value, json};

use super::capability::{
    expand_contracts_template, provider_contracts_template, unsupported_server_message,
};
use crate::cli::pact_broker::main::{
    HALClient, Notice, PactBrokerError, process_notices,
    utils::{get_auth, get_broker_url, get_custom_headers, get_retries, get_ssl_options},
};

/// The URL of one provider contract: the server's templated list URL with the
/// specification and the optional name appended as path segments, then
/// expanded. Only the last of those segments is checked for a suffix the
/// broker strips, so a version such as `2.0.json` can be sent.
fn item_url(
    template: &str,
    provider: &str,
    version: &str,
    specification: &str,
    name: Option<&str>,
) -> Result<String, PactBrokerError> {
    let mut item_template = format!("{}/{{specification}}", template.trim_end_matches('/'));
    let mut values = HashMap::from([
        ("provider".to_string(), provider.to_string()),
        ("version".to_string(), version.to_string()),
        ("specification".to_string(), specification.to_string()),
    ]);
    if let Some(name) = name {
        item_template.push_str("/{name}");
        values.insert("name".to_string(), name.to_string());
    }
    expand_contracts_template(&item_template, &values)
}

/// The key of a provider contract: its specification, then `/` and its name
/// when it has one.
fn contract_key(specification: &str, name: Option<&str>) -> String {
    match name {
        Some(name) => format!("{specification}/{name}"),
        None => specification.to_string(),
    }
}

fn notices_of(body: &Value) -> Vec<Notice> {
    body.get("notices")
        .and_then(Value::as_array)
        .map(|notices| {
            notices
                .iter()
                .filter_map(|notice| serde_json::from_value(notice.clone()).ok())
                .collect()
        })
        .unwrap_or_default()
}

/// Deletes one provider contract and returns the notices the server sent.
async fn delete_contract(
    hal_client: HALClient,
    provider: &str,
    version: &str,
    specification: &str,
    name: Option<&str>,
) -> Result<Vec<Notice>, PactBrokerError> {
    let index = hal_client.clone().fetch("").await?;
    let template = provider_contracts_template(&index).ok_or_else(|| {
        PactBrokerError::ContentError(unsupported_server_message("delete-provider-contract"))
    })?;
    let url = item_url(&template, provider, version, specification, name)?;
    match hal_client.delete(&url).await {
        Ok(body) => Ok(notices_of(&body)),
        Err(PactBrokerError::NotFound(_)) => Err(PactBrokerError::NotFound(format!(
            "No provider contract {} exists for {provider} version {version}",
            contract_key(specification, name)
        ))),
        Err(err) => Err(err),
    }
}

pub fn delete(args: &ArgMatches) -> Result<Value, PactBrokerError> {
    let provider = args
        .get_one::<String>("provider")
        .expect("PROVIDER is required");
    let version = args
        .get_one::<String>("provider-app-version")
        .expect("PROVIDER_APP_VERSION is required");
    let specification = args
        .get_one::<String>("specification")
        .expect("SPECIFICATION has a default");
    let name = args.get_one::<String>("contract-name").map(String::as_str);

    let hal_client = HALClient::with_url(
        get_broker_url(args).trim_end_matches('/'),
        Some(get_auth(args)),
        get_ssl_options(args),
        get_custom_headers(args),
    )
    .with_retry_count(get_retries(args));

    let notices = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(delete_contract(
            hal_client,
            provider,
            version,
            specification,
            name,
        ))?;
    println!(
        "✅ Deleted provider contract {} for provider: {provider} version: {version}",
        contract_key(specification, name)
    );
    process_notices(&notices);
    Ok(json!({}))
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

    /// A server whose index carries `index_links` and which answers every other
    /// request with `status` and `body`. Returns its URL and the requests seen.
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
                let response = if method == Method::GET {
                    json!({"_links": index_links})
                } else {
                    body
                };
                let status = if method == Method::GET { 200 } else { status };
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

    fn deletes(requests: &Arc<Mutex<Vec<String>>>) -> Vec<String> {
        requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.starts_with("DELETE"))
            .cloned()
            .collect()
    }

    #[test]
    fn item_url_appends_the_specification_and_name() {
        let template = "https://h/provider-contracts/provider/{provider}/version/{version}";
        assert_eq!(
            item_url(template, "Foo Bar", "1/2", "oas", None).unwrap(),
            "https://h/provider-contracts/provider/Foo%20Bar/version/1%2F2/oas"
        );
        assert_eq!(
            item_url(template, "p", "v", "asyncapi", Some("events")).unwrap(),
            "https://h/provider-contracts/provider/p/version/v/asyncapi/events"
        );
    }

    #[test]
    fn item_url_encodes_the_specification_as_one_segment() {
        assert_eq!(
            item_url(TEMPLATE, "p", "v", "unsupported-a/b", None).unwrap(),
            "/provider-contracts/provider/p/version/v/unsupported-a%2Fb"
        );
        assert_eq!(
            item_url(TEMPLATE, "p", "v", "a/b", Some("n.1")).unwrap(),
            "/provider-contracts/provider/p/version/v/a%2Fb/n.1"
        );
    }

    #[test]
    fn item_url_sends_a_version_ending_in_a_stripped_suffix() {
        assert_eq!(
            item_url(TEMPLATE, "p", "2.0.json", "oas", None).unwrap(),
            "/provider-contracts/provider/p/version/2.0.json/oas"
        );
        assert_eq!(
            item_url(TEMPLATE, "p", "build 1.zip", "asyncapi", Some("events")).unwrap(),
            "/provider-contracts/provider/p/version/build%201.zip/asyncapi/events"
        );
    }

    #[test]
    fn item_url_refuses_a_stripped_suffix_in_the_final_segment() {
        let err = item_url(TEMPLATE, "p", "v", "oas.json", None).unwrap_err();
        assert_eq!(
            err,
            "specification value 'oas.json' cannot be sent: the Pact Broker strips a trailing '.json' from the final URL path segment"
        );
        assert!(item_url(TEMPLATE, "p", "v", "oas.json", Some("n")).is_ok());
    }

    #[tokio::test]
    async fn sends_delete_to_the_unnamed_item_url() {
        let (base, requests) = spawn_server(links(), 200, json!({"notices": []})).await;
        delete_contract(client(&base), "Foo", "1.0", "oas", None)
            .await
            .unwrap();
        assert_eq!(
            deletes(&requests),
            vec!["DELETE /provider-contracts/provider/Foo/version/1.0/oas"]
        );
    }

    #[tokio::test]
    async fn sends_delete_to_the_named_item_url() {
        let (base, requests) = spawn_server(links(), 200, json!({"notices": []})).await;
        delete_contract(client(&base), "Foo", "1.0", "asyncapi", Some("events"))
            .await
            .unwrap();
        assert_eq!(
            deletes(&requests),
            vec!["DELETE /provider-contracts/provider/Foo/version/1.0/asyncapi/events"]
        );
    }

    #[tokio::test]
    async fn sends_a_specification_with_a_slash_as_one_segment() {
        let (base, requests) = spawn_server(links(), 200, json!({})).await;
        delete_contract(client(&base), "Foo", "1.0", "unsupported-a/b", None)
            .await
            .unwrap();
        assert_eq!(
            deletes(&requests),
            vec!["DELETE /provider-contracts/provider/Foo/version/1.0/unsupported-a%2Fb"]
        );
    }

    #[tokio::test]
    async fn returns_the_notices_of_a_successful_delete() {
        let body = json!({"notices": [{"text": "deployed", "type": "warning"}]});
        let (base, _) = spawn_server(links(), 200, body).await;
        let notices = delete_contract(client(&base), "Foo", "1.0", "oas", None)
            .await
            .unwrap();
        assert_eq!(
            notices,
            vec![Notice {
                text: "deployed".to_string(),
                type_field: "warning".to_string()
            }]
        );
    }

    #[tokio::test]
    async fn reports_a_missing_contract_by_its_key() {
        let (base, _) = spawn_server(links(), 404, json!({})).await;
        let err = delete_contract(client(&base), "Foo", "1.0", "oas", None)
            .await
            .unwrap_err();
        assert_eq!(err, "No provider contract oas exists for Foo version 1.0");
        let err = delete_contract(client(&base), "Foo", "1.0", "oas", Some("a"))
            .await
            .unwrap_err();
        assert_eq!(err, "No provider contract oas/a exists for Foo version 1.0");
    }

    #[tokio::test]
    async fn fails_with_the_notices_of_a_conflict() {
        let body = json!({"notices": [{"text": "still needed", "type": "danger"}]});
        let (base, _) = spawn_server(links(), 409, body).await;
        let err = delete_contract(client(&base), "Foo", "1.0", "oas", None)
            .await
            .unwrap_err();
        match err {
            PactBrokerError::ValidationErrorWithNotices(_, notices) => {
                assert_eq!(notices.len(), 1);
                assert_eq!(notices[0].text, "still needed");
            }
            other => panic!("expected notices, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn refuses_without_the_relation_and_sends_no_delete() {
        for index_links in [json!({}), Value::Null] {
            let (base, requests) = spawn_server(index_links, 200, json!({})).await;
            let err = delete_contract(client(&base), "Foo", "1.0", "oas", None)
                .await
                .unwrap_err();
            assert_eq!(
                err,
                "This PactFlow server keeps one provider contract per provider version, so delete-provider-contract cannot be honoured. Upgrade PactFlow, or publish one document per provider version without a name."
            );
            assert!(deletes(&requests).is_empty());
        }
    }
}
