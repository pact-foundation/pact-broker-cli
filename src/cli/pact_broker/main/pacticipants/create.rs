use crate::cli::pact_broker::main::{
    HALClient, Link, PactBrokerError,
    utils::{get_auth, get_broker_relation, get_broker_url, get_custom_headers, get_ssl_options},
};
use maplit::hashmap;
use std::result::Result::Ok;

pub fn create_or_update_pacticipant(args: &clap::ArgMatches) -> Result<String, PactBrokerError> {
    let broker_url = get_broker_url(args).trim_end_matches('/').to_string();
    let auth = get_auth(args);
    let custom_headers = get_custom_headers(args);
    let ssl_options = get_ssl_options(args);

    let pacticipant_name = args.get_one::<String>("name").unwrap();
    let display_name = args.try_get_one::<String>("display-name").unwrap();
    let main_branch = args.try_get_one::<String>("main-branch").unwrap();
    let repository_url = args.try_get_one::<String>("repository-url").unwrap();

    let res = tokio::runtime::Runtime::new().unwrap().block_on(async {
        let hal_client: HALClient = HALClient::with_url(
            &broker_url,
            Some(auth.clone()),
            ssl_options.clone(),
            custom_headers.clone(),
        );

        let template_values = hashmap! {
            "pacticipant".to_string() => pacticipant_name.to_string(),
        };

        let pacticipant_href = get_broker_relation(
            hal_client.clone(),
            "pb:pacticipant".to_string(),
            broker_url.to_string(),
        )
        .await;
        let pacticipant_entity = match pacticipant_href {
            Ok(pacticipant_href) => {
                let link = Link {
                    name: "pb:pacticipant".to_string(),
                    href: Some(pacticipant_href),
                    templated: true,
                    title: None,
                };
                hal_client.clone().fetch_url(&link, &template_values).await
            }
            Err(err) => return Err(err),
        };

        match &pacticipant_entity {
            Ok(entity) => {
                let pacticipant_href = entity
                    .get("_links")
                    .and_then(|links| links.get("self"))
                    .and_then(|link| link.get("href"))
                    .and_then(|href| href.as_str())
                    .unwrap_or_default()
                    .to_string();
                let mut pacticipant_data = serde_json::json!({
                    "name": pacticipant_name,
                });

                if let Some(display_name) = display_name {
                    pacticipant_data["displayName"] =
                        serde_json::Value::String(display_name.to_string());
                }
                if let Some(main_branch) = main_branch {
                    pacticipant_data["mainBranch"] =
                        serde_json::Value::String(main_branch.to_string());
                }
                if let Some(repository_url) = repository_url {
                    pacticipant_data["repositoryUrl"] =
                        serde_json::Value::String(repository_url.to_string());
                }

                let pacticipant_data_str = pacticipant_data.to_string();
                hal_client
                    .patch_json(&pacticipant_href, &pacticipant_data_str, None)
                    .await
                    .map_err(|e| {
                        PactBrokerError::IoError(format!(
                            "Failed to update pacticipant '{}': {}",
                            pacticipant_name, e
                        ))
                    })?;
                Ok(format!(
                    "Pacticipant '{}' updated successfully",
                    pacticipant_name
                ))
            }
            Err(PactBrokerError::NotFound(_)) => {
                println!("Pacticipant does not exist, creating it at: {}", broker_url);
                let pacticipants_href = get_broker_relation(
                    hal_client.clone(),
                    "pb:pacticipants".to_string(),
                    broker_url.to_string(),
                )
                .await
                .unwrap();
                let mut pacticipant_data = serde_json::json!({
                    "name": pacticipant_name,
                });
                if let Some(display_name) = display_name {
                    println!("Creating pacticipant with display name: {}", display_name);
                    pacticipant_data["displayName"] =
                        serde_json::Value::String(display_name.to_string());
                }
                if let Some(main_branch) = main_branch {
                    println!("Creating pacticipant with main branch: {}", main_branch);
                    pacticipant_data["mainBranch"] =
                        serde_json::Value::String(main_branch.to_string());
                }
                if let Some(repository_url) = repository_url {
                    println!(
                        "Creating pacticipant with repository URL: {}",
                        repository_url
                    );
                    pacticipant_data["repositoryUrl"] =
                        serde_json::Value::String(repository_url.to_string());
                }

                let pacticipant_data_str = pacticipant_data.to_string();
                hal_client
                    .post_json(&pacticipants_href, &pacticipant_data_str, None)
                    .await
                    .map_err(|e| {
                        PactBrokerError::IoError(format!(
                            "Failed to create pacticipant '{}': {}",
                            pacticipant_name, e
                        ))
                    })?;
                Ok(format!(
                    "Pacticipant '{}' created successfully",
                    pacticipant_name
                ))
            }
            Err(err) => return Err(err.clone()),
        }
    });

    match res {
        Ok(message) => {
            println!("{}", message);
            Ok(message)
        }
        Err(err) => Err(err),
    }
}

