//! Retrieval of a contract document that is referenced by URL.
//!
//! Only remote URLs hosted on SwaggerHub are supported as of now; see
//! [`is_supported_contract_url`].
//!
//! Failures are deliberately reported with a single generic message; the
//! underlying cause is only available in debug logs, and credentials are never
//! logged.

use std::time::Duration;

use reqwest_middleware::ClientWithMiddleware;
use serde::Deserialize;
use tracing::debug;

use crate::cli::pact_broker::main::PactBrokerError;

const SUPPORTED_HOST: &str = "api.swaggerhub.com";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// Upper bound on the size of a retrieved contract document.
const MAX_DOCUMENT_BYTES: usize = 10 * 1024 * 1024;

const RETRIEVE_FAILED: &str = "Failed to retrieve the contract from the provided URL";

#[derive(Deserialize)]
struct AccessTokenResponse {
    access_token: String,
}

/// True only for an `https` URL on exactly [`SUPPORTED_HOST`] with no embedded
/// credentials. Anything else is treated as a local file path by the caller.
pub(crate) fn is_supported_contract_url(input: &str) -> bool {
    url::Url::parse(input).is_ok_and(|parsed| {
        parsed.scheme() == "https"
            && parsed.username().is_empty()
            && parsed.password().is_none()
            && parsed.host_str() == Some(SUPPORTED_HOST)
    })
}

fn retrieve_failed() -> PactBrokerError {
    PactBrokerError::ContentError(RETRIEVE_FAILED.to_string())
}

/// Exchanges the PactFlow API token for a short-lived access token.
async fn exchange_token(
    client: &ClientWithMiddleware,
    broker_url: &str,
    api_token: &str,
) -> Result<String, PactBrokerError> {
    let endpoint = format!("{}/auth/sts-token", broker_url.trim_end_matches('/'));
    let response = client
        .post(&endpoint)
        .bearer_auth(api_token)
        .header("Accept", "application/json")
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await
        .map_err(|e| {
            debug!("Token exchange request failed: {e}");
            retrieve_failed()
        })?;
    let status = response.status();
    if !status.is_success() {
        debug!("Token exchange returned {status}");
        return Err(retrieve_failed());
    }
    let body = response.json::<AccessTokenResponse>().await.map_err(|e| {
        debug!("Token exchange response could not be parsed: {e}");
        retrieve_failed()
    })?;
    if body.access_token.is_empty() {
        debug!("Token exchange returned an empty token");
        return Err(retrieve_failed());
    }
    Ok(body.access_token)
}

