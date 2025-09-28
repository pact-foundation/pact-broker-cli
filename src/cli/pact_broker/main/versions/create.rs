use crate::cli::pact_broker::main::{
    HALClient, PactBrokerError,
    utils::{get_auth, get_broker_url, get_ssl_options},
};

pub fn create_or_update_version(args: &clap::ArgMatches) -> Result<String, PactBrokerError> {
    let broker_url = get_broker_url(args).trim_end_matches('/').to_string();
    let auth = get_auth(args);
    let ssl_options = get_ssl_options(args);

    let pacticipant_name = args.get_one::<String>("pacticipant").unwrap();
    let version_number = args.get_one::<String>("version").unwrap();
    let branch_name = args.try_get_one::<String>("branch").unwrap();
    let tags = args
        .get_many::<String>("tag")
        .unwrap_or_default()
        .cloned()
        .collect::<Vec<_>>();

    let result = tokio::runtime::Runtime::new().unwrap().block_on(async {
        let hal_client: HALClient =
            HALClient::with_url(&broker_url, Some(auth.clone()), ssl_options.clone());
        let version_href = format!(
            "{}/pacticipants/{}/versions/{}",
            broker_url, pacticipant_name, version_number
        );

        // create branch version
        if let Some(branch) = branch_name {
            let branch_href = format!(
                "{}/pacticipants/{}/branches/{}/versions/{}",
                broker_url, pacticipant_name, branch, version_number
            );
            let branch_data = serde_json::json!({ "name": branch });
            let branch_data_str = branch_data.to_string();
            let res = hal_client
                .put_json(&branch_href, &branch_data_str, None)
                .await;
            res?;
            println!(
                "Branch '{}' created for version '{}'",
                branch, version_number
            );
        }

        // create tags

        for tag in &tags {
            let tag_href = format!(
                "{}/pacticipants/{}/versions/{}/tags/{}",
                broker_url, pacticipant_name, version_number, tag
            );
            let tag_data = serde_json::json!({ "name": tag });
            let tag_data_str = tag_data.to_string();
            let res = hal_client.put_json(&tag_href, &tag_data_str, None).await;
            res?;
            println!("Tag '{}' created for version '{}'", tag, version_number);
        }
        // if no tags or branches, create version
        if tags.is_empty() && branch_name.is_none() {
            let version_data = serde_json::json!({});
            let version_data_str = version_data.to_string();
            let res = hal_client
                .put_json(&version_href, &version_data_str, None)
                .await;
            match res {
                Ok(_) => {
                    println!(
                        "Version '{}' created or updated successfully",
                        version_number
                    );
                }
                Err(err) => {
                    return Err(PactBrokerError::IoError(format!(
                        "Failed to create or update version '{}': {}",
                        version_number, err
                    )));
                }
            }
        }
        Ok("Version created or updated successfully".to_string())
    });
    result
}

#[cfg(test)]
mod create_or_update_version_tests {
    use super::create_or_update_version;
    use crate::cli::pact_broker::main::subcommands::add_create_or_update_version_subcommand;
    use pact_consumer::prelude::*;
    use pact_models::PactSpecification;
    use serde_json::json;

    #[test]
    fn creates_version_with_tag_and_branch_successfully() {
        // Arrange
        let config = MockServerConfig {
            pact_specification: PactSpecification::V2,
            ..MockServerConfig::default()
        };

        let pacticipant_name = "Foo";
        let version_number = "1";
        let branch_name = "test";
        let tag_name = "prod";

        let pacticipant_path = format!("/pacticipants/{}", pacticipant_name);
        let branch_path = format!(
            "{}/branches/{}/versions/{}",
            pacticipant_path, branch_name, version_number
        );
        let tag_path = format!(
            "{}/versions/{}/tags/{}",
            pacticipant_path, version_number, tag_name
        );

        let pact_broker_service = PactBuilder::new("pact-broker-cli", "Pact Broker")
            // PUT branch
            .interaction("create branch for version", "", |mut i| {
                i.given(format!(
                    "a pacticipant with name {} exists",
                    pacticipant_name
                ));
                i.request
                    .method("PUT")
                    .path(branch_path.clone())
                    .header("Accept", "application/hal+json")
                    .header("Content-Type", "application/json")
                    .body(json!({ "name": branch_name }).to_string());
                i.response
                    .status(201)
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(json!({
                        "_links": {
                            "pb:branch": {
                                "name": branch_name,
                            },
                        }
                    }));
                i
            })
            // PUT tag
            .interaction("create tag for version", "", |mut i| {
                i.request
                    .method("PUT")
                    .path(tag_path.clone())
                    .header("Accept", "application/hal+json")
                    .header("Content-Type", "application/json")
                    .body(json!({ "name": tag_name }).to_string());
                i.response
                    .status(201)
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(json!({ "name": tag_name }));
                i
            })
            .start_mock_server(None, Some(config));

        let mock_server_url = pact_broker_service.url();

        // Arrange CLI args
        let matches = add_create_or_update_version_subcommand()
            .args(crate::cli::add_ssl_arguments())
            .get_matches_from(vec![
                "create-or-update-version",
                "-b",
                mock_server_url.as_str(),
                "--pacticipant",
                pacticipant_name,
                "--version",
                version_number,
                "--branch",
                branch_name,
                "--tag",
                tag_name,
            ]);

        // Act
        let result = create_or_update_version(&matches);

        // Assert
        assert!(result.is_ok());
        let output = result.unwrap();
        println!("Output: {}", output);
        assert!(output.contains("Version created or updated successfully"));
    }

    #[test]
    fn creates_version_without_tag_or_branch() {
        // Arrange
        let config = MockServerConfig {
            pact_specification: PactSpecification::V2,
            ..MockServerConfig::default()
        };

        let pacticipant_name = "Foo";
        let version_number = "1";
        let version_path = format!(
            "/pacticipants/{}/versions/{}",
            pacticipant_name, version_number
        );

        let pact_broker_service = PactBuilder::new("pact-broker-cli", "Pact Broker")
            // PUT version
            .interaction("create version without tag or branch", "", |mut i| {
                i.given(format!(
                    "a pacticipant with name {} exists",
                    pacticipant_name
                ));
                i.request
                    .method("PUT")
                    .path(version_path.clone())
                    .header("Accept", "application/hal+json")
                    .header("Content-Type", "application/json")
                    .body(json!({}).to_string());
                i.response
                    .status(201)
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(json!({}));
                i
            })
            .start_mock_server(None, Some(config));

        let mock_server_url = pact_broker_service.url();

        // Arrange CLI args
        let matches = add_create_or_update_version_subcommand()
            .args(crate::cli::add_ssl_arguments())
            .get_matches_from(vec![
                "create-or-update-version",
                "-b",
                mock_server_url.as_str(),
                "--pacticipant",
                pacticipant_name,
                "--version",
                version_number,
            ]);

        // Act
        let result = create_or_update_version(&matches);

        // Assert
        assert!(result.is_ok());
        let output = result.unwrap();
        println!("Output: {}", output);
        assert!(output.contains("Version created or updated successfully"));
    }
}
