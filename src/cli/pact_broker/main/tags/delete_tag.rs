use maplit::hashmap;

use crate::cli::pact_broker::main::{
    HALClient, Link, PactBrokerError,
    utils::{get_auth, get_broker_relation, get_broker_url, get_ssl_options},
};

pub fn delete_version_tag(args: &clap::ArgMatches) -> Result<String, PactBrokerError> {
    let broker_url = get_broker_url(args).trim_end_matches('/').to_string();
    let auth = get_auth(args);
    let ssl_options = get_ssl_options(args);

    let pacticipant_name = args.get_one::<String>("pacticipant").unwrap();
    let version_number = args.get_one::<String>("version").unwrap();
    let tag_name = args.get_one::<String>("tag").unwrap();

    let res = tokio::runtime::Runtime::new().unwrap().block_on(async {
        let hal_client: HALClient =
            HALClient::with_url(&broker_url, Some(auth.clone()), ssl_options.clone());

        // First, get the pacticipant to access its version-tag relation
        let pb_pacticipant_href_path = get_broker_relation(
            hal_client.clone(),
            "pb:pacticipant".to_string(),
            broker_url.to_string(),
        )
        .await?;

        let pacticipant_template_values = hashmap! {
            "pacticipant".to_string() => pacticipant_name.to_string(),
        };

        let pacticipant_link = Link {
            name: "pb:pacticipant".to_string(),
            href: Some(pb_pacticipant_href_path),
            templated: true,
            title: None,
        };

        let pacticipant_response = hal_client
            .clone()
            .fetch_url(&pacticipant_link, &pacticipant_template_values)
            .await?;

        // Get the version-tag relation from the pacticipant
        let version_tag_href = pacticipant_response
            .get("_links")
            .and_then(|links| links.get("pb:version-tag"))
            .and_then(|link| link.get("href"))
            .and_then(|href| href.as_str())
            .ok_or_else(|| {
                PactBrokerError::NotFound(
                    "pb:version-tag relation not found in pacticipant response".to_string(),
                )
            })?;

        // Build the template values for the version tag
        let tag_template_values = hashmap! {
            "pacticipant".to_string() => pacticipant_name.to_string(),
            "version".to_string() => version_number.to_string(),
            "tag".to_string() => tag_name.to_string(),
        };

        let version_tag_link = Link {
            name: "pb:version-tag".to_string(),
            href: Some(version_tag_href.to_string()),
            templated: true,
            title: None,
        };

        // Delete the tag
        hal_client
            .clone()
            .delete_url(&version_tag_link, &tag_template_values)
            .await
    });

    match res {
        Ok(_) => {
            let message = format!(
                "Successfully deleted tag '{}' from version '{}' of pacticipant '{}'",
                tag_name, version_number, pacticipant_name
            );
            println!("{}", message);
            Ok(message)
        }
        Err(PactBrokerError::NotFound(_)) => {
            let message = format!(
                "Tag '{}' for version '{}' of pacticipant '{}' was not found (may have already been deleted)",
                tag_name, version_number, pacticipant_name
            );
            println!("{}", message);
            Ok(message)
        }
        Err(err) => Err(err),
    }
}

#[cfg(test)]
mod delete_version_tag_tests {
    use crate::cli::pact_broker::main::subcommands::add_delete_version_tag_subcommand;
    use crate::cli::pact_broker::main::tags::delete_tag::delete_version_tag;
    use pact_consumer::prelude::*;
    use pact_models::PactSpecification;
    use serde_json::json;