#[cfg(test)]
mod create_or_update_pacticipant_tests {
    use crate::cli::pact_broker::main::pacticipants::create::create_or_update_pacticipant;
    use crate::cli::pact_broker::main::subcommands::add_create_or_update_pacticipant_subcommand;
    use pact_consumer::builders::InteractionBuilder;
    use pact_consumer::prelude::*;
    use pact_models::PactSpecification;
    use serde_json::json;

    fn setup_mock_server(interactions: Vec<InteractionBuilder>) -> Box<dyn ValidatingMockServer> {
        let config = MockServerConfig {
            pact_specification: PactSpecification::V2,
            ..MockServerConfig::default()
        };
        let mut pact_builder = PactBuilder::new("pact-broker-cli", "Pact Broker");
        for i in interactions {
            pact_builder.push_interaction(&i.build());
        }
        pact_builder.start_mock_server(None, Some(config))
    }

    #[test]
    fn create_pacticipant_when_not_exists() {
        let pacticipant_name = "Condor";
        let repository_url = "http://foo";
        let request_body = json!({
            "name": pacticipant_name,
            "repositoryUrl": repository_url
        });

        // Index resource with pb:pacticipants and pb:pacticipant relations
        let index_interaction = |mut i: InteractionBuilder| {
            i.given("the pacticipant relations are present");
            i.request
                .path("/")
                .header("Accept", "application/hal+json")
                .header("Accept", "application/json");
            i.response
                .header("Content-Type", "application/hal+json;charset=utf-8")
                .json_body(json_pattern!({
                    "_links": {
                        "pb:pacticipants": {
                            "href": term!("http:\\/\\/.*", "http://localhost/pacticipants")
                        },
                        "pb:pacticipant": {
                            "href": term!("http:\\/\\/.*/pacticipants/\\{pacticipant\\}", "http://localhost/pacticipants/{pacticipant}")
                        }
                    }
                }));
            i
        };

        // GET /pacticipants/Condor returns 404
        let get_pacticipant_interaction = |mut i: InteractionBuilder| {
            i.given("'Condor' does not exist in the pact-broker");
            i.request
                .get()
                .path("/pacticipants/Condor")
                .header("Accept", "application/hal+json")
                .header("Accept", "application/json");
            i.response.status(404);
            i
        };

        // POST /pacticipants creates the pacticipant
        let create_pacticipant_interaction = |mut i: InteractionBuilder| {
            i.request
                .post()
                .path("/pacticipants")
                .header("Accept", "application/hal+json")
                .header("Content-Type", "application/json")
                .json_body(request_body.clone());
            i.response
                .status(201)
                .header("Content-Type", "application/hal+json;charset=utf-8")
                .json_body(json_pattern!({
                    "name": pacticipant_name,
                    "repositoryUrl": repository_url,
                    "_links": {
                        "self": {
                            "href": term!( "http://.*","http://localhost/pacticipants/Condor")
                        }
                    }
                }));
            i
        };

        let mock_server = setup_mock_server(vec![
            index_interaction(InteractionBuilder::new(
                "a request for the index resource",
                "",
            )),
            get_pacticipant_interaction(InteractionBuilder::new(
                "a request to retrieve a pacticipant",
                "",
            )),
            create_pacticipant_interaction(InteractionBuilder::new(
                "a request to create a pacticipant",
                "",
            )),
        ]);
        let mock_server_url = mock_server.url();

        let matches = add_create_or_update_pacticipant_subcommand().get_matches_from(vec![
            "create-or-update-pacticipant",
            "-b",
            mock_server_url.as_str(),
            "--name",
            pacticipant_name,
            "--repository-url",
            repository_url,
        ]);

        let result = create_or_update_pacticipant(&matches);

        assert!(result.is_ok());
        assert!(result.unwrap().contains("created successfully"));
    }

