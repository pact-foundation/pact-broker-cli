use maplit::hashmap;

use crate::cli::pact_broker::main::{
    HALClient, PactBrokerError,
    utils::{
        follow_templated_broker_relation, get_auth, get_broker_relation, get_broker_url,
        get_ssl_options,
    },
};

pub fn test_webhook(args: &clap::ArgMatches) -> Result<String, PactBrokerError> {
    let broker_url = get_broker_url(args).trim_end_matches('/').to_string();
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
            .post_json(&pb_webhooks_href_path, &webhook_data_str, None)
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

#[cfg(test)]
mod test_webhook_tests {
    use super::test_webhook;
    use pact_consumer::prelude::*;
    use pact_models::PactSpecification;

    fn build_matches(broker_url: &str, uuid: &str) -> clap::ArgMatches {
        let args = vec!["test-webhook", "-b", broker_url, "--uuid", uuid];
        crate::cli::pact_broker::main::subcommands::add_test_webhook_subcommand()
            .args(crate::cli::add_ssl_arguments())
            .get_matches_from(args)
    }

    #[test]
    fn test_webhook_success() {
        let uuid = "696c5f93-1b7f-44bc-8d03-59440fcaa9a0";
        let config = MockServerConfig {
            pact_specification: PactSpecification::V2,
            ..MockServerConfig::default()
        };

        let pact_broker_service = PactBuilder::new("pact-broker-cli", "Pact Broker")
            .interaction("get webhook relation", "", |mut i| {
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
                            "pb:webhook": {
                                "href": like!(format!("/webhooks/{{uuid}}"))
                            }
                        }
                    }));
                i
            })
            .interaction("get webhook", "", |mut i| {
                i.given(format!(
                    "a webhook with the uuid {} exists",
                    uuid
                ));
                i.request.get().path(format!("/webhooks/{}", uuid));
                i.response
                    .status(200)
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(json_pattern!({
                        "_links": {
                            "pb:execute": {
                                "href": like!(format!("/webhooks/{}/execute", uuid))
                            }
                        }
                    }));
                i
            })
            .interaction("execute webhook", "", |mut i| {
                i.given(format!(
                    "a webhook with the uuid {} exists",
                    uuid
                ));
                i.request.post().path(format!("/webhooks/{}/execute", uuid));
                i.response
                    .status(200)
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(json_pattern!({
                        "request": like!({
                            "headers": like!({
                                "accept": like!("*/*"),
                                "user-agent": like!("Pact Broker v2.116.0"),
                                "content-type": like!("application/json"),
                                "host": like!("example.org"),
                                "content-length": like!("4")
                            }),
                            "url": like!("/")
                        }),
                        "message": like!("For security purposes, the response details are not logged. To enable response logging, configure the webhook_host_whitelist property. See http://example.org/doc/webhooks#whitelist for more information."),
                        "logs": like!("[2025-09-05T02:42:04Z] DEBUG: Webhook context {\"base_url\":\"http://example.org\",\"event_name\":\"test\"}\n[2025-09-05T02:42:04Z] INFO: HTTP/1.1 POST http://example.org\n[2025-09-05T02:42:04Z] INFO: accept: */*\n[2025-09-05T02:42:04Z] INFO: user-agent: Pact Broker v2.116.0\n[2025-09-05T02:42:04Z] INFO: content-type: application/json\n[2025-09-05T02:42:04Z] INFO: host: example.org\n[2025-09-05T02:42:04Z] INFO: content-length: 4\n[2025-09-05T02:42:04Z] INFO: null\n[2025-09-05T02:42:04Z] INFO: For security purposes, the response details are not logged. To enable response logging, configure the webhook_host_whitelist property. See http://example.org/doc/webhooks#whitelist for more information.\n[2025-09-05T02:42:04Z] INFO: Webhook execution failed\n"),
                        "success": like!(false),
                        "_links": like!({
                            "webhook": like!({"href": like!("http://example.org/webhooks/696c5f93-1b7f-44bc-8d03-59440fcaa9a0")}),
                            "try-again": like!({
                                "title": like!("Execute the webhook again"),
                                "href": like!("http://example.org/webhooks/696c5f93-1b7f-44bc-8d03-59440fcaa9a0/execute")
                            })
                        })
                    }));
                i
            })
            .start_mock_server(None, Some(config));
        let mock_server_url = pact_broker_service.url();

        let matches = build_matches(mock_server_url.as_str(), uuid);

        let result = test_webhook(&matches);
        assert!(result.is_ok());
        let json = result.unwrap();
        assert!(json.contains("success"));
    }

    #[test]
    fn test_webhook_not_found() {
        let uuid = "non-existent-uuid";
        let config = MockServerConfig {
            pact_specification: PactSpecification::V2,
            ..MockServerConfig::default()
        };

        let pact_broker_service = PactBuilder::new("pact-broker-cli", "Pact Broker")
            .interaction("get webhook relation", "", |mut i| {
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
                            "pb:webhook": {
                                "href": like!("/webhooks/{uuid}")
                            }
                        }
                    }));
                i
            })
            .interaction("get webhook not found", "", |mut i| {
                i.request.get().path(format!("/webhooks/{}", uuid));
                i.response
                    .status(404)
                    .header("Content-Type", "application/json;charset=utf-8")
                    .json_body(json_pattern!({
                        "error": like!("Not found")
                    }));
                i
            })
            .start_mock_server(None, Some(config));
        let mock_server_url = pact_broker_service.url();

        let matches = build_matches(mock_server_url.as_str(), uuid);

        let result = test_webhook(&matches);
        assert!(result.is_err());
    }
}
