use maplit::hashmap;

use crate::cli::pact_broker::main::{
    HALClient, PactBrokerError,
    utils::{
        follow_templated_broker_relation, get_auth, get_broker_relation, get_broker_url,
        get_ssl_options,
    },
};

pub fn create_webhook(args: &clap::ArgMatches) -> Result<String, PactBrokerError> {
    let broker_url = get_broker_url(args);
    let auth = get_auth(args);
    let ssl_options = get_ssl_options(args);

    let url: Option<&String> = args.try_get_one::<String>("url").unwrap();
    let http_method = args.try_get_one::<String>("request").unwrap();
    let headers = args
        .get_many::<String>("header")
        .unwrap_or_default()
        .cloned()
        .collect::<Vec<_>>();
    let data = args.try_get_one::<String>("data").unwrap();
    let user = args.try_get_one::<String>("user").unwrap();
    let consumer = args.try_get_one::<String>("consumer").unwrap();
    let consumer_label = args.try_get_one::<String>("consumer-label").unwrap();
    let provider = args.try_get_one::<String>("provider").unwrap();
    let provider_label = args.try_get_one::<String>("provider-label").unwrap();
    let description = args.try_get_one::<String>("description").unwrap();
    let contract_content_changed = args.get_flag("contract-content-changed");
    let contract_published = args.get_flag("contract-published");
    let provider_verification_published = args.get_flag("provider-verification-published");
    let provider_verification_failed = args.get_flag("provider-verification-failed");
    let provider_verification_succeeded = args.get_flag("provider-verification-succeeded");
    let contract_requiring_verification_published =
        args.get_flag("contract-requiring-verification-published");
    let team_uuid = args.try_get_one::<String>("team-uuid").unwrap();
    let webhook_uuid = args.try_get_one::<String>("uuid").unwrap_or_default();
    let (username, password) = if let Some(user) = user {
        if let Some((username, password)) = user.split_once(':') {
            (Some(username.to_string()), Some(password.to_string()))
        } else {
            (Some(user.to_string()), None)
        }
    } else {
        (None, None)
    };

    let res = tokio::runtime::Runtime::new().unwrap().block_on(async {
        let hal_client: HALClient =
            HALClient::with_url(&broker_url, Some(auth.clone()), ssl_options.clone());

    let pb_webhooks_href_path = if webhook_uuid.is_some(){
        // use the pb:webhook relation, and template it with webhook uuid and perform a put
        // else post to the pb:webhooks relation and perform a post
        let pb_webhook_href_path = get_broker_relation(
            hal_client.clone(),
            "pb:webhook".to_string(),
            broker_url.to_string(),
        ).await;
        let pb_webhook_href_path = match pb_webhook_href_path {
            Ok(href) => href,
            Err(err) => {
                return Err(err);
            }
        };
        let template_values = hashmap! { "uuid".to_string() => webhook_uuid.clone().unwrap().to_string() };
        let pb_webhook_href_path = follow_templated_broker_relation(
            hal_client.clone(),
            "pb:webhook".to_string(),
            pb_webhook_href_path,
            template_values,
        )
        .await;
        let pb_webhooks_href_path = match pb_webhook_href_path {
            Ok(href) =>
            {
                href.get("_links").unwrap().get("self").unwrap().get("href").unwrap().to_string()
            }
            Err(err) => {
                return Err(err);
            }
        };
        pb_webhooks_href_path.replace("\"", "")
    } else {
        let pb_webhooks_href_path = get_broker_relation(
            hal_client.clone(),
            "pb:webhooks".to_string(),
            broker_url.to_string(),
        ).await;
                pb_webhooks_href_path.unwrap()
        };
        println!("Using pb:webhooks href path: {}", pb_webhooks_href_path);
        let request_params = serde_json::json!({
            "url": url,
            "method": http_method,
            "headers": headers,
            "body": data,
        });
        let mut webhook_data = serde_json::json!({
            "request": request_params,
        });

        if username.is_some() || password.is_some() {
            if let Some(username) = username {
                webhook_data["request"]["username"] = serde_json::Value::String(username.to_string());
            }
            if let Some(password) = password {
                webhook_data["request"]["password"] = serde_json::Value::String(password.to_string());
            }
        }

        if let Some(desc) = description {
            if !desc.is_empty() {
                webhook_data["description"] = serde_json::json!(desc);
            }
        }
        let mut events  = Vec::new();
        if contract_content_changed {
            events.push(serde_json::json!({"name": "contractContentChanged"}));
        }
        if contract_published {
            events.push(serde_json::json!({"name": "contract_published"}));
        }
        if provider_verification_published {
            events.push(serde_json::json!({"name": "providerVerificationPublished"}));
        }
        if provider_verification_failed {
            events.push(serde_json::json!({"name": "providerVerificationFailed"}));
        }
        if provider_verification_succeeded {
            events.push(serde_json::json!({"name": "providerVerificationSucceeded"}));
        }
        if contract_requiring_verification_published {
            events.push(serde_json::json!({"name": "contractRequiringVerificationPublished"}));
        }
        if events.is_empty()  {
            return Err(PactBrokerError::IoError(
                "No events specified for webhook, you must specify at least one of --contract-content-changed, --contract-published, --provider-verification-published, --provider-verification-succeeded or --provider-verification-faile".to_string(),
            ));
        }
        webhook_data["events"] = serde_json::json!(events);
        if let Some(consumer) = consumer {
            if !consumer.is_empty() {
            webhook_data["consumer"] = serde_json::json!({
                "name": consumer,
                "label": consumer_label
            });
            }
        }
        if let Some(provider) = provider {
            if !provider.is_empty() {
            webhook_data["provider"] = serde_json::json!({
                "name": provider,
                "label": provider_label
            });
            }
        }
        if let Some(team_uuid) = team_uuid {
            if !team_uuid.is_empty() {
            webhook_data["teamUuid"] = serde_json::json!(team_uuid);
            }
        }
        let webhook_data_str = webhook_data.to_string();
        if webhook_uuid.is_some(){
            hal_client.put_json(&pb_webhooks_href_path, &webhook_data_str).await
        }else {
            hal_client.post_json(&pb_webhooks_href_path, &webhook_data_str).await
        }
    });

    match res {
        Ok(result) => {
            let json: String = serde_json::to_string(&result).unwrap();
            println!("{}", json);
            Ok(json)
        }
        Err(e) => Err(PactBrokerError::IoError(format!(
            "Failed to create webhook: {}",
            e
        ))),
    }
}

