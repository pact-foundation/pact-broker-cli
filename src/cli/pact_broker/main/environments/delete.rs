use crate::cli::{
    pact_broker::main::{
        HALClient, PactBrokerError,
        utils::{get_auth, get_broker_url, get_ssl_options},
    },
    utils,
};

pub fn delete_environment(args: &clap::ArgMatches) -> Result<String, PactBrokerError> {
    let uuid = args.get_one::<String>("uuid").unwrap().to_string();
    let broker_url = get_broker_url(args).trim_end_matches('/').to_string();
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
                    Err(err) => Err(err)
                }
            }
            Err(err) => Err(err)
        }
    })
}

#[cfg(test)]
mod delete_environment_tests {
    use crate::cli::pact_broker::main::environments::delete::delete_environment;
    use crate::cli::pact_broker::main::subcommands::add_delete_environment_subcommand;
    use pact_consumer::prelude::*;
    use pact_models::PactSpecification;

    fn build_matches(broker_url: &str, uuid: &str, output: &str) -> clap::ArgMatches {
        let args = vec!["delete-environment", "-b", broker_url, "--uuid", uuid];
        add_delete_environment_subcommand().get_matches_from(args)
    }

    #[test]
    fn delete_environment_success() {
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
                        "name": like!("existing name"),
                        "displayName": like!("existing display name"),
                        "production": like!(true)
                    }));
                i
            })
            .interaction("delete environment", "", |mut i| {
                i.given(format!(
                    "an environment with name test and UUID {} exists",
                    uuid
                ));
                i.request.delete().path(format!("/environments/{}", uuid));
                i.response.status(204);
                i
            })
            .start_mock_server(None, Some(config));
        let mock_server_url = pact_broker_service.url();

        let matches = build_matches(mock_server_url.as_str(), uuid, "text");

        let result = delete_environment(&matches);
        assert!(result.is_ok());
        let msg = result.unwrap();
        assert!(msg.contains(&format!("deleted successfully")));
    }
}
