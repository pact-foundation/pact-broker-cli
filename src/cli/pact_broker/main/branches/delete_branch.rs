use maplit::hashmap;

use crate::cli::pact_broker::main::{
    HALClient, PactBrokerError,
    utils::{
        delete_templated_broker_relation, get_auth, get_broker_relation, get_broker_url,
        get_ssl_options,
    },
};

pub fn delete_branch(args: &clap::ArgMatches) -> Result<String, PactBrokerError> {
    let broker_url = get_broker_url(args);
    let auth = get_auth(args);
    let ssl_options = get_ssl_options(args);

    let branch_name = args.get_one::<String>("branch").unwrap();
    let pacticipant_name = args.get_one::<String>("pacticipant").unwrap();
    let error_when_not_found = args
        .try_get_one::<bool>("error-when-not-found")
        .unwrap_or(Some(&true))
        .copied()
        .unwrap_or(true);
    let _verbose = args.get_flag("verbose");

    let res = tokio::runtime::Runtime::new().unwrap().block_on(async {
        let hal_client: HALClient =
            HALClient::with_url(&broker_url, Some(auth.clone()), ssl_options.clone());
        let pb_branch_href_path = get_broker_relation(
            hal_client.clone(),
            "pb:pacticipant-branch".to_string(),
            broker_url.to_string(),
        )
        .await?;

        println!("pb_branch_href_path: {}", pb_branch_href_path);

        let template_values = hashmap! {
            "pacticipant".to_string() => pacticipant_name.to_string(),
            "branch".to_string() => branch_name.to_string()
        };

        delete_templated_broker_relation(
            hal_client.clone(),
            "pb:pacticipant-branch".to_string(),
            pb_branch_href_path,
            template_values,
        )
        .await
    });

    match res {
        Ok(_) => {
            let message = format!(
                "Successfully deleted branch '{}' of pacticipant '{}'",
                branch_name, pacticipant_name
            );
            println!("{}", message);
            Ok(message)
        }
        Err(PactBrokerError::NotFound(_)) => {
            let message = format!(
                "Could not delete branch '{}' of pacticipant '{}' as it was not found",
                branch_name, pacticipant_name
            );
            if error_when_not_found {
                Err(PactBrokerError::NotFound(message.clone()))
            } else {
                println!("{}", message);
                Ok(message)
            }
        }
        Err(err) => Err(err),
    }
}

#[cfg(test)]
mod delete_branch_tests {
    use crate::cli::pact_broker::main::branches::delete_branch::delete_branch;
    use crate::cli::pact_broker::main::subcommands::add_delete_branch_subcommand;
    use pact_consumer::prelude::*;
    use pact_models::prelude::Generator;
    use pact_models::{PactSpecification, generators, pact};
    use serde_json::json;

    #[test]
    fn delete_branch_test() {
        // arrange - set up the pact mock server (as v2 for compatibility with pact-ruby)
        let config = MockServerConfig {
            pact_specification: PactSpecification::V2,
            ..MockServerConfig::default()
        };
        let pacticipant = "Foo";
        let branch = "main";
        let pact_broker_service = PactBuilder::new("pact-broker-cli", "Pact Broker")
            .interaction("a request for the index resource", "", |mut i| {
                i.given("the pb:pacticipant-branch relation exists in the index resource");
                i.request
                    .path("/")
                    .header("Accept", "application/hal+json")
                    .header("Accept", "application/json");

                //    let generators = generators! {
                //         "BODY" => {
                //         "$._links.pb:pacticipant-branch.href" => Generator::MockServerURL(
                //             "/pacticipants/{pacticipant}/branches/{branch})".to_string(),
                //             format!("/pacticipants/(?<pacticipant>[^/]+)/branches/(?<branch>[^/]+)")
                //         )
                //         }
                //     };
                i.response
          .header("Content-Type", "application/hal+json;charset=utf-8")
          .json_body(
            json_pattern!({
              "_links": {
                "pb:pacticipant-branch": {
                  "href": term!("http:\\/\\/.*\\{pacticipant\\}.*\\{branch\\}","http://localhost:55926/pacticipants/{pacticipant}/branches/{branch}"),
                  "title": "Get or delete a pacticipant branch",
                  "templated": true
                }
              }
            })
          );
                //   .generators().add_generators(generators);
                i
            })
            .interaction("a request to delete a branch", "", |mut i| {
                i.given(
                    format!("a branch named {} exists for pacticipant {}", branch, pacticipant),
                );
                i.request
                    .delete()
                    .path(format!("/pacticipants/{}/branches/{}", pacticipant, branch));
                i.response.status(204);
                i
            })
            .start_mock_server(None, Some(config));
        let mock_server_url = pact_broker_service.url();
        println!("Mock server started at: {}", pact_broker_service.url());
        // arrange - set up the command line arguments
        let matches = add_delete_branch_subcommand()
            .args(crate::cli::add_ssl_arguments())
            .get_matches_from(vec![
                "delete-branch",
                "-b",
                mock_server_url.as_str(),
                "--branch",
                branch,
                "--pacticipant",
                pacticipant,
            ]);
        // act
        let sut = delete_branch(&matches);

        // assert
        assert!(sut.is_ok());
        assert_eq!(
            sut.unwrap(),
            format!(
                "Successfully deleted branch '{}' of pacticipant '{}'",
                branch, pacticipant
            )
        );
    }
}
