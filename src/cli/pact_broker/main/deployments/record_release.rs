use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::cli::{
    pact_broker::main::{
        HALClient, PactBrokerError,
        utils::{get_auth, get_broker_url, get_ssl_options},
    },
    utils,
};

pub fn record_release(args: &clap::ArgMatches) -> Result<String, PactBrokerError> {
    // 1. Check broker index link for connection
    // 2, Check version exists "GET /pacticipants/{pacticipant}/versions/{versions}?
    // "{\"number\":\"5556b8149bf8bac76bc30f50a8a2dd4c22c85f30\",\"createdAt\":\"2024-03-17T07:11:23+00:00\",\"_embedded\":{\"branchVersions\":[{\"name\":\"main\",\"latest\":true,\"_links\":{\"self\":{\"title\":\"Branch version\",\"name\":\"main\",\"href\":\"http://localhost:9292/pacticipants/Example%20App/branches/main/versions/5556b8149bf8bac76bc30f50a8a2dd4c22c85f30\"}}}],\"tags\":[{\"name\":\"main\",\"_links\":{\"self\":{\"title\":\"Tag\",\"name\":\"main\",\"href\":\"http://localhost:9292/pacticipants/Example%20App/versions/5556b8149bf8bac76bc30f50a8a2dd4c22c85f30/tags/main\"}}}]},\"_links\":{\"self\":{\"title\":\"Version\",\"name\":\"5556b8149bf8bac76bc30f50a8a2dd4c22c85f30\",\"href\":\"http://localhost:9292/pacticipants/Example%20App/versions/5556b8149bf8bac76bc30f50a8a2dd4c22c85f30\"},\"pb:pacticipant\":{\"title\":\"Pacticipant\",\"name\":\"Example App\",\"href\":\"http://localhost:9292/pacticipants/Example%20App\"},\"pb:tag\":{\"href\":\"http://localhost:9292/pacticipants/Example%20App/versions/5556b8149bf8bac76bc30f50a8a2dd4c22c85f30/tags/{tag}\",\"title\":\"Get, create or delete a tag for this pacticipant version\",\"templated\":true},\"pb:latest-verification-results-where-pacticipant-is-consumer\":{\"title\":\"Latest verification results for consumer version\",\"href\":\"http://localhost:9292/verification-results/consumer/Example%20App/version/5556b8149bf8bac76bc30f50a8a2dd4c22c85f30/latest\"},\"pb:pact-versions\":[{\"title\":\"Pact\",\"name\":\"Pact between Example App (5556b8149bf8bac76bc30f50a8a2dd4c22c85f30) and Example API\",\"href\":\"http://localhost:9292/pacts/provider/Example%20API/consumer/Example%20App/version/5556b8149bf8bac76bc30f50a8a2dd4c22c85f30\"}],\"pb:record-deployment\":[{\"title\":\"Record deployment to Production\",\"name\":\"production\",\"href\":\"http://localhost:9292/pacticipants/Example%20App/versions/5556b8149bf8bac76bc30f50a8a2dd4c22c85f30/deployed-versions/environment/c540ce64-5493-48c5-ab7c-28dae27b166b\"},{\"title\":\"Record deployment to Test\",\"name\":\"test\",\"href\":\"http://localhost:9292/pacticipants/Example%20App/versions/5556b8149bf8bac76bc30f50a8a2dd4c22c85f30/deployed-versions/environment/cf7dcfdb-3645-4b16-b2f7-7ecb4b6045e0\"}],\"pb:record-release\":[{\"title\":\"Record release to Production\",\"name\":\"production\",\"href\":\"http://localhost:9292/pacticipants/Example%20App/versions/5556b8149bf8bac76bc30f50a8a2dd4c22c85f30/released-versions/environment/c540ce64-5493-48c5-ab7c-28dae27b166b\"},{\"title\":\"Record release to Test\",\"name\":\"test\",\"href\":\"http://localhost:9292/pacticipants/Example%20App/versions/5556b8149bf8bac76bc30f50a8a2dd4c22c85f30/released-versions/environment/cf7dcfdb-3645-4b16-b2f7-7ecb4b6045e0\"}],\"curies\":[{\"name\":\"pb\",\"href\":\"http://localhost:9292/doc/{rel}?context=version\",\"templated\":true}]}}"
    // 3. Find the pb:record-release link for the specified environment
    // 4. Send a POST request to the pb:record-release link with an empty payload
    // 5. Handle the response

    let version = args.get_one::<String>("version");
    let pacticipant = args.get_one::<String>("pacticipant");
    let environment = args.get_one::<String>("environment");
    let broker_url = get_broker_url(args).trim_end_matches('/').to_string();
    let auth = get_auth(args);
    let ssl_options = get_ssl_options(args);
    tokio::runtime::Runtime::new().unwrap().block_on(async {
            let hal_client: HALClient = HALClient::with_url(&broker_url, Some(auth.clone()),ssl_options.clone());
            // todo add trim_end_matches to broker url arg parse
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
                #[serde(rename = "pb:record-release")]
                record_release: Vec<Link>,
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
                        match data._links.record_release.iter().find(|x| x.name == Some(environment.unwrap().to_string())) {
                            Some(link) => {
                                let record_release_href = &link.href;

                                // println!("✅ Found environment {} with {}", utils::GREEN.apply_to(environment.unwrap()), utils::GREEN.apply_to(link_record_deployment_href.clone()));

                                // <- "POST /pacticipants/Example%20App/versions/5556b8149bf8bac76bc30f50a8a2dd4c22c85f30/deployed-versions/environment/c540ce64-5493-48c5-ab7c-28dae27b166b HTTP/1.1\r\nAccept: application/hal+json\r\nUser-Agent: Ruby\r\nContent-Type: application/json\r\nHost: localhost:9292\r\nContent-Length: 44\r\n\r\n"
                                // <- "{\"applicationInstance\":\"foo\",\"target\":\"foo\"}"

                                let payload = json!({});
                                let res: Result<Value, PactBrokerError> = hal_client.clone().post_json(&(record_release_href.clone()), &payload.to_string(), None).await;
                                let default_output = "text".to_string();
                                let output = args.get_one::<String>("output").unwrap_or(&default_output);
                                match res {
                                    Ok(res) => {
                                        let message = format!("✅ Recorded release of {} version {} to {} environment in the Pact Broker.", utils::GREEN.apply_to(pacticipant.unwrap()), utils::GREEN.apply_to(version.unwrap()),utils::GREEN.apply_to(environment.unwrap()));
                                            if output == "pretty" {
                                                let json = serde_json::to_string_pretty(&res).unwrap();
                                                println!("{}", json);
                                            } else if output == "json" {
                                                println!("{}", serde_json::to_string(&res).unwrap());
                                            } else if output == "id" {
                                                println!("{}", res["uuid"].to_string().trim_matches('"'));
                                            }
                                            else {
                                                println!("{}", message);

                                            }
                                        Ok(message.to_string())
                                    }
                                    Err(err) => {
                                        Err(match err {
                                            // TODO process output based on user selection
                                            PactBrokerError::LinkError(error) => {
                                                println!("❌ {}", utils::RED.apply_to(error.clone()));
                                                PactBrokerError::LinkError(error)
                                            }
                                            PactBrokerError::ContentError(error) => {
                                                println!("❌ {}", utils::RED.apply_to(error.clone()));
                                                PactBrokerError::ContentError(error)
                                            }
                                            PactBrokerError::IoError(error) => {
                                                println!("❌ {}", utils::RED.apply_to(error.clone()));
                                                PactBrokerError::IoError(error)
                                            }
                                            PactBrokerError::NotFound(error) => {
                                                println!("❌ {}", utils::RED.apply_to(error.clone()));
                                                PactBrokerError::NotFound(error)
                                            }
                                            PactBrokerError::ValidationError(errors) => {
                                                for error in &errors {
                                                    println!("❌ {}", utils::RED.apply_to(error.clone()));
                                                }
                                                PactBrokerError::ValidationError(errors)
                                            }
                                            err => {
                                                println!("❌ {}", utils::RED.apply_to(err.to_string()));
                                                err
                                            }
                                        })
                                    }
                                }
                                        }
                            None => {
                                let message = format!("❌ Environment {} does not exist", utils::RED.apply_to(environment.unwrap()));
                                println!("{}", message);
                                Err(PactBrokerError::NotFound(message))
                            }}
                        }
                        Err(err) => {
                            let message = format!("❌ Failed to record release: {}", err);
                            Err(PactBrokerError::ContentError(message))
                        }
                    }
                }
            Err(err) => {
                Err(match err {
                    // TODO process output based on user selection
                    PactBrokerError::LinkError(error) => {
                        println!("❌ {}", utils::RED.apply_to(error.clone()));
                        PactBrokerError::LinkError(error)
                    }
                    PactBrokerError::ContentError(error) => {
                        println!("❌ {}", utils::RED.apply_to(error.clone()));
                        PactBrokerError::ContentError(error)
                    }
                    PactBrokerError::IoError(error) => {
                        println!("❌ {}", utils::RED.apply_to(error.clone()));
                        PactBrokerError::IoError(error)
                    }
                    PactBrokerError::NotFound(error) => {
                        println!("❌ {}", utils::RED.apply_to(error.clone()));
                        PactBrokerError::NotFound(error)
                    }
                    PactBrokerError::ValidationError(errors) => {
                        for error in &errors {
                            println!("❌ {}", utils::RED.apply_to(error.clone()));
                        }
                        PactBrokerError::ValidationError(errors)
                    }
                    err => {
                        println!("❌ {}", utils::RED.apply_to(err.to_string()));
                        err
                    }
                })
            }
        }})
}

