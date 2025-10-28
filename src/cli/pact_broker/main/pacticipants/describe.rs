use comfy_table::{Table, presets::UTF8_FULL};
use maplit::hashmap;

use crate::{
    cli::pact_broker::main::{HALClient, PactBrokerError},
    cli::pact_broker::main::{
        types::{BrokerDetails, OutputType},
        utils::{follow_templated_broker_relation, get_broker_relation},
    },
};

pub fn describe_pacticipant(
    pacticipant_name: String,
    broker_details: &BrokerDetails,
    output_type: OutputType,
) -> Result<String, PactBrokerError> {
    // setup client with broker url and credentials
    let broker_url = &broker_details.url;
    let auth = &broker_details.auth;
    let custom_headers = &broker_details.custom_headers;
    let ssl_options = &broker_details.ssl_options;

    let res = tokio::runtime::Runtime::new().unwrap().block_on(async {
        // query pact broker index and get hal relation link
        let hal_client: HALClient = HALClient::with_url(
            &broker_url,
            auth.clone(),
            ssl_options.clone(),
            custom_headers.clone(),
        );
        let pb_pacticipant_href_path = get_broker_relation(
            hal_client.clone(),
            "pb:pacticipant".to_string(),
            broker_url.to_string(),
        )
        .await;

        match pb_pacticipant_href_path {
            Ok(_) => {}
            Err(err) => {
                return Err(err);
            }
        }

        let template_values =
            hashmap! { "pacticipant".to_string() => pacticipant_name.to_string() };
        let res = follow_templated_broker_relation(
            hal_client.clone(),
            "pacticipant".to_string(),
            pb_pacticipant_href_path.unwrap(),
            template_values,
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
                    let names = vec![
                        vec!["name"],
                        vec!["displayName"],
                        vec!["mainBranch"],
                        vec!["repositoryUrl"],
                        vec!["createdAt"],
                        vec!["updatedAt"],
                    ];
                    let mut table = Table::new();
                    table.load_preset(UTF8_FULL).set_header(vec![
                        "NAME",
                        "DISPLAY NAME",
                        "MAIN BRANCH",
                        "REPO URL",
                        "CREATED",
                        "UPDATED",
                    ]);
                    let mut values = vec![&result; names.len()];

                    for (i, name) in names.iter().enumerate() {
                        let mut v = &result;
                        for n in name {
                            if let Some(next) = v.get(n) {
                                v = next;
                            } else {
                                v = &serde_json::Value::Null;
                                break;
                            }
                        }
                        values[i] = v;
                    }

                    let records: Vec<String> = values.iter().map(|v| v.to_string()).collect();
                    table.add_row(records.as_slice());
                    println!("{table}");
                    return Ok(table.to_string());
                }

                OutputType::Text => {
                    let text = result.to_string();
                    let names = vec![
                        ("Name", "name"),
                        ("Display Name", "displayName"),
                        ("Main Branch", "mainBranch"),
                        ("Repo URL", "repositoryUrl"),
                        ("Created", "createdAt"),
                        ("Updated", "updatedAt"),
                    ];

                    for (label, key) in names {
                        let value = result.get(key).and_then(|v| v.as_str()).unwrap_or("-");
                        println!("{label}: {value}");
                    }
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
mod describe_pacticipant_tests {
    use super::*;
    use crate::cli::pact_broker::main::types::{BrokerDetails, OutputType};

    use pact_consumer::builders::InteractionBuilder;
    use pact_consumer::prelude::*;
    use pact_models::PactSpecification;

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
    fn describe_pacticipant_json_output() {
        let pacticipant_name = "Foo";
        let repository_url = "http://foo";
        let created_at = "2024-01-01T00:00:00Z";
        let updated_at = "2024-01-02T00:00:00Z";
        let display_name = "Foo Service";
        let main_branch = "main";

        // Index resource with pb:pacticipant relation
        let index_interaction = |mut i: InteractionBuilder| {
            i.given("the pacticipant relations are present");
            i.request
                .path("/")
                .header("Accept", "application/hal+json")
                .header("Accept", "application/json");
            i.response
                .header("Content-Type", "application/hal+json;charset=utf-8")
                .json_body(json_pattern!({
                    "_links": {
                        "pb:pacticipant": {
                            "href": term!("http:\\/\\/.*/pacticipants/\\{pacticipant\\}", "http://localhost/pacticipants/{pacticipant}")
                        }
                    }
                }));
            i
        };

        // GET /pacticipants/Foo returns the pacticipant
        let get_pacticipant_interaction = |mut i: InteractionBuilder| {
            i.given("a pacticipant with name Foo exists");
            i.request
                .get()
                .path("/pacticipants/Foo")
                .header("Accept", "application/hal+json")
                .header("Accept", "application/json");
            i.response
                .status(200)
                .header("Content-Type", "application/hal+json;charset=utf-8")
                .json_body(json_pattern!({
                    "name": like!(pacticipant_name),
                    "displayName": like!(display_name),
                    "repositoryUrl": like!(repository_url),
                    "createdAt": like!(created_at),
                    "_links": {
                        "self": {
                            "href": term!("http:\\/\\/.*", "http://localhost/pacticipants/Foo")
                        }
                    }
                }));
            i
        };

        let mock_server = setup_mock_server(vec![
            index_interaction(InteractionBuilder::new(
                "a request for the index resource",
                "",
            )),
            get_pacticipant_interaction(InteractionBuilder::new(
                "a request to retrieve a pacticipant with repostitory URL",
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

        let result = describe_pacticipant(
            pacticipant_name.to_string(),
            &broker_details,
            OutputType::Json,
        );

        assert!(result.is_ok());
        let json: serde_json::Value = serde_json::from_str(&result.unwrap()).unwrap();
        assert_eq!(json["name"], pacticipant_name);
        assert_eq!(json["repositoryUrl"], repository_url);
        assert_eq!(json["displayName"], display_name);
        assert_eq!(json["createdAt"], created_at);
    }

    #[test]
    fn describe_pacticipant_not_found() {
        let pacticipant_name = "DoesNotExist";

        let index_interaction = |mut i: InteractionBuilder| {
            i.given("the pacticipant relations are present");
            i.request
                .path("/")
                .header("Accept", "application/hal+json")
                .header("Accept", "application/json");
            i.response
                .header("Content-Type", "application/hal+json;charset=utf-8")
                .json_body(json_pattern!({
                    "_links": {
                        "pb:pacticipant": {
                            "href": term!("http:\\/\\/.*/pacticipants/\\{pacticipant\\}", "http://localhost/pacticipants/{pacticipant}")
                        }
                    }
                }));
            i
        };

        let get_pacticipant_interaction = |mut i: InteractionBuilder| {
            i.request
                .get()
                .path("/pacticipants/DoesNotExist")
                .header("Accept", "application/hal+json")
                .header("Accept", "application/json");
            i.response.status(404);
            i
        };

        let mock_server = setup_mock_server(vec![
            index_interaction(InteractionBuilder::new(
                "a request for the index resource",
                "",
            )),
            get_pacticipant_interaction(InteractionBuilder::new(
                "a request to retrieve a non-existent pacticipant",
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

        let result = describe_pacticipant(
            pacticipant_name.to_string(),
            &broker_details,
            OutputType::Json,
        );

        assert!(result.is_err());
    }
}
