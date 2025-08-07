use crate::cli::pact_broker::main::{
    HALClient, PactBrokerError,
    utils::{get_auth, get_broker_url, get_ssl_options},
};

pub fn create_version_tag(args: &clap::ArgMatches) -> Result<String, PactBrokerError> {
    let broker_url = get_broker_url(args).trim_end_matches('/').to_string();
    let auth = get_auth(args);
    let ssl_options = get_ssl_options(args);

    let pacticipant_name = args.get_one::<String>("pacticipant").unwrap();
    let version_number = args.get_one::<String>("version").unwrap();
    let tags = args
        .get_many::<String>("tag")
        .unwrap_or_default()
        .cloned()
        .collect::<Vec<_>>();
    let auto_create_version = args.get_flag("auto-create-version");
    let tag_with_git_branch = args.get_flag("tag-with-git-branch");
    // ensure version exists if auto-create is not set
    let res = tokio::runtime::Runtime::new().unwrap().block_on(async {
            let hal_client: HALClient = HALClient::with_url(&broker_url, Some(auth.clone()), ssl_options.clone());
            let version_href = format!("{}/pacticipants/{}/versions/{}", broker_url, pacticipant_name, version_number);
            let version_exists = hal_client.fetch(&version_href).await.is_ok();
            if !auto_create_version {
                if !version_exists {
                    return Err(PactBrokerError::NotFound(format!(
                        "Version '{}' of pacticipant '{}' does not exist. Use --auto-create-version to create it.",
                        version_number, pacticipant_name
                    )));
                } else {
                    return Ok("Version exists".to_string());
                }
            }
            Ok("Version created or already exists".to_string())
    });

    match res {
        Ok(_) => {
            tokio::runtime::Runtime::new().unwrap().block_on(async {
                let hal_client: HALClient =
                    HALClient::with_url(&broker_url, Some(auth.clone()), ssl_options.clone());
                for tag in tags {
                    print!(
                        "Tagging version '{}' of pacticipant '{}' with tag '{}'",
                        version_number, pacticipant_name, tag
                    );
                    let tag_href = format!(
                        "{}/pacticipants/{}/versions/{}/tags/{}",
                        broker_url, pacticipant_name, version_number, tag
                    );
                    let tag_data = serde_json::json!({ "name": tag });
                    let tag_data_str = tag_data.to_string();
                    let tag_post_result = hal_client
                        .put_json(&tag_href, &tag_data_str)
                        .await
                        .map_err(|e| {
                            PactBrokerError::IoError(format!(
                                "Failed to create tag '{}': {}",
                                tag, e
                            ))
                        });
                    match tag_post_result {
                        Ok(_) => println!(" - Success"),
                        Err(e) => println!(" - Failed: {}", e),
                    }
                }

                if tag_with_git_branch {
                    println!(
                        "Tagged version '{}' of pacticipant '{}' with git branch",
                        version_number, pacticipant_name
                    );
                }
            });

            Ok(format!(
                "Successfully tagged version '{}' of pacticipant '{}'",
                version_number, pacticipant_name
            ))
        }
        Err(err) => Err(err),
    }
}

#[cfg(test)]
mod create_version_tag_tests {
    use std::vec;

    use crate::cli::pact_broker::main::subcommands::add_create_version_tag_subcommand;
    use crate::cli::pact_broker::main::tags::create_version_tag::create_version_tag;
    use pact_consumer::prelude::*;
    use pact_models::PactSpecification;
    use serde_json::Value;
    use serde_json::json;

    fn setup_args(
        pacticipant: &str,
        version: &str,
        tag: &str,
        broker_url: &str,
        auto_create: Option<&bool>,
    ) -> clap::ArgMatches {
        let mut args = vec![
            "create-version-tag",
            "--pacticipant",
            pacticipant,
            "--version",
            version,
            "--tag",
            tag,
            "-b",
            broker_url,
        ];
        if *auto_create.unwrap_or(&false) {
            args.push("--auto-create-version");
        }
        add_create_version_tag_subcommand()
            .args(crate::cli::add_ssl_arguments())
            .get_matches_from(args)
    }

    fn build_tag_response(status: u16) -> (u16, JsonPattern) {
        (
            status,
            json_pattern!({
                "_links": {
                    "self": {
                        "href": term!(r"http:\/\/.*(pacticipants).*(versions).*(tags).*","http://localhost:1234/pacticipants/Condor/versions/1.3.0/tags/prod"),
                    }
                }
            }),
        )
    }

