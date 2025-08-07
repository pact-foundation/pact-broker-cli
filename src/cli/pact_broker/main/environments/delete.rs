use crate::cli::{
    pact_broker::main::{
        HALClient, PactBrokerError,
        utils::{get_auth, get_broker_url, get_ssl_options},
    },
    utils,
};

pub fn delete_environment(args: &clap::ArgMatches) -> Result<String, PactBrokerError> {
    let uuid = args.get_one::<String>("uuid").unwrap().to_string();
    let broker_url = get_broker_url(args);
    let auth = get_auth(args);
    let ssl_options = get_ssl_options(args);
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let hal_client: HALClient =
            HALClient::with_url(&broker_url, Some(auth.clone()), ssl_options.clone());
        let res = hal_client
            .clone()
            .fetch(&(broker_url.clone() + "/environments/" + &uuid))
            .await;
        match res {
            Ok(_) => {
                let name = res.clone().unwrap()["name"].to_string();
                let res = hal_client
                    .clone()
                    .delete(&(broker_url.clone() + "/environments/" + &uuid))
                    .await;
                match res {
                    Ok(_) => {
                        let message = format!(
                            "✅ Environment {} with UUID {} deleted successfully",
                            utils::GREEN.apply_to(name.trim_matches('"')),
                            utils::GREEN.apply_to(uuid.trim_matches('"'))
                        );
                        println!("{}", message);
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
