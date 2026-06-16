//! Structs and functions for interacting with a Pact Broker

use std::collections::HashMap;
use std::ops::Not;
use std::panic::RefUnwindSafe;
use std::str::from_utf8;

use anyhow::anyhow;
use futures::stream::*;

use itertools::Itertools;
use maplit::hashmap;

use pact_models::http_utils;
use pact_models::http_utils::HttpAuth;
use pact_models::json_utils::json_to_string;

#[derive(Debug, Clone)]
pub struct CustomHeaders {
    pub headers: std::collections::HashMap<String, String>,
}
use pact_models::pact::{Pact, load_pact_from_json};
use regex::{Captures, Regex};
use reqwest::{Method, Url};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use serde_with::skip_serializing_none;
use tracing::{debug, error, info, trace, warn};
pub mod branches;
pub mod can_i_deploy;
pub mod deployments;
pub mod environments;
pub mod pact_publish;
pub mod pacticipants;
pub mod pacts;
pub mod provider_states;
pub mod subcommands;
pub mod tags;
#[cfg(test)]
pub mod test_utils;
pub mod types;
pub mod utils;
pub mod verification;
pub mod versions;
pub mod webhooks;
// for otel
use crate::cli::utils::{CYAN, GREEN, RED, YELLOW};
use http::Extensions;
use opentelemetry::Context;
use opentelemetry::global;
use opentelemetry_http::HeaderInjector;
use reqwest::Request;
use reqwest::Response;
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use reqwest_middleware::{Middleware, Next};
use reqwest_retry::{DefaultRetryableStrategy, Retryable, RetryableStrategy};
use reqwest_tracing::TracingMiddleware;

use crate::cli::pact_broker::main::types::SslOptions;

