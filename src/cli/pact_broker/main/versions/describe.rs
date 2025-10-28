use comfy_table::{Table, presets::UTF8_FULL};
use maplit::hashmap;
use serde_json::Value;

use crate::cli::pact_broker::main::{
    HALClient, PactBrokerError,
    types::{OutputType, SslOptions},
    utils::{
        follow_templated_broker_relation, get_auth, get_broker_relation, get_broker_url,
        get_ssl_options, follow_broker_relation,
    },
};
use crate::cli::pact_broker::main::HttpAuth;

pub fn describe_version(args: &clap::ArgMatches) -> Result<String, PactBrokerError> {
    let broker_url = get_broker_url(args).trim_end_matches('/').to_string();
    let auth = get_auth(args);
    let ssl_options = get_ssl_options(args);

    let version: Option<&String> = args.try_get_one::<String>("version").unwrap();
    let latest: Option<&String> = args.try_get_one::<String>("latest").unwrap();
    let environment: Option<&String> = args.try_get_one::<String>("environment").unwrap();
    let deployed_only = args.get_flag("deployed");
    let released_only = args.get_flag("released");
    let output_type: OutputType = args
        .get_one::<String>("output")
        .and_then(|s| s.parse().ok())
        .unwrap_or(OutputType::Table);
    let pacticipant_name = args.get_one::<String>("pacticipant").unwrap();

    // If environment is specified, use environment-based queries
    if let Some(env_name) = environment {
        return describe_version_by_environment(
            &broker_url, &auth, &ssl_options, pacticipant_name, env_name,
            deployed_only, released_only, output_type
        );
    }

    let pb_relation_href = if latest.is_some() {
        "pb:latest-tagged-version".to_string()
    } else {
        "pb:latest-version".to_string()
    };

    let res = tokio::runtime::Runtime::new().unwrap().block_on(async {
        let hal_client: HALClient =
            HALClient::with_url(&broker_url, Some(auth.clone()), ssl_options.clone());
        let pb_version_href_path =
            get_broker_relation(hal_client.clone(), pb_relation_href, broker_url.to_string()).await;

        follow_templated_broker_relation(
            hal_client.clone(),
            "pb:pacticipant-version".to_string(),
            pb_version_href_path.unwrap(),
            hashmap! {
                "pacticipant".to_string() => pacticipant_name.to_string(),
                "version".to_string() => version.cloned().unwrap_or_default(),
                "tag".to_string() => latest.cloned().unwrap_or_default(),
            },
        )
        .await
    });

    match res {
        Ok(result) => match output_type {
            OutputType::Json => {
                let json: String = serde_json::to_string(&result).unwrap();
                println!("{}", json);
                return Ok(json);
            }
            OutputType::Table => {
                let mut table = Table::new();
                table
                    .load_preset(UTF8_FULL)
                    .set_header(vec!["NAME", "TAGS"]);

                let version_number = result.get("number").and_then(|v| v.as_str()).unwrap_or("-");

                let tags = result
                    .get("_embedded")
                    .and_then(|v| v.get("tags"))
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|tag| tag.get("name").and_then(|n| n.as_str()))
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_else(|| "-".to_string());

                table.add_row(vec![version_number, &tags]);
                println!("{table}");
                return Ok(table.to_string());
            }

            OutputType::Text => {
                return Err(PactBrokerError::NotFound(
                    "Text output is not supported for describe versions".to_string(),
                ));
            }
            OutputType::Pretty => {
                let json: String = serde_json::to_string(&result).unwrap();
                println!("{}", json);
                return Ok(json);
            }
        },
        Err(PactBrokerError::NotFound(_)) => Err(PactBrokerError::NotFound(format!(
            "Pacticipant version not found"
        ))),

        Err(err) => Err(err),
    }
}