#[cfg(test)]
mod record_release_tests {
    use super::record_release;
    use crate::cli::pact_broker::main::subcommands::add_record_release_subcommand;
    use pact_consumer::prelude::*;
    use pact_models::PactSpecification;
    use serde_json::json;

    #[test]
    fn records_release_successfully() {
        // Arrange
        let config = MockServerConfig {
            pact_specification: PactSpecification::V2,
            ..MockServerConfig::default()
        };

        let pacticipant_name = "Foo";
        let version_number = "5556b8149bf8bac76bc30f50a8a2dd4c22c85f30";
        let environment_name = "test";
        let record_release_path = format!(
            "/pacticipants/{}/versions/{}/released-versions/environment/{}",
            pacticipant_name, version_number, "16926ef3-590f-4e3f-838e-719717aa88c9"
        );

        let pact_broker_service = PactBuilder::new("pact-broker-cli", "Pact Broker")
            // Pacticipant version with test environment available for release
            .interaction("a request for a pacticipant version", "", |mut i| {
                i.given("version 5556b8149bf8bac76bc30f50a8a2dd4c22c85f30 of pacticipant Foo exists with a test environment available for release");
                i.request
                    .path(format!("/pacticipants/{}/versions/{}", pacticipant_name, version_number))
                    .header("Accept", "application/hal+json")
                    .header("Accept", "application/json");
                i.response
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(json_pattern!({
                        "_links": {
                            "pb:record-release": [
                                {
                                    "name": environment_name,
                                    "href": term!("http:\\/\\/.*", format!("http://localhost{}", record_release_path))
                                }
                            ]
                        }
                    }));
                i
            })
            // POST to record release
            .interaction("a request to record a release", "", |mut i| {
                i.given("version 5556b8149bf8bac76bc30f50a8a2dd4c22c85f30 of pacticipant Foo exists with a test environment available for release");
                i.request
                    .method("POST")
                    .path(record_release_path.clone())
                    .header("Accept", "application/hal+json")
                    .header("Content-Type", "application/json")
                    .body(json!({}).to_string());
                i.response
                    .status(201)
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(json_pattern!(like!({})));
                i
            })
            .start_mock_server(None, Some(config));

        let mock_server_url = pact_broker_service.url();

        // Arrange CLI args
        let matches = add_record_release_subcommand().get_matches_from(vec![
            "record-release",
            "-b",
            mock_server_url.as_str(),
            "--pacticipant",
            pacticipant_name,
            "--version",
            version_number,
            "--environment",
            environment_name,
        ]);

        // Act
        let result = record_release(&matches);

        // Assert
        assert!(result.is_ok());
        let output = result.unwrap();
        println!("{}", output);
        assert!(output.contains("✅ Recorded release"));
    }
}