#[cfg(test)]
mod create_webhook_tests {
    use crate::cli::pact_broker::main::subcommands::add_create_webhook_subcommand;
    use crate::cli::pact_broker::main::webhooks::create::create_webhook;
    use pact_consumer::prelude::*;
    use pact_models::PactSpecification;
    use serde_json::{Value, json};

    #[test]
    fn when_a_valid_webhook_with_a_team_specified_is_submitted() {
        // arrange - set up the pact mock server (as v2 for compatibility with pact-ruby)
        let config = MockServerConfig {
            pact_specification: PactSpecification::V2,
            ..MockServerConfig::default()
        };

        let description = "a webhook";
        let event_type = "contract-content-changed";
        let event_name = "contractContentChanged";
        let http_method = "POST";
        let url = "https://webhook";
        let headers = vec!["Foo:bar".to_string(), "Bar:foo".to_string()];
        let body = r#"{"some":"body"}"#;
        let team_uuid = "2abbc12a-427d-432a-a521-c870af1739d9";

        let request_body: Value = json!({
            "description": description,
            "events": [
                { "name": event_name }
            ],
            "request": {
                "url": url,
                "method": http_method,
                "headers": headers,
                "body": body
            },
            "teamUuid": team_uuid
        });

        let mut response_body = json_pattern!({
            "description": like!(description),
            "teamUuid": like!(team_uuid),
            "_links": {
                "self": {
                    "href": term!(r"http://.*", "http://localhost:1234/some-url" ),
                    "title": like!("A title")
                }
            }
        });

        let pact_broker_service = PactBuilder::new("pact-broker-cli", "PactFlow")
            .interaction("a request for the index resource", "", |mut i| {
                i.given("the pb:webhooks relation exists in the index resource");
                i.request
                    .path("/")
                    .header("Accept", "application/hal+json")
                    .header("Accept", "application/json");
                i.response
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(json_pattern!({
                        "_links": {
                            "pb:webhooks": {
                                "href": term!(r"http://localhost/webhooks", "http://localhost/webhooks"),
                            }
                        }
                    }));
                i
            })
            .interaction("a request to create a webhook for a team", "", |mut i| {
                i.given("a team with UUID 2abbc12a-427d-432a-a521-c870af1739d9 exists");
                i.request
                    .post()
                    .path("/webhooks")
                    .header("Accept", "application/hal+json")
                    .header("Content-Type", "application/json")
                    .json_body(request_body.clone());
                i.response
                    .status(201)
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(response_body);
                i
            })
            .start_mock_server(None, Some(config));
        let mock_server_url = pact_broker_service.url();
        println!("Mock server started at: {}", pact_broker_service.url());

        // arrange - set up the command line arguments
        let matches = add_create_webhook_subcommand()
            .args(crate::cli::add_ssl_arguments())
            .get_matches_from(vec![
                "create-webhook",
                url,
                "-b",
                mock_server_url.as_str(),
                "--description",
                description,
                format!("--{}", event_type).as_str(),
                "--request",
                http_method,
                "--header",
                "Foo:bar Bar:foo",
                "--data",
                body,
                "--team-uuid",
                team_uuid,
            ]);

        // act
        let sut = create_webhook(&matches);

        // assert
        assert!(sut.is_ok());
        let json_result = sut.unwrap();
        let json_value: Value = serde_json::from_str(&json_result).unwrap();
        assert_eq!(json_value["description"], description);
        assert_eq!(json_value["teamUuid"], team_uuid);
        assert!(
            json_value["_links"]["self"]["href"]
                .as_str()
                .unwrap()
                .starts_with("http://")
        );
    }
}
