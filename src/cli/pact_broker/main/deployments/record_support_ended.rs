use serde_json::json;

use crate::cli::{
    pact_broker::main::{
        HALClient, PactBrokerError,
        utils::{get_auth, get_broker_url, get_ssl_options, handle_error},
    },
    utils,
};

pub fn record_support_ended(args: &clap::ArgMatches) -> Result<String, PactBrokerError> {
    // 1. Check broker index link for connection
    // <- "GET /? HTTP/1.1\r\nAccept: application/hal+json\r\nUser-Agent: Ruby\r\nHost: localhost:9292\r\n\r\n"
    // -> "HTTP/1.1 200 OK\r\n"
    // 2. Call environments and check the specified enviroment exists, get the environment link
    // <- "GET /environments? HTTP/1.1\r\nAccept: application/hal+json\r\nUser-Agent: Ruby\r\nHost: localhost:9292\r\n\r\n"
    // -> "HTTP/1.1 200 OK\r\n"
    // 3. Call the environment link and check the specified version exists, get the version link
    // <- "GET /environments/c540ce64-5493-48c5-ab7c-28dae27b166b? HTTP/1.1\r\nAccept: application/hal+json\r\nUser-Agent: Ruby\r\nHost: localhost:9292\r\n\r\n"
    // -> "HTTP/1.1 200 OK\r\n"
    // 4. Call the /environments/c540ce64-5493-48c5-ab7c-28dae27b166b/deployed-versions/currently-deployed?pacticipant=Example+App link, and check our app is currently deployed
    // <- "GET /environments/c540ce64-5493-48c5-ab7c-28dae27b166b/deployed-versions/currently-deployed?pacticipant=Example+App HTTP/1.1\r\nAccept: application/hal+json\r\nUser-Agent: Ruby\r\nHost: localhost:9292\r\n\r\n"
    // -> "HTTP/1.1 200 OK\r\n"
    // 5. perform a patch request to the /environments/c540ce64-5493-48c5-ab7c-28dae27b166b/deployed-versions/9b756f93-19a2-4ca7-ae36-c0b917ac1f21 link to set currentlyDeployed to false
    // <- "PATCH /deployed-versions/9b756f93-19a2-4ca7-ae36-c0b917ac1f21 HTTP/1.1\r\nAccept: application/hal+json\r\nUser-Agent: Ruby\r\nContent-Type: application/merge-patch+json\r\nHost: localhost:9292\r\nContent-Length: 27\r\n\r\n"
    // <- "{\"currentlyDeployed\":false}"

    let pacticipant = args.get_one::<String>("pacticipant");
    let environment = args.get_one::<String>("environment");
    let broker_url = get_broker_url(args);
    let auth = get_auth(args);
    let ssl_options = get_ssl_options(args);

    tokio::runtime::Runtime::new().unwrap().block_on(async {
                let hal_client: HALClient = HALClient::with_url(&broker_url, Some(auth.clone()),ssl_options.clone());


                let res = hal_client.clone()
                    .fetch(&(broker_url.clone() + "/"))
                    .await;
                match res {
                    Ok(_) => {
                    }
                    Err(err) => {
                        handle_error(err);
                    }
                }

                #[derive(Debug, serde::Deserialize)]
                struct Environment {
                    uuid: String,
                    name: String,
                    }

                let res = hal_client.clone()
                    .fetch(&(broker_url.clone() + "/environments?"))
                    .await;
                    match res {
                        Ok(response) => {
                            let environments: Vec<Environment> = response["_embedded"]["environments"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .map(|env| serde_json::from_value(env.clone()).unwrap())
                                .collect();
                            let environment_exists = environments.iter().any(|env| env.name == environment.clone().unwrap().to_string());
                            if environment_exists {
                                let environment_uuid = &environments.iter().find(|env| env.name == environment.clone().unwrap().to_string()).unwrap().uuid;
                                // Use environment_uuid in step 3

                                // 3. Call the environment link and check the specified version exists, get the version link
                                let res = hal_client.clone()
                                .fetch(&(broker_url.clone() + "/environments/" + &environment_uuid + "?"))
                                .await;
                            match res {
                                Ok(result) => {
                                    // print!("✅ Environment found");
                                    // print!("🧹 Undeploying {} from {} environment", pacticipant.unwrap(), environment.unwrap());
                                    // print!("Result JSON: {:#?}", result);
                                    // todo - handle application instance

                                    let currently_supported_released_link = result["_links"]["pb:currently-supported-released-versions"]["href"].as_str().unwrap();
                                    let pacticipant_query = format!("?pacticipant={}", urlencoding::encode(pacticipant.unwrap()));

                                    let res = hal_client.clone()
                                        .fetch(&(currently_supported_released_link.to_owned() + &pacticipant_query))
                                        .await;
                                    match res {
                                        Ok(result) => {
                                            // Handle success
                                            // print!("🧹 Found currently deployed versions");
                                            // print!("Result JSON: {:#?}", result);
                                            if let Some(embedded) = result["_embedded"].as_object() {
                                                if let Some(released_versions) = embedded["releasedVersions"].as_array() {
                                                    if released_versions.len() == 0 {
                                                        let message = format!("❌ No currently released versions found for {} in {} environment", pacticipant.unwrap(), environment.unwrap());
                                                        println!("{}", message.clone());
                                                        return Err(PactBrokerError::NotFound(message));
                                                    }
                                                    for released_version in released_versions {
                                                        let pacticipant_name = released_version["_embedded"]["pacticipant"]["name"].as_str().unwrap();
                                                        if pacticipant_name == pacticipant.unwrap() {
                                                            let self_href = released_version["_links"]["self"]["href"].as_str().unwrap();
                                                            // Send a patch request with the user's payload to selfHref
                                                            // print!("🧹 Undeploying {} from {} environment", pacticipant.unwrap(), environment.unwrap());
                                                            // print!("🧹 Sending a patch request to {}", self_href);
                                                            let mut payload = json!({});
                                                            payload["currentlySupported"] = serde_json::Value::Bool(false);
                                                            // let pacticipant_query = format!("?pacticipant={}", urlencoding::encode(pacticipant.unwrap()));
                                                            let res = hal_client.clone().patch_json(self_href, &payload.to_string(),None).await;
                                                            match res {
                                                                Ok(_value) => {
                                                                    // Handle success
                                                                    print!("✅ ♻️ Recorded support ended {} from {} environment", utils::GREEN.apply_to(pacticipant.unwrap()), utils::GREEN.apply_to(environment.unwrap()));
                                                                }
                                                                Err(err) => {
                                                                    handle_error(err);
                                                                }
                                                            }
                                                        } else {
                                                            let message = format!("❌ No currently released versions found for {} in {} environment", pacticipant.unwrap(), environment.unwrap());
                                                            println!("{}", utils::RED.apply_to(message.clone()));
                                                            return Err(PactBrokerError::NotFound(message));
                                                        }
                                                    }
                                                    return Ok("".to_string());
                                                } else {
                                                    let message = format!("❌ No currently released versions found for {} in {} environment", pacticipant.unwrap(), environment.unwrap());  
                                                    println!("{}", utils::RED.apply_to(message.clone()));
                                                    return Err(PactBrokerError::NotFound(message));
                                                }
                                                }
                                            else {
                                                let message = format!("❌ Could not process hal relation link");
                                                println!("{}", utils::RED.apply_to(message.clone()));
                                                return Err(PactBrokerError::NotFound(message));
                                            }
                                        }
                                        Err(err) => {
                                            handle_error(err);
                                        }
                                    }
                                }
                                Err(err) => {
                                    handle_error(err);
                                }
                            }
                            } else {
                                let message = format!("❌ Environment {} not found", environment.unwrap());
                                println!("{}", message.clone());
                                return Err(PactBrokerError::NotFound(message));
                            }
                        }
                        Err(err) => {
                            handle_error(err);
                            }
                        }
                    Ok("Support ended recorded successfully".to_string())
            })
}
