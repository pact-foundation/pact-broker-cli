//! Utility functions

use std::collections::HashMap;
use std::time::Duration;

use comfy_table::{Table, presets::UTF8_FULL};
use futures::StreamExt;
use maplit::hashmap;
use pact_models::http_utils::HttpAuth;
use reqwest_middleware::RequestBuilder;
use serde_json::Value;
use tokio::time::sleep;
use tracing::{trace, warn};

use crate::{cli::pact_broker::main::types::SslOptions, dbg_as_curl};

use super::{CustomHeaders, HALClient, Link, PactBrokerError};

/// Retries a request on failure
pub(crate) async fn with_retries(
    retries: u8,
    request: RequestBuilder,
) -> Result<reqwest::Response, reqwest_middleware::Error> {
    match &request.try_clone() {
        None => {
            warn!("with_retries: Could not retry the request as it is not cloneable");
            request.send().await
        }
        Some(rb) => futures::stream::iter((1..=retries).step_by(1))
            .fold(
                (
                    None::<Result<reqwest::Response, reqwest_middleware::Error>>,
                    rb.try_clone(),
                ),
                |(response, request), attempt| async move {
                    match request {
                        Some(request_builder) => match response {
                            None => {
                                let next = request_builder.try_clone();
                                (Some(dbg_as_curl!(request_builder).send().await), next)
                            }
                            Some(response) => {
                                trace!(
                                    "with_retries: attempt {}/{} is {:?}",
                                    attempt, retries, response
                                );
                                match response {
                                    Ok(ref res) => {
                                        if res.status().is_server_error() {
                                            match request_builder.try_clone() {
                                                None => (Some(response), None),
                                                Some(rb) => {
                                                    sleep(Duration::from_millis(
                                                        10_u64.pow(attempt as u32),
                                                    ))
                                                    .await;
                                                    (Some(request_builder.send().await), Some(rb))
                                                }
                                            }
                                        } else {
                                            (Some(response), None)
                                        }
                                    }
                                    Err(ref err) => {
                                        if err.is_status() {
                                            if err.status().unwrap_or_default().is_server_error() {
                                                match request_builder.try_clone() {
                                                    None => (Some(response), None),
                                                    Some(rb) => {
                                                        sleep(Duration::from_millis(
                                                            10_u64.pow(attempt as u32),
                                                        ))
                                                        .await;
                                                        (
                                                            Some(request_builder.send().await),
                                                            Some(rb),
                                                        )
                                                    }
                                                }
                                            } else {
                                                (Some(response), None)
                                            }
                                        } else {
                                            (Some(response), None)
                                        }
                                    }
                                }
                            }
                        },
                        None => (response, None),
                    }
                },
            )
            .await
            .0
            .unwrap(),
    }
}

// pub(crate) fn as_safe_ref(
//     interaction: &dyn Interaction,
// ) -> Box<dyn Interaction + Send + Sync + RefUnwindSafe> {
//     if let Some(v4) = interaction.as_v4_sync_message() {
//         Box::new(v4)
//     } else if let Some(v4) = interaction.as_v4_async_message() {
//         Box::new(v4)
//     } else {
//         let v4 = interaction.as_v4_http().unwrap();
//         Box::new(v4)
//     }
// }

pub(crate) fn generate_table(res: &Value, columns: Vec<&str>, names: Vec<Vec<&str>>) -> Table {
    let mut table = Table::new();
    table.load_preset(UTF8_FULL).set_header(columns);
    if let Some(items) = res.get("pacts").unwrap().as_array() {
        for item in items {
            let mut values = vec![item; names.len()];

            for (i, name) in names.iter().enumerate() {
                for n in name.clone() {
                    values[i] = values[i].get(n).unwrap();
                }
            }

            let records: Vec<String> = values.iter().map(|v| v.to_string()).collect();
            table.add_row(records.as_slice());
        }
    };
    table
}

pub(crate) fn get_broker_url(args: &clap::ArgMatches) -> String {
    args.get_one::<String>("broker-base-url")
        .expect("url is required")
        .to_string()
}
pub(crate) fn get_ssl_options(args: &clap::ArgMatches) -> SslOptions {
    SslOptions {
        skip_ssl: args
            .get_one::<bool>("skip-ssl-verification")
            .copied()
            .unwrap_or(false),
        ssl_cert_path: args
            .get_one::<String>("ssl-certificate")
            .map(|s| s.to_string()),
        use_root_trust_store: args
            .get_one::<bool>("ssl-trust-store")
            .copied()
            .unwrap_or(true),
    }
}

