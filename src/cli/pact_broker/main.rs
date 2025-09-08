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
pub mod subcommands;
pub mod tags;
#[cfg(test)]
pub mod test_utils;
pub mod types;
pub mod utils;
pub mod verification;
pub mod versions;
pub mod webhooks;
use utils::with_retries;

use crate::cli::pact_broker::main::types::SslOptions;

fn is_true(object: &serde_json::Map<String, Value>, field: &str) -> bool {
    match object.get(field) {
        Some(json) => match *json {
            serde_json::Value::Bool(b) => b,
            _ => false,
        },
        None => false,
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
        Ok(mime) => {
            match (
                mime.type_().as_str(),
                mime.subtype().as_str(),
                mime.suffix(),
            ) {
                ("application", "json", None) => true,
                ("application", "hal", Some(mime::JSON)) => true,
                _ => false,
            }
        }
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
        };
        buffer == *other
    }
}

impl<'a> PartialEq<&'a str> for PactBrokerError {
    fn eq(&self, other: &&str) -> bool {
        let message = match self {
            PactBrokerError::LinkError(s) => s.clone(),
            PactBrokerError::ContentError(s) => s.clone(),
            PactBrokerError::IoError(s) => s.clone(),
            PactBrokerError::NotFound(s) => s.clone(),
            PactBrokerError::UrlError(s) => s.clone(),
            PactBrokerError::ValidationError(errors) => errors.iter().join(", "),
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
            href: find_entry(link_data, &"href".to_string()).map(|(_, href)| as_string(&href)),
            templated: is_true(link_data, "templated"),
            title: link_data.get("title").map(|title| as_string(title)),
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
    client: reqwest::Client,
    url: String,
    path_info: Option<Value>,
    auth: Option<HttpAuth>,
    ssl_options: SslOptions,
    retries: u8,
}

impl HALClient {
    /// Initialise a client with the URL and authentication
    pub fn with_url(url: &str, auth: Option<HttpAuth>, ssl_options: SslOptions) -> HALClient {
        HALClient {
            url: url.to_string(),
            auth: auth.clone(),
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
            let path_info = self.clone().fetch("/".into()).await?;
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
                        .map(|link_data| Link::from_json(&link.to_string(), &link_data))
                        .ok_or_else(|| PactBrokerError::LinkError(format!("Link is malformed, expected an object but got {}. URL: '{}', LINK: '{}'",
                            link_data, self.url, link))),
                    None => Err(PactBrokerError::LinkError(format!("Link '{}' was not found in the response, only the following links where found: {:?}. URL: '{}', LINK: '{}'",
                        link, json.as_object().unwrap_or(&json!({}).as_object().unwrap()).keys().join(", "), self.url, link)))
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
            self.clone().parse_link_url(&link, &template_values)
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
        self.fetch(joined_url.path().into()).await
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
            self.clone().parse_link_url(&link, &template_values)
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
        self.delete(joined_url.path().into()).await
    }

    pub async fn fetch(self, path: &str) -> Result<Value, PactBrokerError> {
        info!("Fetching path '{}' from pact broker", path);

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
                HttpAuth::User(username, password) => {
                    self.client.get(url).basic_auth(username, password.clone())
                }
                HttpAuth::Token(token) => self.client.get(url).bearer_auth(token),
                _ => self.client.get(url),
            },
            None => self.client.get(url),
        }
        .header("accept", "application/hal+json, application/json");

        let response = utils::with_retries(self.retries, request_builder)
            .await
            .map_err(|err| {
                PactBrokerError::IoError(format!(
                    "Failed to access pact broker path '{}' - {}. URL: '{}'",
                    &path, err, &self.url,
                ))
            })?;

        self.parse_broker_response(path.to_string(), response).await
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
        };

        let response = utils::with_retries(self.retries, request_builder)
            .await
            .map_err(|err| {
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
        } else if status_code.as_u16() == 400 {
            let body = response.bytes().await.map_err(|_| {
                PactBrokerError::IoError(format!(
                    "Failed to download response body for path '{}'. URL: '{}'",
                    &path, self.url
                ))
            })?;

            if is_json_content_type {
                let errors = serde_json::from_slice(&body)
            .map_err(|err| PactBrokerError::ContentError(
              format!("Did not get a valid HAL response body from pact broker path '{}' - {}. URL: '{}'",
                      path, err, self.url)
            ))?;
                Err(handle_validation_errors(errors))
            } else {
                let body = from_utf8(&body)
                    .map(|b| b.to_string())
                    .unwrap_or_else(|err| format!("could not read body: {}", err));
                error!("Request to pact broker path '{}' failed: {}", path, body);
                Err(PactBrokerError::IoError(format!(
                    "Request to pact broker path '{}' failed: {}. URL: '{}'",
                    path, status_code, self.url
                )))
            }
        } else {
            Err(PactBrokerError::IoError(format!(
                "Request to pact broker path '{}' failed: {}. URL: '{}'",
                path, status_code, self.url
            )))
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
        Some(json) => match json.get(&link) {
          Some(link_data) => link_data.as_array()
              .map(|link_data| link_data.iter().map(|link_json| match link_json {
                Value::Object(data) => Link::from_json(&link, data),
                Value::String(s) => Link { name: link.to_string(), href: Some(s.clone()), templated: false, title: None },
                _ => Link { name: link.to_string(), href: Some(link_json.to_string()), templated: false, title: None }
              }).collect())
              .ok_or_else(|| PactBrokerError::LinkError(format!("Link is malformed, expected an object but got {}. URL: '{}', LINK: '{}'",
                  link_data, self.url, link))),
          None => Err(PactBrokerError::LinkError(format!("Link '{}' was not found in the response, only the following links where found: {:?}. URL: '{}', LINK: '{}'",
            link, json.as_object().unwrap_or(&json!({}).as_object().unwrap()).keys().join(", "), self.url, link)))
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
            base_url.join(&url.path())?
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
        let response = with_retries(self.retries, request_builder).await;
        match response {
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

    fn with_doc_context(self, doc_attributes: &[Link]) -> Result<HALClient, PactBrokerError> {
        let links: serde_json::Map<String, serde_json::Value> = doc_attributes
            .iter()
            .map(|link| (link.name.clone(), link.as_json()))
            .collect();
        let links_json = json!({
          "_links": json!(links)
        });
        Ok(self.update_path_info(links_json))
    }
}

fn handle_validation_errors(body: Value) -> PactBrokerError {
    match &body {
        Value::Object(attrs) => {
            if let Some(errors) = attrs.get("errors") {
                match errors {
                    Value::Array(values) => PactBrokerError::ValidationError(
                        values.iter().map(|v| json_to_string(v)).collect(),
                    ),
                    Value::Object(errors) => PactBrokerError::ValidationError(
                        errors
                            .iter()
                            .map(|(field, errors)| match errors {
                                Value::String(error) => format!("{}: {}", field, error),
                                Value::Array(errors) => format!(
                                    "{}: {}",
                                    field,
                                    errors.iter().map(|err| json_to_string(err)).join(", ")
                                ),
                                _ => format!("{}: {}", field, errors),
                            })
                            .collect(),
                    ),
                    Value::String(s) => PactBrokerError::ValidationError(vec![s.clone()]),
                    _ => PactBrokerError::ValidationError(vec![errors.to_string()]),
                }
            } else {
                PactBrokerError::ValidationError(vec![body.to_string()])
            }
        }
        Value::String(s) => PactBrokerError::ValidationError(vec![s.clone()]),
        _ => PactBrokerError::ValidationError(vec![body.to_string()]),
    }
}

impl HALClient {
    pub fn setup(url: &str, auth: Option<HttpAuth>, ssl_options: SslOptions) -> HALClient {
        let mut builder = reqwest::ClientBuilder::new().user_agent(format!(
            "{}/{}",
            env!("CARGO_PKG_NAME"),
            env!("CARGO_PKG_VERSION")
        ));

        debug!("Using ssl_options: {:?}", ssl_options);
        if let Some(ref path) = ssl_options.ssl_cert_path {
            if let Ok(cert_bytes) = std::fs::read(path) {
                if let Ok(cert) = reqwest::Certificate::from_pem_bundle(&cert_bytes) {
                    debug!("Adding SSL certificate from path: {}", path);
                    for c in cert {
                        builder = builder.add_root_certificate(c.clone());
                    }
                }
            } else {
                debug!(
                    "Could not read SSL certificate from provided path: {}",
                    path
                );
            }
        }
        if ssl_options.skip_ssl {
            builder = builder.danger_accept_invalid_certs(true);
            debug!("Skipping SSL certificate validation");
        }
        if !ssl_options.use_root_trust_store {
            builder = builder.tls_built_in_root_certs(false);
            debug!("Disabling root trust store for SSL");
        }

        HALClient {
            client: builder.build().unwrap(),
            url: url.to_string(),
            path_info: None,
            auth,
            retries: 3,
            ssl_options,
        }
    }
}

pub fn links_from_json(json: &Value) -> Vec<Link> {
    match json.get("_links") {
        Some(json) => match json {
            Value::Object(v) => v
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
        },
        None => vec![],
    }
}

/// Fetches the pacts from the broker that match the provider name
pub async fn fetch_pacts_from_broker(
    broker_url: &str,
    provider_name: &str,
    auth: Option<HttpAuth>,
    ssl_options: SslOptions,
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

    let mut hal_client = HALClient::with_url(broker_url, auth, ssl_options);
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

    let mut hal_client = HALClient::with_url(broker_url, auth, ssl_options);
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

            if pfv.embedded.pacts.len() == 0 {
                return Err(anyhow!(PactBrokerError::NotFound(format!(
                    "No pacts were found for this provider"
                ))));
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
        None => Err(PactBrokerError::NotFound(format!(
            "No pacts were found for this provider"
        ))),
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

async fn publish_provider_tags(
    hal_client: &HALClient,
    links: &[Link],
    provider_tags: Vec<String>,
    version: &str,
    headers: Option<HashMap<String, String>>,
) -> Result<(), PactBrokerError> {
    let hal_client = hal_client
        .clone()
        .with_doc_context(links)?
        .navigate("pb:provider", &hashmap! {})
        .await?;
    match hal_client.find_link("pb:version-tag") {
        Ok(link) => {
            for tag in &provider_tags {
                let template_values = hashmap! {
                  "version".to_string() => version.to_string(),
                  "tag".to_string() => tag.clone()
                };
                match hal_client
                    .clone()
                    .put_json(
                        hal_client
                            .clone()
                            .parse_link_url(&link, &template_values)?
                            .as_str(),
                        "{}",
                        headers.clone(),
                    )
                    .await
                {
                    Ok(_) => debug!("Pushed tag {} for provider version {}", tag, version),
                    Err(err) => {
                        error!(
                            "Failed to push tag {} for provider version {}",
                            tag, version
                        );
                        return Err(err);
                    }
                }
            }
            Ok(())
        }
        Err(_) => Err(PactBrokerError::LinkError(
            "Can't publish provider tags as there is no 'pb:version-tag' link".to_string(),
        )),
    }
}

async fn publish_provider_branch(
    hal_client: &HALClient,
    links: &[Link],
    branch: &str,
    version: &str,
    headers: Option<HashMap<String, String>>,
) -> Result<(), PactBrokerError> {
    let hal_client = hal_client
        .clone()
        .with_doc_context(links)?
        .navigate("pb:provider", &hashmap! {})
        .await?;

    match hal_client.find_link("pb:branch-version") {
    Ok(link) => {
      let template_values = hashmap! {
        "branch".to_string() => branch.to_string(),
        "version".to_string() => version.to_string(),
      };
      match hal_client.clone().put_json(hal_client.clone().parse_link_url(&link, &template_values)?.as_str(), "{}",headers).await {
        Ok(_) => debug!("Pushed branch {} for provider version {}", branch, version),
        Err(err) => {
          error!("Failed to push branch {} for provider version {}", branch, version);
          return Err(err);
        }
      }
      Ok(())
    },
    Err(_) => Err(PactBrokerError::LinkError("Can't publish provider branch as there is no 'pb:branch-version' link. Please ugrade to Pact Broker version 2.86.0 or later for branch support".to_string()))
  }
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
