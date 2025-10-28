use maplit::hashmap;
use url::Url;

use crate::cli::pact_broker::main::{
    HALClient, PactBrokerError,
    utils::{
        follow_templated_broker_relation, get_auth, get_broker_relation, get_broker_url,
        get_ssl_options,
    },
};

pub fn delete_webhook(args: &clap::ArgMatches) -> Result<String, PactBrokerError> {
    let broker_url = get_broker_url(args).trim_end_matches('/').to_string();
    let auth = get_auth(args);
    let ssl_options = get_ssl_options(args);

    let webhook_uuid = args
        .try_get_one::<String>("uuid")
        .unwrap()
        .ok_or_else(|| PactBrokerError::IoError("Webhook UUID is required".to_string()))?;

    let res = tokio::runtime::Runtime::new().unwrap().block_on(async {
        let hal_client: HALClient =
            HALClient::with_url(&broker_url, Some(auth.clone()), ssl_options.clone());

        // Get the pb:webhook relation from the index
        let pb_webhook_href_path = get_broker_relation(
            hal_client.clone(),
            "pb:webhook".to_string(),
            broker_url.to_string(),
        )
        .await?;

        // Template the webhook relation with the UUID
        let template_values = hashmap! { "uuid".to_string() => webhook_uuid.to_string() };
        let webhook_response = follow_templated_broker_relation(
            hal_client.clone(),
            "pb:webhook".to_string(),
            pb_webhook_href_path,
            template_values,
        )
        .await?;

        // Extract the self link from the webhook response
        let webhook_url = webhook_response
            .get("_links")
            .and_then(|links| links.get("self"))
            .and_then(|self_link| self_link.get("href"))
            .and_then(|href| href.as_str())
            .ok_or_else(|| {
                PactBrokerError::IoError("Could not find webhook self link".to_string())
            })?;

        // Extract the path from the webhook URL
        let webhook_path = if webhook_url.starts_with("http") {
            // Parse URL and extract path
            let parsed_url = webhook_url.parse::<Url>().map_err(|e| {
                PactBrokerError::UrlError(format!("Invalid webhook URL: {}", e))
            })?;
            parsed_url.path().to_string()
        } else {
            webhook_url.to_string()
        };

        // Send DELETE request to the webhook path with proper headers
        let broker_url = broker_url.parse::<Url>().map_err(|e| {
            PactBrokerError::UrlError(format!("Invalid broker URL: {}", e))
        })?;
        let full_webhook_url = broker_url.join(&webhook_path).map_err(|e| {
            PactBrokerError::UrlError(format!("Failed to join webhook path: {}", e))
        })?;

        let client = reqwest::Client::new();
        let request_builder = match auth {
            crate::cli::pact_broker::main::HttpAuth::User(username, password) => {
                client.delete(full_webhook_url).basic_auth(&username, password.as_ref())
            }
            crate::cli::pact_broker::main::HttpAuth::Token(token) => {
                client.delete(full_webhook_url).bearer_auth(&token)
            }
            _ => client.delete(full_webhook_url),
        }
        .header("Accept", "application/hal+json");

        let response = request_builder.send().await.map_err(|e| {
            PactBrokerError::IoError(format!("Failed to send DELETE request: {}", e))
        })?;

        if response.status().is_success() {
            Ok(serde_json::json!({}))
        } else if response.status() == 404 {
            Err(PactBrokerError::NotFound("Webhook not found".to_string()))
        } else {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            Err(PactBrokerError::IoError(format!(
                "DELETE request failed with status {}: {}",
                status, body
            )))
        }
    });

    match res {
        Ok(_) => {
            let message = format!("Webhook with UUID {} successfully deleted", webhook_uuid);
            println!("{}", message);
            Ok(message)
        }
        Err(PactBrokerError::NotFound(_)) => {
            let message = format!("Webhook with UUID {} was not found", webhook_uuid);
            println!("{}", message);
            Ok(message)
        }
        Err(err) => Err(err),
    }
}

