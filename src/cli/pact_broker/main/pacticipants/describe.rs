use comfy_table::{Table, presets::UTF8_FULL};
use maplit::hashmap;

use crate::{
    cli::pact_broker::main::{
        types::{BrokerDetails, OutputType},
        utils::{follow_templated_broker_relation, get_broker_relation},
    },
    pact_broker::main::{HALClient, PactBrokerError},
};

pub fn describe_pacticipant(
    pacticipant_name: String,
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
