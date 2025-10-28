use crate::cli::{
    pact_broker::main::{
        HALClient, PactBrokerError,
        utils::{get_auth, get_broker_url, get_custom_headers, get_ssl_options},
    },
    utils,
};

pub fn describe_environment(args: &clap::ArgMatches) -> Result<String, PactBrokerError> {
    let uuid = args.get_one::<String>("uuid").unwrap().to_string();
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
        let res = hal_client
            .fetch(&(broker_url + "/environments/" + &uuid))
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
                    let res_uuid = res["uuid"].to_string();
                    let res_name = res["name"].to_string();
                    let res_display_name = res["displayName"].to_string();
                    let res_production = res["production"].to_string();
                    let res_created_at = res["createdAt"].to_string();
                    let res_contacts = res["contacts"].as_array();

                    println!("✅");
                    println!("UUID {}", utils::GREEN.apply_to(res_uuid.trim_matches('"')));
                    println!(
                        "Name: {}",
                        utils::GREEN.apply_to(res_name.trim_matches('"'))
                    );
                    println!(
                        "Display Name: {}",
                        utils::GREEN.apply_to(res_display_name.trim_matches('"'))
                    );
                    println!(
                        "Production: {}",
                        utils::GREEN.apply_to(res_production.trim_matches('"'))
                    );
                    println!(
                        "Created At: {}",
                        utils::GREEN.apply_to(res_created_at.trim_matches('"'))
                    );
                    if let Some(contacts) = res_contacts {
                        println!("Contacts:");
                        for contact in contacts {
                            println!(" - Contact:");
                            if let Some(name) = contact["name"].as_str() {
                                println!("  - Name: {}", name);
                            }
                            if let Some(email) = contact["email"].as_str() {
                                println!("  - Email: {}", email);
                            }
                        }
                    }
                }

                Ok("".to_string())
            }
            Err(err) => Err(err),
        }
    })
}

#[cfg(test)]
mod describe_environment_tests {
    use super::describe_environment;
    use pact_consumer::prelude::*;
    use pact_models::PactSpecification;

    fn build_matches(broker_url: &str, uuid: &str, output: &str) -> clap::ArgMatches {
        let args = vec![
            "describe-environment",
            "-b",
            broker_url,
            "--uuid",
            uuid,
            "--output",
            output,
        ];
        crate::cli::pact_broker::main::subcommands::add_describe_environment_subcommand()
            .get_matches_from(args)
    }

    #[test]
    fn describe_environment_success_text_output() {
        let uuid = "16926ef3-590f-4e3f-838e-719717aa88c9";
        let config = MockServerConfig {
            pact_specification: PactSpecification::V2,
            ..MockServerConfig::default()
        };

        let pact_broker_service = PactBuilder::new("pact-broker-cli", "Pact Broker")
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
                        "uuid": like!(uuid),
                        "name": like!("existing name"),
                        "displayName": like!("existing display name"),
                        "production": like!(true),
                        "createdAt": like!("2024-06-01T12:00:00Z"),
                    }));
                i
            })
            .start_mock_server(None, Some(config));
        let mock_server_url = pact_broker_service.url();

        let matches = build_matches(mock_server_url.as_str(), uuid, "text");

        let result = describe_environment(&matches);
        assert!(result.is_ok());
    }

    #[test]
    fn describe_environment_success_json_output() {
        let uuid = "16926ef3-590f-4e3f-838e-719717aa88c9";
        let config = MockServerConfig {
            pact_specification: PactSpecification::V2,
            ..MockServerConfig::default()
        };

        let pact_broker_service = PactBuilder::new("pact-broker-cli", "Pact Broker")
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
                        "uuid": like!(uuid),
                        "name": like!("existing name"),
                        "displayName": like!("existing display name"),
                        "production": like!(true),
                        "createdAt": like!("2024-06-01T12:00:00Z"),
                    }));
                i
            })
            .start_mock_server(None, Some(config));
        let mock_server_url = pact_broker_service.url();

        let matches = build_matches(mock_server_url.as_str(), uuid, "json");

        let result = describe_environment(&matches);
        assert!(result.is_ok());
    }

    #[test]
    fn describe_environment_not_found() {
        let uuid = "non-existent-uuid";
        let config = MockServerConfig {
            pact_specification: PactSpecification::V2,
            ..MockServerConfig::default()
        };

        let pact_broker_service = PactBuilder::new("pact-broker-cli", "Pact Broker")
            .interaction("get environment not found", "", |mut i| {
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

        let matches = build_matches(mock_server_url.as_str(), uuid, "text");

        let result = describe_environment(&matches);
        assert!(result.is_err());
    }
}