#[cfg(test)]
mod delete_webhook_tests {
    use super::delete_webhook;
    use crate::cli::pact_broker::main::subcommands::add_delete_webhook_subcommand;
    use pact_consumer::builders::InteractionBuilder;
    use pact_consumer::prelude::*;
    use pact_models::PactSpecification;
    use serde_json::json;

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

    fn base_args(mock_server_url: &str, uuid: &str) -> Vec<String> {
        vec![
            "delete-webhook".to_string(),
            "-b".to_string(),
            mock_server_url.to_string(),
            "--uuid".to_string(),
            uuid.to_string(),
        ]
    }

    fn index_interaction() -> impl Fn(InteractionBuilder) -> InteractionBuilder {
        |mut i: InteractionBuilder| {
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
                    "href": term!("http:\\/\\/.*\\/webhooks\\/.*", "http://localhost/webhooks/{uuid}")
                    }
                }
                }));
            i
        }
    }

    #[test]
    fn delete_webhook_successfully() {
        let uuid = "d2181b32-8b03-4daf-8cc0-d9168b2f6fac";
        
        let webhook_get_interaction = |mut i: InteractionBuilder| {
            i.given(format!("a webhook with uuid {} exists", uuid));
            i.request
                .get()
                .path(format!("/webhooks/{}", uuid))
                .header("Accept", "application/hal+json")
                .header("Accept", "application/json");
            i.response
                .status(200)
                .header("Content-Type", "application/hal+json;charset=utf-8")
                .json_body(json!({
                    "uuid": uuid,
                    "description": "an example webhook",
                    "_links": {
                        "self": {
                            "href": format!("http://localhost/webhooks/{}", uuid)
                        }
                    }
                }));
            i
        };

        let webhook_delete_interaction = |mut i: InteractionBuilder| {
            i.given(format!("a webhook with uuid {} exists", uuid));
            i.request
                .delete()
                .path(format!("/webhooks/{}", uuid))
                .header("Accept", "application/hal+json");
            i.response.status(204);
            i
        };

        let mock_server = setup_mock_server(vec![
            index_interaction()(InteractionBuilder::new(
                "a request for the index resource",
                "",
            )),
            webhook_get_interaction(InteractionBuilder::new(
                "a request to get a webhook by UUID",
                "",
            )),
            webhook_delete_interaction(InteractionBuilder::new(
                "a request to delete a webhook",
                "",
            )),
        ]);
        let mock_server_url = mock_server.url();

        let matches = add_delete_webhook_subcommand()
            .get_matches_from(base_args(mock_server_url.as_str(), uuid));

        let result = delete_webhook(&matches);

        if result.is_err() {
            println!("Delete webhook error: {:?}", result.as_ref().err().unwrap());
        }
        assert!(result.is_ok());
        let message = result.unwrap();
        assert!(message.contains("successfully deleted"));
        assert!(message.contains(uuid));
    }

    #[test]
    fn delete_webhook_not_found() {
        let uuid = "non-existent-uuid";
        
        let webhook_get_interaction = |mut i: InteractionBuilder| {
            i.given(format!("a webhook with uuid {} does not exist", uuid));
            i.request
                .get()
                .path(format!("/webhooks/{}", uuid))
                .header("Accept", "application/hal+json")
                .header("Accept", "application/json");
            i.response.status(404);
            i
        };

        let mock_server = setup_mock_server(vec![
            index_interaction()(InteractionBuilder::new(
                "a request for the index resource",
                "",
            )),
            webhook_get_interaction(InteractionBuilder::new(
                "a request to get a non-existent webhook by UUID",
                "",
            )),
        ]);
        let mock_server_url = mock_server.url();

        let matches = add_delete_webhook_subcommand()
            .get_matches_from(base_args(mock_server_url.as_str(), uuid));

        let result = delete_webhook(&matches);

        assert!(result.is_ok());
        let message = result.unwrap();
        assert!(message.contains("was not found"));
        assert!(message.contains(uuid));
    }
}