// setup client with broker url and credentials
pub(crate) fn get_auth(args: &clap::ArgMatches) -> HttpAuth {
    let token = args.try_get_one::<String>("broker-token");
    let username = args.try_get_one::<String>("broker-username");
    let password = args.try_get_one::<String>("broker-password");
    let auth;

    match token {
        Ok(Some(token)) => {
            auth = HttpAuth::Token(token.to_string());
        }
        Ok(None) => match username {
            Ok(Some(username)) => match password {
                Ok(Some(password)) => {
                    auth = HttpAuth::User(username.to_string(), Some(password.to_string()));
                }
                Ok(None) => {
                    auth = HttpAuth::User(username.to_string(), None);
                }
                Err(_) => todo!(),
            },
            Ok(None) => {
                auth = HttpAuth::None;
            }
            Err(_) => todo!(),
        },
        Err(_) => todo!(),
    }

    auth
}

// Parse custom headers from CLI arguments in curl format ("Header-Name: Value")
pub(crate) fn get_custom_headers(args: &clap::ArgMatches) -> Option<CustomHeaders> {
    let custom_headers = args.get_many::<String>("custom-header");

    if let Some(header_strings) = custom_headers {
        let mut headers = std::collections::HashMap::new();

        for header_str in header_strings {
            if let Some(colon_pos) = header_str.find(':') {
                let name = header_str[..colon_pos].trim().to_string();
                let value = header_str[colon_pos + 1..].trim().to_string();

                if !name.is_empty() && !value.is_empty() {
                    headers.insert(name, value);
                }
            }
        }

        if !headers.is_empty() {
            return Some(CustomHeaders { headers });
        }
    }

    None
}

#[cfg(test)]
mod custom_headers_tests {
    use super::*;
    use clap::ArgMatches;

    fn create_test_args(headers: Vec<&str>) -> ArgMatches {
        use clap::{Arg, Command};

        let app = Command::new("test").arg(
            Arg::new("custom-header")
                .long("custom-header")
                // .short('H')
                .action(clap::ArgAction::Append)
                .value_name("HEADER")
                .help("Custom header in curl format"),
        );

        let mut args = vec!["test"];
        for header in headers {
            args.push("--custom-header");
            args.push(header);
        }

        app.try_get_matches_from(args).unwrap()
    }

    #[test]
    fn test_get_custom_headers_none() {
        let args = create_test_args(vec![]);
        let result = get_custom_headers(&args);
        assert!(result.is_none());
    }

    #[test]
    fn test_get_custom_headers_single_header() {
        let args = create_test_args(vec!["Authorization: Bearer token123"]);
        let result = get_custom_headers(&args);

        assert!(result.is_some());
        let custom_headers = result.unwrap();
        assert_eq!(custom_headers.headers.len(), 1);
        assert_eq!(
            custom_headers.headers.get("Authorization"),
            Some(&"Bearer token123".to_string())
        );
    }

    #[test]
    fn test_get_custom_headers_multiple_headers() {
        let args = create_test_args(vec![
            "Authorization: Bearer token123",
            "X-API-Key: secret456",
            "Content-Type: application/json",
        ]);
        let result = get_custom_headers(&args);

        assert!(result.is_some());
        let custom_headers = result.unwrap();
        assert_eq!(custom_headers.headers.len(), 3);
        assert_eq!(
            custom_headers.headers.get("Authorization"),
            Some(&"Bearer token123".to_string())
        );
        assert_eq!(
            custom_headers.headers.get("X-API-Key"),
            Some(&"secret456".to_string())
        );
        assert_eq!(
            custom_headers.headers.get("Content-Type"),
            Some(&"application/json".to_string())
        );
    }

    #[test]
    fn test_get_custom_headers_with_spaces() {
        let args = create_test_args(vec!["  X-Custom-Header  :  value with spaces  "]);
        let result = get_custom_headers(&args);

        assert!(result.is_some());
        let custom_headers = result.unwrap();
        assert_eq!(custom_headers.headers.len(), 1);
        assert_eq!(
            custom_headers.headers.get("X-Custom-Header"),
            Some(&"value with spaces".to_string())
        );
    }

    #[test]
    fn test_get_custom_headers_cloudflare_access() {
        let args = create_test_args(vec![
            "CF-Access-Client-Id: client-id-123",
            "CF-Access-Client-Secret: secret-456",
        ]);
        let result = get_custom_headers(&args);

        assert!(result.is_some());
        let custom_headers = result.unwrap();
        assert_eq!(custom_headers.headers.len(), 2);
        assert_eq!(
            custom_headers.headers.get("CF-Access-Client-Id"),
            Some(&"client-id-123".to_string())
        );
        assert_eq!(
            custom_headers.headers.get("CF-Access-Client-Secret"),
            Some(&"secret-456".to_string())
        );
    }

    #[test]
    fn test_get_custom_headers_invalid_format_no_colon() {
        let args = create_test_args(vec!["InvalidHeader"]);
        let result = get_custom_headers(&args);
        assert!(result.is_none());
    }