/// Describe versions deployed/released to a specific environment
fn describe_version_by_environment(
    broker_url: &str,
    auth: &HttpAuth,
    ssl_options: &SslOptions,
    pacticipant_name: &str,
    environment_name: &str,
    deployed_only: bool,
    released_only: bool,
    output_type: OutputType,
) -> Result<String, PactBrokerError> {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let hal_client: HALClient =
            HALClient::with_url(broker_url, Some(auth.clone()), ssl_options.clone());

        // First, get the environment UUID
        let environments_href = get_broker_relation(
            hal_client.clone(),
            "pb:environments".to_string(),
            broker_url.to_string(),
        )
        .await?;

        let environments_response = follow_broker_relation(
            hal_client.clone(),
            "pb:environments".to_string(),
            environments_href,
        )
        .await?;

        // Find the environment UUID by name
        let environment_uuid = find_environment_uuid(&environments_response, environment_name)
            .ok_or_else(|| {
                PactBrokerError::NotFound(format!("Environment '{}' not found", environment_name))
            })?;

        // Query endpoints based on flags
        let mut all_versions = Vec::new();
        
        if deployed_only || (!deployed_only && !released_only) {
            // Get currently deployed versions
            let deployed_path = format!(
                "/environments/{}/deployed-versions/currently-deployed",
                environment_uuid
            );
            
            if let Ok(deployed_response) = hal_client
                .clone()
                .fetch(&format!("{}{}", broker_url, deployed_path))
                .await {
                let deployed_versions = filter_versions_by_pacticipant(&deployed_response, pacticipant_name);
                all_versions.extend(deployed_versions);
            }
        }
        
        if released_only || (!deployed_only && !released_only) {
            // Get currently supported released versions
            let released_path = format!(
                "/environments/{}/released-versions/currently-supported",
                environment_uuid
            );
            
            if let Ok(released_response) = hal_client
                .clone()
                .fetch(&format!("{}{}", broker_url, released_path))
                .await {
                let released_versions = filter_versions_by_pacticipant(&released_response, pacticipant_name);
                all_versions.extend(released_versions);
            }
        }

        format_environment_versions_output(all_versions, environment_name, output_type)
    })
}

/// Find environment UUID by name from environments response
fn find_environment_uuid(environments_response: &Value, environment_name: &str) -> Option<String> {
    environments_response
        .get("_embedded")
        .and_then(|e| e.get("environments"))
        .and_then(|envs| envs.as_array())
        .and_then(|envs| {
            envs.iter().find(|env| {
                env.get("name")
                    .and_then(|n| n.as_str())
                    .map(|n| n == environment_name)
                    .unwrap_or(false)
            })
        })
        .and_then(|env| env.get("uuid"))
        .and_then(|uuid| uuid.as_str())
        .map(|s| s.to_string())
}

