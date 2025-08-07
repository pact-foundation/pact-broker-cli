use crate::{
    cli::pact_broker::main::{
        types::{BrokerDetails, OutputType},
        utils::{follow_broker_relation, generate_table, get_broker_relation},
    },
    pact_broker::main::{HALClient, PactBrokerError},
};

pub fn list_latest_pact_versions(
    broker_details: &BrokerDetails,
    output_type: OutputType,
    _verbose: bool,
) -> Result<String, PactBrokerError> {
    // setup client with broker url and credentials
    let broker_url = &broker_details.url;
    let auth = &broker_details.auth;
    let ssl_options = &broker_details.ssl_options;

    let res = tokio::runtime::Runtime::new().unwrap().block_on(async {
        // query pact broker index and get hal relation link
        let hal_client: HALClient =
            HALClient::with_url(&broker_url, auth.clone(), ssl_options.clone());
        let pb_latest_pact_versions_href_path = get_broker_relation(
            hal_client.clone(),
            "pb:latest-pact-versions".to_string(),
            broker_url.to_string(),
        )
        .await;

        match pb_latest_pact_versions_href_path {
            Ok(_) => {}
            Err(err) => {
                return Err(err);
            }
        }

        // query the hal relation link to get the latest pact versions
        let res = follow_broker_relation(
            hal_client.clone(),
            "pb:latest-pact-versions".to_string(),
            pb_latest_pact_versions_href_path.unwrap(),
        )
        .await;
        match res {
            Ok(result) => match output_type {
                OutputType::Json => {
                    let json: String = serde_json::to_string(&result).unwrap();
                    println!("{}", json);
                    return Ok(json);
                }
                OutputType::Table => {
                    let table = generate_table(
                        &result,
                        vec!["CONSUMER", "CONSUMER_VERSION", "PROVIDER", "CREATED_AT"],
                        vec![
                            vec!["_embedded", "consumer", "name"],
                            vec!["_embedded", "consumer", "_embedded", "version", "number"],
                            vec!["_embedded", "provider", "name"],
                            vec!["createdAt"],
                        ],
                    );
                    println!("{table}");
                    return Ok(table.to_string());
                }

                OutputType::Text => {
                    let text = result.to_string();
                    println!("{:?}", text);
                    return Ok(text);
                }
                OutputType::Pretty => {
                    let json: String = serde_json::to_string(&result).unwrap();
                    println!("{}", json);
                    return Ok(json);
                }
            },
            Err(err) => Err(err),
        }
    });
    match res {
        Ok(result) => {
            return Ok(result);
        }
        Err(err) => {
            return Err(err);
        }
    }
}

#[cfg(test)]
mod lists_latest_pact_versions_tests {
    use crate::cli::pact_broker::main::pacts::list_latest_pact_versions::list_latest_pact_versions;
    use crate::cli::pact_broker::main::types::{BrokerDetails, OutputType, SslOptions};
    use pact_consumer::prelude::*;
    use pact_models::PactSpecification;
    use serde_json::json;

    #[test]
    fn lists_latest_pact_versions_test() {
        // arrange - set up the pact mock server
        let config = MockServerConfig {
            pact_specification: PactSpecification::V2,
            ..MockServerConfig::default()
        };

        let body = json!(
                            {
            "_links": {
                "self": {
                    "href": "http://example.org/pacts/latest"
                }
            },
            "pacts": [
                {
                    "_links": {
                        "self": [{
                            "href": "http://example.org/pacts/provider/Pricing%20Service/consumer/Condor/latest"
                        },{
                            "href": "http://example.org/pacts/provider/Pricing%20Service/consumer/Condor/version/1.3.0"
                        }]
                    },
                    "_embedded": {
                        "consumer": {
                            "name": "Condor",
                            "_links": {
                                "self": {
                                    "href": "http://example.org/pacticipants/Condor"
                                }
                            },
                            "_embedded": {
                                "version": {
                                    "number": "1.3.0"
                                }
                            }
                        },
                        "provider": {
                            "_links": {
                                "self": {
                                    "href": "http://example.org/pacticipants/Pricing%20Service"
                                }
                            },
                            "name": "Pricing Service"
                        }
                    }
                }
            ]
        }
                        );

        let pact_broker_service = PactBuilder::new("pact_broker_cli", "Pact Broker")
            .interaction("a request for the index resource", "", |mut i| {
                i.given("the pb:latest-pact-versions relation exists in the index resource");
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
                            "pb:latest-pact-versions": {
                                "href": term!("http:\\/\\/.*","http://localhost/pacts/latest"),
                            }
                        }
                    }));
                i
            })
            .interaction("a request to list the latest pacts", "", |mut i| {
                i.given("a pact between Condor and the Pricing Service exists");
                i.request.get().path("/pacts/latest");
                i.response
                    .status(200)
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(body.clone());
                i
            })
            .start_mock_server(None, Some(config));
        let mock_server_url = pact_broker_service.url();

        // arrange - set up the broker details
        let broker_details = BrokerDetails {
            url: mock_server_url.to_string(),
            auth: None,
            ssl_options: SslOptions::default(),
        };

        // act
        let result = list_latest_pact_versions(&broker_details, OutputType::Json, false);

        // assert
        assert!(result.is_ok());
        let output = result.unwrap();
        let output_json: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(output_json, body)
    }
}
