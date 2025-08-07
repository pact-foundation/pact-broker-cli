use comfy_table::{Table, presets::UTF8_FULL};

use crate::{
    cli::pact_broker::main::{
        types::{BrokerDetails, OutputType},
        utils::{follow_broker_relation, get_broker_relation},
    },
    pact_broker::main::{HALClient, PactBrokerError},
};

pub fn list_pacticipants(
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
