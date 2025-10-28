use crate::{
    cli::pact_broker::main::{HALClient, PactBrokerError},
    cli::pact_broker::main::types::{BrokerDetails, OutputType},
    cli::pact_broker::main::utils::{follow_templated_broker_relation, generate_table},
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

pub fn get_pacts(
    broker_details: &BrokerDetails,
    provider: &str,
    consumer: Option<&str>,
    branch: Option<&str>,
    latest: bool,
    output_type: OutputType,
    download: bool,
    download_dir: &str,
) -> Result<String, PactBrokerError> {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        // Create HAL client
        let hal_client = HALClient::with_url(
            &broker_details.url,
            broker_details.auth.clone(),
            broker_details.ssl_options.clone(),
        );

        // Build the appropriate HAL relation and template parameters
        let (relation, path) = build_pacts_path(provider, consumer, branch, latest);
        
        // Create template parameters for the HAL relation
        let mut template_params = HashMap::new();
        template_params.insert("provider".to_string(), provider.to_string());
        if let Some(consumer_name) = consumer {
            template_params.insert("consumer".to_string(), consumer_name.to_string());
        }
        if let Some(branch_name) = branch {
            template_params.insert("branch".to_string(), branch_name.to_string());
        }

        // Follow the HAL relation with template parameters
        let result = follow_templated_broker_relation(
            hal_client.clone(),
            relation,
            path,
            template_params,
        ).await?;

        // Parse the response based on the structure
        let pacts_data = if let Some(pacts_array) = result.get("_links").and_then(|links| links.get("pb:pacts")) {
            // Handle case where pacts are in _links.pacts
            json!({
                "pacts": pacts_array,
            })
        } else {
            result
        };

        // Download pacts if requested
        if download {
            download_pacts(&pacts_data, &hal_client, download_dir).await?;
        }

        let output = match output_type {
            OutputType::Json => serde_json::to_string_pretty(&pacts_data).unwrap(),
            OutputType::Table => generate_pacts_table(&pacts_data, consumer.is_some()),
            OutputType::Text => generate_pacts_table(&pacts_data, consumer.is_some()),
            OutputType::Pretty => serde_json::to_string_pretty(&pacts_data).unwrap(),
        };
        
        println!("{}", output);
        Ok(output)
    })
}

fn build_pacts_path(provider: &str, consumer: Option<&str>, branch: Option<&str>, latest: bool) -> (String, String) {
    match (consumer, branch, latest) {
        // Provider + Consumer + Branch + Latest
        (Some(_), Some(_), true) => (
            "pb:latest-branch-pact-versions".to_string(),
            format!("/pacts/provider/{}/consumer/{}/branch/{}/latest", provider, consumer.unwrap(), branch.unwrap())
        ),
        // Provider + Consumer + Branch (no latest)
        (Some(_), Some(_), false) => (
            "pb:branch-pact-versions".to_string(),
            format!("/pacts/provider/{}/consumer/{}/branch/{}", provider, consumer.unwrap(), branch.unwrap())
        ),
        // Provider + Consumer + Main Branch + Latest
        (Some(_), None, true) => (
            "pb:latest-main-branch-pact-versions".to_string(),
            format!("/pacts/provider/{}/consumer/{}/branch/latest", provider, consumer.unwrap())
        ),
        // Provider + Consumer + Main Branch (no latest)
        (Some(_), None, false) => (
            "pb:main-branch-pact-versions".to_string(),
            format!("/pacts/provider/{}/consumer/{}/branch", provider, consumer.unwrap())
        ),
        // Provider + Branch + Latest (any consumer)
        (None, Some(_), true) => (
            "pb:latest-provider-pacts-with-branch".to_string(),
            format!("/pacts/provider/{}/branch/{}/latest", provider, branch.unwrap())
        ),
        // Provider + Branch (any consumer, no latest)
        (None, Some(_), false) => (
            "pb:provider-pacts-with-branch".to_string(),
            format!("/pacts/provider/{}/branch/{}", provider, branch.unwrap())
        ),
        // Provider + Main Branch + Latest (any consumer)
        (None, None, true) => (
            "pb:latest-provider-pacts-with-main-branch".to_string(),
            format!("/pacts/provider/{}/branch/latest", provider)
        ),
        // Provider + Main Branch (any consumer, no latest)
        (None, None, false) => (
            "pb:provider-pacts-with-main-branch".to_string(),
            format!("/pacts/provider/{}/branch", provider)
        ),
    }
}

