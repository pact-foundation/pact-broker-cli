use crate::cli::{
    pact_broker::main::{
        HALClient, PactBrokerError,
        utils::{get_auth, get_broker_url, get_ssl_options},
    },
    utils,
};
use serde_json::json;

pub fn update_environment(args: &clap::ArgMatches) -> Result<String, PactBrokerError> {
    let uuid = args.get_one::<String>("uuid").unwrap().to_string();
    let name = args.get_one::<String>("name");
    let display_name = args.get_one::<String>("display-name");
    let production = args.get_flag("production");
    let contact_name = args.get_one::<String>("contact-name");
    let contact_email_address = args.get_one::<String>("contact-email-address");
    let broker_url = get_broker_url(args);
    let auth = get_auth(args);
    let ssl_options = get_ssl_options(args);

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let hal_client: HALClient =
            HALClient::with_url(&broker_url, Some(auth.clone()), ssl_options.clone());

        let mut payload = json!({});
        payload["uuid"] = serde_json::Value::String(uuid);
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
            .post_json(&(broker_url + "/environments"), &payload.to_string())
            .await;

        let default_output = "text".to_string();
        let output = args.get_one::<String>("output").unwrap_or(&default_output);
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
                    println!("{}", serde_json::to_string(&res).unwrap());
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