/// Filter versions by pacticipant name
fn filter_versions_by_pacticipant(versions_response: &Value, pacticipant_name: &str) -> Vec<Value> {
    versions_response
        .get("_embedded")
        .and_then(|e| {
            e.get("deployedVersions")
                .or_else(|| e.get("releasedVersions"))
                .or_else(|| e.get("currentlyDeployedVersions"))
                .or_else(|| e.get("currentlySupportedVersions"))
        })
        .and_then(|versions| versions.as_array())
        .map(|versions| {
            versions
                .iter()
                .filter(|version| {
                    version
                        .get("_embedded")
                        .and_then(|e| e.get("pacticipant"))
                        .and_then(|p| p.get("name"))
                        .and_then(|n| n.as_str())
                        .map(|n| n == pacticipant_name)
                        .unwrap_or(false)
                })
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

/// Format environment versions output
fn format_environment_versions_output(
    versions: Vec<Value>,
    environment_name: &str,
    output_type: OutputType,
) -> Result<String, PactBrokerError> {
    match output_type {
        OutputType::Json => {
            let json = serde_json::to_string(&versions)
                .map_err(|e| PactBrokerError::ContentError(e.to_string()))?;
            println!("{}", json);
            Ok(json)
        }
        OutputType::Table => {
            let mut table = Table::new();
            table
                .load_preset(UTF8_FULL)
                .set_header(vec!["VERSION", "STATUS", "ENVIRONMENT", "APPLICATION INSTANCE"]);

            for version in &versions {
                let version_number = version
                    .get("_embedded")
                    .and_then(|e| e.get("version"))
                    .and_then(|v| v.get("number"))
                    .and_then(|n| n.as_str())
                    .unwrap_or("-");

                let status = if version.get("currentlyDeployed").and_then(|v| v.as_bool()).unwrap_or(false) {
                    "Deployed"
                } else if version.get("currentlyReleased").and_then(|v| v.as_bool()).unwrap_or(false) {
                    "Released"
                } else {
                    "Unknown"
                };

                let application_instance = version
                    .get("applicationInstance")
                    .and_then(|ai| ai.as_str())
                    .unwrap_or("-");

                table.add_row(vec![version_number, status, environment_name, application_instance]);
            }

            let table_str = table.to_string();
            println!("{}", table_str);
            Ok(table_str)
        }
        OutputType::Text => Err(PactBrokerError::NotFound(
            "Text output is not supported for environment versions".to_string(),
        )),
        OutputType::Pretty => {
            let json = serde_json::to_string_pretty(&versions)
                .map_err(|e| PactBrokerError::ContentError(e.to_string()))?;
            println!("{}", json);
            Ok(json)
        }
    }
}

#[cfg(test)]
mod describe_version_tests {
    use super::describe_version;
    use crate::cli::pact_broker::main::subcommands::add_describe_version_subcommand;
    use pact_consumer::prelude::*;
    use pact_models::PactSpecification;
    use serde_json::json;

    #[test]
    fn describe_version_returns_version_and_tags_as_table() {
        // Arrange
        let config = MockServerConfig {
            pact_specification: PactSpecification::V2,
            ..MockServerConfig::default()
        };

        let pacticipant_name = "Condor";
        let version_number = "1.3.0";
        let tag_name = "prod";

        let version_path = format!(
            "/pacticipants/{}/versions/{}",
            pacticipant_name, version_number
        );

        let pact_broker_service = PactBuilder::new("pact-broker-cli", "Pact Broker")
            .interaction("a request for the index resource for records_undeployment_successfully", "", |mut i| {
                i.given("the pb:pacticipant-version relation exists in the index resource");
                i.request
                    .path("/")
                    .header("Accept", "application/hal+json")
                    .header("Accept", "application/json");
                i.response
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(json_pattern!({
                        "_links": {
                            "pb:pacticipant-version": {
                                "href": term!("http:\\/\\/.*",format!("http://localhost{}",version_path)),
                            },
                            "pb:latest-version": {
                                "href": term!("http:\\/\\/.*",format!("http://localhost{}",version_path)),
                            },
                            "pb:latest-tagged-version": {
                                "href": term!("http:\\/\\/.*",format!("http://localhost{}",version_path)),
                            }
                        }
                    }));
                i
            })
            .interaction("get pacticipant version", "", |mut i| {
                i.given(format!(
                    "'{}' exists in the pact-broker with version {}, tagged with '{}'",
                    pacticipant_name, version_number, tag_name
                ));
                i.request
                    .method("GET")
                    .path(version_path.clone())
                    .header("Accept", "application/hal+json")
                    .header("Accept", "application/json");
                i.response
                    .status(200)
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(json!({
                        "number": version_number,
                        "_embedded": {
                            "tags": [
                                { "name": tag_name }
                            ]
                        }
                    }));
                i
            })
            .start_mock_server(None, Some(config));

        let mock_server_url = pact_broker_service.url();

        // Arrange CLI args
        let matches = add_describe_version_subcommand().get_matches_from(vec![
            "describe-versions",
            "-b",
            mock_server_url.as_str(),
            "--pacticipant",
            pacticipant_name,
            "--version",
            version_number,
            "--output",
            "table",
        ]);

        // Act
        let result = describe_version(&matches);

        // Assert
        assert!(result.is_ok());
        let output = result.unwrap();
        println!("Output: {}", output);
        assert!(output.contains(version_number));
        assert!(output.contains(tag_name));
    }

    #[test]
    fn describe_version_returns_json_output() {
        // Arrange
        let config = MockServerConfig {
            pact_specification: PactSpecification::V2,
            ..MockServerConfig::default()
        };

        let pacticipant_name = "Condor";
        let version_number = "1.2.3";

        let version_path = format!(
            "/pacticipants/{}/versions/{}",
            pacticipant_name, version_number
        );

        let pact_broker_service = PactBuilder::new("pact-broker-cli", "Pact Broker")
              .interaction("a request for the index resource for records_undeployment_successfully", "", |mut i| {
                i.given("the pb:pacticipant-version relation exists in the index resource");
                i.request
                    .path("/")
                    .header("Accept", "application/hal+json")
                    .header("Accept", "application/json");
                i.response
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(json_pattern!({
                        "_links": {
                            "pb:pacticipant-version": {
                                "href": term!("http:\\/\\/.*",format!("http://localhost{}",version_path)),
                            },
                            "pb:latest-version": {
                                "href": term!("http:\\/\\/.*",format!("http://localhost{}",version_path)),
                            },
                            "pb:latest-tagged-version": {
                                "href": term!("http:\\/\\/.*",format!("http://localhost{}",version_path)),
                            }
                        }
                    }));
                i
            })
            .interaction("get pacticipant version for json", "", |mut i| {
                i.given(format!(
                    "'{}' exists in the pact-broker with the latest version {}",
                    pacticipant_name, version_number
                ));
                i.request
                    .method("GET")
                    .path(version_path.clone())
                    .header("Accept", "application/hal+json")
                    .header("Accept", "application/json");
                i.response
                    .status(200)
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(json!({
                        "number": version_number,
                        "_embedded": {
                            "tags": []
                        }
                    }));
                i
            })
            .start_mock_server(None, Some(config));

        let mock_server_url = pact_broker_service.url();

        // Arrange CLI args
        let matches = add_describe_version_subcommand().get_matches_from(vec![
            "describe-versions",
            "-b",
            mock_server_url.as_str(),
            "--pacticipant",
            pacticipant_name,
            "--version",
            version_number,
            "--output",
            "json",
        ]);

        // Act
        let result = describe_version(&matches);

        // Assert
        assert!(result.is_ok());
        let output = result.unwrap();
        println!("Output: {}", output);
        assert!(output.contains(version_number));
        assert!(output.contains("\"tags\":[]"));
    }

    #[test]
    fn describe_version_returns_not_found_error() {
        // Arrange
        let config = MockServerConfig {
            pact_specification: PactSpecification::V2,
            ..MockServerConfig::default()
        };

        let pacticipant_name = "Baz";
        let version_number = "9.9.9";

        let version_path = format!(
            "/pacticipants/{}/versions/{}",
            pacticipant_name, version_number
        );

        let pact_broker_service = PactBuilder::new("pact-broker-cli", "Pact Broker")
              .interaction("a request for the index resource for records_undeployment_successfully", "", |mut i| {
                i.given("the pb:pacticipant-version relation exists in the index resource");
                i.request
                    .path("/")
                    .header("Accept", "application/hal+json")
                    .header("Accept", "application/json");
                i.response
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(json_pattern!({
                        "_links": {
                            "pb:pacticipant-version": {
                                "href": term!("http:\\/\\/.*",format!("http://localhost{}",version_path)),
                            },
                            "pb:latest-version": {
                                "href": term!("http:\\/\\/.*",format!("http://localhost{}",version_path)),
                            },
                            "pb:latest-tagged-version": {
                                "href": term!("http:\\/\\/.*",format!("http://localhost{}",version_path)),
                            }
                        }
                    }));
                i
            })
            .interaction("get non-existent pacticipant version", "", |mut i| {
                // i.given(format!(
                //     "no pacticipant with name {} and version {} exists",
                //     pacticipant_name, version_number
                // ));
                i.request
                    .method("GET")
                    .path(version_path.clone())
                    .header("Accept", "application/hal+json")
                    .header("Accept", "application/json");
                i.response
                    .status(404)
                    .header("Content-Type", "application/json;charset=utf-8")
                    .json_body(json!({
                        "error": "The requested document was not found on this server."
                    }));
                i
            })
            .start_mock_server(None, Some(config));

        let mock_server_url = pact_broker_service.url();

        // Arrange CLI args
        let matches = add_describe_version_subcommand().get_matches_from(vec![
            "describe-versions",
            "-b",
            mock_server_url.as_str(),
            "--pacticipant",
            pacticipant_name,
            "--version",
            version_number,
            "--output",
            "table",
        ]);

        // Act
        let result = describe_version(&matches);

        // Assert
        assert!(result.is_err());
        let err = result.unwrap_err();
        let err_str = format!("{:?}", err);
        println!("Error: {}", err_str);
        assert!(err_str.contains("Pacticipant version not found"));
    }
}