    #[test]
    fn delete_version_tag_when_exists_returns_success() {
        let config = MockServerConfig {
            pact_specification: PactSpecification::V2,
            ..MockServerConfig::default()
        };

        let pacticipant = "Condor";
        let version = "1.3.0";
        let tag = "prod";

        let pact_broker_service = PactBuilder::new("pact-broker-cli", "Pact Broker")
            .interaction("a request for the index resource", "", |mut i| {
                i.given("the pb:pacticipant relation exists in the index resource");
                i.request
                    .path("/")
                    .header("Accept", "application/hal+json")
                    .header("Accept", "application/json");
                i.response
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(json_pattern!({
                        "_links": {
                            "pb:pacticipant": {
                                "href": term!("http:\\/\\/.*\\{pacticipant\\}","http://localhost/pacticipants/{pacticipant}"),
                                "title": like!("Fetch pacticipant by name"),
                                "templated": true
                            }
                        }
                    }));
                i
            })
            .interaction("a request to get pacticipant", "", |mut i| {
                i.given(format!("'{}' exists in the pact-broker", pacticipant));
                i.request
                    .get()
                    .path(format!("/pacticipants/{}", pacticipant))
                    .header("Accept", "application/hal+json")
                    .header("Accept", "application/json");
                i.response
                    .status(200)
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(json_pattern!({
                        "_links": {
                            "pb:version-tag": {
                                "href": term!("http:\\/\\/.*\\{pacticipants\\}.*\\{versions\\}.*\\{tags\\}.*","http://localhost/pacticipants/{pacticipant}/versions/{version}/tags/{tag}"),
                                "title": like!("Get, create or delete tag"),
                                "templated": true
                            }
                        }
                    }));
                i
            })
            .interaction("a request to delete a tag", "", |mut i| {
                i.given(format!("'{}' exists in the pact-broker with version {}, tagged with '{}'", pacticipant, version, tag));
                i.request
                    .delete()
                    .path(format!("/pacticipants/{}/versions/{}/tags/{}", pacticipant, version, tag));
                i.response.status(204);
                i
            })
            .start_mock_server(None, Some(config));

        let mock_server_url = pact_broker_service.url();
        let matches = add_delete_version_tag_subcommand().get_matches_from(vec![
            "delete-version-tag",
            "-b",
            mock_server_url.as_str(),
            "--pacticipant",
            pacticipant,
            "--version",
            version,
            "--tag",
            tag,
        ]);

        let result = delete_version_tag(&matches);

        assert!(result.is_ok());
        assert!(result.unwrap().contains("Successfully deleted tag"));
    }

    #[test]
    fn delete_version_tag_when_not_exists_returns_success_with_message() {
        let config = MockServerConfig {
            pact_specification: PactSpecification::V2,
            ..MockServerConfig::default()
        };

        let pacticipant = "Condor";
        let version = "1.3.0";
        let tag = "prod";

        let pact_broker_service = PactBuilder::new("pact-broker-cli", "Pact Broker")
            .interaction("a request for the index resource", "", |mut i| {
                i.given("the pb:pacticipant relation exists in the index resource");
                i.request
                    .path("/")
                    .header("Accept", "application/hal+json")
                    .header("Accept", "application/json");
                i.response
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(json_pattern!({
                        "_links": {
                            "pb:pacticipant": {
                                "href": term!("http:\\/\\/.*\\{pacticipant\\}","http://localhost/pacticipants/{pacticipant}"),
                                "title": like!("Fetch pacticipant by name"),
                                "templated": true
                            }
                        }
                    }));
                i
            })
            .interaction("a request to get pacticipant", "", |mut i| {
                i.given(format!("'{}' exists in the pact-broker", pacticipant));
                i.request
                    .get()
                    .path(format!("/pacticipants/{}", pacticipant))
                    .header("Accept", "application/hal+json")
                    .header("Accept", "application/json");
                i.response
                    .status(200)
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(json_pattern!({
                        "_links": {
                            "pb:version-tag": {
                                "href": term!("http:\\/\\/.*\\/pacticipants\\/.*\\/versions\\/\\{version\\}\\/tags\\/\\{tag\\}","http://localhost/pacticipants/{pacticipant}/versions/{version}/tags/{tag}"),
                                "title": like!("Get, create or delete tag"),
                                "templated": true
                            }
                        }
                    }));
                i
            })
            .interaction("a request to delete a non-existent tag", "", |mut i| {
                i.request
                    .delete()
                    .path(format!("/pacticipants/{}/versions/{}/tags/{}", pacticipant, version, tag));
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
        let matches = add_delete_version_tag_subcommand().get_matches_from(vec![
            "delete-version-tag",
            "-b",
            mock_server_url.as_str(),
            "--pacticipant",
            pacticipant,
            "--version",
            version,
            "--tag",
            tag,
        ]);

        let result = delete_version_tag(&matches);

        assert!(result.is_ok());
        assert!(result.unwrap().contains("was not found"));
    }
}
