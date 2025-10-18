use crate::cli::{
    pact_broker::main::{
        HALClient, PactBrokerError,
        utils::{get_auth, get_broker_url, get_ssl_options},
    },
    utils,
};
use serde_json::json;

pub fn update_environment(args: &clap::ArgMatches) -> Result<String, PactBrokerError> {
    let default_output = "text".to_string();
    let output = args.get_one::<String>("output").unwrap_or(&default_output);
    let uuid = args.get_one::<String>("uuid").unwrap().to_string();
    let name = args.get_one::<String>("name");
    let display_name = args.get_one::<String>("display-name");
    let production = args.get_flag("production");
    let contact_name = args.get_one::<String>("contact-name");
    let contact_email_address = args.get_one::<String>("contact-email-address");
    let broker_url = get_broker_url(args).trim_end_matches('/').to_string();
    let auth = get_auth(args);
    let ssl_options = get_ssl_options(args);
    let environments_href = format!("{}/environments/{}", broker_url, uuid.clone());
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let hal_client: HALClient =
            HALClient::with_url(&broker_url, Some(auth.clone()), ssl_options.clone());

        // check if the uuid url exists, if not return an error, otherwise continue

        let get_uuid_result = hal_client.clone().fetch(&environments_href);

        match get_uuid_result.await {
            Ok(_) => {}
            Err(err) => {
                return Err(match err {
                    PactBrokerError::NotFound(_error) => {
                        let message = format!("Environment with UUID {} not found", uuid);
                        println!("❌ {}", utils::RED.apply_to(message.clone()));
                        if output == "json" {
                            return Err(PactBrokerError::NotFound("{}".to_string()));
                        }
                        PactBrokerError::NotFound(message)
                    }
                    other => {
                        println!("❌ {}", utils::RED.apply_to(other.to_string()));
                        other
                    }
                });
            }
        }

        let mut payload = json!({});
        // payload["uuid"] = serde_json::Value::String(uuid.clone());
        payload["production"] = serde_json::Value::Bool(production);
        if let Some(name) = name {
            payload["name"] = serde_json::Value::String(name.to_string());
        } else {
            let message = "❌ Name is required".to_string();
            println!("{}", message.clone());
            return Err(PactBrokerError::ValidationError(vec![message]));
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
                contact_map.insert(
                    "email".to_string(),
                    serde_json::Value::String(contact_email_address.to_string()),
                );
            } else {
                payload["contacts"] = serde_json::Value::Array(vec![{
                    let mut map = serde_json::Map::new();
                    map.insert(
                        "email".to_string(),
                        serde_json::Value::String(contact_email_address.to_string()),
                    );
                    serde_json::Value::Object(map)
                }]);
            }
        }
        let res = hal_client
            .put_json(&(environments_href), &payload.to_string(), None)
            .await;

        let columns = vec![
            "ID",
            "NAME",
            "DISPLAY NAME",
            "PRODUCTION",
            "CONTACT NAME",
            "CONTACT EMAIL ADDRESS",
        ];
        let names = vec![
            vec!["id"],
            vec!["name"],
            vec!["displayName"],
            vec!["production"],
            vec!["contactName"],
            vec!["contactEmailAddress"],
        ];
        match res {
            Ok(res) => {
                let uuid: String = res["uuid"].to_string();
                let message = format!(
                    "✅ Updated {} environment in the Pact Broker with UUID {}",
                    utils::GREEN.apply_to(name.unwrap()),
                    utils::GREEN.apply_to(uuid.trim_matches('"'))
                );
                if output == "pretty" {
                    let json = serde_json::to_string_pretty(&res).unwrap();
                    println!("{}", json);
                } else if output == "json" {
                    let json = serde_json::to_string(&res).unwrap();
                    println!("{}", json);
                    return Ok(json);
                } else if output == "id" {
                    println!("{}", res["uuid"].to_string().trim_matches('"'));
                } else if output == "table" {
                    let table =
                        crate::cli::pact_broker::main::utils::generate_table(&res, columns, names);
                    println!("{table}");
                } else {
                    println!("{}", message);
                }
                Ok(message)
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
mod update_environment_tests {
    use crate::cli::pact_broker::main::environments::update::update_environment;
    use crate::cli::pact_broker::main::subcommands::add_update_environment_subcommand;
    use crate::cli::utils;
    use pact_consumer::prelude::*;
    use pact_models::PactSpecification;

    fn build_matches(
        broker_url: &str,
        uuid: &str,
        name: &str,
        display_name: &str,
        production: bool,
        output: &str,
    ) -> clap::ArgMatches {
        let mut args = vec![
            "update-environment",
            "-b",
            broker_url,
            "--uuid",
            uuid,
            "--name",
            name,
            "--display-name",
            display_name,
            "--output",
            output,
        ];
        if production {
            args.push("--production");
        }
        add_update_environment_subcommand().get_matches_from(args)
    }

    #[test]
    fn update_environment_success_text_output() {
        let uuid = "16926ef3-590f-4e3f-838e-719717aa88c9";
        let config = MockServerConfig {
            pact_specification: PactSpecification::V2,
            ..MockServerConfig::default()
        };

        let response_body = json_pattern!({
            "uuid": uuid,
            "name": like!("new name"),
            "displayName": like!("new display name"),
            "production": like!(false),
            "updatedAt": like!("2021-05-28T13:34:54+10:00")
        });

        let pact_broker_service = PactBuilder::new("pact-broker-cli", "Pact Broker")
            // .interaction("get index", "", |mut i| {
            //     i.request.get().path("/").header("Accept", "application/hal+json");
            //     i.response.status(200).header("Content-Type", "application/hal+json;charset=utf-8")
            //         .json_body(json_pattern!({
            //             "_links": {
            //                 "pb:environments": {},
            //                 "pb:environment": {
            //                     "href": term!("http:\\/\\/.*","http://localhost/environments/{uuid}")
            //                 }
            //             }
            //         }));
            //     i
            // })
            .interaction("get environment", "", |mut i| {
                i.given(format!(
                    "an environment with name test and UUID {} exists",
                    uuid
                ));
                i.request.get().path(format!("/environments/{}", uuid));
                i.response
                    .status(200)
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(json_pattern!({
                        "name": like!("existing name"),
                        "displayName": like!("existing display name"),
                        "production": like!(true)
                    }));
                i
            })
            .interaction("put environment", "", |mut i| {
                i.given(format!(
                    "an environment with name test and UUID {} exists",
                    uuid
                ));
                i.request
                    .put()
                    .path(format!("/environments/{}", uuid))
                    .header("Content-Type", "application/json")
                    .json_body(json_pattern!({
                        "name": like!("new%20name"),
                        "displayName": like!("new display name"),
                        "production": like!(false)
                    }));
                i.response
                    .status(200)
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(response_body);
                i
            })
            .start_mock_server(None, Some(config));
        let mock_server_url = pact_broker_service.url();

        let matches = build_matches(
            mock_server_url.as_str(),
            uuid,
            "new name",
            "new display name",
            false,
            "text",
        );

        let result = update_environment(&matches);
        assert!(result.is_ok());
        let msg = result.unwrap();
        assert!(msg.contains(&format!(
            "Updated {} environment in the Pact Broker",
            utils::GREEN.apply_to("new name")
        )));
    }

    #[test]
    fn update_environment_success_json_output() {
        let uuid = "16926ef3-590f-4e3f-838e-719717aa88c9";
        let config = MockServerConfig {
            pact_specification: PactSpecification::V2,
            ..MockServerConfig::default()
        };

        let response_body = json_pattern!({
            "uuid": like!(uuid),
            "name": like!("new name"),
            "displayName": like!("new display name"),
            "production": like!(false),
            "updatedAt": like!("2021-05-28T13:34:54+10:00")
        });

        let pact_broker_service = PactBuilder::new("pact-broker-cli", "Pact Broker")
            // .interaction("get index", "", |mut i| {
            //     i.request.get().path("/").header("Accept", "application/hal+json");
            //     i.response.status(200).header("Content-Type", "application/hal+json;charset=utf-8")
            //         .json_body(json_pattern!({
            //             "_links": {
            //                 "pb:environments": {},
            //                 "pb:environment": {
            //                     "href": term!("http:\\/\\/.*","http://localhost/environments/{uuid}")
            //                 }
            //             }
            //         }));
            //     i
            // })
            .interaction("get environment", "", |mut i| {
                i.request.get().path(format!("/environments/{}", uuid));
                i.given(format!(
                    "an environment with name test and UUID {} exists",
                    uuid
                ));
                i.response
                    .status(200)
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(json_pattern!({
                        "name": like!("existing%20name"),
                        "displayName": like!("existing display name"),
                        "production": like!(true)
                    }));
                i
            })
            .interaction("put environment", "", |mut i| {
                i.given(format!(
                    "an environment with name test and UUID {} exists",
                    uuid
                ));
                i.request
                    .put()
                    .path(format!("/environments/{}", uuid))
                    .header("Content-Type", "application/json")
                    .json_body(json_pattern!({
                        "name": like!("new%20name"),
                        "displayName": like!("new display name"),
                        "production": like!(false)
                    }));
                i.response
                    .status(200)
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(response_body);
                i
            })
            .start_mock_server(None, Some(config));
        let mock_server_url = pact_broker_service.url();

        let matches = build_matches(
            mock_server_url.as_str(),
            uuid,
            "new name",
            "new display name",
            false,
            "json",
        );

        let result = update_environment(&matches);
        assert!(result.is_ok());
        let msg = result.unwrap();
        println!("msg: {}", msg);
        assert!(msg.contains("updatedAt"));
    }

    #[test]
    fn update_environment_not_found() {
        let uuid = "16926ef3-590f-4e3f-838e-719717aa88c9";
        let config = MockServerConfig {
            pact_specification: PactSpecification::V2,
            ..MockServerConfig::default()
        };

        let pact_broker_service = PactBuilder::new("pact-broker-cli", "Pact Broker")
            .interaction("update_environment_not_found", "", |mut i| {
                i.request.get().path(format!("/environments/{}", uuid));
                i.response
                    .status(404)
                    .header("Content-Type", "application/json;charset=utf-8")
                    .json_body(json_pattern!({
                        "error": like!("Not found")
                    }));
                i
            })
            .start_mock_server(None, Some(config));
        let mock_server_url = pact_broker_service.url();

        let matches = build_matches(
            mock_server_url.as_str(),
            uuid,
            "new name",
            "new display name",
            false,
            "json",
        );

        let result = update_environment(&matches);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("{}"));
    }

    #[test]
    #[ignore]
    fn update_environment_unsuccessful() {
        let uuid = "16926ef3-590f-4e3f-838e-719717aa88c9";
        let config = MockServerConfig {
            pact_specification: PactSpecification::V2,
            ..MockServerConfig::default()
        };

        let error_body = json_pattern!({
            "error": like!({"message": "error" })
        });

        let pact_broker_service = PactBuilder::new("pact-broker-cli", "Pact Broker")
            // .interaction("get index", "", |mut i| {
            //     i.request.get().path("/").header("Accept", "application/hal+json");
            //     i.response.status(200).header("Content-Type", "application/hal+json;charset=utf-8")
            //         .json_body(json_pattern!({
            //             "_links": {
            //                 "pb:environments": {},
            //                 "pb:environment": {
            //                     "href": term!("http:\\/\\/.*","http://localhost/environments/{uuid}")
            //                 }
            //             }
            //         }));
            //     i
            // })
            .interaction("get environment", "", |mut i| {
                i.given(format!(
                    "an environment with name test and UUID {} exists",
                    uuid
                ));
                i.request.get().path(format!("/environments/{}", uuid));
                i.response
                    .status(200)
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(json_pattern!({
                        "name": like!("existing name"),
                        "displayName": like!("existing display name"),
                        "production": like!(true)
                    }));
                i
            })
            .interaction("put environment responds with error", "", |mut i| {
                i.pending(true); // "consumer side testing only - does not require provider verification"
                i.given(format!(
                    "an environment with name test and UUID {} exists",
                    uuid
                ));
                i.request.put().path(format!("/environments/{}", uuid));
                i.response
                    .status(400)
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(error_body);
                i
            })
            .start_mock_server(None, Some(config));
        let mock_server_url = pact_broker_service.url();

        let matches = build_matches(
            mock_server_url.as_str(),
            uuid,
            "new name",
            "new display name",
            false,
            "json",
        );

        let result = update_environment(&matches);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("error"));
    }
}
