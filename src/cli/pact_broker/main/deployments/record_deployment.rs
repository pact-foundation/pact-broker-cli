use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::cli::{
    pact_broker::main::{
        HALClient, PactBrokerError,
        utils::{get_auth, get_broker_url, get_custom_headers, get_ssl_options},
    },
    utils,
};

pub fn record_deployment(args: &clap::ArgMatches) -> Result<String, PactBrokerError> {
    let version = args.get_one::<String>("version");
    let pacticipant = args.get_one::<String>("pacticipant");
    let environment = args.get_one::<String>("environment");
    let application_instance = args.get_one::<String>("application-instance");
    let broker_url = get_broker_url(args).trim_end_matches('/').to_string();
    let auth = get_auth(args);
    let custom_headers = get_custom_headers(args);
    let ssl_options = get_ssl_options(args);
    tokio::runtime::Runtime::new().unwrap().block_on(async {
                let hal_client: HALClient = HALClient::with_url(&broker_url, Some(auth.clone()), ssl_options.clone(), custom_headers.clone());

                let res = hal_client.clone()
                    .fetch(
                        &(broker_url.clone()
                            + "/pacticipants/"
                            + &pacticipant.unwrap()
                            + "/versions/"
                            + &version.unwrap()),
                    )
                    .await;

                #[derive(Debug, Deserialize, Serialize)]
                struct PacticipantVersions {
                    _links: Links,
                }

                #[derive(Debug, Deserialize, Serialize)]
                struct Links {
                    #[serde(rename = "pb:record-deployment")]
                    record_deployment: Vec<Link>,
                }

                #[derive(Debug, Deserialize, Serialize)]
                struct Link {
                    href: String,
                    name: Option<String>,
                    title: Option<String>,
                    templated: Option<bool>,
                }

                match res {
                    Ok(res) => {

                        let result: Result<PacticipantVersions, serde_json::Error> = serde_json::from_value(res);
                        match result {
                            Ok(data) => {
                            match data._links.record_deployment.iter().find(|x| x.name == Some(environment.unwrap().to_string())) {
                                Some(link) => {
                                    let link_record_deployment_href = &link.href;

                                    // <- "POST /pacticipants/Example%20App/versions/5556b8149bf8bac76bc30f50a8a2dd4c22c85f30/deployed-versions/environment/c540ce64-5493-48c5-ab7c-28dae27b166b HTTP/1.1\r\nAccept: application/hal+json\r\nUser-Agent: Ruby\r\nContent-Type: application/json\r\nHost: localhost:9292\r\nContent-Length: 44\r\n\r\n"
                                    // <- "{\"applicationInstance\":\"foo\",\"target\":\"foo\"}"


                                    let mut payload = json!({});
                                    payload["target"] = serde_json::Value::String(environment.unwrap().to_string());
                                    if let Some(application_instance) = application_instance {
                                        payload["applicationInstance"] = serde_json::Value::String(application_instance.to_string());
                                    }
                                    let res: Result<Value, PactBrokerError> = hal_client.clone().post_json(&(link_record_deployment_href.clone()), &payload.to_string(), None).await;
                                    let default_output = "text".to_string();
                                    let output = args.get_one::<String>("output").unwrap_or(&default_output);
                                    match res {
                                        Ok(res) => {
                                            let message = format!("✅ Recorded deployment of {} version {} to {} environment{} in the Pact Broker.", utils::GREEN.apply_to(pacticipant.unwrap()), utils::GREEN.apply_to(version.unwrap()),utils::GREEN.apply_to(environment.unwrap()), application_instance.map(|instance| format!(" (application instance {})", utils::GREEN.apply_to(instance))).unwrap_or_default());

                                                if output == "pretty" {
                                                    let json = serde_json::to_string_pretty(&res).unwrap();
                                                    println!("{}", json);
                                                    return Ok(json);
                                                } else if output == "json" {
                                                    let json = serde_json::to_string(&res).unwrap();
                                                    println!("{}", json);
                                                    return Ok(json);
                                                } else if output == "id" {
                                                    println!("{}", res["uuid"].to_string().trim_matches('"'));
                                                }
                                                else {
                                                    println!("{}", message);
                                                }
                                            Ok(message)
                                        }
            Err(err) => Err(match err {
                err => err,
            }),
                                    }
                                            }
                                None => {
                                    let message = format!("❌ Environment {} does not exist", utils::RED.apply_to(environment.unwrap()));
                                    println!("{}", message);
                                    Err(PactBrokerError::NotFound(message))
                                }}
                            }
                            Err(err) => {
                                let message = format!("❌ Failed to record deployment: {}", err);
                                Err(PactBrokerError::ContentError(message))
                            }
                        }
                    }
            Err(err) => Err(match err {
                err => err,
            }),
            }})
}

