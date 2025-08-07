use maplit::hashmap;

use crate::cli::pact_broker::main::{
    HALClient, PactBrokerError,
    utils::{
        follow_templated_broker_relation, get_auth, get_broker_relation, get_broker_url,
        get_ssl_options,
    },
};

pub fn test_webhook(args: &clap::ArgMatches) -> Result<String, PactBrokerError> {
    let broker_url = get_broker_url(args);
    let auth = get_auth(args);
    let ssl_options = get_ssl_options(args);
    let webhook_uuid = args.try_get_one::<String>("uuid").unwrap_or_default();

    let res = tokio::runtime::Runtime::new().unwrap().block_on(async {
        let hal_client: HALClient =
            HALClient::with_url(&broker_url, Some(auth.clone()), ssl_options.clone());

        let pb_webhook_href_path = get_broker_relation(
            hal_client.clone(),
            "pb:webhook".to_string(),
            broker_url.to_string(),
        )
        .await;
        let pb_webhook_href_path = match pb_webhook_href_path {
            Ok(href) => href,
            Err(err) => {
                return Err(err);
            }
        };
        let template_values =
            hashmap! { "uuid".to_string() => webhook_uuid.clone().unwrap().to_string() };
        let pb_webhook_href_path = follow_templated_broker_relation(
            hal_client.clone(),
            "webhook".to_string(),
            pb_webhook_href_path,
            template_values,
        )
        .await;
        let pb_webhooks_href_path = match pb_webhook_href_path {
            Ok(href) => href
                .get("_links")
                .unwrap()
                .get("pb:execute")
                .unwrap()
                .get("href")
                .unwrap()
                .to_string()
                .replace("\"", ""),
            Err(err) => {
                return Err(err);
            }
        };

        let webhook_data = serde_json::json!({});

        let webhook_data_str = webhook_data.to_string();
        println!(
            "Executing webhook at: {} with data: {}",
            pb_webhooks_href_path, webhook_data_str
        );
        hal_client
            .post_json(&pb_webhooks_href_path, &webhook_data_str)
            .await
    });

    match res {
        Ok(result) => {
            let json: String = serde_json::to_string(&result).unwrap();
            println!("{}", json);
            Ok(json)
        }
        Err(e) => Err(PactBrokerError::IoError(format!(
            "Failed to execute webhook: {}",
            e
        ))),
    }
}