async fn download_pacts(pacts_data: &Value, hal_client: &HALClient, download_dir: &str) -> Result<(), PactBrokerError> {
    // Create download directory if it doesn't exist
    fs::create_dir_all(download_dir).map_err(|e| {
        PactBrokerError::IoError(format!("Failed to create download directory: {}", e))
    })?;

    // Extract pacts array from the data
    let pacts: Vec<&Value> = if let Some(pacts_array) = pacts_data.get("pacts").and_then(|p| p.as_array()) {
        pacts_array.iter().collect()
    } else if let Some(single_pact) = pacts_data.get("pact") {
        // Handle single pact response
        vec![single_pact]
    } else {
        // Try to find pacts in _links.pb:pacts
        if let Some(pacts_links) = pacts_data.get("_links").and_then(|l| l.get("pb:pacts")).and_then(|p| p.as_array()) {
            pacts_links.iter().collect()
        } else {
            return Err(PactBrokerError::ContentError("No pacts found to download".to_string()));
        }
    };

    tracing::info!("Downloading {} pact(s) to {}", pacts.len(), download_dir);

    for pact in pacts {
        // Get the pact URL from the self link
        let pact_url = if let Some(self_link) = pact.get("_links").and_then(|l| l.get("self")).and_then(|s| s.get("href")) {
            self_link.as_str().unwrap_or_default()
        } else if let Some(href) = pact.get("href") {
            href.as_str().unwrap_or_default()
        } else {
            continue;
        };

        // Download the pact content first to get the proper metadata
        let pact_content = hal_client.clone().fetch(pact_url).await?;
        
        // Extract consumer and provider names from the downloaded pact content
        let consumer_name = pact_content.get("_links")
            .and_then(|l| l.get("pb:consumer"))
            .and_then(|c| c.get("name"))
            .and_then(|n| n.as_str())
            .or_else(|| pact.get("name").and_then(|n| n.as_str()))
            .unwrap_or("unknown");

        let provider_name = pact_content.get("_links")
            .and_then(|l| l.get("pb:provider"))
            .and_then(|p| p.get("name"))
            .and_then(|n| n.as_str())
            .unwrap_or("unknown");

        let version = pact_content.get("_links")
            .and_then(|l| l.get("pb:consumer-version"))
            .and_then(|v| v.get("name"))
            .and_then(|n| n.as_str())
            .unwrap_or("unknown");

        // Generate filename
        let filename = format!("{}-{}-{}.json", consumer_name, provider_name, version);
        let file_path = Path::new(download_dir).join(&filename);

        // Download the pact content
        tracing::info!("Downloading pact: {}", filename);
        
        // Save to file
        let content_str = serde_json::to_string_pretty(&pact_content).map_err(|e| {
            PactBrokerError::ContentError(format!("Failed to serialize pact content: {}", e))
        })?;
        
        fs::write(&file_path, content_str).map_err(|e| {
            PactBrokerError::IoError(format!("Failed to write pact file {}: {}", filename, e))
        })?;

        println!("  → {}", file_path.display());
    }

    Ok(())
}

fn generate_pacts_table(result: &Value, _has_consumer_filter: bool) -> String {
    // Generate a table showing the pacts data
    generate_table(
        result,
        vec!["CONSUMER", "TITLE", "LINK"],
        vec![
            vec!["name"],
            vec!["title"],
            vec!["href"],
        ],
    ).to_string()
}

#[cfg(test)]
mod get_pacts_tests {
    use super::*;
    use crate::cli::pact_broker::main::types::{BrokerDetails, OutputType, SslOptions};
    use pact_consumer::prelude::*;
    use pact_models::PactSpecification;
    use serde_json::json;

    #[test]
    fn test_build_pacts_path_provider_only() {
        let (relation, path) = build_pacts_path("TestProvider", None, None, false);
        assert_eq!(relation, "pb:provider-pacts-with-main-branch");
        assert_eq!(path, "/pacts/provider/TestProvider/branch");
    }