    #[test]
    fn test_get_custom_headers_invalid_format_empty_name() {
        let args = create_test_args(vec![": value"]);
        let result = get_custom_headers(&args);
        assert!(result.is_none());
    }

    #[test]
    fn test_get_custom_headers_invalid_format_empty_value() {
        let args = create_test_args(vec!["Header:"]);
        let result = get_custom_headers(&args);
        assert!(result.is_none());
    }

    #[test]
    fn test_get_custom_headers_mixed_valid_invalid() {
        let args = create_test_args(vec![
            "Valid-Header: valid-value",
            "InvalidHeader",
            ": empty-name",
            "Empty-Value:",
            "Another-Valid: another-value",
        ]);
        let result = get_custom_headers(&args);

        assert!(result.is_some());
        let custom_headers = result.unwrap();
        assert_eq!(custom_headers.headers.len(), 2);
        assert_eq!(
            custom_headers.headers.get("Valid-Header"),
            Some(&"valid-value".to_string())
        );
        assert_eq!(
            custom_headers.headers.get("Another-Valid"),
            Some(&"another-value".to_string())
        );
    }

    #[test]
    fn test_get_custom_headers_duplicate_keys() {
        let args = create_test_args(vec!["X-Test: first-value", "X-Test: second-value"]);
        let result = get_custom_headers(&args);

        assert!(result.is_some());
        let custom_headers = result.unwrap();
        assert_eq!(custom_headers.headers.len(), 1);
        // Last value should win
        assert_eq!(
            custom_headers.headers.get("X-Test"),
            Some(&"second-value".to_string())
        );
    }

    #[test]
    fn test_get_custom_headers_special_characters() {
        let args = create_test_args(vec!["X-Special: value@#$%^&*()", "X-Unicode: café"]);
        let result = get_custom_headers(&args);

        assert!(result.is_some());
        let custom_headers = result.unwrap();
        assert_eq!(custom_headers.headers.len(), 2);
        assert_eq!(
            custom_headers.headers.get("X-Special"),
            Some(&"value@#$%^&*()".to_string())
        );
        assert_eq!(
            custom_headers.headers.get("X-Unicode"),
            Some(&"café".to_string())
        );
    }
}

pub async fn get_broker_relation(
    hal_client: HALClient,
    relation: String,
    _broker_url: String,
) -> Result<String, PactBrokerError> {
    let index_res: Result<Value, PactBrokerError> = hal_client.clone().fetch("/").await;
    match index_res {
        Ok(_) => {
            let index_res_clone = index_res.clone().unwrap();
            let relation_value = index_res_clone.get("_links").unwrap().get(&relation);

            if relation_value.is_none() {
                return Err(PactBrokerError::NotFound(format!(
                    "Could not find relation '{}'",
                    &relation
                )));
            }

            Ok(relation_value
                .unwrap()
                .get("href")
                .unwrap()
                .to_string()
                .replace("\"", ""))
        }
        Err(err) => {
            return Err(err);
        }
    }
}

pub async fn follow_broker_relation(
    hal_client: HALClient,
    relation: String,
    relation_href: String,
) -> Result<Value, PactBrokerError> {
    let link = Link {
        name: relation,
        href: Some(relation_href),
        templated: false,
        title: None,
    };
    let template_values = hashmap! {};
    hal_client.fetch_url(&link, &template_values).await
}
pub async fn follow_templated_broker_relation(
    hal_client: HALClient,
    relation: String,
    relation_href: String,
    template_values: HashMap<String, String>,
) -> Result<Value, PactBrokerError> {
    let link = Link {
        name: relation,
        href: Some(relation_href),
        templated: true,
        title: None,
    };
    hal_client.fetch_url(&link, &template_values).await
}
pub async fn delete_templated_broker_relation(
    hal_client: HALClient,
    relation: String,
    relation_href: String,
    template_values: HashMap<String, String>,
) -> Result<Value, PactBrokerError> {
    let link = Link {
        name: relation,
        href: Some(relation_href),
        templated: true,
        title: None,
    };
    hal_client.delete_url(&link, &template_values).await
}

// Helper function to handle errors
pub(crate) fn handle_error(err: PactBrokerError) -> PactBrokerError {
    match err.clone() {
        PactBrokerError::LinkError(error)
        | PactBrokerError::ContentError(error)
        | PactBrokerError::IoError(error)
        | PactBrokerError::NotFound(error) => {
            println!("❌ {}", error);
        }
        PactBrokerError::ValidationError(errors) => {
            for error in errors {
                println!("❌ {}", error);
            }
        }
        _ => {
            println!("❌ {}", err);
        }
    }
    PactBrokerError::from(err)
}
