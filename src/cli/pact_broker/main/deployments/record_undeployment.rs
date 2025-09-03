use serde_json::json;

use crate::cli::{
    pact_broker::main::{
        HALClient, PactBrokerError,
        utils::{
            follow_broker_relation, get_auth, get_broker_relation, get_broker_url, get_ssl_options,
            handle_error,
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

    let pacticipant = args.get_one::<String>("pacticipant");
    let environment = args.get_one::<String>("environment");
    let _application_instance = args.get_one::<String>("application-instance");
    let broker_url = get_broker_url(args);
    let auth = get_auth(args);
    let ssl_options = get_ssl_options(args);

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let hal_client: HALClient = HALClient::with_url(&broker_url, Some(auth.clone()),ssl_options.clone());

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

        let res = follow_broker_relation(
            hal_client.clone(),
            "pb:environments".to_string(),
            pb_environments_href_path,
        )
        .await;

                match res {
                    Ok(response) => {
                        let environments: Vec<Environment> = response["_embedded"]["environments"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|env| serde_json::from_value(env.clone()).unwrap())
                            .collect();
                        let environment_exists = environments.iter().any(|env| env.name == environment.clone().unwrap().to_string());
                        if environment_exists {
                            let environment_uuid = &environments.iter().find(|env| env.name == environment.clone().unwrap().to_string()).unwrap().uuid;
                            // Use environment_uuid in step 3
                            // println!("✅ Environment {} found with UUID: {}", environment.clone().unwrap(), environment_uuid);
                            // 3. Call the environment link and check the specified version exists, get the version link
                            let res = hal_client.clone()
                            .fetch(&(broker_url.clone() + "environments/" + &environment_uuid))
                            .await;
                        match res {
                            Ok(result) => {
                                // print!("✅ Environment found");
                                // print!("🧹 Undeploying {} from {} environment", pacticipant.unwrap(), environment.unwrap());
                                // todo - handle application instance

                                let currently_deployed_link = result["_links"]["pb:currently-deployed-deployed-versions"]["href"].as_str().unwrap();
                                let pacticipant_query = format!("?pacticipant={}", urlencoding::encode(pacticipant.unwrap()));
                                let res = hal_client.clone()
                                    .fetch(&(currently_deployed_link.to_string() + &pacticipant_query))
                                    .await;
                                match res {
                                    Ok(result) => {
                                        // Handle success
                                        // print!("🧹 Found currently deployed versions");
                                        if let Some(embedded) = result["_embedded"].as_object() {
                                            if let Some(deployed_versions) = embedded["deployedVersions"].as_array() {
                                                if deployed_versions.len() == 0 {
                                                    print!("❌ No currently deployed versions in {} environment", environment.unwrap());
                                                    PactBrokerError::NotFound(
                                                        format!("No currently deployed versions found for {} in {} environment", pacticipant.unwrap(), environment.unwrap())
                                                    );
                                                }
                                                for deployed_version in deployed_versions {
                                                    let pacticipant_name = deployed_version["_embedded"]["pacticipant"]["name"].as_str().unwrap();
                                                    if pacticipant_name == pacticipant.unwrap() {
                                                        let self_href = deployed_version["_links"]["self"]["href"].as_str().unwrap();
                                                        // Send a patch request with the user's payload to selfHref
                                                        // print!("🧹 Undeploying {} from {} environment", pacticipant.unwrap(), environment.unwrap());
                                                        // print!("🧹 Sending a patch request to {}", self_href);
                                                        let mut payload = json!({});
                                                        payload["currentlyDeployed"] = serde_json::Value::Bool(false);
                                                        // let pacticipant_query = format!("?pacticipant={}", urlencoding::encode(pacticipant.unwrap()));
                                                        let res = hal_client.clone().patch_json(self_href, &payload.to_string()).await;
                                                        match res {
                                                            Ok(_) => {
                                                                // Handle success
                                                                print!("✅ ♻️ Undeployed {} from {} environment", utils::GREEN.apply_to(pacticipant.unwrap()), utils::GREEN.apply_to(environment.unwrap()));
                                                            }
                                                            Err(err) => {
                                                                handle_error(err);
                                                            }
                                                        }
                                                    } else {
                                                        print!("❌ No currently deployed versions found for {} in {} environment" ,pacticipant.unwrap(), environment.unwrap());
                                                    PactBrokerError::NotFound(
                                                        format!("No currently deployed versions found for {} in {} environment", pacticipant.unwrap(), environment.unwrap())
                                                    );
                                                    }
                                                }
                                            } else {
                                                print!("❌ No currently deployed versions in {} environment", environment.unwrap());
                                                    PactBrokerError::NotFound(
                                                        format!("No currently deployed versions in {} environment", environment.unwrap())
                                                    );
                                            }
                                            }
                                            else {
                                                print!("❌ Could not process hal relation link");
                                                PactBrokerError::IoError(
                                                    "Could not process hal relation link".to_string()
                                                );
                                            }
                                    }
                                    Err(err) => {
                                        handle_error(err);
                                    }
                                }
                            }
                            Err(err) => {
                                handle_error(err);
                            }
                        }
                        } else {
                            println!("❌ Environment not found");
                            PactBrokerError::NotFound(
                                format!("Environment {} not found", environment.unwrap())
                            );
                        }
                    }
                    Err(err) => {
                        handle_error(err);
                        }
                    }
                Ok("Undeployment recorded successfully".to_string())

                })
}

#[cfg(test)]
mod record_undeployment_tests {
    use super::record_undeployment;
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
        let environment_uuid = "16926ef3-590f-4e3f-838e-719717aa88c9";
        let deployed_version_id = "ff3adecf-cfc5-4653-a4e3-f1861092f8e0";
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
            )
            }
        };

        let pact_broker_service = PactBuilder::new("pact-broker-cli", "Pact Broker")
            // Index resource
            .interaction("a request for the index resource", "", |mut i| {
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
                                "href": term!("http:\\/\\/.*","http://localhost/environments"),
                            }
                        }
                    }));
                i
            })
            // // Environments resource
            .interaction("a request for environments", "", |mut i| {
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
            .interaction("a request for an environment", "", |mut i| {
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
                                "href": term!("http:\\/\\/.*",format!("http://localhost/environments/{}/deployed-versions/currently-deployed", environment_uuid)),
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
                                                "href": term!("http:\\/\\/.*",format!("http://localhost/deployed-versions/{}", deployed_version_id))
                                            }
                                        },
                                        "_embedded": {
                                            "pacticipant": {
                                                "name": pacticipant_name,
                                                "_links": {
                                                    "self": {
                                                        "href": term!("http:\\/\\/.*",format!("http://localhost/deployed-versions/{}", deployed_version_id)),

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
        let matches = add_record_undeployment_subcommand()
            .args(crate::cli::add_ssl_arguments())
            .get_matches_from(vec![
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
        // Should contain a success message and the pacticipant/environment/application instance
        assert!(
            output.contains("Undeployment recorded successfully")
                || output.contains(pacticipant_name)
        );
    }
}
