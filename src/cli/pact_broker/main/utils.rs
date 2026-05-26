//! Utility functions

use std::collections::HashMap;
use std::time::Duration;

use comfy_table::{Table, presets::UTF8_FULL};
use maplit::hashmap;
use pact_models::http_utils::HttpAuth;
use reqwest::StatusCode;
use serde_json::Value;

use crate::cli::pact_broker::main::types::SslOptions;

use super::{CustomHeaders, HALClient, Link, PactBrokerError};

/// Computes how long to wait before the next retry attempt.
///
/// For `429 Too Many Requests` responses the server may include a `Retry-After`
/// header.  When the parsed delay is present, the delay is that value plus up to
/// 20 % extra, capped so the additional buffer never exceeds 60 seconds.  This
/// spreads retries out across the new rate-limit window instead of hammering the
/// server the instant it opens.
///
/// For all other retryable status codes (and for 429 without a `Retry-After`
/// header) exponential back-off is used: `500 × 2^(attempt − 1)` milliseconds,
/// giving 500 ms, 1 s, 2 s, 4 s, 8 s, … on successive retries.
///
/// # Arguments
///
/// * `status` - The HTTP status code of the failed response.
/// * `retry_after` - Duration parsed from the `Retry-After` header, if present.
///   Supports both the integer-seconds form and the HTTP-date form.
/// * `attempt` - The current attempt number (1-based), used for exponential back-off.
///
/// # Returns
///
/// The [`Duration`] to sleep before the next attempt.
pub(crate) fn compute_retry_delay(
    status: StatusCode,
    retry_after: Option<Duration>,
    attempt: u32,
) -> Duration {
    if status == StatusCode::TOO_MANY_REQUESTS {
        if let Some(base) = retry_after {
            let secs = base.as_secs();
            let extra = (secs / 5).min(60); // ≈ 20 % of secs, capped at 60 s
            return Duration::from_secs(secs + extra);
        }
    }
    Duration::from_millis(500 * 2_u64.pow(attempt.saturating_sub(1)))
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

/// Reads the `--retries` / `PACT_BROKER_HTTP_RETRIES` value from parsed CLI arguments.
///
/// Falls back to `8` if the argument is absent (which should not happen in practice
/// because the flag carries a `default_value`).
pub(crate) fn get_retries(args: &clap::ArgMatches) -> u8 {
    args.get_one::<u8>("retries").copied().unwrap_or(8)
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
mod retry_tests {
    use std::time::Duration;

    use reqwest::StatusCode;

    use super::compute_retry_delay;

    // MARK: compute_retry_delay unit tests

    #[test]
    fn delay_for_429_without_retry_after_uses_exponential_backoff() {
        // attempt=2: 500 * 2^(2-1) = 1000 ms
        let delay = compute_retry_delay(StatusCode::TOO_MANY_REQUESTS, None, 2);
        assert_eq!(delay, Duration::from_millis(1000));
    }

    #[test]
    fn delay_for_5xx_uses_exponential_backoff() {
        // attempt=3: 500 * 2^(3-1) = 2000 ms
        let delay = compute_retry_delay(StatusCode::INTERNAL_SERVER_ERROR, None, 3);
        assert_eq!(delay, Duration::from_millis(2000));
    }

    #[test]
    fn delay_starts_at_500ms_on_first_attempt() {
        // attempt=1: 500 * 2^0 = 500 ms
        let delay = compute_retry_delay(StatusCode::INTERNAL_SERVER_ERROR, None, 1);
        assert_eq!(delay, Duration::from_millis(500));
    }

    #[test]
    fn delay_for_429_with_retry_after_applies_1_2x_multiplier() {
        // Retry-After: 10 s → 10 + min(10/5, 60) = 10 + 2 = 12
        let delay = compute_retry_delay(
            StatusCode::TOO_MANY_REQUESTS,
            Some(Duration::from_secs(10)),
            1,
        );
        assert_eq!(delay, Duration::from_secs(12));
    }

    #[test]
    fn delay_for_429_with_large_retry_after_caps_extra_at_60_seconds() {
        // Retry-After: 400 s → 400 + min(80, 60) = 460
        let delay = compute_retry_delay(
            StatusCode::TOO_MANY_REQUESTS,
            Some(Duration::from_secs(400)),
            1,
        );
        assert_eq!(delay, Duration::from_secs(460));
    }

    #[test]
    fn delay_for_429_with_retry_after_at_cap_boundary() {
        // Retry-After: 300 s → 300 + min(60, 60) = 360
        let delay = compute_retry_delay(
            StatusCode::TOO_MANY_REQUESTS,
            Some(Duration::from_secs(300)),
            1,
        );
        assert_eq!(delay, Duration::from_secs(360));
    }

    #[test]
    fn delay_for_5xx_ignores_retry_after_value() {
        // Retry-After is irrelevant for non-429 status codes.
        let delay_with = compute_retry_delay(
            StatusCode::INTERNAL_SERVER_ERROR,
            Some(Duration::from_secs(999)),
            2,
        );
        let delay_without = compute_retry_delay(StatusCode::INTERNAL_SERVER_ERROR, None, 2);
        assert_eq!(delay_with, delay_without);
    }

    #[test]
    fn delay_for_429_with_zero_retry_after_adds_no_extra() {
        // Retry-After: 0 s → 0 + min(0, 60) = 0
        let delay = compute_retry_delay(
            StatusCode::TOO_MANY_REQUESTS,
            Some(Duration::ZERO),
            1,
        );
        assert_eq!(delay, Duration::from_secs(0));
    }
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
    let index_res: Result<Value, PactBrokerError> = hal_client.fetch("").await;
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
