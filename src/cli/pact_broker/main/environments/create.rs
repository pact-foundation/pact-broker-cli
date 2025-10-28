use serde_json::json;

use crate::cli::{
    pact_broker::main::{
        HALClient, PactBrokerError,
        utils::{
            get_auth, get_broker_relation, get_broker_url, get_custom_headers, get_ssl_options,
        },
    },
    utils,
};

pub fn create_environment(args: &clap::ArgMatches) -> Result<String, PactBrokerError> {
    let name = args.get_one::<String>("name");
    let display_name = args.get_one::<String>("display-name");
    let production = args.get_flag("production");
    let contact_name = args.get_one::<String>("contact-name");
    let contact_email_address = args.get_one::<String>("contact-email-address");
    let broker_url = get_broker_url(args).trim_end_matches('/').to_string();
    let auth = get_auth(args);
    let custom_headers = get_custom_headers(args);
    let ssl_options = get_ssl_options(args);

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let hal_client: HALClient = HALClient::with_url(
            &broker_url,
            Some(auth.clone()),
            ssl_options.clone(),
            custom_headers.clone(),
        );
        let pb_environments_href_path = get_broker_relation(
            hal_client.clone(),
            "pb:environments".to_string(),
            broker_url.to_string(),
        )
        .await?;

        let mut payload = json!({});
        payload["production"] = serde_json::Value::Bool(production);
        if let Some(name) = name {
            payload["name"] = serde_json::Value::String(name.to_string());
        } else {
            let message = "Environment name is required";
            println!("❌ {}", utils::RED.apply_to(message));
            return Err(PactBrokerError::ValidationError(vec![message.to_string()]));
        }
        if let Some(contact_name) = contact_name {
            payload["contacts"] = serde_json::Value::Array(vec![{
                let mut map = serde_json::Map::new();
                map.insert(
                    "name".to_string(),
                    serde_json::Value::String(contact_name.to_string()),
                );
                serde_json::Value::Object(map)
            }]);
        }
        if let Some(display_name) = display_name {
            payload["displayName"] = serde_json::Value::String(display_name.to_string());
        }
        if let Some(contact_email_address) = contact_email_address {
            if payload["contacts"].is_array() {
                let contacts = payload["contacts"].as_array_mut().unwrap();
                let contact = contacts.get_mut(0).unwrap();
                let contact_map = contact.as_object_mut().unwrap();
                let details = contact_map
                    .entry("details")
                    .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
                if let serde_json::Value::Object(details_map) = details {
                    details_map.insert(
                        "emailAddress".to_string(),
                        serde_json::Value::String(contact_email_address.to_string()),
                    );
                }
            } else {
                payload["contacts"] = serde_json::Value::Array(vec![{
                    let mut map = serde_json::Map::new();
                    let mut details_map = serde_json::Map::new();
                    details_map.insert(
                        "emailAddress".to_string(),
                        serde_json::Value::String(contact_email_address.to_string()),
                    );
                    map.insert(
                        "details".to_string(),
                        serde_json::Value::Object(details_map),
                    );
                    serde_json::Value::Object(map)
                }]);
            }
        }

        let res = hal_client
            .post_json(
                &pb_environments_href_path,
                &json!(payload).to_string(),
                None,
            )
            .await;

        let default_output: String = "text".to_string();
        let output: &String = args.get_one::<String>("output").unwrap_or(&default_output);
        match res {
            Ok(res) => {
                if output == "pretty" {
                    let json = serde_json::to_string_pretty(&res).unwrap();
                    println!("{}", json);
                } else if output == "json" {
                    println!("{}", serde_json::to_string(&res).unwrap());
                } else if output == "id" {
                    println!("{}", res["uuid"].to_string().trim_matches('"'));
                } else {
                    let uuid = res["uuid"].to_string();
                    println!(
                        "✅ Created {} environment in the Pact Broker with UUID {}",
                        utils::GREEN.apply_to(name.unwrap()),
                        utils::GREEN.apply_to(uuid.trim_matches('"'))
                    );
                }
                Ok(format!("Successfully created environment"))
            }
            Err(err) => Err(err),
        }
    })
}

#[cfg(test)]
mod create_environment_tests {
    use crate::cli::pact_broker::main::subcommands::add_create_environment_subcommand;
    use crate::cli::pact_broker::main::{environments::create::create_environment, test_utils};
    use pact_consumer::prelude::*;
    use pact_models::PactSpecification;
    use serde_json::json;

    #[test]
    fn create_environment_test() {
        // arrange - set up the pact mock server (as v2 for compatibility with pact-ruby)
        let config = MockServerConfig {
            pact_specification: PactSpecification::V2,
            ..MockServerConfig::default()
        };

        let name = "test";
        let display_name = "Test";
        let production = false;
        let contact_name = "Foo team";
        let contact_email_address = "foo@bar.com";
        let request_body = json!({
                "name": name,
                "displayName": display_name,
                "production": production,
                "contacts": [{
                    "name": contact_name,
                    "details": {
                    "emailAddress": contact_email_address
                    }
                }],
        });

        let mut response_body = json_pattern!({
            "uuid": like!("ffe683ef-dcd7-4e4f-877d-f6eb3db8e86e")
        });
        test_utils::merge_json_objects(&mut response_body, &request_body);

        let pact_broker_service = PactBuilder::new("pact-broker-cli", "Pact Broker")
            .interaction(
                "a request for the index resource for create_environment_test",
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
                                "href": term!("http:\\/\\/.*","http://localhost/environments"),
                            }
                          }
                        }));
                    i
                },
            )
            .interaction("a request to create an environment", "", |mut i| {
                i.request
                    .post()
                    .path("/environments")
                    .header("Accept", "application/hal+json")
                    .header("Content-Type", "application/json")
                    .json_body(request_body);

                i.response
                    .status(201)
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(response_body);
                i
            })
            .start_mock_server(None, Some(config));
        let mock_server_url = pact_broker_service.url();
        // arrange - set up the command line arguments
        let matches = add_create_environment_subcommand().get_matches_from(vec![
            "create-environment",
            "-b",
            mock_server_url.as_str(),
            "--name",
            name,
            "--display-name",
            display_name,
            "--contact-name",
            contact_name,
            "--contact-email-address",
            contact_email_address,
        ]);
        // act
        let sut = create_environment(&matches);

        // assert
        assert!(sut.is_ok());
        assert_eq!(sut.unwrap(), format!("Successfully created environment",));
    }
}