    #[test]
    fn test_build_pacts_path_provider_and_consumer() {
        let (relation, path) = build_pacts_path("TestProvider", Some("TestConsumer"), None, false);
        assert_eq!(relation, "pb:main-branch-pact-versions");
        assert_eq!(path, "/pacts/provider/TestProvider/consumer/TestConsumer/branch");
    }

    #[test]
    fn test_build_pacts_path_provider_consumer_and_branch() {
        let (relation, path) = build_pacts_path("TestProvider", Some("TestConsumer"), Some("feature"), false);
        assert_eq!(relation, "pb:branch-pact-versions");
        assert_eq!(path, "/pacts/provider/TestProvider/consumer/TestConsumer/branch/feature");
    }

    #[test]
    fn test_build_pacts_path_provider_consumer_branch_latest() {
        let (relation, path) = build_pacts_path("TestProvider", Some("TestConsumer"), Some("feature"), true);
        assert_eq!(relation, "pb:latest-branch-pact-versions");
        assert_eq!(path, "/pacts/provider/TestProvider/consumer/TestConsumer/branch/feature/latest");
    }

    #[test]
    fn test_build_pacts_path_provider_and_branch() {
        let (relation, path) = build_pacts_path("TestProvider", None, Some("feature"), false);
        assert_eq!(relation, "pb:provider-pacts-with-branch");
        assert_eq!(path, "/pacts/provider/TestProvider/branch/feature");
    }

    #[test]
    fn test_build_pacts_path_provider_branch_latest() {
        let (relation, path) = build_pacts_path("TestProvider", None, Some("feature"), true);
        assert_eq!(relation, "pb:latest-provider-pacts-with-branch");
        assert_eq!(path, "/pacts/provider/TestProvider/branch/feature/latest");
    }

    #[test]
    fn test_build_pacts_path_provider_latest_main() {
        let (relation, path) = build_pacts_path("TestProvider", None, None, true);
        assert_eq!(relation, "pb:latest-provider-pacts-with-main-branch");
        assert_eq!(path, "/pacts/provider/TestProvider/branch/latest");
    }

    #[test]
    fn test_generate_pacts_table() {
        use serde_json::json;
        
        let test_data = json!({
            "pacts": [
                {
                    "name": "TestConsumer",
                    "title": "Pact between TestConsumer and TestProvider",
                    "href": "http://example.org/pacts/provider/TestProvider/consumer/TestConsumer/latest"
                }
            ]
        });

        let result = generate_pacts_table(&test_data, false);
        assert!(result.contains("TestConsumer"));
        assert!(result.contains("Pact between TestConsumer and TestProvider"));
        assert!(result.contains("http://example.org/pacts/provider/TestProvider/consumer/TestConsumer/latest"));
    }

    #[test]
    fn get_pacts_all_consumers_main_branch() {
        // arrange - set up the pact mock server
        let config = MockServerConfig {
            pact_specification: PactSpecification::V2,
            ..MockServerConfig::default()
        };

        let body = json!({
            "_links": {
                "self": {
                    "href": "http://example.org/pacts/provider/Provider/branch/"
                }
            },
            "pacts": [
                {
                    "_links": {
                        "self": {
                            "href": "http://example.org/pacts/provider/Provider/consumer/Consumer/version/1.0.0"
                        }
                    },
                    "_embedded": {
                        "consumer": {
                            "name": "Consumer",
                            "_embedded": {
                                "version": {
                                    "number": "1.0.0",
                                    "branch": "main"
                                }
                            }
                        },
                        "provider": {
                            "name": "Provider"
                        }
                    },
                    "createdAt": "2024-01-01T00:00:00Z"
                }
            ]
        });

        let pact_broker_service = PactBuilder::new("pact-broker-cli", "Pact Broker")
            .interaction("a request to get pacts for provider on main branch", "", |mut i| {
                i.given("pacts exist for the provider");
                i.request
                    .get()
                    .path("/pacts/provider/TestProvider/branch")
                    .header("Accept", "application/hal+json")
                    .header("Accept", "application/json");
                i.response
                    .status(200)
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(body.clone());
                i
            })
            .start_mock_server(None, Some(config));
        let mock_server_url = pact_broker_service.url();

        // arrange - set up the broker details
        let broker_details = BrokerDetails {
            url: mock_server_url.to_string(),
            auth: None,
            ssl_options: SslOptions::default(),
        };

        // act
        let result = get_pacts(&broker_details, "TestProvider", None, None, false, OutputType::Json, false, "./pacts");

        // assert
        assert!(result.is_ok(), "Expected success but got error: {:?}", result.err());
        let output = result.unwrap();
        let output_json: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(output_json, body);
    }

