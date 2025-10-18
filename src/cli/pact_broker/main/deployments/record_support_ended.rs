use serde_json::json;

use crate::cli::{
    pact_broker::main::{
        HALClient, PactBrokerError,
        utils::{get_auth, get_broker_url, get_ssl_options},
    },
    utils,
};

pub fn record_support_ended(args: &clap::ArgMatches) -> Result<String, PactBrokerError> {
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
    let version = args.get_one::<String>("version");
    let broker_url = get_broker_url(args).trim_end_matches('/').to_string();
    let auth = get_auth(args);
    let ssl_options = get_ssl_options(args);

    tokio::runtime::Runtime::new().unwrap().block_on(async {
                let hal_client: HALClient = HALClient::with_url(&broker_url, Some(auth.clone()),ssl_options.clone());


                let res = hal_client.clone()
                    .fetch(&(broker_url.clone() + "/"))
                    .await;
                match res {
                    Ok(_) => {
                    }
                    Err(err) => {
                        return Err(err);
                    }
                }

                #[derive(Debug, serde::Deserialize)]
                struct Environment {
                    uuid: String,
                    name: String,
                    }

                let res = hal_client.clone()
                    .fetch(&(broker_url.clone() + "/environments?"))
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

                                // 3. Call the environment link and check the specified version exists, get the version link
                                let res = hal_client.clone()
                                .fetch(&(broker_url.clone() + "/environments/" + &environment_uuid + "?"))
                                .await;
                            match res {
                                Ok(result) => {
                                    // print!("✅ Environment found");
                                    // print!("🧹 Undeploying {} from {} environment", pacticipant.unwrap(), environment.unwrap());
                                    // print!("Result JSON: {:#?}", result);
                                    // todo - handle application instance

                                    let currently_supported_released_link = result["_links"]["pb:currently-supported-released-versions"]["href"].as_str().unwrap();
                                    let pacticipant_query = format!("?pacticipant={}", urlencoding::encode(pacticipant.unwrap()));

                                    let res = hal_client.clone()
                                        .fetch(&(currently_supported_released_link.to_owned() + &pacticipant_query))
                                        .await;
                                    match res {
                                        Ok(result) => {
                                            // Handle success
                                            // print!("🧹 Found currently deployed versions");
                                            // print!("Result JSON: {:#?}", result);
                                            if let Some(embedded) = result["_embedded"].as_object() {
                                                if let Some(released_versions) = embedded["releasedVersions"].as_array() {
                                                    if released_versions.len() == 0 {
                                                        let message = format!("❌ No currently released versions found for {} in {} environment", pacticipant.unwrap(), environment.unwrap());
                                                        println!("{}", message.clone());
                                                        return Err(PactBrokerError::NotFound(message));
                                                    }
                                                    for released_version in released_versions {
                                                        let pacticipant_name = released_version["_embedded"]["pacticipant"]["name"].as_str().unwrap();
                                                        if pacticipant_name == pacticipant.unwrap() && version.unwrap() == released_version["_embedded"]["version"]["number"].as_str().unwrap() {
                                                            let self_href = released_version["_links"]["self"]["href"].as_str().unwrap();
                                                            // Send a patch request with the user's payload to selfHref
                                                            // print!("🧹 Undeploying {} from {} environment", pacticipant.unwrap(), environment.unwrap());
                                                            // print!("🧹 Sending a patch request to {}", self_href);
                                                            let mut payload = json!({});
                                                            payload["currentlySupported"] = serde_json::Value::Bool(false);
                                                            // let pacticipant_query = format!("?pacticipant={}", urlencoding::encode(pacticipant.unwrap()));
                                                            let res = hal_client.clone().patch_json(self_href, &payload.to_string(),None).await;
                                                            match res {
                                                                Ok(_value) => {
                                                                    // Handle success
                                                                    let message = format!(
                                                                        "Recorded support ended for application {}, version {} from {} environment",
                                                                        utils::GREEN.apply_to(pacticipant.unwrap()),
                                                                        utils::GREEN.apply_to(version.unwrap()),
                                                                        utils::GREEN.apply_to(environment.unwrap())
                                                                    );
                                                                    println!("✅ ♻️ {}", message);
                                                                    return Ok(message);
                                                                }
                                                                Err(err) => return Err(err),
                                                            }
                                                        } else {
                                                            let message = format!("❌ No currently released versions found for {} in {} environment", pacticipant.unwrap(), environment.unwrap());
                                                            println!("{}", utils::RED.apply_to(message.clone()));
                                                            return Err(PactBrokerError::NotFound(message));
                                                        }
                                                    }
                                                    return Ok("".to_string());
                                                } else {
                                                    let message = format!("❌ No currently released versions found for {} in {} environment", pacticipant.unwrap(), environment.unwrap());  
                                                    println!("{}", utils::RED.apply_to(message.clone()));
                                                    return Err(PactBrokerError::NotFound(message));
                                                }
                                                }
                                            else {
                                                let message = format!("❌ Could not process hal relation link");
                                                println!("{}", utils::RED.apply_to(message.clone()));
                                                return Err(PactBrokerError::NotFound(message));
                                            }
                                        }
                                        Err(err) => return Err(err),
                                    }
                                }
                                Err(err) => return Err(err),
                            }
                            } else {
                                let message = format!("❌ Environment {} not found", environment.unwrap());
                                println!("{}", message.clone());
                                return Err(PactBrokerError::NotFound(message));
                            }
                        }
                            Err(err) => return Err(err),
                        }
            })
}

