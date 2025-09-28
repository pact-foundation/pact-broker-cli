use crate::cli::{
    pact_broker::main::{
        HALClient, PactBrokerError,
        utils::{
            follow_broker_relation, get_auth, get_broker_relation, get_broker_url, get_ssl_options,
        },
    },
    utils,
};
use comfy_table::Table;
use comfy_table::presets::UTF8_FULL;

pub fn list_environments(args: &clap::ArgMatches) -> Result<String, PactBrokerError> {
    let broker_url = get_broker_url(args).trim_end_matches('/').to_string();
    let auth = get_auth(args);
    let ssl_options = get_ssl_options(args);
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let hal_client: HALClient =
            HALClient::with_url(&broker_url, Some(auth.clone()), ssl_options.clone());
        let pb_environments_href_path = get_broker_relation(
            hal_client.clone(),
            "pb:environments".to_string(),
            broker_url.to_string(),
        )
        .await?;

        let res = follow_broker_relation(
            hal_client,
            "pb:environments".to_string(),
            pb_environments_href_path,
        )
        .await;

        let default_output = "text".to_string();
        let output = args.get_one::<String>("output").unwrap_or(&default_output);
        match res {
            Ok(res) => {
                if output == "pretty" {
                    let json = serde_json::to_string_pretty(&res).unwrap();
                    println!("{}", json);
                } else if output == "json" {
                    println!("{}", serde_json::to_string(&res).unwrap());
                } else {
                    let mut table = Table::new();

                    #[derive(Debug, serde::Deserialize)]
                    struct Environment {
                        uuid: String,
                        name: String,
                        #[serde(rename = "displayName")]
                        display_name: String,
                        production: bool,
                    }

                    table.load_preset(UTF8_FULL).set_header(vec![
                        "UUID",
                        "NAME",
                        "DISPLAY NAME",
                        "PRODUCTION",
                    ]);

                    if let Some(embedded) = res["_embedded"].as_object() {
                        if let Some(environments) = embedded["environments"].as_array() {
                            for environment in environments {
                                let environment: Environment =
                                    serde_json::from_value(environment.clone()).unwrap();
                                table.add_row(vec![
                                    environment.uuid,
                                    environment.name,
                                    environment.display_name,
                                    environment.production.to_string(),
                                ]);
                            }
                        }
                    }

                    println!("{table}");
                }

                Ok("".to_string())
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
    })
}

#[cfg(test)]
mod list_environments_tests {
    use crate::cli::pact_broker::main::environments::list::list_environments;
    use crate::cli::pact_broker::main::subcommands::add_list_environments_subcommand;
    use pact_consumer::prelude::*;
    use pact_models::PactSpecification;

    #[test]
    fn list_environments_test() {
        // arrange - set up the pact mock server (as v2 for compatibility with pact-ruby)
        let config = MockServerConfig {
            pact_specification: PactSpecification::V2,
            ..MockServerConfig::default()
        };

        let response_body = json_pattern!({
            "_embedded": {
                "environments": each_like!({
                    "uuid": "78e85fb2-9df1-48da-817e-c9bea6294e01",
                    "name": "test",
                    "displayName": "Test",
                    "production": false,
                    "contacts": [{
                        "name": "Foo team",
                        "details": {
                            "emailAddress": "foo@bar.com"
                        }
                    }]
                }, min = 1)
            }
        });

        let pact_broker_service = PactBuilder::new("pact-broker-cli", "Pact Broker")
            .interaction(
                "a request for the index resource for list_environments_test",
                "",
                |mut i| {
                    i.given("the pb:environments relation exists in the index resource");
                    i.request
                        .get()
                        .path("/")
                        .header("Accept", "application/hal+json")
                        .header("Accept", "application/json");
                    i.response
                        .status(200)
                        .header("Content-Type", "application/hal+json;charset=utf-8")
                        .json_body(json_pattern!({
                            "_links": {
                                "pb:environments": {
                                    "href": term!("http:\\/\\/.*","http://localhost/environments"),
                                }
                            }
                        }));
                    i
                },
            )
            .interaction("a request to list the environments", "", |mut i| {
                i.given("an environment exists");
                i.request
                    .get()
                    .path("/environments")
                    .header("Accept", "application/hal+json")
                    .header("Accept", "application/json");
                i.response
                    .status(200)
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(response_body);
                i
            })
            .start_mock_server(None, Some(config));
        let mock_server_url = pact_broker_service.url();

        // arrange - set up the command line arguments
        let matches = add_list_environments_subcommand()
            .args(crate::cli::add_ssl_arguments())
            .get_matches_from(vec![
                "list-environments",
                "-b",
                mock_server_url.as_str(),
                "--output",
                "text",
            ]);

        // act
        let sut = list_environments(&matches);

        // assert
        assert!(sut.is_ok());
    }
}