    #[test] 
    fn get_pacts_specific_consumer_and_branch() {
        // arrange - set up the pact mock server
        let config = MockServerConfig {
            pact_specification: PactSpecification::V2,
            ..MockServerConfig::default()
        };

        let body = json!({
            "_links": {
                "self": {
                    "href": "http://example.org/pacts/provider/Provider/consumer/Consumer/branch/feature"
                }
            },
            "pacts": [
                {
                    "_links": {
                        "self": {
                            "href": "http://example.org/pacts/provider/Provider/consumer/Consumer/version/1.1.0"
                        }
                    },
                    "_embedded": {
                        "consumer": {
                            "name": "Consumer",
                            "_embedded": {
                                "version": {
                                    "number": "1.1.0",
                                    "branch": "feature"
                                }
                            }
                        },
                        "provider": {
                            "name": "Provider"
                        }
                    },
                    "createdAt": "2024-01-02T00:00:00Z"
                }
            ]
        });

        let pact_broker_service = PactBuilder::new("pact-broker-cli", "Pact Broker")
            .interaction("a request to get pacts for provider and consumer on specific branch", "", |mut i| {
                i.given("pacts exist for the provider and consumer on the branch");
                i.request
                    .get()
                    .path("/pacts/provider/TestProvider/consumer/TestConsumer/branch/feature")
                    .header("Accept", "application/hal+json")
                    .header("Accept", "application/json");
                i.response
                    .status(200)
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(body.clone());
                i
            })
            .start_mock_server(None, Some(config));
        let mock_server_url = pact_broker_service.url();

        // arrange - set up the broker details
        let broker_details = BrokerDetails {
            url: mock_server_url.to_string(),
            auth: None,
            ssl_options: SslOptions::default(),
        };

        // act
        let result = get_pacts(&broker_details, "TestProvider", Some("TestConsumer"), Some("feature"), false, OutputType::Json, false, "./pacts");

        // assert
        assert!(result.is_ok(), "Expected success but got error: {:?}", result.err());
        let output = result.unwrap();
        let output_json: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(output_json, body);
    }

    #[test]
    fn get_pacts_latest_for_branch() {
        // arrange - set up the pact mock server
        let config = MockServerConfig {
            pact_specification: PactSpecification::V2,
            ..MockServerConfig::default()
        };

        let body = json!({
            "_links": {
                "self": {
                    "href": "http://example.org/pacts/provider/Provider/branch/main/latest"
                }
            },
            "pact": {
                "_links": {
                    "self": {
                        "href": "http://example.org/pacts/provider/Provider/consumer/Consumer/version/1.0.0"
                    }
                },
                "_embedded": {
                    "consumer": {
                        "name": "Consumer",
                        "_embedded": {
                            "version": {
                                "number": "1.0.0",
                                "branch": "main"
                            }
                        }
                    },
                    "provider": {
                        "name": "Provider"
                    }
                },
                "createdAt": "2024-01-01T00:00:00Z"
            }
        });

        let pact_broker_service = PactBuilder::new("pact-broker-cli", "Pact Broker")
            .interaction("a request to get latest pacts for provider on branch", "", |mut i| {
                i.given("pacts exist for the provider on the branch");
                i.request
                    .get()
                    .path("/pacts/provider/TestProvider/branch/main/latest")
                    .header("Accept", "application/hal+json")
                    .header("Accept", "application/json");
                i.response
                    .status(200)
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(body.clone());
                i
            })
            .start_mock_server(None, Some(config));
        let mock_server_url = pact_broker_service.url();

        // arrange - set up the broker details
        let broker_details = BrokerDetails {
            url: mock_server_url.to_string(),
            auth: None,
            ssl_options: SslOptions::default(),
        };

        // act
        let result = get_pacts(&broker_details, "TestProvider", None, Some("main"), true, OutputType::Json, false, "./pacts");

        // assert
        assert!(result.is_ok(), "Expected success but got error: {:?}", result.err());
        let output = result.unwrap();
        let output_json: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(output_json, body);
    }
}