#[cfg(test)]
mod record_deployment_tests {
    use super::record_deployment;
    use crate::cli::pact_broker::main::subcommands::add_record_deployment_subcommand;
    use pact_consumer::prelude::*;
    use pact_models::PactSpecification;
    use serde_json::json;

    #[test]
    fn records_deployment_successfully() {
        let config = MockServerConfig {
            pact_specification: PactSpecification::V2,
            ..MockServerConfig::default()
        };

        let pacticipant_name = "Foo";
        let version_number = "5556b8149bf8bac76bc30f50a8a2dd4c22c85f30";
        let environment_name = "test";
        let application_instance = "blue";
        let record_deployment_path = format!(
            "/pacticipants/{}/versions/{}/deployed-versions/environment/{}",
            pacticipant_name, version_number, "16926ef3-590f-4e3f-838e-719717aa88c9"
        );

        let pact_broker_service = PactBuilder::new("pact-broker-cli", "Pact Broker")
            // Pacticipant version with test environment available for deployment
            .interaction("a request for a pacticipant version", "", |mut i| {
                i.given("version 5556b8149bf8bac76bc30f50a8a2dd4c22c85f30 of pacticipant Foo exists with a test environment available for deployment");
                i.request
                    .path(format!("/pacticipants/{}/versions/{}", pacticipant_name, version_number))
                    .header("Accept", "application/hal+json")
                    .header("Accept", "application/json");
                i.response
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(json_pattern!({
                        "_links": {
                            "pb:record-deployment": [
                                {
                                    "name": environment_name,
                                    "href": term!("http:\\/\\/.*", format!("http://localhost{}", record_deployment_path))
                                }
                            ]
                        }
                    }));
                i
            })
            // POST to record deployment
            .interaction("a request to record a deployment", "", |mut i| {
                i.given("version 5556b8149bf8bac76bc30f50a8a2dd4c22c85f30 of pacticipant Foo exists with a test environment available for deployment");
                i.request
                    .method("POST")
                    .path(record_deployment_path.clone())
                    .header("Accept", "application/hal+json")
                    .header("Content-Type", "application/json")
                    .body(json!({
                        "applicationInstance": application_instance,
                        "target": environment_name
                    }).to_string());
                i.response
                    .status(201)
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(json_pattern!({
                        "target": application_instance
                    }));
                i
            })
            .start_mock_server(None, Some(config));

        let mock_server_url = pact_broker_service.url();

        // Arrange CLI args
        let matches = add_record_deployment_subcommand().get_matches_from(vec![
            "record-deployment",
            "-b",
            mock_server_url.as_str(),
            "--pacticipant",
            pacticipant_name,
            "--version",
            version_number,
            "--environment",
            environment_name,
            "--application-instance",
            application_instance,
        ]);

        // Act
        let result = record_deployment(&matches);

        // Assert
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("✅ Recorded deployment"));
    }

    #[test]
    fn records_deployment_successfully_json_output() {
        let config = MockServerConfig {
            pact_specification: PactSpecification::V2,
            ..MockServerConfig::default()
        };

        let pacticipant_name = "Foo";
        let version_number = "5556b8149bf8bac76bc30f50a8a2dd4c22c85f30";
        let environment_name = "test";
        let application_instance = "blue";
        let record_deployment_path = format!(
            "/pacticipants/{}/versions/{}/deployed-versions/environment/{}",
            pacticipant_name, version_number, "16926ef3-590f-4e3f-838e-719717aa88c9"
        );

        let pact_broker_service = PactBuilder::new("pact-broker-cli", "Pact Broker")
            .interaction("a request for a pacticipant version", "", |mut i| {
                i.given("version 5556b8149bf8bac76bc30f50a8a2dd4c22c85f30 of pacticipant Foo exists with a test environment available for deployment");
                i.request
                    .path(format!("/pacticipants/{}/versions/{}", pacticipant_name, version_number))
                    .header("Accept", "application/hal+json")
                    .header("Accept", "application/json");
                i.response
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(json_pattern!({
                        "_links": {
                            "pb:record-deployment": [
                                {
                                    "name": environment_name,
                                    "href": term!("http:\\/\\/.*", format!("http://localhost{}", record_deployment_path))
                                }
                            ]
                        }
                    }));
                i
            })
            .interaction("a request to record a deployment", "", |mut i| {
                i.given("version 5556b8149bf8bac76bc30f50a8a2dd4c22c85f30 of pacticipant Foo exists with a test environment available for deployment");
                i.request
                    .method("POST")
                    .path(record_deployment_path.clone())
                    .header("Accept", "application/hal+json")
                    .header("Content-Type", "application/json")
                    .body(json!({
                        "applicationInstance": application_instance,
                        "target": environment_name
                    }).to_string());
                i.response
                    .status(201)
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(json_pattern!({
                        "target": application_instance
                    }));
                i
            })
            .start_mock_server(None, Some(config));

        let mock_server_url = pact_broker_service.url();

        let matches = add_record_deployment_subcommand().get_matches_from(vec![
            "record-deployment",
            "-b",
            mock_server_url.as_str(),
            "--pacticipant",
            pacticipant_name,
            "--version",
            version_number,
            "--environment",
            environment_name,
            "--application-instance",
            application_instance,
            "--output",
            "json",
        ]);

        let result = record_deployment(&matches);

        assert!(result.is_ok());
        let output = result.unwrap();
        let json: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(json["target"], application_instance);
    }

    #[test]
    fn returns_error_when_environment_not_available() {
        let config = MockServerConfig {
            pact_specification: PactSpecification::V2,
            ..MockServerConfig::default()
        };

        let pacticipant_name = "Foo";
        let version_number = "5556b8149bf8bac76bc30f50a8a2dd4c22c85f30";
        let environment_name = "foo"; // not available
        let pact_broker_service = PactBuilder::new("pact-broker-cli", "Pact Broker")
            .interaction("a request for a pacticipant version", "", |mut i| {
                i.given("version 5556b8149bf8bac76bc30f50a8a2dd4c22c85f30 of pacticipant Foo exists with 2 environments that aren't test available for deployment");
                i.request
                    .path(format!("/pacticipants/{}/versions/{}", pacticipant_name, version_number))
                    .header("Accept", "application/hal+json")
                    .header("Accept", "application/json");
                i.response
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(json_pattern!({
                        "_links": {
                            "pb:record-deployment": [
                                like!({
                                    "name": "prod",
                                    "href": "href"
                                }),
                                like!({
                                    "name": "dev",
                                    "href": "href"
                                })
                            ]
                        }
                    }));
                i
            })
            .start_mock_server(None, Some(config));

        let mock_server_url = pact_broker_service.url();

        let matches = add_record_deployment_subcommand().get_matches_from(vec![
            "record-deployment",
            "-b",
            mock_server_url.as_str(),
            "--pacticipant",
            pacticipant_name,
            "--version",
            version_number,
            "--environment",
            environment_name,
        ]);

        let result = record_deployment(&matches);

        assert!(result.is_err());
        let err = result.err().unwrap().to_string();
        assert!(err.contains("Environment"));
        assert!(err.contains("foo"));
        assert!(err.contains("does not exist"));
    }
}
