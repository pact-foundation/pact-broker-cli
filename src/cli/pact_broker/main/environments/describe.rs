use crate::cli::{
    pact_broker::main::{
        HALClient, PactBrokerError,
        utils::{get_auth, get_broker_url, get_ssl_options},
    },
    utils,
};

pub fn describe_environment(args: &clap::ArgMatches) -> Result<String, PactBrokerError> {
    let uuid = args.get_one::<String>("uuid").unwrap().to_string();
    let broker_url = get_broker_url(args);
    let auth = get_auth(args);
    let ssl_options = get_ssl_options(args);

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let hal_client: HALClient =
            HALClient::with_url(&broker_url, Some(auth.clone()), ssl_options.clone());
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