    #[test]
    fn update_pacticipant_when_exists() {
        let pacticipant_name = "Foo";
        let repository_url = "http://foo";
        let request_body = json!({
            "name": pacticipant_name,
            "repositoryUrl": repository_url
        });

        // Index resource with pb:pacticipants and pb:pacticipant relations
        let index_interaction = |mut i: InteractionBuilder| {
            i.given("the pacticipant relations are present");
            i.request
                .path("/")
                .header("Accept", "application/hal+json")
                .header("Accept", "application/json");
            i.response
                .header("Content-Type", "application/hal+json;charset=utf-8")
                .json_body(json_pattern!({
                    "_links": {
                        "pb:pacticipants": {
                            "href": term!( "http://.*", "http://localhost/pacticipants")
                        },
                        "pb:pacticipant": {
                            "href": term!("http:\\/\\/.*/pacticipants/\\{pacticipant\\}", "http://localhost/pacticipants/{pacticipant}")
                        }
                    }
                }));
            i
        };

        // GET /pacticipants/Foo returns 200
        let get_pacticipant_interaction = |mut i: InteractionBuilder| {
            i.given("a pacticipant with name Foo exists");
            i.request
                .get()
                .path("/pacticipants/Foo")
                .header("Accept", "application/hal+json")
                .header("Accept", "application/json");
            i.response
                .status(200)
                .header("Content-Type", "application/hal+json;charset=utf-8")
                .json_body(json_pattern!({
                    "_links": {
                        "self": {
                            "href": term!("http:\\/\\/.*", "http://localhost/pacticipants/Foo")
                        }
                    }
                }));
            i
        };

        // PATCH /pacticipants/Foo updates the pacticipant
        let update_pacticipant_interaction = |mut i: InteractionBuilder| {
            i.given("a pacticipant with name Foo exists");
            i.request
                .method("PATCH")
                .path("/pacticipants/Foo")
                .header("Accept", "application/hal+json")
                .header("Content-Type", "application/merge-patch+json")
                .json_body(request_body.clone());
            i.response
                .status(200)
                .header("Content-Type", "application/hal+json;charset=utf-8")
                .json_body(json_pattern!({
                    "name": pacticipant_name,
                    "repositoryUrl": repository_url,
                    "_links": {
                        "self": {
                            "href": term!("http:\\/\\/.*", "http://localhost/pacticipants/Foo")
                        }
                    }
                }));
            i
        };

        let mock_server = setup_mock_server(vec![
            index_interaction(InteractionBuilder::new(
                "a request for the index resource",
                "",
            )),
            get_pacticipant_interaction(InteractionBuilder::new(
                "a request to retrieve a pacticipant",
                "",
            )),
            update_pacticipant_interaction(InteractionBuilder::new(
                "a request to update a pacticipant",
                "",
            )),
        ]);
        let mock_server_url = mock_server.url();

        let matches = add_create_or_update_pacticipant_subcommand().get_matches_from(vec![
            "create-or-update-pacticipant",
            "-b",
            mock_server_url.as_str(),
            "--name",
            pacticipant_name,
            "--repository-url",
            repository_url,
        ]);

        let result = create_or_update_pacticipant(&matches);

        assert!(result.is_ok());
        assert!(result.unwrap().contains("updated successfully"));
    }
}