/// Fetches the document at `url`, authenticating with a token obtained by
/// exchanging `api_token` with the broker at `broker_url`.
///
/// `accept` is sent as the `Accept` header so the document is returned in the
/// format the caller declared for the contract.
pub(crate) async fn fetch_remote_contract(
    client: &ClientWithMiddleware,
    broker_url: &str,
    api_token: &str,
    url: &str,
    accept: &str,
) -> Result<String, PactBrokerError> {
    let access_token = exchange_token(client, broker_url, api_token).await?;
    let response = client
        .get(url)
        .bearer_auth(access_token)
        .header("Accept", accept)
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await
        .map_err(|e| {
            debug!("Contract request failed: {e}");
            retrieve_failed()
        })?;
    let status = response.status();
    if !status.is_success() {
        debug!("Contract request returned {status}");
        return Err(retrieve_failed());
    }
    if response
        .content_length()
        .is_some_and(|len| len > MAX_DOCUMENT_BYTES as u64)
    {
        debug!("Contract response exceeds {MAX_DOCUMENT_BYTES} bytes");
        return Err(retrieve_failed());
    }
    let bytes = response.bytes().await.map_err(|e| {
        debug!("Contract response could not be read: {e}");
        retrieve_failed()
    })?;
    if bytes.len() > MAX_DOCUMENT_BYTES {
        debug!("Contract response exceeds {MAX_DOCUMENT_BYTES} bytes");
        return Err(retrieve_failed());
    }
    String::from_utf8(bytes.to_vec()).map_err(|e| {
        debug!("Contract response is not valid UTF-8: {e}");
        retrieve_failed()
    })
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use axum::{
        Router,
        http::{HeaderMap, StatusCode, Uri},
        routing::{get, post},
    };
    use reqwest_middleware::ClientBuilder;

    use super::{
        MAX_DOCUMENT_BYTES, RETRIEVE_FAILED, fetch_remote_contract, is_supported_contract_url,
    };
    use crate::cli::pact_broker::main::PactBrokerError;

    type Seen = Arc<Mutex<Vec<String>>>;

    /// Serves `/auth/sts-token` and `/doc`, recording each request as
    /// `"<METHOD> <path> auth=<Authorization header> accept=<Accept header>"`.
    async fn spawn(
        token_status: StatusCode,
        token_body: &'static str,
        doc_status: StatusCode,
    ) -> (String, Seen) {
        let seen: Seen = Arc::default();
        let record = |method: &'static str, seen: Seen| {
            move |uri: Uri, headers: HeaderMap| {
                let header = |name: &str| {
                    headers
                        .get(name)
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("")
                        .to_string()
                };
                seen.lock().unwrap().push(format!(
                    "{method} {} auth={} accept={}",
                    uri.path(),
                    header("authorization"),
                    header("accept")
                ));
            }
        };
        let (on_token, on_doc) = (record("POST", seen.clone()), record("GET", seen.clone()));
        let router = Router::new()
            .route(
                "/auth/sts-token",
                post(move |uri: Uri, headers: HeaderMap| async move {
                    on_token(uri, headers);
                    (token_status, token_body)
                }),
            )
            .route(
                "/doc",
                get(move |uri: Uri, headers: HeaderMap| async move {
                    on_doc(uri, headers);
                    (doc_status, "openapi: 3.0.0")
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        (base, seen)
    }

    async fn fetch(base: &str) -> Result<String, PactBrokerError> {
        let client = ClientBuilder::new(reqwest::Client::new()).build();
        fetch_remote_contract(
            &client,
            base,
            "api-token",
            &format!("{base}/doc"),
            "application/yaml",
        )
        .await
    }

    #[tokio::test]
    async fn exchanges_token_then_fetches_document_with_it() {
        let (base, seen) = spawn(
            StatusCode::OK,
            r#"{"access_token":"sts-123","expires_in":3600}"#,
            StatusCode::OK,
        )
        .await;

        assert_eq!(fetch(&base).await.unwrap(), "openapi: 3.0.0");
        assert_eq!(
            *seen.lock().unwrap(),
            vec![
                "POST /auth/sts-token auth=Bearer api-token accept=application/json",
                "GET /doc auth=Bearer sts-123 accept=application/yaml",
            ]
        );
    }

    #[tokio::test]
    async fn failures_use_one_generic_message_and_reveal_nothing() {
        let ok_token = r#"{"access_token":"sts-123","expires_in":3600}"#;
        let cases = [
            (
                "token endpoint rejects",
                StatusCode::UNAUTHORIZED,
                ok_token,
                StatusCode::OK,
            ),
            (
                "token body malformed",
                StatusCode::OK,
                "not json",
                StatusCode::OK,
            ),
            (
                "token empty",
                StatusCode::OK,
                r#"{"access_token":""}"#,
                StatusCode::OK,
            ),
            (
                "document not found",
                StatusCode::OK,
                ok_token,
                StatusCode::NOT_FOUND,
            ),
            (
                "document forbidden",
                StatusCode::OK,
                ok_token,
                StatusCode::FORBIDDEN,
            ),
        ];
        for (name, token_status, token_body, doc_status) in cases {
            let (base, _) = spawn(token_status, token_body, doc_status).await;
            let error = fetch(&base).await.unwrap_err();
            let message = error.to_string();
            assert!(
                matches!(&error, PactBrokerError::ContentError(m) if m == RETRIEVE_FAILED),
                "{name}: {message}"
            );
            for leaked in [
                "sts",
                "token",
                "swagger",
                "401",
                "403",
                "404",
                "sts-123",
                "api-token",
            ] {
                assert!(
                    !message.to_lowercase().contains(leaked),
                    "{name}: leaked '{leaked}' in '{message}'"
                );
            }
        }
    }

    #[tokio::test]
    async fn unreachable_server_uses_the_same_generic_message() {
        let error = fetch("http://127.0.0.1:1").await.unwrap_err();
        assert!(matches!(&error, PactBrokerError::ContentError(m) if m == RETRIEVE_FAILED));
    }

    #[test]
    fn supported_urls() {
        for url in [
            "https://api.swaggerhub.com/apis/OWNER/petstore-api/1.0.0",
            "https://api.swaggerhub.com/apis/O/a/1?x=y#z",
            "HTTPS://API.SWAGGERHUB.COM/apis/O/a/1",
        ] {
            assert!(is_supported_contract_url(url), "{url}");
        }
    }

    #[test]
    fn everything_else_is_not_a_supported_url() {
        for input in [
            "tests/fixtures/provider-contract.yaml",
            "https://",
            "https:///apis/x",
            "http://api.swaggerhub.com/apis/O/a/1",
            "ftp://api.swaggerhub.com/apis/O/a/1",
            "https://example.com/apis/O/a/1",
            "https://api.swaggerhub.com.evil.com/a",
            "https://evil.api.swaggerhub.com/a",
            "https://api.swaggerhub.com@evil.com/a",
            "https://user@api.swaggerhub.com/a",
            "https://user:pw@api.swaggerhub.com/a",
        ] {
            assert!(!is_supported_contract_url(input), "{input}");
        }
    }

    #[tokio::test]
    async fn oversized_document_uses_the_generic_message() {
        let router = Router::new()
            .route(
                "/auth/sts-token",
                post(|| async { r#"{"access_token":"sts-123","expires_in":3600}"# }),
            )
            .route("/doc", get(|| async { "x".repeat(MAX_DOCUMENT_BYTES + 1) }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });

        let error = fetch(&base).await.unwrap_err();
        assert!(matches!(&error, PactBrokerError::ContentError(m) if m == RETRIEVE_FAILED));
    }
}