pub fn process_notices(notices: &[Notice]) {
    for notice in notices {
        let notice_text = notice.text.to_string();
        let formatted_text = notice_text
            .split_whitespace()
            .map(|word| {
                if word.starts_with("https") || word.starts_with("http") {
                    format!("{}", CYAN.apply_to(word))
                } else {
                    match notice.type_field.as_str() {
                        "success" => format!("{}", GREEN.apply_to(word)),
                        "warning" | "prompt" => format!("{}", YELLOW.apply_to(word)),
                        "error" | "danger" => format!("{}", RED.apply_to(word)),
                        _ => word.to_string(),
                    }
                }
            })
            .collect::<Vec<String>>()
            .join(" ");
        println!("{}", formatted_text);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Notice {
    pub text: String,
    #[serde(rename = "type")]
    pub type_field: String,
}

fn is_true(object: &serde_json::Map<String, Value>, field: &str) -> bool {
    match object.get(field) {
        Some(serde_json::Value::Bool(b)) => *b,
        _ => false,
    }
}

fn as_string(json: &Value) -> String {
    match *json {
        serde_json::Value::String(ref s) => s.clone(),
        _ => format!("{}", json),
    }
}

fn content_type(response: &reqwest::Response) -> String {
    match response.headers().get("content-type") {
        Some(value) => value.to_str().unwrap_or("text/plain").into(),
        None => "text/plain".to_string(),
    }
}

fn json_content_type(response: &reqwest::Response) -> bool {
    match content_type(response).parse::<mime::Mime>() {
        Ok(mime) => matches!(
            (
                mime.type_().as_str(),
                mime.subtype().as_str(),
                mime.suffix()
            ),
            ("application", "json", None) | ("application", "hal", Some(mime::JSON))
        ),
        Err(_) => false,
    }
}

fn find_entry(map: &serde_json::Map<String, Value>, key: &str) -> Option<(String, Value)> {
    match map.keys().find(|k| k.to_lowercase() == key.to_lowercase()) {
        Some(k) => map.get(k).map(|v| (key.to_string(), v.clone())),
        None => None,
    }
}

/// Errors that can occur with a Pact Broker
#[derive(Debug, Clone, thiserror::Error)]
pub enum PactBrokerError {
    /// Error with a HAL link
    #[error("Error with a HAL link - {0}")]
    LinkError(String),
    /// Error with the content of a HAL resource
    #[error("Error with the content of a HAL resource - {0}")]
    ContentError(String),
    #[error("IO Error - {0}")]
    /// IO Error
    IoError(String),
    /// Link/Resource was not found
    #[error("Link/Resource was not found - {0}")]
    NotFound(String),
    /// Invalid URL
    #[error("Invalid URL - {0}")]
    UrlError(String),
    /// Validation error
    #[error("failed validation - {0:?}")]
    ValidationError(Vec<String>),
    /// Validation error with notices
    #[error("failed validation - {0:?}")]
    ValidationErrorWithNotices(Vec<String>, Vec<Notice>),
}

impl PartialEq<String> for PactBrokerError {
    fn eq(&self, other: &String) -> bool {
        let mut buffer = String::new();
        match self {
            PactBrokerError::LinkError(s) => buffer.push_str(s),
            PactBrokerError::ContentError(s) => buffer.push_str(s),
            PactBrokerError::IoError(s) => buffer.push_str(s),
            PactBrokerError::NotFound(s) => buffer.push_str(s),
            PactBrokerError::UrlError(s) => buffer.push_str(s),
            PactBrokerError::ValidationError(errors) => {
                buffer.push_str(errors.iter().join(", ").as_str())
            }
            PactBrokerError::ValidationErrorWithNotices(errors, _) => {
                buffer.push_str(errors.iter().join(", ").as_str())
            }
        };
        buffer == *other
    }
}

impl PartialEq<&str> for PactBrokerError {
    fn eq(&self, other: &&str) -> bool {
        let message = match self {
            PactBrokerError::LinkError(s) => s.clone(),
            PactBrokerError::ContentError(s) => s.clone(),
            PactBrokerError::IoError(s) => s.clone(),
            PactBrokerError::NotFound(s) => s.clone(),
            PactBrokerError::UrlError(s) => s.clone(),
            PactBrokerError::ValidationError(errors) => errors.iter().join(", "),
            PactBrokerError::ValidationErrorWithNotices(errors, _) => errors.iter().join(", "),
        };
        message.as_str() == *other
    }
}

impl From<url::ParseError> for PactBrokerError {
    fn from(err: url::ParseError) -> Self {
        PactBrokerError::UrlError(format!("{}", err))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
/// Structure to represent a HAL link
pub struct Link {
    /// Link name
    pub name: String,
    /// Link HREF
    pub href: Option<String>,
    /// If the link is templated (has expressions in the HREF that need to be expanded)
    pub templated: bool,
    /// Link title
    pub title: Option<String>,
}

impl Link {
    /// Create a link from serde JSON data
    pub fn from_json(link: &str, link_data: &serde_json::Map<String, serde_json::Value>) -> Link {
        Link {
            name: link.to_string(),
            href: find_entry(link_data, "href").map(|(_, href)| as_string(&href)),
            templated: is_true(link_data, "templated"),
            title: link_data.get("title").map(as_string),
        }
    }

    /// Converts the Link into a JSON representation
    pub fn as_json(&self) -> serde_json::Value {
        match (self.href.clone(), self.title.clone()) {
            (Some(href), Some(title)) => json!({
              "href": href,
              "title": title,
              "templated": self.templated
            }),
            (Some(href), None) => json!({
              "href": href,
              "templated": self.templated
            }),
            (None, Some(title)) => json!({
              "title": title,
              "templated": self.templated
            }),
            (None, None) => json!({
              "templated": self.templated
            }),
        }
    }
}

impl Default for Link {
    fn default() -> Self {
        Link {
            name: "link".to_string(),
            href: None,
            templated: false,
            title: None,
        }
    }
}

/// HAL aware HTTP client
#[derive(Clone)]
pub struct HALClient {
    pub url: String,
    pub client: ClientWithMiddleware,
    path_info: Option<Value>,
    auth: Option<HttpAuth>,
    custom_headers: Option<CustomHeaders>,
    ssl_options: SslOptions,
    pub retries: u8,
}

struct OtelPropagatorMiddleware;

#[async_trait::async_trait]
impl Middleware for OtelPropagatorMiddleware {
    async fn handle(
        &self,
        mut req: Request,
        extensions: &mut Extensions,
        next: Next<'_>,
    ) -> reqwest_middleware::Result<Response> {
        let cx = Context::current();
        let mut headers = reqwest::header::HeaderMap::new();
        global::get_text_map_propagator(|propagator| {
            propagator.inject_context(&cx, &mut HeaderInjector(&mut headers))
        });
        headers.append(
            "baggage",
            reqwest::header::HeaderValue::from_static("is_synthetic=true"),
        );

        for (key, value) in headers.iter() {
            req.headers_mut().append(key, value.clone());
        }

        next.run(req, extensions).await
    }
}

/// Parses the value of a `Retry-After` response header into a [`Duration`].
///
/// The header may be either:
/// - an integer number of seconds (`Retry-After: 120`), or
/// - an HTTP-date (`Retry-After: Fri, 31 Dec 1999 23:59:59 GMT`).
///
/// For a date in the past the returned duration is [`Duration::ZERO`].
/// Returns `None` if the header is absent or unparseable.
///
/// # Arguments
///
/// * `response` - The HTTP response to inspect.
///
/// # Returns
///
/// The wait duration indicated by the header, or `None` if absent/unparseable.
fn parse_retry_after(response: &Response) -> Option<std::time::Duration> {
    let header_value = response
        .headers()
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?;

    // Try the decimal-seconds form first (e.g. "120").
    if let Ok(secs) = header_value.trim().parse::<u64>() {
        return Some(std::time::Duration::from_secs(secs));
    }

    // Fall back to the HTTP-date form (e.g. "Fri, 31 Dec 1999 23:59:59 GMT").
    if let Ok(system_time) = httpdate::parse_http_date(header_value) {
        let delay = system_time
            .duration_since(std::time::SystemTime::now())
            .unwrap_or_default();
        return Some(delay);
    }

    None
}

/// Middleware that retries transient HTTP failures and honours `Retry-After` headers.
///
/// Uses [`DefaultRetryableStrategy`] to classify responses: 5xx, 408, and 429 are
/// treated as transient and retried.
///
/// For `429 Too Many Requests` responses the `Retry-After` header is read when
/// present; both the decimal-seconds form (`Retry-After: 120`) and the HTTP-date
/// form (`Retry-After: Fri, 31 Dec 1999 23:59:59 GMT`) are supported.  The parsed
/// delay is passed to [`utils::compute_retry_delay`], which adds a ≈20 % jitter
/// (capped at 60 s) to spread simultaneous retries across the new rate-limit window.
///
/// All other transient failures use exponential back-off (`10^attempt` ms).
///
/// Requests with streaming bodies that cannot be cloned produce an error on the first
/// transient failure without retrying.
struct RetryMiddleware {
    /// Maximum number of total attempts, including the initial send.  `0` means
    /// one attempt with no retries (same as `1`).
    max_attempts: u8,
}

#[async_trait::async_trait]
impl Middleware for RetryMiddleware {
    async fn handle(
        &self,
        req: Request,
        extensions: &mut Extensions,
        next: Next<'_>,
    ) -> reqwest_middleware::Result<Response> {
        let max_retries = self.max_attempts.saturating_sub(1) as u32;
        let mut n_past_retries: u32 = 0;

        loop {
            // Clone the request so we still hold it for subsequent retry iterations.
            let cloned = req.try_clone().ok_or_else(|| {
                reqwest_middleware::Error::Middleware(anyhow::anyhow!(
                    "Request object is not cloneable. Are you passing a streaming body?"
                ))
            })?;

            let result = next.clone().run(cloned, extensions).await;

            // Classify the response using reqwest-retry's built-in strategy.
            if let Some(Retryable::Transient) = DefaultRetryableStrategy.handle(&result)
                && n_past_retries < max_retries
            {
                let delay = if let Ok(ref resp) = result {
                    utils::compute_retry_delay(
                        resp.status(),
                        parse_retry_after(resp),
                        n_past_retries + 1,
                    )
                } else {
                    utils::compute_retry_delay(
                        reqwest::StatusCode::INTERNAL_SERVER_ERROR,
                        None,
                        n_past_retries + 1,
                    )
                };
                trace!(
                    attempt = n_past_retries + 1,
                    max_attempts = self.max_attempts,
                    delay_ms = delay.as_millis(),
                    "retrying transient HTTP failure"
                );
                tokio::time::sleep(delay).await;
                n_past_retries += 1;
                continue;
            }

            break result;
        }
    }
}

pub trait WithCurrentSpan {
    fn with_current_span<F, R>(&self, f: F) -> R
    where
        F: FnOnce() -> R;
}

impl<T> WithCurrentSpan for T {
    fn with_current_span<F, R>(&self, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        let span = tracing::Span::current();
        let _enter = span.enter();
        f()
    }
}

impl HALClient {
    /// Helper method to apply custom headers to a request builder
    fn apply_custom_headers(
        &self,
        mut builder: reqwest_middleware::RequestBuilder,
    ) -> reqwest_middleware::RequestBuilder {
        if let Some(ref custom_headers) = self.custom_headers {
            for (name, value) in &custom_headers.headers {
                builder = builder.header(name, value);
            }
        }
        builder
    }

    /// Initialise a client with the URL and authentication
    pub fn with_url(
        url: &str,
        auth: Option<HttpAuth>,
        ssl_options: SslOptions,
        custom_headers: Option<CustomHeaders>,
    ) -> HALClient {
        HALClient {
            url: url.to_string(),
            auth: auth.clone(),
            custom_headers,
            ssl_options: ssl_options.clone(),
            ..HALClient::setup(url, auth, ssl_options)
        }
    }

    fn update_path_info(self, path_info: serde_json::Value) -> HALClient {
        HALClient {
            client: self.client.clone(),
            url: self.url.clone(),
            path_info: Some(path_info),
            auth: self.auth,
            custom_headers: self.custom_headers,
            retries: self.retries,
            ssl_options: self.ssl_options,
        }
    }

    /// Navigate to the resource from the link name
    pub async fn navigate(
        self,
        link: &'static str,
        template_values: &HashMap<String, String>,
    ) -> Result<HALClient, PactBrokerError> {
        trace!(
            "navigate(link='{}', template_values={:?})",
            link, template_values
        );

        let client = if self.path_info.is_none() {
            let path_info = self.fetch("").await?;
            self.update_path_info(path_info)
        } else {
            self
        };

        let path_info = client.clone().fetch_link(link, template_values).await?;
        let client = client.update_path_info(path_info);

        Ok(client)
    }

    fn find_link(&self, link: &'static str) -> Result<Link, PactBrokerError> {
        match self.path_info {
            None => Err(PactBrokerError::LinkError(format!("No previous resource has been fetched from the pact broker. URL: '{}', LINK: '{}'",
                self.url, link))),
            Some(ref json) => match json.get("_links") {
                Some(json) => match json.get(link) {
                    Some(link_data) => link_data.as_object()
                        .map(|link_data| Link::from_json(link, link_data))
                        .ok_or_else(|| PactBrokerError::LinkError(format!("Link is malformed, expected an object but got {}. URL: '{}', LINK: '{}'",
                            link_data, self.url, link))),
                    None => Err(PactBrokerError::LinkError(format!("Link '{}' was not found in the response, only the following links where found: {:?}. URL: '{}', LINK: '{}'",
                        link, json.as_object().unwrap_or(json!({}).as_object().unwrap()).keys().join(", "), self.url, link)))
                },
                None => Err(PactBrokerError::LinkError(format!("Expected a HAL+JSON response from the pact broker, but got a response with no '_links'. URL: '{}', LINK: '{}'",
                    self.url, link)))
            }
        }
    }

    async fn fetch_link(
        self,
        link: &'static str,
        template_values: &HashMap<String, String>,
    ) -> Result<Value, PactBrokerError> {
        trace!(
            "fetch_link(link='{}', template_values={:?})",
            link, template_values
        );

        let link_data = self.find_link(link)?;

        self.fetch_url(&link_data, template_values).await
    }

    /// Fetch the resource at the Link from the Pact broker
    pub async fn fetch_url(
        self,
        link: &Link,
        template_values: &HashMap<String, String>,
    ) -> Result<Value, PactBrokerError> {
        debug!(
            "fetch_url(link={:?}, template_values={:?})",
            link, template_values
        );

        let link_url = if link.templated {
            debug!("Link URL is templated");
            self.parse_link_url(link, template_values)
        } else {
            link.href.clone().ok_or_else(|| {
                PactBrokerError::LinkError(format!(
                    "Link is malformed, there is no href. URL: '{}', LINK: '{}'",
                    self.url, link.name
                ))
            })
        }?;

        let base_url = self.url.parse::<Url>()?;
        let joined_url = base_url.join(&link_url)?;
        self.fetch(joined_url.path()).await
    }
    pub async fn delete_url(
        self,
        link: &Link,
        template_values: &HashMap<String, String>,
    ) -> Result<Value, PactBrokerError> {
        debug!(
            "fetch_url(link={:?}, template_values={:?})",
            link, template_values
        );

        let link_url = if link.templated {
            debug!("Link URL is templated");
            self.parse_link_url(link, template_values)
        } else {
            link.href.clone().ok_or_else(|| {
                PactBrokerError::LinkError(format!(
                    "Link is malformed, there is no href. URL: '{}', LINK: '{}'",
                    self.url, link.name
                ))
            })
        }?;

        let base_url = self.url.parse::<Url>()?;
        debug!("base_url: {}", base_url);
        debug!("link_url: {}", link_url);
        let joined_url = base_url.join(&link_url)?;
        debug!("joined_url: {}", joined_url);
        self.delete(joined_url.path()).await
    }

    pub async fn fetch(&self, path: &str) -> Result<Value, PactBrokerError> {
        info!("Fetching path '{}' from pact broker", path);
        trace!(%path, broker_url = %self.url, ">> fetch");
        let url = self.resolve_path(path)?;
        debug!("Final broker URL: {}", url);

        let mut request_builder = match self.auth {
            Some(ref auth) => match auth {
                HttpAuth::User(username, password) => {
                    self.client.get(url).basic_auth(username, password.clone())
                }
                HttpAuth::Token(token) => self.client.get(url).bearer_auth(token),
                _ => self.client.get(url),
            },
            None => self.client.get(url),
        }
        .header("accept", "application/hal+json, application/json");

        // Apply custom headers if present
        request_builder = self.apply_custom_headers(request_builder);

        let response = request_builder.send().await.map_err(|err| {
            PactBrokerError::IoError(format!(
                "Failed to access pact broker path '{}' - {}. URL: '{}'",
                &path, err, &self.url,
            ))
        })?;

        self.parse_broker_response(path.to_string(), response).await
    }

    fn resolve_path(&self, path: &str) -> Result<Url, PactBrokerError> {
        let broker_url = self.url.parse::<Url>()?;
        let context_path = broker_url.path();
        let url = if path.is_empty() {
            broker_url
        } else if !context_path.is_empty() && context_path != "/" {
            if path.starts_with(context_path) {
                let mut base_url = broker_url.clone();
                base_url.set_path("/");
                base_url.join(path)?
            } else if path.starts_with("/") {
                let mut base_url = broker_url.clone();
                base_url.set_path(path);
                base_url
            } else {
                let mut base_url = broker_url.clone();
                let mut cp = context_path.to_string();
                cp.push('/');
                base_url.set_path(cp.as_str());
                base_url.join(path)?
            }
        } else {
            broker_url.join(path)?
        };
        Ok(url)
    }

    pub async fn delete(self, path: &str) -> Result<Value, PactBrokerError> {
        info!("Deleting path '{}' from pact broker", path);

        let broker_url = self.url.parse::<Url>()?;
        let context_path = broker_url.path();
        let url = if context_path.is_empty().not()
            && context_path != "/"
            && path.starts_with(context_path)
        {
            let mut base_url = broker_url.clone();
            base_url.set_path("/");
            base_url.join(path)?
        } else {
            broker_url.join(path)?
        };

        let request_builder = match self.auth {
            Some(ref auth) => match auth {
                HttpAuth::User(username, password) => self
                    .client
                    .delete(url)
                    .basic_auth(username, password.clone()),
                HttpAuth::Token(token) => self.client.delete(url).bearer_auth(token),
                _ => self.client.delete(url),
            },
            None => self.client.delete(url),
        }
        .header("Accept", "application/hal+json");

        let response = request_builder.send().await.map_err(|err| {
            PactBrokerError::IoError(format!(
                "Failed to delete pact broker path '{}' - {}. URL: '{}'",
                &path, err, &self.url,
            ))
        })?;

        self.parse_broker_response(path.to_string(), response).await
    }

    async fn parse_broker_response(
        &self,
        path: String,
        response: reqwest::Response,
    ) -> Result<Value, PactBrokerError> {
        let is_json_content_type = json_content_type(&response);
        let content_type = content_type(&response);
        let status_code = response.status();

        if status_code.is_success() {
            if is_json_content_type {
                response.json::<Value>()
            .await
            .map_err(|err| PactBrokerError::ContentError(
              format!("Did not get a valid HAL response body from pact broker path '{}' - {}. URL: '{}'",
                      path, err, self.url)
            ))
            } else if status_code.as_u16() == 204 {
                Ok(json!({}))
            } else {
                debug!("Request from broker was a success, but the response body was not JSON");
                Err(PactBrokerError::ContentError(format!(
                    "Did not get a valid HAL response body from pact broker path '{}', content type is '{}'. URL: '{}'",
                    path, content_type, self.url
                )))
            }
        } else if status_code.as_u16() == 404 {
            Err(PactBrokerError::NotFound(format!(
                "Request to pact broker path '{}' failed: {}. URL: '{}'",
                path, status_code, self.url
            )))
        } else {
            // Handle any error status code (400, 422, 409, etc.)
            let body = response.bytes().await.map_err(|_| {
                PactBrokerError::IoError(format!(
                    "Failed to download response body for path '{}'. URL: '{}'",
                    &path, self.url
                ))
            })?;

            if is_json_content_type {
                match serde_json::from_slice::<Value>(&body) {
                    Ok(json_body) => {
                        if json_body.get("errors").is_some() || json_body.get("notices").is_some() {
                            Err(handle_validation_errors(json_body))
                        } else {
                            Err(PactBrokerError::IoError(format!(
                                "Request to pact broker path '{}' failed: {}. Response: {}. URL: '{}'",
                                path, status_code, json_body, self.url
                            )))
                        }
                    }
                    Err(_) => {
                        let body_text = from_utf8(&body)
                            .map(|b| b.to_string())
                            .unwrap_or_else(|err| format!("could not read body: {}", err));
                        error!(
                            "Request to pact broker path '{}' failed: {}",
                            path, body_text
                        );
                        Err(PactBrokerError::IoError(format!(
                            "Request to pact broker path '{}' failed: {}. URL: '{}'",
                            path, status_code, self.url
                        )))
                    }
                }
            } else {
                let body_text = from_utf8(&body)
                    .map(|b| b.to_string())
                    .unwrap_or_else(|err| format!("could not read body: {}", err));
                error!(
                    "Request to pact broker path '{}' failed: {}",
                    path, body_text
                );
                Err(PactBrokerError::IoError(format!(
                    "Request to pact broker path '{}' failed: {}. URL: '{}'",
                    path, status_code, self.url
                )))
            }
        }
    }

    fn parse_link_url(
        &self,
        link: &Link,
        values: &HashMap<String, String>,
    ) -> Result<String, PactBrokerError> {
        match link.href {
            Some(ref href) => {
                debug!("templated URL = {}", href);
                let re = Regex::new(r"\{(\w+)}").unwrap();
                let final_url = re.replace_all(href, |caps: &Captures| {
                    let lookup = caps.get(1).unwrap().as_str();
                    trace!("Looking up value for key '{}'", lookup);
                    match values.get(lookup) {
                        Some(val) => urlencoding::encode(val.as_str()).to_string(),
                        None => {
                            warn!(
                                "No value was found for key '{}', mapped values are {:?}",
                                lookup, values
                            );
                            format!("{{{}}}", lookup)
                        }
                    }
                });
                debug!("final URL = {}", final_url);
                Ok(final_url.to_string())
            }
            None => Err(PactBrokerError::LinkError(format!(
                "Expected a HAL+JSON response from the pact broker, but got a link with no HREF. URL: '{}', LINK: '{}'",
                self.url, link.name
            ))),
        }
    }

    /// Iterate over all the links by name
    pub fn iter_links(&self, link: &str) -> Result<Vec<Link>, PactBrokerError> {
        match self.path_info {
      None => Err(PactBrokerError::LinkError(format!("No previous resource has been fetched from the pact broker. URL: '{}', LINK: '{}'",
        self.url, link))),
      Some(ref json) => match json.get("_links") {
        Some(json) => match json.get(link) {
          Some(link_data) => link_data.as_array()
              .map(|link_data| link_data.iter().map(|link_json| match link_json {
                Value::Object(data) => Link::from_json(link, data),
                Value::String(s) => Link { name: link.to_string(), href: Some(s.clone()), templated: false, title: None },
                _ => Link { name: link.to_string(), href: Some(link_json.to_string()), templated: false, title: None }
              }).collect())
              .ok_or_else(|| PactBrokerError::LinkError(format!("Link is malformed, expected an object but got {}. URL: '{}', LINK: '{}'",
                  link_data, self.url, link))),
          None => Err(PactBrokerError::LinkError(format!("Link '{}' was not found in the response, only the following links where found: {:?}. URL: '{}', LINK: '{}'",
            link, json.as_object().unwrap_or(json!({}).as_object().unwrap()).keys().join(", "), self.url, link)))
        },
        None => Err(PactBrokerError::LinkError(format!("Expected a HAL+JSON response from the pact broker, but got a response with no '_links'. URL: '{}', LINK: '{}'",
          self.url, link)))
      }
    }
    }

    pub async fn post_json(
        &self,
        url: &str,
        body: &str,
        headers: Option<HashMap<String, String>>,
    ) -> Result<serde_json::Value, PactBrokerError> {
        trace!("post_json(url='{}', body='{}')", url, body);

        self.send_document(url, body, Method::POST, headers).await
    }

    pub async fn put_json(
        &self,
        url: &str,
        body: &str,
        headers: Option<HashMap<String, String>>,
    ) -> Result<serde_json::Value, PactBrokerError> {
        trace!("put_json(url='{}', body='{}')", url, body);

        self.send_document(url, body, Method::PUT, headers).await
    }
    pub async fn patch_json(
        &self,
        url: &str,
        body: &str,
        headers: Option<HashMap<String, String>>,
    ) -> Result<serde_json::Value, PactBrokerError> {
        trace!("put_json(url='{}', body='{}')", url, body);

        self.send_document(url, body, Method::PATCH, headers).await
    }

    async fn send_document(
        &self,
        url: &str,
        body: &str,
        method: Method,
        headers: Option<HashMap<String, String>>,
    ) -> Result<Value, PactBrokerError> {
        let method_type = method.clone();
        debug!("Sending JSON to {} using {}: {}", url, method, body);

        let base_url = &self.url.parse::<Url>()?;
        let url = if url.starts_with("/") {
            base_url.join(url)?
        } else {
            let url = url.parse::<Url>()?;
            base_url.join(url.path())?
        };

        let request_builder = match self.auth {
            Some(ref auth) => match auth {
                HttpAuth::User(username, password) => self
                    .client
                    .request(method, url.clone())
                    .basic_auth(username, password.clone()),
                HttpAuth::Token(token) => {
                    self.client.request(method, url.clone()).bearer_auth(token)
                }
                _ => self.client.request(method, url.clone()),
            },
            None => self.client.request(method, url.clone()),
        }
        .header("Accept", "application/hal+json")
        .body(body.to_string());

        // Add any additional headers if provided

        let request_builder = if let Some(ref headers) = headers {
            headers
                .iter()
                .fold(request_builder, |builder, (key, value)| {
                    builder.header(key.as_str(), value.as_str())
                })
        } else {
            request_builder
        };

        let request_builder = if method_type == Method::PATCH {
            request_builder.header("Content-Type", "application/merge-patch+json")
        } else {
            request_builder.header("Content-Type", "application/json")
        };
        match request_builder.send().await {
            Ok(res) => {
                self.parse_broker_response(url.path().to_string(), res)
                    .await
            }
            Err(err) => Err(PactBrokerError::IoError(format!(
                "Failed to send JSON to the pact broker URL '{}' - IoError {}",
                url, err
            ))),
        }
    }
}

fn handle_validation_errors(body: Value) -> PactBrokerError {
    match &body {
        Value::Object(attrs) => {
            // Extract notices if present
            let notices: Vec<Notice> = attrs
                .get("notices")
                .and_then(|n| n.as_array())
                .map(|notices_array| {
                    notices_array
                        .iter()
                        .filter_map(|notice| serde_json::from_value::<Notice>(notice.clone()).ok())
                        .collect()
                })
                .unwrap_or_default();

            if let Some(errors) = attrs.get("errors") {
                let error_messages = match errors {
                    Value::Array(values) => values.iter().map(json_to_string).collect(),
                    Value::Object(errors) => errors
                        .iter()
                        .map(|(field, errors)| match errors {
                            Value::String(error) => format!("{}: {}", field, error),
                            Value::Array(errors) => format!(
                                "{}: {}",
                                field,
                                errors.iter().map(json_to_string).join(", ")
                            ),
                            _ => format!("{}: {}", field, errors),
                        })
                        .collect(),
                    Value::String(s) => vec![s.clone()],
                    _ => vec![errors.to_string()],
                };

                if !notices.is_empty() {
                    PactBrokerError::ValidationErrorWithNotices(error_messages, notices)
                } else {
                    PactBrokerError::ValidationError(error_messages)
                }
            } else if !notices.is_empty() {
                // Even if there are no explicit errors, notices might contain error information
                let notice_messages = notices.iter().map(|n| n.text.clone()).collect();
                PactBrokerError::ValidationErrorWithNotices(notice_messages, notices)
            } else {
                PactBrokerError::ValidationError(vec![body.to_string()])
            }
        }
        Value::String(s) => PactBrokerError::ValidationError(vec![s.clone()]),
        _ => PactBrokerError::ValidationError(vec![body.to_string()]),
    }
}

impl HALClient {
    /// Builds the reqwest-middleware client stack for a given retry count and SSL
    /// configuration.
    ///
    /// The middleware chain is (outermost → innermost):
    /// 1. [`TracingMiddleware`] — adds OpenTelemetry trace context to every request.
    /// 2. [`OtelPropagatorMiddleware`] — injects baggage / W3C trace propagation headers.
    /// 3. [`RetryMiddleware`] — retries transient 5xx / 408 / 429 failures, honouring
    ///    any `Retry-After` header present on the response.
    ///
    /// # Arguments
    ///
    /// * `retries` - Maximum number of total attempts (including the first send).
    /// * `ssl_options` - TLS configuration (custom CA cert, skip-verify, …).
    ///
    /// # Returns
    ///
    /// A fully configured [`ClientWithMiddleware`] ready for use.
    fn build_middleware_client(retries: u8, ssl_options: &SslOptions) -> ClientWithMiddleware {
        let mut builder = reqwest::Client::builder().user_agent(format!(
            "{}/{}",
            env!("CARGO_PKG_NAME"),
            env!("CARGO_PKG_VERSION")
        ));

        debug!("Using ssl_options: {:?}", ssl_options);
        if let Some(ref path) = ssl_options.ssl_cert_path {
            if let Ok(cert_bytes) = std::fs::read(path) {
                match reqwest::Certificate::from_pem_bundle(&cert_bytes) {
                    Ok(certs) => {
                        debug!("Adding SSL certificate from path: {}", path);
                        if ssl_options.use_root_trust_store {
                            // Merge custom cert into the native root store.
                            builder = builder.tls_certs_merge(certs);
                        } else {
                            // Use ONLY the provided certificate; bypass all built-in roots.
                            debug!(
                                "Disabling root trust store for SSL — using only the provided certificate"
                            );
                            builder = builder.tls_certs_only(certs);
                        }
                    }
                    Err(err) => {
                        debug!(
                            "Could not parse SSL certificate from path {}: {}",
                            path, err
                        );
                    }
                }
            } else {
                debug!(
                    "Could not read SSL certificate from provided path: {}",
                    path
                );
            }
        } else if !ssl_options.use_root_trust_store {
            debug!(
                "ssl-trust-store disabled but no custom certificate provided; proceeding with system roots"
            );
        }
        if ssl_options.skip_ssl {
            builder = builder.danger_accept_invalid_certs(true);
            debug!("Skipping SSL certificate validation");
        }

        let built_client = builder.build().expect("failed to build reqwest client");
        ClientBuilder::new(built_client)
            .with(TracingMiddleware::default())
            .with(OtelPropagatorMiddleware)
            .with(RetryMiddleware {
                max_attempts: retries,
            })
            .build()
    }

    pub fn setup(url: &str, auth: Option<HttpAuth>, ssl_options: SslOptions) -> HALClient {
        let retries = std::env::var("PACT_BROKER_HTTP_RETRIES")
            .ok()
            .and_then(|v| v.parse::<u8>().ok())
            .unwrap_or(8);

        let client = Self::build_middleware_client(retries, &ssl_options);

        HALClient {
            client,
            url: url.to_string(),
            path_info: None,
            auth,
            custom_headers: None,
            retries,
            ssl_options,
        }
    }

    /// Sets the number of HTTP request retry attempts, overriding the default (3) or any
    /// value read from the `PACT_BROKER_HTTP_RETRIES` environment variable.
    ///
    /// CLI command handlers call this after construction to apply the `--retries` flag value.
    /// This rebuilds the internal HTTP client so the new retry count takes effect immediately.
    pub fn with_retry_count(mut self, retries: u8) -> Self {
        self.retries = retries;
        self.client = Self::build_middleware_client(retries, &self.ssl_options);
        self
    }
}

pub fn links_from_json(json: &Value) -> Vec<Link> {
    match json.get("_links") {
        Some(Value::Object(v)) => v
            .iter()
            .map(|(link, json)| match json {
                Value::Object(attr) => Link::from_json(link, attr),
                _ => Link {
                    name: link.clone(),
                    ..Link::default()
                },
            })
            .collect(),
        _ => vec![],
    }
}

/// Fetches the pacts from the broker that match the provider name
pub async fn fetch_pacts_from_broker(
    broker_url: &str,
    provider_name: &str,
    auth: Option<HttpAuth>,
    ssl_options: SslOptions,
    custom_headers: Option<CustomHeaders>,
) -> anyhow::Result<
    Vec<
        anyhow::Result<(
            Box<dyn Pact + Send + Sync + RefUnwindSafe>,
            Option<PactVerificationContext>,
            Vec<Link>,
        )>,
    >,
> {
    trace!(
        "fetch_pacts_from_broker(broker_url='{}', provider_name='{}', auth={})",
        broker_url,
        provider_name,
        auth.clone().unwrap_or_default()
    );

    let mut hal_client = HALClient::with_url(broker_url, auth, ssl_options, custom_headers);
    let template_values = hashmap! { "provider".to_string() => provider_name.to_string() };

    hal_client = hal_client
        .navigate("pb:latest-provider-pacts", &template_values)
        .await
        .map_err(move |err| match err {
            PactBrokerError::NotFound(_) => PactBrokerError::NotFound(format!(
                "No pacts for provider '{}' where found in the pact broker. URL: '{}'",
                provider_name, broker_url
            )),
            _ => err,
        })?;

    let pact_links = hal_client.clone().iter_links("pacts")?;

    let results: Vec<_> = futures::stream::iter(pact_links)
        .map(|ref pact_link| {
          match pact_link.href {
            Some(_) => Ok((hal_client.clone(), pact_link.clone())),
            None => Err(
              PactBrokerError::LinkError(
                format!(
                  "Expected a HAL+JSON response from the pact broker, but got a link with no HREF. URL: '{}', LINK: '{:?}'",
                  &hal_client.url,
                  pact_link
                )
              )
            )
          }
        })
        .and_then(|(hal_client, pact_link)| async {
          let pact_json = hal_client.fetch_url(
            &pact_link.clone(),
            &template_values.clone()
          ).await?;
          Ok((pact_link, pact_json))
        })
        .map(|result| {
          match result {
            Ok((pact_link, pact_json)) => {
              let href = pact_link.href.unwrap_or_default();
              let links = links_from_json(&pact_json);
              load_pact_from_json(href.as_str(), &pact_json)
                .map(|pact| (pact, None, links))
            },
            Err(err) => Err(err.into())
          }
        })
        .into_stream()
        .collect()
        .await;

    Ok(results)
}

/// Fetch Pacts from the broker using the "provider-pacts-for-verification" endpoint
#[allow(clippy::too_many_arguments)]
pub async fn fetch_pacts_dynamically_from_broker(
    broker_url: &str,
    provider_name: String,
    pending: bool,
    include_wip_pacts_since: Option<String>,
    provider_tags: Vec<String>,
    provider_branch: Option<String>,
    consumer_version_selectors: Vec<ConsumerVersionSelector>,
    auth: Option<HttpAuth>,
    ssl_options: SslOptions,
    headers: Option<HashMap<String, String>>,
    custom_headers: Option<CustomHeaders>,
) -> anyhow::Result<
    Vec<
        Result<
            (
                Box<dyn Pact + Send + Sync + RefUnwindSafe>,
                Option<PactVerificationContext>,
                Vec<Link>,
            ),
            PactBrokerError,
        >,
    >,
> {
    trace!(
        "fetch_pacts_dynamically_from_broker(broker_url='{}', provider_name='{}', pending={}, \
    include_wip_pacts_since={:?}, provider_tags: {:?}, consumer_version_selectors: {:?}, auth={})",
        broker_url,
        provider_name,
        pending,
        include_wip_pacts_since,
        provider_tags,
        consumer_version_selectors,
        auth.clone().unwrap_or_default()
    );

    let mut hal_client = HALClient::with_url(broker_url, auth, ssl_options, custom_headers);
    let template_values = hashmap! { "provider".to_string() => provider_name.clone() };

    hal_client = hal_client
        .navigate("pb:provider-pacts-for-verification", &template_values)
        .await
        .map_err(move |err| match err {
            PactBrokerError::NotFound(_) => PactBrokerError::NotFound(format!(
                "No pacts for provider '{}' were found in the pact broker. URL: '{}'",
                provider_name.clone(),
                broker_url
            )),
            _ => err,
        })?;

    // Construct the Pacts for verification payload
    let pacts_for_verification = PactsForVerificationRequest {
        provider_version_tags: provider_tags,
        provider_version_branch: provider_branch,
        include_wip_pacts_since,
        consumer_version_selectors,
        include_pending_status: pending,
    };
    let request_body = serde_json::to_string(&pacts_for_verification).unwrap();

    // Post the verification request
    let response = match hal_client.find_link("self") {
        Ok(link) => {
            let link = hal_client.clone().parse_link_url(&link, &hashmap! {})?;
            match hal_client
                .clone()
                .post_json(link.as_str(), request_body.as_str(), headers)
                .await
            {
                Ok(res) => Some(res),
                Err(err) => {
                    info!("error response for pacts for verification: {} ", err);
                    return Err(anyhow!(err));
                }
            }
        }
        Err(e) => return Err(anyhow!(e)),
    };

    // Find all of the Pact links
    let pact_links = match response {
        Some(v) => {
            let pfv: PactsForVerificationResponse = serde_json::from_value(v)
                .map_err(|err| {
                    trace!(
                        "Failed to deserialise PactsForVerificationResponse: {}",
                        err
                    );
                    err
                })
                .unwrap_or(PactsForVerificationResponse {
                    embedded: PactsForVerificationBody { pacts: vec![] },
                });
            trace!(?pfv, "got pacts for verification response");

            if pfv.embedded.pacts.is_empty() {
                return Err(anyhow!(PactBrokerError::NotFound(
                    "No pacts were found for this provider".to_string()
                )));
            };

            let links: Result<Vec<(Link, PactVerificationContext)>, PactBrokerError> = pfv.embedded.pacts.iter().map(| p| {
          match p.links.get("self") {
            Some(l) => Ok((l.clone(), p.into())),
            None => Err(
              PactBrokerError::LinkError(
                format!(
                  "Expected a HAL+JSON response from the pact broker, but got a link with no HREF. URL: '{}', PATH: '{:?}'",
                  &hal_client.url,
                  &p.links,
                )
              )
            )
          }
        }).collect();

            links
        }
        None => Err(PactBrokerError::NotFound(
            "No pacts were found for this provider".to_string(),
        )),
    }?;

    let results: Vec<_> = futures::stream::iter(pact_links)
      .map(|(ref pact_link, ref context)| {
        match pact_link.href {
          Some(_) => Ok((hal_client.clone(), pact_link.clone(), context.clone())),
          None => Err(
            PactBrokerError::LinkError(
              format!(
                "Expected a HAL+JSON response from the pact broker, but got a link with no HREF. URL: '{}', LINK: '{:?}'",
                &hal_client.url,
                pact_link
              )
            )
          )
        }
      })
      .and_then(|(hal_client, pact_link, context)| async {
        let pact_json = hal_client.fetch_url(
          &pact_link.clone(),
          &template_values.clone()
        ).await?;
        Ok((pact_link, pact_json, context))
      })
      .map(|result| {
        match result {
          Ok((pact_link, pact_json, context)) => {
            let href = pact_link.href.unwrap_or_default();
            let links = links_from_json(&pact_json);
            load_pact_from_json(href.as_str(), &pact_json)
              .map(|pact| (pact, Some(context), links))
              .map_err(|err| PactBrokerError::ContentError(format!("{}", err)))
          },
          Err(err) => Err(err)
        }
      })
      .into_stream()
      .collect()
      .await;

    Ok(results)
}

/// Fetch the Pact from the given URL, using any required authentication. This will use a GET
/// request to the given URL and parse the result into a Pact model. It will also look for any HAL
/// links in the response, returning those if found.
pub async fn fetch_pact_from_url(
    url: &str,
    auth: &Option<HttpAuth>,
) -> anyhow::Result<(Box<dyn Pact + Send + Sync + RefUnwindSafe>, Vec<Link>)> {
    let url = url.to_string();
    let auth = auth.clone();
    let (url, pact_json) =
        tokio::task::spawn_blocking(move || http_utils::fetch_json_from_url(&url, &auth)).await??;
    let pact = load_pact_from_json(&url, &pact_json)?;
    let links = links_from_json(&pact_json);
    Ok((pact, links))
}

#[skip_serializing_none]
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
/// Structure to represent a HAL link
pub struct ConsumerVersionSelector {
    /// Application name to filter the results on
    pub consumer: Option<String>,
    /// Tag
    pub tag: Option<String>,
    /// Fallback tag if Tag doesn't exist
    pub fallback_tag: Option<String>,
    /// Only select the latest (if false, this selects all pacts for a tag)
    pub latest: Option<bool>,
    /// Applications that have been deployed or released
    pub deployed_or_released: Option<bool>,
    /// Applications that have been deployed
    pub deployed: Option<bool>,
    /// Applications that have been released
    pub released: Option<bool>,
    /// Applications in a given environment
    pub environment: Option<String>,
    /// Applications with the default branch set in the broker
    pub main_branch: Option<bool>,
    /// Applications with the given branch
    pub branch: Option<String>,
    /// Applications that match the the provider version branch sent during verification
    pub matching_branch: Option<bool>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
struct PactsForVerificationResponse {
    #[serde(rename(deserialize = "_embedded"))]
    pub embedded: PactsForVerificationBody,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
struct PactsForVerificationBody {
    pub pacts: Vec<PactForVerification>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
struct PactForVerification {
    pub short_description: String,
    #[serde(rename(deserialize = "_links"))]
    pub links: HashMap<String, Link>,
    pub verification_properties: Option<PactVerificationProperties>,
}

#[skip_serializing_none]
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
/// Request to send to determine the pacts to verify
pub struct PactsForVerificationRequest {
    /// Provider tags to use for determining pending pacts (if enabled)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub provider_version_tags: Vec<String>,
    /// Enable pending pacts feature
    pub include_pending_status: bool,
    /// Find WIP pacts after given date
    pub include_wip_pacts_since: Option<String>,
    /// Detailed pact selection criteria , see https://docs.pact.io/pact_broker/advanced_topics/consumer_version_selectors/
    pub consumer_version_selectors: Vec<ConsumerVersionSelector>,
    /// Current provider version branch if used (instead of tags)
    pub provider_version_branch: Option<String>,
}

#[skip_serializing_none]
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
/// Provides the context on why a Pact was included
pub struct PactVerificationContext {
    /// Description
    pub short_description: String,
    /// Properties
    pub verification_properties: PactVerificationProperties,
}

impl From<&PactForVerification> for PactVerificationContext {
    fn from(value: &PactForVerification) -> Self {
        PactVerificationContext {
            short_description: value.short_description.clone(),
            verification_properties: value.verification_properties.clone().unwrap_or_default(),
        }
    }
}

#[skip_serializing_none]
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
/// Properties associated with the verification context
pub struct PactVerificationProperties {
    #[serde(default)]
    /// If the Pact is pending
    pub pending: bool,
    /// Notices provided by the Pact Broker
    pub notices: Vec<HashMap<String, String>>,
}

#[cfg(test)]
mod hal_client_custom_headers_tests {
    use super::*;
    use crate::cli::pact_broker::main::types::SslOptions;
    use std::collections::HashMap;

    fn create_test_custom_headers() -> CustomHeaders {
        let mut headers = HashMap::new();
        headers.insert("Authorization".to_string(), "Bearer test-token".to_string());
        headers.insert("X-API-Key".to_string(), "secret-key".to_string());
        CustomHeaders { headers }
    }

    fn create_cloudflare_custom_headers() -> CustomHeaders {
        let mut headers = HashMap::new();
        headers.insert(
            "CF-Access-Client-Id".to_string(),
            "client-id-123".to_string(),
        );
        headers.insert(
            "CF-Access-Client-Secret".to_string(),
            "secret-456".to_string(),
        );
        CustomHeaders { headers }
    }

    #[test]
    fn test_hal_client_with_custom_headers() {
        let custom_headers = Some(create_test_custom_headers());
        let ssl_options = SslOptions::default();

        let client = HALClient::with_url(
            "https://test.example.com",
            None,
            ssl_options,
            custom_headers.clone(),
        );

        assert_eq!(client.url, "https://test.example.com");
        assert!(client.custom_headers.is_some());

        let headers = client.custom_headers.unwrap();
        assert_eq!(headers.headers.len(), 2);
        assert_eq!(
            headers.headers.get("Authorization"),
            Some(&"Bearer test-token".to_string())
        );
        assert_eq!(
            headers.headers.get("X-API-Key"),
            Some(&"secret-key".to_string())
        );
    }

    #[test]
    fn test_hal_client_with_cloudflare_headers() {
        let custom_headers = Some(create_cloudflare_custom_headers());
        let ssl_options = SslOptions::default();

        let client = HALClient::with_url(
            "https://pact-broker.example.com",
            None,
            ssl_options,
            custom_headers.clone(),
        );

        assert!(client.custom_headers.is_some());

        let headers = client.custom_headers.unwrap();
        assert_eq!(headers.headers.len(), 2);
        assert_eq!(
            headers.headers.get("CF-Access-Client-Id"),
            Some(&"client-id-123".to_string())
        );
        assert_eq!(
            headers.headers.get("CF-Access-Client-Secret"),
            Some(&"secret-456".to_string())
        );
    }

    #[test]
    fn test_hal_client_without_custom_headers() {
        let ssl_options = SslOptions::default();

        let client = HALClient::with_url("https://test.example.com", None, ssl_options, None);

        assert!(client.custom_headers.is_none());
    }

    #[test]
    fn test_hal_client_with_auth_and_custom_headers() {
        let auth = Some(HttpAuth::Token("bearer-token".to_string()));
        let custom_headers = Some(create_test_custom_headers());
        let ssl_options = SslOptions::default();

        let client = HALClient::with_url(
            "https://test.example.com",
            auth.clone(),
            ssl_options,
            custom_headers,
        );

        assert!(client.auth.is_some());
        assert!(client.custom_headers.is_some());

        if let Some(HttpAuth::Token(token)) = client.auth {
            assert_eq!(token, "bearer-token");
        }
    }

    #[test]
    fn test_apply_custom_headers_with_mock_request() {
        use reqwest::Client;
        use reqwest_middleware::ClientBuilder;

        let custom_headers = Some(create_test_custom_headers());
        let ssl_options = SslOptions::default();

        let client = HALClient::with_url(
            "https://test.example.com",
            None,
            ssl_options,
            custom_headers,
        );

        // Create a mock request builder to test header application
        let reqwest_client = Client::new();
        let middleware_client = ClientBuilder::new(reqwest_client).build();
        let request_builder = middleware_client.get("https://test.example.com/test");

        // Apply custom headers
        let modified_builder = client.apply_custom_headers(request_builder);

        // Build the request to inspect headers
        let request = modified_builder.build().unwrap();

        // Check that custom headers were applied
        assert!(request.headers().contains_key("authorization"));
        assert!(request.headers().contains_key("x-api-key"));

        assert_eq!(
            request
                .headers()
                .get("authorization")
                .unwrap()
                .to_str()
                .unwrap(),
            "Bearer test-token"
        );
        assert_eq!(
            request
                .headers()
                .get("x-api-key")
                .unwrap()
                .to_str()
                .unwrap(),
            "secret-key"
        );
    }

    #[test]
    fn test_apply_custom_headers_without_headers() {
        use reqwest::Client;
        use reqwest_middleware::ClientBuilder;

        let ssl_options = SslOptions::default();

        let client = HALClient::with_url("https://test.example.com", None, ssl_options, None);

        // Create a mock request builder
        let reqwest_client = Client::new();
        let middleware_client = ClientBuilder::new(reqwest_client).build();
        let request_builder = middleware_client.get("https://test.example.com/test");

        // Apply custom headers (should be no-op)
        let modified_builder = client.apply_custom_headers(request_builder);

        // Build the request to inspect headers
        let request = modified_builder.build().unwrap();

        // Should not contain our test headers
        assert!(!request.headers().contains_key("authorization"));
        assert!(!request.headers().contains_key("x-api-key"));
    }

    #[test]
    fn test_custom_headers_struct_creation() {
        let mut headers = HashMap::new();
        headers.insert("Test-Header".to_string(), "test-value".to_string());

        let custom_headers = CustomHeaders { headers };

        assert_eq!(custom_headers.headers.len(), 1);
        assert_eq!(
            custom_headers.headers.get("Test-Header"),
            Some(&"test-value".to_string())
        );
    }

    #[test]
    fn test_custom_headers_empty() {
        let headers = HashMap::new();
        let custom_headers = CustomHeaders { headers };

        assert_eq!(custom_headers.headers.len(), 0);
        assert!(custom_headers.headers.is_empty());
    }

    #[test]
    fn test_custom_headers_case_sensitivity() {
        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), "application/json".to_string());
        headers.insert("Content-Type".to_string(), "text/plain".to_string());

        let custom_headers = CustomHeaders { headers };

        // Both should exist as separate entries (case sensitive keys)
        assert_eq!(custom_headers.headers.len(), 2);
        assert_eq!(
            custom_headers.headers.get("content-type"),
            Some(&"application/json".to_string())
        );
        assert_eq!(
            custom_headers.headers.get("Content-Type"),
            Some(&"text/plain".to_string())
        );
    }
}

#[cfg(test)]
mod tests {
    use expectest::expect;
    use expectest::prelude::*;

    use pact_consumer::prelude::*;

    use super::*;

    #[test]
    fn resolve_path_test() {
        let client = HALClient::with_url("not a URL", None, SslOptions::default(), None);
        expect!(client.resolve_path("/any")).to(be_err());

        let client = HALClient::with_url(
            "http://localhost-ip4:1234",
            None,
            SslOptions::default(),
            None,
        );
        expect!(client.resolve_path(""))
            .to(be_ok().value(Url::parse("http://localhost-ip4:1234").unwrap()));
        expect!(client.resolve_path("/"))
            .to(be_ok().value(Url::parse("http://localhost-ip4:1234").unwrap()));
        expect!(client.resolve_path("/any"))
            .to(be_ok().value(Url::parse("http://localhost-ip4:1234/any").unwrap()));
        expect!(client.resolve_path("any"))
            .to(be_ok().value(Url::parse("http://localhost-ip4:1234/any").unwrap()));
        expect!(client.resolve_path("any/sub-path"))
            .to(be_ok().value(Url::parse("http://localhost-ip4:1234/any/sub-path").unwrap()));
        expect!(client.resolve_path("/base-path"))
            .to(be_ok().value(Url::parse("http://localhost-ip4:1234/base-path").unwrap()));
        expect!(client.resolve_path("/base-path/"))
            .to(be_ok().value(Url::parse("http://localhost-ip4:1234/base-path/").unwrap()));
        expect!(client.resolve_path("/base-path/sub-path"))
            .to(be_ok().value(Url::parse("http://localhost-ip4:1234/base-path/sub-path").unwrap()));

        let client = HALClient::with_url(
            "http://localhost-ip4:1234/base-path",
            None,
            SslOptions::default(),
            None,
        );
        expect!(client.resolve_path(""))
            .to(be_ok().value(Url::parse("http://localhost-ip4:1234/base-path").unwrap()));
        expect!(client.resolve_path("/"))
            .to(be_ok().value(Url::parse("http://localhost-ip4:1234").unwrap()));
        expect!(client.resolve_path("/any"))
            .to(be_ok().value(Url::parse("http://localhost-ip4:1234/any").unwrap()));
        expect!(client.resolve_path("any"))
            .to(be_ok().value(Url::parse("http://localhost-ip4:1234/base-path/any").unwrap()));
        expect!(client.resolve_path("any/sub-path"))
            .to(be_ok()
                .value(Url::parse("http://localhost-ip4:1234/base-path/any/sub-path").unwrap()));
        expect!(client.resolve_path("/base-path"))
            .to(be_ok().value(Url::parse("http://localhost-ip4:1234/base-path").unwrap()));
        expect!(client.resolve_path("/base-path/"))
            .to(be_ok().value(Url::parse("http://localhost-ip4:1234/base-path/").unwrap()));
        expect!(client.resolve_path("/base-path/sub-path"))
            .to(be_ok().value(Url::parse("http://localhost-ip4:1234/base-path/sub-path").unwrap()));
    }

    #[test_log::test(tokio::test)]
    async fn navigate_first_retrieves_the_root_resource() {
        let pact_broker =
            PactBuilderAsync::new("RustPactVerifier", "PactBrokerStub")
                .interaction("a request to a hal resource", "", |mut i| async move {
                    i.request.path("/");
                    i.response
          .header("Content-Type", "application/hal+json")
          .body("{\"_links\":{\"next\":{\"href\":\"/abc\"},\"prev\":{\"href\":\"/def\"}}}");
                    i
                })
                .await
                .interaction("a request to next", "", |mut i| async move {
                    i.request.path("/abc");
                    i.response
                        .header("Content-Type", "application/json")
                        .json_body(json_pattern!("Yay! You found your way here"));
                    i
                })
                .await
                .start_mock_server(None, Some(MockServerConfig::default()));

        let client = HALClient::with_url(
            pact_broker.url().as_str(),
            None,
            SslOptions::default(),
            None,
        );
        let result = client.navigate("next", &hashmap! {}).await.unwrap();
        expect!(result.path_info).to(be_some().value(serde_json::Value::String(
            "Yay! You found your way here".to_string(),
        )));
    }

    #[test_log::test(tokio::test)]
    async fn navigate_will_not_retrieve_the_root_resource_if_a_path_is_already_set() {
        let pact_broker = PactBuilderAsync::new("RustPactVerifier", "PactBrokerStub")
            .interaction("a request to next", "", |mut i| async move {
                i.request.path("/abc");
                i.response
                    .header("Content-Type", "application/json")
                    .json_body(json_pattern!("Yay! You found your way here"));
                i
            })
            .await
            .start_mock_server(None, Some(MockServerConfig::default()));

        let mut client = HALClient::with_url(
            pact_broker.url().as_str(),
            None,
            SslOptions::default(),
            None,
        );
        client.path_info = Some(json!({
          "_links": {
            "next": { "href": "/abc" },
            "prev": { "href": "/def" }
          }
        }));
        let result = client.navigate("next", &hashmap! {}).await.unwrap();
        expect!(result.path_info).to(be_some().value(serde_json::Value::String(
            "Yay! You found your way here".to_string(),
        )));
    }

    #[test_log::test(tokio::test)]
    async fn navigate_takes_context_paths_into_account() {
        let pact_broker = PactBuilderAsync::new("RustPactVerifier", "PactBrokerStub")
      .interaction("a request to a hal resource with base path", "", |mut i| async move {
        i.request.path("/base-path");
        i.response
          .header("Content-Type", "application/hal+json")
          .body("{\"_links\":{\"next\":{\"href\":\"/base-path/abc\"},\"prev\":{\"href\":\"/base-path/def\"}}}");
        i
      })
      .await
      .interaction("a request to next with a base path", "", |mut i| async move {
        i.request.path("/base-path/abc");
        i.response
          .header("Content-Type", "application/json")
          .json_body(json_pattern!("Yay! You found your way here"));
        i
      })
      .await
      .start_mock_server(None, Some(MockServerConfig::default()));

        let client = HALClient::with_url(
            pact_broker.url().join("/base-path").unwrap().as_str(),
            None,
            SslOptions::default(),
            None,
        );
        let result = client.navigate("next", &hashmap! {}).await.unwrap();
        expect!(result.path_info).to(be_some().value(serde_json::Value::String(
            "Yay! You found your way here".to_string(),
        )));
    }
}

// MARK: RetryMiddleware integration tests

#[cfg(test)]
mod retry_middleware_tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use axum::{Router, body::Body, http::StatusCode, response::Response, routing::get};
    use tokio::net::TcpListener;

    use super::{HALClient, SslOptions};

    async fn spawn_test_server(router: Router) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        format!("http://{}", addr)
    }

    fn hal_client(base_url: &str, retries: u8) -> HALClient {
        HALClient::with_url(base_url, None, SslOptions::default(), None).with_retry_count(retries)
    }

    #[tokio::test]
    async fn retries_on_429_too_many_requests() {
        let request_count = Arc::new(AtomicUsize::new(0));
        let count = request_count.clone();

        let router = Router::new().route(
            "/",
            get(move || {
                let count = count.clone();
                async move {
                    let n = count.fetch_add(1, Ordering::SeqCst);
                    if n < 2 {
                        Response::builder()
                            .status(StatusCode::TOO_MANY_REQUESTS)
                            .body(Body::from("{\"_links\":{}}"))
                            .unwrap()
                    } else {
                        Response::builder()
                            .status(StatusCode::OK)
                            .header("content-type", "application/hal+json")
                            .body(Body::from("{\"_links\":{}}"))
                            .unwrap()
                    }
                }
            }),
        );

        let base_url = spawn_test_server(router).await;
        let client = hal_client(&base_url, 3);
        let result = client.fetch("").await;

        assert!(result.is_ok(), "expected OK but got: {:?}", result.err());
        assert_eq!(request_count.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn does_not_retry_404_not_found() {
        let request_count = Arc::new(AtomicUsize::new(0));
        let count = request_count.clone();

        let router = Router::new().route(
            "/",
            get(move || {
                let count = count.clone();
                async move {
                    count.fetch_add(1, Ordering::SeqCst);
                    StatusCode::NOT_FOUND
                }
            }),
        );

        let base_url = spawn_test_server(router).await;
        let client = hal_client(&base_url, 3);
        let _ = client.fetch("").await;

        assert_eq!(
            request_count.load(Ordering::SeqCst),
            1,
            "404 should not be retried"
        );
    }

    #[tokio::test]
    async fn retries_on_500_internal_server_error() {
        let request_count = Arc::new(AtomicUsize::new(0));
        let count = request_count.clone();

        let router = Router::new().route(
            "/",
            get(move || {
                let count = count.clone();
                async move {
                    let n = count.fetch_add(1, Ordering::SeqCst);
                    if n == 0 {
                        StatusCode::INTERNAL_SERVER_ERROR
                    } else {
                        StatusCode::OK
                    }
                }
            }),
        );

        let base_url = spawn_test_server(router).await;
        let client = hal_client(&base_url, 3);
        let _ = client.fetch("").await;

        assert_eq!(
            request_count.load(Ordering::SeqCst),
            2,
            "500 should be retried once"
        );
    }

    #[tokio::test]
    async fn honours_integer_retry_after_header() {
        // Retry-After: 0 exercises the header path without real delay.
        let request_count = Arc::new(AtomicUsize::new(0));
        let count = request_count.clone();

        let router = Router::new().route(
            "/",
            get(move || {
                let count = count.clone();
                async move {
                    let n = count.fetch_add(1, Ordering::SeqCst);
                    if n == 0 {
                        Response::builder()
                            .status(StatusCode::TOO_MANY_REQUESTS)
                            .header("Retry-After", "0")
                            .body(Body::from("{\"_links\":{}}"))
                            .unwrap()
                    } else {
                        Response::builder()
                            .status(StatusCode::OK)
                            .header("content-type", "application/hal+json")
                            .body(Body::from("{\"_links\":{}}"))
                            .unwrap()
                    }
                }
            }),
        );

        let base_url = spawn_test_server(router).await;
        let client = hal_client(&base_url, 3);
        let result = client.fetch("").await;

        assert!(result.is_ok());
        assert_eq!(request_count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn honours_http_date_retry_after_header() {
        // An HTTP-date Retry-After in the past should result in a zero delay,
        // confirming that the date form is parsed rather than silently ignored.
        let request_count = Arc::new(AtomicUsize::new(0));
        let count = request_count.clone();

        let router = Router::new().route(
            "/",
            get(move || {
                let count = count.clone();
                async move {
                    let n = count.fetch_add(1, Ordering::SeqCst);
                    if n == 0 {
                        Response::builder()
                            .status(StatusCode::TOO_MANY_REQUESTS)
                            // A date well in the past → delay is immediately 0.
                            .header("Retry-After", "Thu, 01 Jan 1970 00:00:00 GMT")
                            .body(Body::from("{\"_links\":{}}"))
                            .unwrap()
                    } else {
                        Response::builder()
                            .status(StatusCode::OK)
                            .header("content-type", "application/hal+json")
                            .body(Body::from("{\"_links\":{}}"))
                            .unwrap()
                    }
                }
            }),
        );

        let base_url = spawn_test_server(router).await;
        let client = hal_client(&base_url, 3);
        let result = client.fetch("").await;

        assert!(result.is_ok());
        assert_eq!(
            request_count.load(Ordering::SeqCst),
            2,
            "HTTP-date Retry-After should be parsed and retry should happen"
        );
    }

    #[tokio::test]
    async fn sends_one_request_when_retries_is_zero() {
        let request_count = Arc::new(AtomicUsize::new(0));
        let count = request_count.clone();

        let router = Router::new().route(
            "/",
            get(move || {
                let count = count.clone();
                async move {
                    count.fetch_add(1, Ordering::SeqCst);
                    Response::builder()
                        .status(StatusCode::TOO_MANY_REQUESTS)
                        .body(Body::empty())
                        .unwrap()
                }
            }),
        );

        let base_url = spawn_test_server(router).await;
        let client = hal_client(&base_url, 0);
        let _ = client.fetch("").await;

        assert_eq!(
            request_count.load(Ordering::SeqCst),
            1,
            "retries=0 should send exactly one request"
        );
    }

    #[tokio::test]
    async fn returns_last_failure_when_all_retries_exhausted() {
        let router = Router::new().route(
            "/",
            get(|| async {
                Response::builder()
                    .status(StatusCode::TOO_MANY_REQUESTS)
                    .body(Body::empty())
                    .unwrap()
            }),
        );

        let base_url = spawn_test_server(router).await;
        let client = hal_client(&base_url, 2);
        let result = client.fetch("").await;

        assert!(
            result.is_err(),
            "all retries exhausted should return error, got: {:?}",
            result
        );
    }
}
