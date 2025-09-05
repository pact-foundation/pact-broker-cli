use comfy_table::{Table, presets::UTF8_FULL};
use maplit::hashmap;

use crate::cli::pact_broker::main::{
    HALClient, PactBrokerError,
    types::OutputType,
    utils::{
        follow_templated_broker_relation, get_auth, get_broker_relation, get_broker_url,
        get_ssl_options,
    },
};

pub fn describe_version(args: &clap::ArgMatches) -> Result<String, PactBrokerError> {
    let broker_url = get_broker_url(args).trim_end_matches('/').to_string();
    let auth = get_auth(args);
    let ssl_options = get_ssl_options(args);

    let version: Option<&String> = args.try_get_one::<String>("version").unwrap();
    let latest: Option<&String> = args.try_get_one::<String>("latest").unwrap();
    let output_type: OutputType = args
        .get_one::<String>("output")
        .and_then(|s| s.parse().ok())
        .unwrap_or(OutputType::Table);
    let pacticipant_name = args.get_one::<String>("pacticipant").unwrap();

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

#[cfg(test)]
mod describe_version_tests {
    use super::describe_version;
    use crate::cli::{
        add_ssl_arguments, pact_broker::main::subcommands::add_describe_version_subcommand,
    };
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
        let matches = add_describe_version_subcommand()
            .args(add_ssl_arguments())
            .get_matches_from(vec![
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
        let matches = add_describe_version_subcommand()
            .args(add_ssl_arguments())
            .get_matches_from(vec![
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
        let matches = add_describe_version_subcommand()
            .args(add_ssl_arguments())
            .get_matches_from(vec![
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
