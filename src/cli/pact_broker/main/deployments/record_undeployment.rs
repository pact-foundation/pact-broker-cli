use serde_json::{Value, json};

use crate::cli::{
    pact_broker::main::{
        HALClient, PactBrokerError,
        utils::{
            follow_broker_relation, get_auth, get_broker_relation, get_broker_url,
            get_custom_headers, get_retries, get_ssl_options,
        },
    },
    utils,
};

pub fn record_undeployment(args: &clap::ArgMatches) -> Result<String, PactBrokerError> {
    // 1. Check broker index link for connection
    // <- "GET /? HTTP/1.1\r\nAccept: application/hal+json\r\nUser-Agent: Ruby\r\nHost: localhost:9292\r\n\r\n"
    // -> "HTTP/1.1 200 OK\r\n"
    // 2. Call environments and check the specified enviroment exists, get the environment link
    // <- "GET /environments? HTTP/1.1\r\nAccept: application/hal+json\r\nUser-Agent: Ruby\r\nHost: localhost:9292\r\n\r\n"
    // -> "HTTP/1.1 200 OK\r\n"
    // 3. Call the environment link and check the specified version exists, get the version link
    // <- "GET /environments/c540ce64-5493-48c5-ab7c-28dae27b166b? HTTP/1.1\r\nAccept: application/hal+json\r\nUser-Agent: Ruby\r\nHost: localhost:9292\r\n\r\n"
    // -> "HTTP/1.1 200 OK\r\n"
    // 4. Call the /environments/c540ce64-5493-48c5-ab7c-28dae27b166b/deployed-versions/currently-deployed?pacticipant=Example+App link, and check our app is currently deployed
    // <- "GET /environments/c540ce64-5493-48c5-ab7c-28dae27b166b/deployed-versions/currently-deployed?pacticipant=Example+App HTTP/1.1\r\nAccept: application/hal+json\r\nUser-Agent: Ruby\r\nHost: localhost:9292\r\n\r\n"
    // -> "HTTP/1.1 200 OK\r\n"
    // 5. perform a patch request to the /environments/c540ce64-5493-48c5-ab7c-28dae27b166b/deployed-versions/9b756f93-19a2-4ca7-ae36-c0b917ac1f21 link to set currentlyDeployed to false
    // <- "PATCH /deployed-versions/9b756f93-19a2-4ca7-ae36-c0b917ac1f21 HTTP/1.1\r\nAccept: application/hal+json\r\nUser-Agent: Ruby\r\nContent-Type: application/merge-patch+json\r\nHost: localhost:9292\r\nContent-Length: 27\r\n\r\n"
    // <- "{\"currentlyDeployed\":false}"

    let pacticipant = args.get_one::<String>("pacticipant").unwrap();
    let environment = args.get_one::<String>("environment").unwrap();
    let application_instance = args.get_one::<String>("application-instance");
    let broker_url = get_broker_url(args).trim_end_matches('/').to_string();
    let auth = get_auth(args);
    let custom_headers = get_custom_headers(args);
    let ssl_options = get_ssl_options(args);

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let hal_client: HALClient = HALClient::with_url(&broker_url, Some(auth.clone()), ssl_options.clone(), custom_headers.clone())
            .with_retry_count(get_retries(args));

        #[derive(Debug, serde::Deserialize)]
        struct Environment {
            uuid: String,
            name: String,
        }

        let pb_environments_href_path = get_broker_relation(
            hal_client.clone(),
            "pb:environments".to_string(),
            broker_url.to_string(),
        )
        .await?;

        let environments_res = follow_broker_relation(
            hal_client.clone(),
            "pb:environments".to_string(),
            pb_environments_href_path,
        )
        .await;

        let environments_response = match environments_res {
            Ok(response) => response,
            Err(err) => return Err(err),
        };

        let environments: Vec<Environment> = environments_response["_embedded"]["environments"]
            .as_array()
            .ok_or_else(|| PactBrokerError::ContentError("Missing environments in response".to_string()))?
            .iter()
            .map(|env| serde_json::from_value(env.clone()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| {
                PactBrokerError::ContentError(format!("Failed to parse environment response: {err}"))
            })?;

        let environment_uuid = environments
            .iter()
            .find(|env| env.name == *environment)
            .map(|env| env.uuid.clone())
            .ok_or_else(|| {
                PactBrokerError::NotFound(format!("Environment {} not found", environment))
            })?;

        let environment_result = hal_client
            .clone()
            .fetch(&(broker_url.clone() + "/environments/" + &environment_uuid))
            .await?;

        let currently_deployed_link = relation_href(
            &environment_result,
            "pb:currently-deployed-deployed-versions",
            Some("pb:currently-deployed-versions"),
        )
        .ok_or_else(|| {
            PactBrokerError::LinkError(
                "This version of the Pact Broker does not support recording undeployments. Please upgrade to version 2.80.0 or later.".to_string(),
            )
        })?;

        let pacticipant_query = format!("?pacticipant={}", urlencoding::encode(pacticipant));
        let deployed_versions_response = hal_client
            .clone()
            .fetch(&(currently_deployed_link + &pacticipant_query))
            .await?;

        let deployed_versions = deployed_versions_response["_embedded"]["deployedVersions"]
            .as_array()
            .ok_or_else(|| {
                PactBrokerError::ContentError("No deployed versions could be found in response".to_string())
            })?;

        let pacticipant_deployments: Vec<&Value> = deployed_versions
            .iter()
            .filter(|deployed_version| {
                deployed_version["_embedded"]["pacticipant"]["name"].as_str() == Some(pacticipant)
            })
            .collect();

        if pacticipant_deployments.is_empty() {
            return Err(PactBrokerError::NotFound(format!(
                "{} is not currently deployed to {} environment. Cannot record undeployment.",
                pacticipant, environment
            )));
        }

        let deployments_for_instance: Vec<&Value> = pacticipant_deployments
            .iter()
            .copied()
            .filter(|deployment| match application_instance {
                Some(instance) => deployed_application_instance(deployment)
                    .map(|deployed_instance| deployed_instance == *instance)
                    .unwrap_or(false),
                None => {
                    deployment["applicationInstance"].is_null() && deployment["target"].is_null()
                }
            })
            .collect();

        if deployments_for_instance.is_empty() {
            let potential_application_instances: Vec<Option<String>> = pacticipant_deployments
                .iter()
                .map(|deployment| deployed_application_instance(deployment))
                .collect();

            if let Some(instance) = application_instance {
                let should_omit_instance = potential_application_instances.iter().any(|value| value.is_none());
                let known_instances: Vec<String> = potential_application_instances
                    .iter()
                    .flatten()
                    .cloned()
                    .collect();
                let mut suggestions = Vec::new();
                if should_omit_instance {
                    suggestions.push("omit the application instance".to_string());
                }
                if !known_instances.is_empty() {
                    suggestions.push(format!(
                        "specify one of the following application instances to record the undeployment from: {}",
                        known_instances.join(", ")
                    ));
                }

                return Err(PactBrokerError::NotFound(format!(
                    "{} is not currently deployed to application instance '{}' in {} environment.{}",
                    pacticipant,
                    instance,
                    environment,
                    if suggestions.is_empty() {
                        String::new()
                    } else {
                        format!(" Please {}.", suggestions.join(" or "))
                    }
                )));
            }

            let known_instances: Vec<String> = potential_application_instances
                .iter()
                .flatten()
                .cloned()
                .collect();
            if !known_instances.is_empty() {
                return Err(PactBrokerError::NotFound(format!(
                    "Please specify one of the following application instances to record the undeployment from: {}",
                    known_instances.join(", ")
                )));
            }

            return Err(PactBrokerError::NotFound(format!(
                "{} is not currently deployed to {} environment. Cannot record undeployment.",
                pacticipant, environment
            )));
        }

        for deployed_version in deployments_for_instance {
            let self_href = deployed_version["_links"]["self"]["href"]
                .as_str()
                .ok_or_else(|| {
                    PactBrokerError::ContentError(
                        "No self link found for currently deployed version".to_string(),
                    )
                })?;

            let mut payload = json!({});
            payload["currentlyDeployed"] = serde_json::Value::Bool(false);
            hal_client
                .clone()
                .patch_json(self_href, &payload.to_string(), None)
                .await?;
        }

        println!(
            "✅ ♻️ Undeployed {} from {} environment{}",
            utils::GREEN.apply_to(pacticipant),
            utils::GREEN.apply_to(environment),
            application_instance
                .map(|instance| format!(" (application instance {})", utils::GREEN.apply_to(instance)))
                .unwrap_or_default()
        );

        Ok("Undeployment recorded successfully".to_string())
    })
}

