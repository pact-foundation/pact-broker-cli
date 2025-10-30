use comfy_table::{Table, presets::UTF8_FULL};

use crate::{
    cli::pact_broker::main::{HALClient, PactBrokerError},
    cli::pact_broker::main::{
        types::{BrokerDetails, OutputType},
        utils::{follow_broker_relation, get_broker_relation},
    },
};

pub fn list_pacticipants(
    broker_details: &BrokerDetails,
    output_type: OutputType,
) -> Result<String, PactBrokerError> {
    // setup client with broker url and credentials
    let broker_url = &broker_details.url;
    let auth = &broker_details.auth;
    let ssl_options = &broker_details.ssl_options;
    let custom_headers = &broker_details.custom_headers;

    let res = tokio::runtime::Runtime::new().unwrap().block_on(async {
        // query pact broker index and get hal relation link
        let hal_client: HALClient = HALClient::with_url(
            &broker_url,
            auth.clone(),
            ssl_options.clone(),
            custom_headers.clone(),
        );
        let pb_pacticipants_href_path = get_broker_relation(
            hal_client.clone(),
            "pb:pacticipants".to_string(),
            broker_url.to_string(),
        )
        .await;

        match pb_pacticipants_href_path {
            Ok(_) => {}
            Err(err) => {
                return Err(err);
            }
        }

        // query the hal relation link to get the latest pact versions
        let res = follow_broker_relation(
            hal_client.clone(),
            "pacticipants".to_string(),
            pb_pacticipants_href_path.unwrap(),
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
                    let names = vec![vec!["name"], vec!["displayName"]];
                    let mut table = Table::new();
                    table
                        .load_preset(UTF8_FULL)
                        .set_header(vec!["NAME", "DISPLAY NAME"]);
                    if let Some(items) = result.get("pacticipants").and_then(|v| v.as_array()) {
                        for item in items {
                            let mut values = vec![item; names.len()];

                            for (i, name) in names.iter().enumerate() {
                                for n in name.clone() {
                                    values[i] = values[i].get(n).unwrap();
                                }
                            }

                            let records: Vec<String> =
                                values.iter().map(|v| v.to_string()).collect();
                            table.add_row(records.as_slice());
                        }
                    }
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
mod list_pacticipants_tests {
    use super::*;
    use crate::cli::pact_broker::main::types::{BrokerDetails, OutputType};

    use pact_consumer::builders::InteractionBuilder;
    use pact_consumer::prelude::*;
    use pact_models::{PactSpecification, pact};

    fn setup_mock_server(interactions: Vec<InteractionBuilder>) -> Box<dyn ValidatingMockServer> {
        let config = MockServerConfig {
            pact_specification: PactSpecification::V2,
            ..MockServerConfig::default()
        };
        let mut pact_builder = PactBuilder::new("pact-broker-cli", "Pact Broker");
        for i in interactions {
            pact_builder.push_interaction(&i.build());
        }
        pact_builder.start_mock_server(None, Some(config))
    }

    #[test]
    fn list_pacticipants_json_output() {
        // Pacticipant data
        let pacticipant1 = serde_json::json!({
            "name": "Pricing Service",
            "displayName": "Pricing Service"
        });
        let pacticipant2 = serde_json::json!({
            "name": "Condor",
            "displayName": "Condor"
        });

        // Index resource with pb:pacticipants relation
        let index_interaction = |mut i: InteractionBuilder| {
            i.given("the pb:pacticipants relation exists in the index resource");
            i.request
                .path("/")
                .header("Accept", "application/hal+json")
                .header("Accept", "application/json");
            i.response
                .header("Content-Type", "application/hal+json;charset=utf-8")
                .json_body(json_pattern!({
                    "_links": {
                        "pb:pacticipants": {
                            "href": term!("http:\\/\\/.*/pacticipants", "http://localhost/pacticipants")
                        }
                    }
                }));
            i
        };

        // GET /pacticipants returns the list of pacticipants
        let get_pacticipants_interaction = |mut i: InteractionBuilder| {
            i.given(format!(
                "the '{}' and '{}' already exist in the pact-broker",
                pacticipant1["name"].as_str().unwrap(),
                pacticipant2["name"].as_str().unwrap()
            ));
            i.request
                .get()
                .path("/pacticipants")
                .header("Accept", "application/hal+json")
                .header("Accept", "application/json");
            i.response
                .status(200)
                .header("Content-Type", "application/hal+json;charset=utf-8")
                .json_body(json_pattern!({
                    "pacticipants": [
                        pacticipant2.clone(),
                        pacticipant1.clone()
                    ]
                }));
            i
        };

        let mock_server = setup_mock_server(vec![
            index_interaction(InteractionBuilder::new(
                "a request for the index resource",
                "",
            )),
            get_pacticipants_interaction(InteractionBuilder::new(
                "a request to retrieve pacticipants",
                "",
            )),
        ]);
        let mock_server_url = mock_server.url();

        let broker_details = BrokerDetails {
            url: mock_server_url.to_string(),
            auth: None,
            ssl_options: Default::default(),
            custom_headers: None,
        };

        let result = list_pacticipants(&broker_details, OutputType::Json);

        assert!(result.is_ok());
        let json: serde_json::Value = serde_json::from_str(&result.unwrap()).unwrap();
        let pacticipants = json["pacticipants"].as_array().unwrap();
        assert_eq!(pacticipants.len(), 2);
        assert_eq!(pacticipants[0]["name"], pacticipant2["name"]);
        assert_eq!(pacticipants[0]["displayName"], pacticipant2["displayName"]);
        assert_eq!(pacticipants[1]["name"], pacticipant1["name"]);
        assert_eq!(pacticipants[1]["displayName"], pacticipant1["displayName"]);
    }
}