#[cfg(test)]
mod record_support_ended_tests {
    use super::record_support_ended;
    use crate::cli::{pact_broker::main::subcommands::add_record_support_ended_subcommand, utils};
    use pact_consumer::prelude::*;
    use pact_models::{PactSpecification, generators, prelude::Generator};
    use serde_json::json;

    #[test]
    fn records_support_ended_successfully() {
        // Arrange
        let config = MockServerConfig {
            pact_specification: PactSpecification::V2,
            ..MockServerConfig::default()
        };

        let pacticipant_name = "Foo";
        let environment_name = "test";
        let environment_display_name = "Test";
        let environment_uuid = "16926ef3-590f-4e3f-838e-719717aa88c9";
        let released_version_id = "ff3adecf-cfc5-4653-a4e3-f1861092f8e0";
        let application_version = "5556b8149bf8bac76bc30f50a8a2dd4c22c85f30";
        let test_environment_path = format!("/environments/{}", environment_uuid);
        let currently_supported_released_versions_path = format!(
            "/environments/{}/released-versions/currently-supported",
            environment_uuid
        );
        let released_version_path = format!("/released-versions/{}", released_version_id);

        let released_version_response = json_pattern!({
            "currentlySupported": false,
            "_embedded": {
                "version": {
                    "number": like!(released_version_id)
                }
            }
        });

        let currently_supported_released_versions_path_generators = generators! {
            "BODY" => {
                "$._links.pb:currently-supported-released-versions.href" => Generator::MockServerURL(
                    format!("/environments/{}/released-versions/currently-supported", environment_uuid),
                    format!(".*(\\/environments\\/{}\\/released-versions\\/currently-supported)", environment_uuid)
                )
            }
        };
        let released_version_path_generators = generators! {
            "BODY" => {
                "$._embedded.releasedVersions[0]._links.self.href" => Generator::MockServerURL(
                    format!("/released-versions/{}", released_version_id),
                    format!(".*(\\/released-versions\\/{}\\/currently-supported)", released_version_id)
                )
            }
        };

        let pact_broker_service = PactBuilder::new("pact-broker-cli", "Pact Broker")
            // Index resource
            .interaction("a request for the index resource for records_support_ended_successfully", "", |mut i| {
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
            // Environments resource
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
                            "pb:currently-supported-released-versions": {
                                "href": term!("http:\\/\\/.*",format!("http://localhost/environments/{}/released-versions/currently-supported", environment_uuid)),
                            }
                        }
                    }))
                    .generators()
                    .add_generators(currently_supported_released_versions_path_generators);
                i
            })
            // Released versions for pacticipant
            .interaction(
                "a request to list released versions for pacticipant",
                "",
                |mut i| {
                    i.given(format!("version {} of pacticipant {} exists with a {} environment is released with id {}", application_version, pacticipant_name, environment_name, released_version_id));
                    i.request
                        .path(currently_supported_released_versions_path.as_str())
                        .query_param("pacticipant", pacticipant_name)
                        .header("Accept", "application/hal+json")
                        .header("Accept", "application/json");
                    i.response
                        .header("Content-Type", "application/hal+json;charset=utf-8")
                        .json_body(json_pattern!({
                            "_embedded": {
                                "releasedVersions": [
                                    {
                                        "_links": {
                                            "self": {
                                                "href": term!("http:\\/\\/.*",format!("http://localhost/released-versions/{}", released_version_id))
                                            }
                                        },
                                        "_embedded": {
                                            "pacticipant": {
                                                "name": pacticipant_name,
                                                "_links": {
                                                    "self": {
                                                        "href": term!("http:\\/\\/.*",format!("http://localhost/released-versions/{}", released_version_id)),
                                                    }
                                                },
                                            },
                                            "version": {"number":application_version},
                                        }
                                    }
                                ]
                            }
                        }))
                        .generators()
                        .add_generators(released_version_path_generators);
                    i
                },
            )
            // PATCH to mark support ended
            .interaction(
                "a request to mark a released version as not currently supported",
                "",
                |mut i| {
                    i.given(format!("version {} of pacticipant {} exists with a {} environment is released with id {}", application_version, pacticipant_name, environment_name, released_version_id));
                    i.request
                        .method("PATCH")
                        .path(released_version_path)
                        .header("Accept", "application/hal+json")
                        .header("Content-Type", "application/merge-patch+json")
                        .body(json!({ "currentlySupported": false }).to_string());
                    i.response
                        .status(200)
                        .header("Content-Type", "application/hal+json;charset=utf-8")
                        .json_body(released_version_response);
                    i
                },
            )
            .start_mock_server(None, Some(config));

        let mock_server_url = pact_broker_service.url();

        // Arrange CLI args
        let matches = add_record_support_ended_subcommand().get_matches_from(vec![
            "record-support-ended",
            "-b",
            mock_server_url.as_str(),
            "--pacticipant",
            pacticipant_name,
            "--environment",
            environment_name,
            "--version",
            application_version,
        ]);

        // Act
        let result = record_support_ended(&matches);

        // Assert
        assert!(result.is_ok());
        let output = result.unwrap();
        println!("Output: {}", output);
        // Should contain a success message and the pacticipant/environment
        assert!(output.contains(&format!(
            "Recorded support ended for application {}, version {} from {} environment",
            utils::GREEN.apply_to("Foo").to_string(),
            utils::GREEN.apply_to(application_version).to_string(),
            utils::GREEN.apply_to("test").to_string()
        )));
    }
}