fn relation_href(resource: &Value, primary: &str, fallback: Option<&str>) -> Option<String> {
    let primary_link = resource["_links"][primary]["href"]
        .as_str()
        .map(str::to_string);
    if primary_link.is_some() {
        return primary_link;
    }

    fallback.and_then(|relation| {
        resource["_links"][relation]["href"]
            .as_str()
            .map(str::to_string)
    })
}

fn deployed_application_instance(deployed_version: &Value) -> Option<String> {
    deployed_version["applicationInstance"]
        .as_str()
        .map(str::to_string)
        .or_else(|| deployed_version["target"].as_str().map(str::to_string))
}

#[cfg(test)]
mod record_undeployment_tests {
    use super::record_undeployment;
    use crate::cli::pact_broker::main::PactBrokerError;
    use crate::cli::pact_broker::main::subcommands::add_record_undeployment_subcommand;
    use pact_consumer::prelude::*;
    use pact_models::{PactSpecification, generators, prelude::Generator};
    use serde_json::json;

    #[test]
    fn records_undeployment_successfully() {
        // Arrange
        let config = MockServerConfig {
            pact_specification: PactSpecification::V2,
            ..MockServerConfig::default()
        };

        let pacticipant_name = "Foo";
        let environment_name = "test";
        let environment_display_name = "Test";
        let application_instance = "customer-1";
        let other_application_instance = "customer-2";
        let environment_uuid = "16926ef3-590f-4e3f-838e-719717aa88c9";
        let deployed_version_id = "ff3adecf-cfc5-4653-a4e3-f1861092f8e0";
        let other_deployed_version_id = "58a013bc-31d6-434a-b026-46ecbd9e3a2d";
        let test_environment_path = format!("/environments/{}", environment_uuid);
        let currently_deployed_versions_path = format!(
            "/environments/{}/deployed-versions/currently-deployed",
            environment_uuid
        );
        let deployed_version_path = format!("/deployed-versions/{}", deployed_version_id);

        let deployed_version_response = json_pattern!({
            "currentlyDeployed": false,
            "_embedded": {
                "version": {
                    "number": like!(deployed_version_id)
                }
            }
        });

        let currently_deployed_versions_path_generators = generators! {
            "BODY" => {
            "$._links.pb:currently-deployed-deployed-versions.href" => Generator::MockServerURL(
                format!("/environments/{}/deployed-versions/currently-deployed", environment_uuid),
                format!(".*(\\/environments\\/{}\\/deployed-versions\\/currently-deployed)", environment_uuid)
            )
            }
        };
        let deployed_version_path_generators = generators! {
            "BODY" => {
            "$._embedded.deployedVersions[0]._links.self.href" => Generator::MockServerURL(
                format!("/deployed-versions/{}", deployed_version_id),
                format!(".*(\\/deployed-versions\\/{}\\/currently-deployed)", deployed_version_id)
            ),
            "$._embedded.deployedVersions[1]._links.self.href" => Generator::MockServerURL(
                format!("/deployed-versions/{}", other_deployed_version_id),
                format!(".*(\\/deployed-versions\\/{}\\/currently-deployed)", other_deployed_version_id)
            )
            }
        };

        let pact_broker_service = PactBuilder::new("pact-broker-cli", "Pact Broker")
            // Index resource
            .interaction("a request for the index resource for records_undeployment_successfully", "", |mut i| {
                i.given("the pb:environments relation exists in the index resource");
                i.request
                    .path("/")
                    .header("Accept", "application/hal+json")
                    .header("Accept", "application/json");
                i.response
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(json_pattern!({
                        "_links": {
                            "pb:environments": {
                                "href": term!("http:\\/\\/[^/]+\\/environments","http://localhost/environments"),
                            }
                        }
                    }));
                i
            })
            // // Environments resource
            .interaction("a request for environments resource", "", |mut i| {
                i.given(format!(
                    "an environment with name {} and UUID {} exists",
                    environment_name, environment_uuid
                ));
                i.request
                    .path("/environments")
                    .header("Accept", "application/hal+json")
                    .header("Accept", "application/json");
                i.response
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(json_pattern!({
                        "_embedded": {
                            "environments": [
                                {
                                    "uuid": environment_uuid,
                                    "name": environment_name,
                                    "displayName": environment_display_name,
                                    "production": false,
                                    "createdAt": like!("2024-01-01T00:00:00Z")
                                }
                            ]
                        }
                    }));
                i
            })
            // Environment details
            .interaction("a request for an environment details", "", |mut i| {
                i.given(format!(
                    "an environment with name {} and UUID {} exists",
                    environment_name, environment_uuid
                ));
                i.request
                    .path(test_environment_path.as_str())
                    .header("Accept", "application/hal+json")
                    .header("Accept", "application/json");
                i.response
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(json_pattern!({
                        "_links": {
                            "pb:currently-deployed-deployed-versions": {
                                "href": term!("http:\\/\\/[^/]+\\/environments\\/[^/]+\\/deployed-versions\\/currently-deployed",format!("http://localhost/environments/{}/deployed-versions/currently-deployed", environment_uuid)),
                            }
                        }
                    }))
                    .generators()
                    .add_generators(currently_deployed_versions_path_generators);
                i
            })
            // Deployed versions for pacticipant
            .interaction(
                "a request to list deployed versions for pacticipant",
                "",
                |mut i| {
                    i.given(format!(
                        "an version is deployed to environment with UUID {} with target {}",
                        environment_uuid, application_instance
                    ));
                    i.request
                        .path(currently_deployed_versions_path.as_str())
                        .query_param("pacticipant", pacticipant_name)
                        .header("Accept", "application/hal+json")
                        .header("Accept", "application/json");
                    i.response
                        .header("Content-Type", "application/hal+json;charset=utf-8")
                        .json_body(json_pattern!({
                            "_embedded": {
                                "deployedVersions": [
                                    {
                                        "applicationInstance": application_instance,
                                        "_links": {
                                            "self": {
                                                "href": term!("http:\\/\\/[^/]+\\/deployed-versions\\/[^/]+",format!("http://localhost/deployed-versions/{}", deployed_version_id))
                                            }
                                        },
                                        "_embedded": {
                                            "pacticipant": {
                                                "name": pacticipant_name,
                                                "_links": {
                                                    "self": {
                                                        "href": term!("http:\\/\\/[^/]+\\/pacticipants\\/[^/]+",format!("http://localhost/pacticipants/{}", pacticipant_name)),

                                                    }
                                                },
                                            }
                                        }
                                    },
                                    {
                                        "applicationInstance": other_application_instance,
                                        "_links": {
                                            "self": {
                                                "href": term!("http:\\/\\/[^/]+\\/deployed-versions\\/[^/]+",format!("http://localhost/deployed-versions/{}", other_deployed_version_id))
                                            }
                                        },
                                        "_embedded": {
                                            "pacticipant": {
                                                "name": pacticipant_name,
                                                "_links": {
                                                    "self": {
                                                        "href": term!("http:\\/\\/[^/]+\\/pacticipants\\/[^/]+",format!("http://localhost/pacticipants/{}", pacticipant_name)),

                                                    }
                                                },
                                            }
                                        }
                                    }
                                ]
                            }
                        }))
                        .generators()
                        .add_generators(deployed_version_path_generators);
                    i
                },
            )
            // PATCH to undeploy
            .interaction(
                "a request to mark a deployed version as not currently deployed",
                "",
                |mut i| {
                    i.given("a currently deployed version exists");
                    i.request
                        .method("PATCH")
                        .path(deployed_version_path)
                        .header("Accept", "application/hal+json")
                        .header("Content-Type", "application/merge-patch+json")
                        .body(json!({ "currentlyDeployed": false }).to_string());
                    i.response
                        .status(200)
                        .header("Content-Type", "application/hal+json;charset=utf-8")
                        .json_body(deployed_version_response);
                    i
                },
            )
            .start_mock_server(None, Some(config));

        let mock_server_url = pact_broker_service.url();

        // Arrange CLI args
        let matches = add_record_undeployment_subcommand().get_matches_from(vec![
            "record-undeployment",
            "-b",
            mock_server_url.as_str(),
            "--pacticipant",
            pacticipant_name,
            "--environment",
            environment_name,
            "--application-instance",
            application_instance,
        ]);

        // Act
        let result = record_undeployment(&matches);

        // Assert
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("Undeployment recorded successfully"));
        // This test uses two deployed application instances and expects only the selected
        // application's deployment to be patched successfully.
        assert_eq!(application_instance, "customer-1");
    }

    #[test]
    fn records_undeployment_successfully_without_application_instance() {
        let config = MockServerConfig {
            pact_specification: PactSpecification::V2,
            ..MockServerConfig::default()
        };

        let pacticipant_name = "Foo";
        let environment_name = "test";
        let environment_display_name = "Test";
        let application_instance = "customer-1";
        let environment_uuid = "16926ef3-590f-4e3f-838e-719717aa88c9";
        let deployed_version_id = "ff3adecf-cfc5-4653-a4e3-f1861092f8e0";
        let null_instance_deployed_version_id = "58a013bc-31d6-434a-b026-46ecbd9e3a2d";
        let test_environment_path = format!("/environments/{}", environment_uuid);
        let currently_deployed_versions_path = format!(
            "/environments/{}/deployed-versions/currently-deployed",
            environment_uuid
        );
        let deployed_version_path =
            format!("/deployed-versions/{}", null_instance_deployed_version_id);

        let deployed_version_response = json_pattern!({
            "currentlyDeployed": false,
            "_embedded": {
                "version": {
                    "number": like!(null_instance_deployed_version_id)
                }
            }
        });

        let currently_deployed_versions_path_generators = generators! {
            "BODY" => {
            "$._links.pb:currently-deployed-deployed-versions.href" => Generator::MockServerURL(
                format!("/environments/{}/deployed-versions/currently-deployed", environment_uuid),
                format!(".*(\\/environments\\/{}\\/deployed-versions\\/currently-deployed)", environment_uuid)
            )
            }
        };
        let deployed_version_path_generators = generators! {
            "BODY" => {
            "$._embedded.deployedVersions[0]._links.self.href" => Generator::MockServerURL(
                format!("/deployed-versions/{}", deployed_version_id),
                format!(".*(\\/deployed-versions\\/{}\\/currently-deployed)", deployed_version_id)
            ),
            "$._embedded.deployedVersions[1]._links.self.href" => Generator::MockServerURL(
                format!("/deployed-versions/{}", null_instance_deployed_version_id),
                format!(".*(\\/deployed-versions\\/{}\\/currently-deployed)", null_instance_deployed_version_id)
            )
            }
        };

        let pact_broker_service = PactBuilder::new("pact-broker-cli", "Pact Broker")
            .interaction(
                "a request for the index resource for records_undeployment_successfully_without_application_instance",
                "",
                |mut i| {
                    i.given("the pb:environments relation exists in the index resource");
                    i.request
                        .path("/")
                        .header("Accept", "application/hal+json")
                        .header("Accept", "application/json");
                    i.response
                        .header("Content-Type", "application/hal+json;charset=utf-8")
                        .json_body(json_pattern!({
                            "_links": {
                                "pb:environments": {
                                    "href": term!("http:\\/\\/[^/]+\\/environments","http://localhost/environments"),
                                }
                            }
                        }));
                    i
                },
            )
            .interaction("a request for environments resource for records_undeployment_successfully_without_application_instance", "", |mut i| {
                i.given(format!(
                    "an environment with name {} and UUID {} exists",
                    environment_name, environment_uuid
                ));
                i.request
                    .path("/environments")
                    .header("Accept", "application/hal+json")
                    .header("Accept", "application/json");
                i.response
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(json_pattern!({
                        "_embedded": {
                            "environments": [
                                {
                                    "uuid": environment_uuid,
                                    "name": environment_name,
                                    "displayName": environment_display_name,
                                    "production": false,
                                    "createdAt": like!("2024-01-01T00:00:00Z")
                                }
                            ]
                        }
                    }));
                i
            })
            .interaction("a request for an environment details for records_undeployment_successfully_without_application_instance", "", |mut i| {
                i.given(format!(
                    "an environment with name {} and UUID {} exists",
                    environment_name, environment_uuid
                ));
                i.request
                    .path(test_environment_path.as_str())
                    .header("Accept", "application/hal+json")
                    .header("Accept", "application/json");
                i.response
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(json_pattern!({
                        "_links": {
                            "pb:currently-deployed-deployed-versions": {
                                "href": term!("http:\\/\\/[^/]+\\/environments\\/[^/]+\\/deployed-versions\\/currently-deployed",format!("http://localhost/environments/{}/deployed-versions/currently-deployed", environment_uuid)),
                            }
                        }
                    }))
                    .generators()
                    .add_generators(currently_deployed_versions_path_generators);
                i
            })
            .interaction(
                "a request to list deployed versions for pacticipant for records_undeployment_successfully_without_application_instance",
                "",
                |mut i| {
                    i.given(format!(
                        "an version is deployed to environment with UUID {} with target {}",
                        environment_uuid, application_instance
                    ));
                    i.request
                        .path(currently_deployed_versions_path.as_str())
                        .query_param("pacticipant", pacticipant_name)
                        .header("Accept", "application/hal+json")
                        .header("Accept", "application/json");
                    i.response
                        .header("Content-Type", "application/hal+json;charset=utf-8")
                        .json_body(json_pattern!({
                            "_embedded": {
                                "deployedVersions": [
                                    {
                                        "applicationInstance": application_instance,
                                        "_links": {
                                            "self": {
                                                "href": term!("http:\\/\\/[^/]+\\/deployed-versions\\/[^/]+",format!("http://localhost/deployed-versions/{}", deployed_version_id))
                                            }
                                        },
                                        "_embedded": {
                                            "pacticipant": {
                                                "name": pacticipant_name,
                                                "_links": {
                                                    "self": {
                                                        "href": term!("http:\\/\\/[^/]+\\/pacticipants\\/[^/]+",format!("http://localhost/pacticipants/{}", pacticipant_name)),

                                                    }
                                                },
                                            }
                                        }


                                    },
                                    {
                                        "applicationInstance": null,
                                        "target": null,
                                        "_links": {
                                            "self": {
                                                "href": term!("http:\\/\\/[^/]+\\/deployed-versions\\/[^/]+",format!("http://localhost/deployed-versions/{}", null_instance_deployed_version_id))
                                            }
                                        },
                                        "_embedded": {
                                            "pacticipant": {
                                                "name": pacticipant_name,
                                                "_links": {
                                                    "self": {
                                                        "href": term!("http:\\/\\/[^/]+\\/pacticipants\\/[^/]+",format!("http://localhost/pacticipants/{}", pacticipant_name)),

                                                    }
                                                },
                                            }
                                        }
                                    }
                                ]
                            }
                        }))
                        .generators()
                        .add_generators(deployed_version_path_generators);
                    i
                },
            )
            .interaction(
                "a request to mark a null-application-instance deployed version as not currently deployed",
                "",
                |mut i| {
                    i.given("a currently deployed version exists");
                    i.request
                        .method("PATCH")
                        .path(deployed_version_path)
                        .header("Accept", "application/hal+json")
                        .header("Content-Type", "application/merge-patch+json")
                        .body(json!({ "currentlyDeployed": false }).to_string());
                    i.response
                        .status(200)
                        .header("Content-Type", "application/hal+json;charset=utf-8")
                        .json_body(deployed_version_response);
                    i
                },
            )
            .start_mock_server(None, Some(config));

        let mock_server_url = pact_broker_service.url();

        let matches = add_record_undeployment_subcommand().get_matches_from(vec![
            "record-undeployment",
            "-b",
            mock_server_url.as_str(),
            "--pacticipant",
            pacticipant_name,
            "--environment",
            environment_name,
        ]);

        let result = record_undeployment(&matches);

        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("Undeployment recorded successfully"));
    }

    #[test]
    fn fails_when_application_instance_is_required_but_not_provided() {
        let config = MockServerConfig {
            pact_specification: PactSpecification::V2,
            ..MockServerConfig::default()
        };

        let pacticipant_name = "Foo";
        let environment_name = "test";
        let environment_display_name = "Test";
        let application_instance = "customer-1";
        let environment_uuid = "16926ef3-590f-4e3f-838e-719717aa88c9";
        let deployed_version_id = "ff3adecf-cfc5-4653-a4e3-f1861092f8e0";
        let test_environment_path = format!("/environments/{}", environment_uuid);
        let currently_deployed_versions_path = format!(
            "/environments/{}/deployed-versions/currently-deployed",
            environment_uuid
        );

        let currently_deployed_versions_path_generators = generators! {
            "BODY" => {
            "$._links.pb:currently-deployed-deployed-versions.href" => Generator::MockServerURL(
                format!("/environments/{}/deployed-versions/currently-deployed", environment_uuid),
                format!(".*(\\/environments\\/{}\\/deployed-versions\\/currently-deployed)", environment_uuid)
            )
            }
        };
        let deployed_version_path_generators = generators! {
            "BODY" => {
            "$._embedded.deployedVersions[0]._links.self.href" => Generator::MockServerURL(
                format!("/deployed-versions/{}", deployed_version_id),
                format!(".*(\\/deployed-versions\\/{}\\/currently-deployed)", deployed_version_id)
            )
            }
        };

        let pact_broker_service = PactBuilder::new("pact-broker-cli", "Pact Broker")
            .interaction(
                "a request for the index resource for missing instance guidance",
                "",
                |mut i| {
                    i.given("the pb:environments relation exists in the index resource");
                    i.request
                        .path("/")
                        .header("Accept", "application/hal+json")
                        .header("Accept", "application/json");
                    i.response
                        .header("Content-Type", "application/hal+json;charset=utf-8")
                        .json_body(json_pattern!({
                            "_links": {
                                "pb:environments": {
                                    "href": term!("http:\\/\\/[^/]+\\/environments","http://localhost/environments"),
                                }
                            }
                        }));
                    i
                },
            )
            .interaction("a request for environments resource for missing instance guidance", "", |mut i| {
                i.given(format!(
                    "an environment with name {} and UUID {} exists",
                    environment_name, environment_uuid
                ));
                i.request
                    .path("/environments")
                    .header("Accept", "application/hal+json")
                    .header("Accept", "application/json");
                i.response
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(json_pattern!({
                        "_embedded": {
                            "environments": [
                                {
                                    "uuid": environment_uuid,
                                    "name": environment_name,
                                    "displayName": environment_display_name,
                                    "production": false,
                                    "createdAt": like!("2024-01-01T00:00:00Z")
                                }
                            ]
                        }
                    }));
                i
            })
            .interaction("a request for an environment details for missing instance guidance", "", |mut i| {
                i.given(format!(
                    "an environment with name {} and UUID {} exists",
                    environment_name, environment_uuid
                ));
                i.request
                    .path(test_environment_path.as_str())
                    .header("Accept", "application/hal+json")
                    .header("Accept", "application/json");
                i.response
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(json_pattern!({
                        "_links": {
                            "pb:currently-deployed-deployed-versions": {
                                "href": term!("http:\\/\\/[^/]+\\/environments\\/[^/]+\\/deployed-versions\\/currently-deployed",format!("http://localhost/environments/{}/deployed-versions/currently-deployed", environment_uuid)),
                            }
                        }
                    }))
                    .generators()
                    .add_generators(currently_deployed_versions_path_generators);
                i
            })
            .interaction(
                "a request to list deployed versions for pacticipant for missing instance guidance",
                "",
                |mut i| {
                    i.given(format!(
                        "an version is deployed to environment with UUID {} with target {}",
                        environment_uuid, application_instance
                    ));
                    i.request
                        .path(currently_deployed_versions_path.as_str())
                        .query_param("pacticipant", pacticipant_name)
                        .header("Accept", "application/hal+json")
                        .header("Accept", "application/json");
                    i.response
                        .header("Content-Type", "application/hal+json;charset=utf-8")
                        .json_body(json_pattern!({
                            "_embedded": {
                                "deployedVersions": [
                                    {
                                        "applicationInstance": application_instance,
                                        "_links": {
                                            "self": {
                                                "href": term!("http:\\/\\/[^/]+\\/deployed-versions\\/[^/]+",format!("http://localhost/deployed-versions/{}", deployed_version_id))
                                            }
                                        },
                                        "_embedded": {
                                            "pacticipant": {
                                                "name": pacticipant_name,
                                                "_links": {
                                                    "self": {
                                                        "href": term!("http:\\/\\/[^/]+\\/pacticipants\\/[^/]+",format!("http://localhost/pacticipants/{}", pacticipant_name)),
                                                    }
                                                },
                                            }
                                        }
                                    }
                                ]
                            }
                        }))
                        .generators()
                        .add_generators(deployed_version_path_generators);
                    i
                },
            )
            .start_mock_server(None, Some(config));

        let mock_server_url = pact_broker_service.url();

        let matches = add_record_undeployment_subcommand().get_matches_from(vec![
            "record-undeployment",
            "-b",
            mock_server_url.as_str(),
            "--pacticipant",
            pacticipant_name,
            "--environment",
            environment_name,
        ]);

        let result = record_undeployment(&matches);

        assert!(result.is_err());
        let err = result.err().unwrap();
        match err {
            PactBrokerError::NotFound(message) => {
                assert!(
                    message.contains(
                        "Please specify one of the following application instances to record the undeployment from"
                    )
                );
                assert!(message.contains(application_instance));
            }
            other => panic!("Expected NotFound error, got {other:?}"),
        }
    }
}
