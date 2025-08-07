use comfy_table::{Table, presets::UTF8_FULL};
use maplit::hashmap;

use crate::cli::pact_broker::main::{
    HALClient, PactBrokerError,
    types::OutputType,
    utils::{
        follow_templated_broker_relation, get_auth, get_broker_relation, get_broker_url,
        get_ssl_options,
    },
};

pub fn describe_versions(args: &clap::ArgMatches) -> Result<String, PactBrokerError> {
    let broker_url = get_broker_url(args);
    let auth = get_auth(args);
    let ssl_options = get_ssl_options(args);

    let version: Option<&String> = args.try_get_one::<String>("version").unwrap();
    let latest: Option<&String> = args.try_get_one::<String>("latest").unwrap();
    let output_type: OutputType = args
        .get_one::<String>("output")
        .and_then(|s| s.parse().ok())
        .unwrap_or(OutputType::Table);
    let pacticipant_name = args.get_one::<String>("pacticipant").unwrap();

    let pb_relation_href = if latest.is_some() {
        "pb:latest-tagged-version".to_string()
    } else {
        "pb:latest-version".to_string()
    };

    let res = tokio::runtime::Runtime::new().unwrap().block_on(async {
        let hal_client: HALClient =
            HALClient::with_url(&broker_url, Some(auth.clone()), ssl_options.clone());
        let pb_version_href_path =
            get_broker_relation(hal_client.clone(), pb_relation_href, broker_url.to_string()).await;

        follow_templated_broker_relation(
            hal_client.clone(),
            "pb:pacticipant-version".to_string(),
            pb_version_href_path.unwrap(),
            hashmap! {
                "pacticipant".to_string() => pacticipant_name.to_string(),
                "version".to_string() => version.cloned().unwrap_or_default(),
                "tag".to_string() => latest.cloned().unwrap_or_default(),
            },
        )
        .await
    });

    match res {
        Ok(result) => match output_type {
            OutputType::Json => {
                let json: String = serde_json::to_string(&result).unwrap();
                println!("{}", json);
                return Ok(json);
            }
            OutputType::Table => {
                let mut table = Table::new();
                table
                    .load_preset(UTF8_FULL)
                    .set_header(vec!["NAME", "TAGS"]);

                let version_number = result.get("number").and_then(|v| v.as_str()).unwrap_or("-");

                let tags = result
                    .get("_embedded")
                    .and_then(|v| v.get("tags"))
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|tag| tag.get("name").and_then(|n| n.as_str()))
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_else(|| "-".to_string());

                table.add_row(vec![version_number, &tags]);
                println!("{table}");
                return Ok(table.to_string());
            }

            OutputType::Text => {
                return Err(PactBrokerError::NotFound(
                    "Text output is not supported for describe versions".to_string(),
                ));
            }
            OutputType::Pretty => {
                let json: String = serde_json::to_string(&result).unwrap();
                println!("{}", json);
                return Ok(json);
            }
        },
        Err(PactBrokerError::NotFound(_)) => Err(PactBrokerError::NotFound(format!(
            "Pacticipant version not found"
        ))),

        Err(err) => Err(err),
    }
}