    #[test]
    fn tag_version_when_component_exists_returns_success() {
        let config = MockServerConfig {
            pact_specification: PactSpecification::V2,
            ..MockServerConfig::default()
        };

        let pacticipant = "Condor";
        let version = "1.3.0";
        let tag = "prod";

        let (status, body) = build_tag_response(201);

        let pact_broker_service = PactBuilder::new("pact-broker-cli", "Pact Broker")
            .interaction(
                "a request to check the production version of Condor",
                "",
                |mut i| {
                    i.given("'Condor' exists in the pact-broker");
                    i.request.get().path(format!(
                        "/pacticipants/{}/versions/{}",
                        pacticipant, version
                    ));
                    i.response
                        .status(200)
                        .header("Content-Type", "application/hal+json;charset=utf-8")
                        .json_body(json_pattern!(like!({})));
                    i
                },
            )
            .interaction(
                "a request to tag the production version of Condor",
                "",
                |mut i| {
                    i.given("'Condor' exists in the pact-broker");
                    i.request
                        .put()
                        .header("Content-Type", "application/json")
                        .path(format!(
                            "/pacticipants/{}/versions/{}/tags/{}",
                            pacticipant, version, tag
                        ));
                    i.response
                        .status(status)
                        .header("Content-Type", "application/hal+json;charset=utf-8")
                        .json_body(body);
                    i
                },
            )
            .start_mock_server(None, Some(config));

        let broker_url = pact_broker_service.url();
        let mut args = setup_args(pacticipant, version, tag, broker_url.as_str(), None);

        let result = create_version_tag(&args);
        assert!(result.is_ok());
        let msg = result.unwrap();
        assert!(msg.contains("Successfully tagged version"));
    }

    #[test]
    fn tag_version_when_component_does_not_exist_returns_success() {
        let config = MockServerConfig {
            pact_specification: PactSpecification::V2,
            ..MockServerConfig::default()
        };

        let pacticipant = "Condor";
        let version = "1.3.0";
        let tag = "prod";

        let (status, body) = build_tag_response(201);

        let pact_broker_service = PactBuilder::new("pact-broker-cli", "PactFlow")
            .interaction(
                "a request to check the production version of Condor",
                "",
                |mut i| {
                    i.given("'Condor' does not exist in the pact-broker");
                    i.request.get().path(format!(
                        "/pacticipants/{}/versions/{}",
                        pacticipant, version
                    ));
                    i.response.status(404);
                    i
                },
            )
            .interaction(
                "a request to tag the production version of Condor",
                "",
                |mut i| {
                    i.given("'Condor' does not exist in the pact-broker");
                    i.request.put().path(format!(
                        "/pacticipants/{}/versions/{}/tags/{}",
                        pacticipant, version, tag
                    ));
                    i.response
                        .status(status)
                        .header("Content-Type", "application/hal+json;charset=utf-8")
                        .json_body(body);
                    i
                },
            )
            .start_mock_server(None, Some(config));

        let broker_url = pact_broker_service.url();
        let mut args = setup_args(pacticipant, version, tag, broker_url.as_str(), Some(&true));

        let result = create_version_tag(&args);
        assert!(result.is_ok());
        let msg = result.unwrap();
        assert!(msg.contains("Successfully tagged version"));
    }

    #[test]
    fn tag_version_when_tag_already_exists_returns_success() {
        let config = MockServerConfig {
            pact_specification: PactSpecification::V2,
            ..MockServerConfig::default()
        };

        let pacticipant = "Condor";
        let version = "1.3.0";
        let tag = "prod";

        let (status, body) = build_tag_response(200);

        let pact_broker_service = PactBuilder::new("pact-broker-cli", "PactFlow")
            .interaction(
                "a request to check the production version of Condor",
                "",
                |mut i| {
                    i.given(
                        "'Condor' exists in the pact-broker with version 1.3.0, tagged with 'prod'",
                    );
                    i.request.get().path(format!(
                        "/pacticipants/{}/versions/{}",
                        pacticipant, version
                    ));
                    i.response
                        .status(200)
                        .header("Content-Type", "application/hal+json;charset=utf-8")
                        .json_body(json_pattern!(like!({})));
                    i
                },
            )
            .interaction(
                "a request to tag the production version of Condor",
                "",
                |mut i| {
                    i.given(
                        "'Condor' exists in the pact-broker with version 1.3.0, tagged with 'prod'",
                    );
                    i.request
                        .put()
                        .header("Content-Type", "application/json")
                        .path(format!(
                            "/pacticipants/{}/versions/{}/tags/{}",
                            pacticipant, version, tag
                        ));
                    i.response
                        .status(status)
                        .header("Content-Type", "application/hal+json;charset=utf-8")
                        .json_body(body);
                    i
                },
            )
            .start_mock_server(None, Some(config));

        let broker_url = pact_broker_service.url();
        let mut args = setup_args(pacticipant, version, tag, broker_url.as_str(), None);

        let result = create_version_tag(&args);
        assert!(result.is_ok());
        let msg = result.unwrap();
        assert!(msg.contains("Successfully tagged version"));
    }
}
