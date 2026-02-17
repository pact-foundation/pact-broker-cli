use maplit::hashmap;

use crate::cli::pact_broker::main::{
    HALClient, PactBrokerError,
    utils::{
        follow_templated_broker_relation, get_auth, get_broker_relation, get_broker_url,
        get_custom_headers, get_ssl_options,
    },
};

/// Represents the operation to perform on a webhook
enum WebhookOperation {
    /// Create a new webhook (use POST)
    Create,
    /// Update an existing webhook (use PUT)
    Update,
}

pub fn create_webhook(args: &clap::ArgMatches) -> Result<String, PactBrokerError> {
    let broker_url = get_broker_url(args).trim_end_matches('/').to_string();
    let auth = get_auth(args);
    let custom_headers = get_custom_headers(args);
    let ssl_options = get_ssl_options(args);

    let url = args.try_get_one::<String>("url");
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
            HALClient::with_url(&broker_url, Some(auth.clone()), ssl_options.clone(), custom_headers.clone());
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
    let webhook_endpoint_info: Result<(String, WebhookOperation), PactBrokerError> = if webhook_uuid.is_some() {
        // use the pb:webhook relation, and template it with webhook uuid and perform a put
        let template_values =
            hashmap! { "uuid".to_string() => webhook_uuid.clone().unwrap().to_string() };
        let pb_webhook_href_path = follow_templated_broker_relation(
            hal_client.clone(),
            "pb:webhook".to_string(),
            pb_webhook_href_path,
            template_values,
        )
        .await;
        match pb_webhook_href_path {
            Ok(href) => {
                let href = href
                    .get("_links")
                    .unwrap()
                    .get("self")
                    .unwrap()
                    .get("href")
                    .unwrap()
                    .to_string()
                    .replace("\"", "");
                Ok((href, WebhookOperation::Update))
            }
            Err(PactBrokerError::NotFound(_)) => {
                // Webhook doesn't exist, fall back to creating it via POST to pb:webhooks
                get_broker_relation(
                    hal_client.clone(),
                    "pb:webhooks".to_string(),
                    broker_url.to_string(),
                ).await.map(|s| (s, WebhookOperation::Create))
            }
            Err(err) => Err(err),
        }
    } else {
        get_broker_relation(
            hal_client.clone(),
            "pb:webhooks".to_string(),
            broker_url.to_string(),
        ).await.map(|s| (s, WebhookOperation::Create))
    };
    let (webhook_endpoint_url, operation) = webhook_endpoint_info?;
        let request_params = serde_json::json!({
            "method": http_method,
            "headers": headers,
            "body": data,
        });
        let request_params = if let Ok(Some(url)) = url {
            let mut params = request_params.as_object().unwrap().clone();
            params.insert("url".to_string(), serde_json::Value::String(url.to_string()));
            serde_json::Value::Object(params)
        } else {
            request_params
        };
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
            events.push(serde_json::json!({"name": "contract_content_changed"}));
        }
        if contract_published {
            events.push(serde_json::json!({"name": "contract_published"}));
        }
        if provider_verification_published {
            events.push(serde_json::json!({"name": "provider_verification_published"}));
        }
        if provider_verification_failed {
            events.push(serde_json::json!({"name": "provider_verification_failed"}));
        }
        if provider_verification_succeeded {
            events.push(serde_json::json!({"name": "provider_verification_succeeded"}));
        }
        if contract_requiring_verification_published {
            events.push(serde_json::json!({"name": "contract_requiring_verification_published"}));
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
            });
            if consumer_label.is_some() && !consumer_label.unwrap().is_empty() {
                webhook_data["consumer"]["label"] = serde_json::json!(consumer_label);
            }
        }
        }
        if let Some(provider) = provider {
            if !provider.is_empty() {
            webhook_data["provider"] = serde_json::json!({
                "name": provider,
            });
            if provider_label.is_some() && !provider_label.unwrap().is_empty() {
                webhook_data["provider"]["label"] = serde_json::json!(provider_label);
            }
        }
        }
        if let Some(team_uuid) = team_uuid {
            if !team_uuid.is_empty() {
            webhook_data["teamUuid"] = serde_json::json!(team_uuid);
            }
        }
        let webhook_data_str = webhook_data.to_string();
        match operation {
            WebhookOperation::Update => {
                hal_client.put_json(&webhook_endpoint_url, &webhook_data_str, None).await
            }
            WebhookOperation::Create => {
                hal_client.post_json(&webhook_endpoint_url, &webhook_data_str, None).await
            }
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
    use super::create_webhook;
    use crate::cli::pact_broker::main::subcommands::{
        add_create_or_update_webhook_subcommand, add_create_webhook_subcommand,
    };
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

    fn base_args(mock_server_url: &str) -> Vec<&str> {
        vec![
            "create-webhook",
            "https://webhook",
            "-b",
            mock_server_url,
            "--description",
            "a webhook",
            "--request",
            "POST",
            "--header",
            "Foo:bar Bar:foo",
            "--user",
            "username:password",
            "--data",
            "{\"some\":\"body\"}",
            "--consumer",
            "Condor",
            "--provider",
            "Pricing Service",
            "--contract-content-changed",
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
                    "pb:webhooks": {
                    "href": term!("http:\\/\\/.*\\/webhooks", "http://localhost/webhooks")
                    },
                    "pb:webhook": {
                    "href": term!("http:\\/\\/.*\\/webhooks\\/.*", "http://localhost/webhooks/{uuid}")
                    },
                    "pb:pacticipants": {
                    "href": term!("http:\\/\\/.*\\/pacticipants", "http://localhost/pacticipants")
                    },
                    "pb:pacticipant": {
                    "href": term!("http:\\/\\/.*\\/pacticipants\\/\\{pacticipant\\}", "http://localhost/pacticipants/{pacticipant}")
                    }
                }
                }));
            i
        }
    }
    fn index_interaction_with_webhook_relation(
        uuid: &String,
    ) -> impl Fn(InteractionBuilder) -> InteractionBuilder {
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
                    "pb:webhooks": {
                    "href": term!("http:\\/\\/.*\\/webhooks", "http://localhost/webhooks")
                    },
                    "pb:webhook": {
                    "href": term!("http:\\/\\/.*\\/webhooks\\/.*", "http://localhost/webhooks/{uuid}")
                    },
                    "pb:pacticipants": {
                    "href": term!("http:\\/\\/.*\\/pacticipants", "http://localhost/pacticipants")
                    },
                    "pb:pacticipant": {
                    "href": term!("http:\\/\\/.*\\/pacticipants\\/\\{pacticipant\\}", "http://localhost/pacticipants/{pacticipant}")
                    }
                }
                }));
            i
        }
    }

    #[test]
    fn create_webhook_with_json_body_for_consumer_and_provider() {
        let request_body = json!({
            "description": "a webhook",
            "events": [ { "name": "contract_content_changed" } ],
            "request": {
                "url": "https://webhook",
                "method": "POST",
                "headers": ["Foo:bar", "Bar:foo"],
                "body": "{\"some\":\"body\"}",
                "username": "username",
                "password": "password"
            },
            "consumer": { "name": "Condor" },
            "provider": { "name": "Pricing Service" }
        });

        let response_body = json_pattern!({
            "description": "a webhook",
            "request": {
                "body": json!({ "some": "body" }).to_string()
            },
            "events": [ { "name": "contract_content_changed" } ],
            "_links": {
                "self": {
                    "href": term!(r"http://.*","http://localhost:1234/some-url"),
                    "title": like!("A title")
                }
            }
        });

        let create_webhook_interaction = |mut i: InteractionBuilder| {
            i.given("the 'Pricing Service' and 'Condor' already exist in the pact-broker");
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
        };

        let mock_server = setup_mock_server(vec![
            index_interaction()(InteractionBuilder::new(
                "a request for the index resource",
                "",
            )),
            create_webhook_interaction(InteractionBuilder::new(
                "a request to create a webhook with a JSON body for a consumer and provider",
                "",
            )),
        ]);
        let mock_server_url = mock_server.url();

        let matches =
            add_create_webhook_subcommand().get_matches_from(base_args(mock_server_url.as_str()));

        let result = create_webhook(&matches);

        assert!(result.is_ok());
        let json = result.unwrap();
        assert!(json.contains("a webhook"));
        assert!(json.contains("contract_content_changed"));
    }

    #[test]
    fn create_webhook_with_all_event_types() {
        let event_names = vec![
            "contract_content_changed",
            "contract_published",
            "provider_verification_published",
            "provider_verification_failed",
            "provider_verification_succeeded",
        ];

        let request_body = json!({
            "description": "a webhook",
            "events": event_names.iter().map(|n| json!({ "name": n })).collect::<Vec<_>>(),
            "request": {
                "url": "https://webhook",
                "method": "POST",
                "headers": ["Foo:bar", "Bar:foo"],
                "body": "{\"some\":\"body\"}",
                "username": "username",
                "password": "password"
            },
            "consumer": { "name": "Condor" },
            "provider": { "name": "Pricing Service" }
        });

        let response_body = json_pattern!({
            "description": "a webhook",
            "request": {
                "body": json!({ "some": "body" }).to_string()
            },
            "events": event_names.iter().map(|n| json!({ "name": n })).collect::<Vec<_>>(),
            "_links": {
                "self": {
                    "href": term!(r"http://.*","http://localhost:1234/some-url"),
                    "title": like!("A title")
                }
            }
        });

        let interaction = |mut i: InteractionBuilder| {
            i.given("the 'Pricing Service' and 'Condor' already exist in the pact-broker");
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
        };

        let mock_server = setup_mock_server(vec![
            index_interaction()(InteractionBuilder::new(
                "a request for the index resource",
                "",
            )),
            interaction(InteractionBuilder::new(
                "a request to create a webhook with every possible event type",
                "",
            )),
        ]);
        let mock_server_url = mock_server.url();

        let mut args = base_args(mock_server_url.as_str());
        args.retain(|&a| a != "--contract-content-changed");
        args.push("--contract-content-changed");
        args.push("--contract-published");
        args.push("--provider-verification-published");
        args.push("--provider-verification-succeeded");
        args.push("--provider-verification-failed");

        let matches = add_create_webhook_subcommand()
            .get_matches_from(args.iter().map(|s| *s).collect::<Vec<_>>());

        let result = create_webhook(&matches);

        assert!(result.is_ok());
        let json = result.unwrap();
        for n in event_names {
            assert!(json.contains(n));
        }
    }

    #[test]
    fn create_webhook_with_xml_body() {
        let xml_body = "<xml></xml>";

        let request_body = json!({
            "description": "a webhook",
            "events": [ { "name": "contract_content_changed" } ],
            "request": {
                "url": "https://webhook",
                "method": "POST",
                "headers": ["Foo:bar", "Bar:foo"],
                "body": xml_body,
                "username": "username",
                "password": "password"
            },
            "consumer": { "name": "Condor" },
            "provider": { "name": "Pricing Service" }
        });

        let response_body = json_pattern!({
            "description": "a webhook",
            "request": {
                "body": xml_body
            },
            "events": [ { "name": "contract_content_changed" } ],
            "_links": {
                "self": {
                    "href": term!(r"http://.*","http://localhost:1234/some-url"),
                    "title": like!("A title")
                }
            }
        });

        let interaction = |mut i: InteractionBuilder| {
            i.given("the 'Pricing Service' and 'Condor' already exist in the pact-broker");
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
        };

        let mock_server = setup_mock_server(vec![
            index_interaction()(InteractionBuilder::new(
                "a request for the index resource",
                "",
            )),
            interaction(InteractionBuilder::new(
                "a request to create a webhook with a non-JSON body for a consumer and provider",
                "",
            )),
        ]);
        let mock_server_url = mock_server.url();
        let mut args = base_args(mock_server_url.as_str());
        let idx = args.iter().position(|&a| a == "--data").unwrap() + 1;
        args[idx] = xml_body;
        let matches = add_create_webhook_subcommand()
            .get_matches_from(args.iter().map(|s| *s).collect::<Vec<_>>());

        let result = create_webhook(&matches);

        assert!(result.is_ok());
        let json = result.unwrap();
        assert!(json.contains(xml_body));
    }

    // #[test]
    // this test fails as url is a required param and the test seeks to remove it
    // so we need to modify the subcommand to make it optional
    fn create_webhook_invalid_missing_url() {
        let request_body = json!({
            "description": "a webhook",
            "events": [ { "name": "contract_content_changed" } ],
            "request": {
                "method": "POST",
                "headers": ["Foo:bar", "Bar:foo"],
                "body": "{\"some\":\"body\"}",
                "username": "username",
                "password": "password"
            },
            "consumer": { "name": "Condor" },
            "provider": { "name": "Pricing Service" }
        });

        let response_body = json!({
            "errors": {
                "request.url": ["Some error"]
            }
        });

        let interaction = |mut i: InteractionBuilder| {
            i.given("the 'Pricing Service' and 'Condor' already exist in the pact-broker");
            i.request
                .post()
                .path("/webhooks")
                .header("Accept", "application/hal+json")
                .header("Content-Type", "application/json")
                .json_body(request_body.clone());
            i.response
                .status(400)
                .header("Content-Type", "application/hal+json;charset=utf-8")
                .json_body(response_body.clone());
            i
        };

        let mock_server = setup_mock_server(vec![
            index_interaction()(InteractionBuilder::new(
                "a request for the index resource",
                "",
            )),
            interaction(InteractionBuilder::new(
                "an invalid request to create a webhook for a consumer and provider",
                "",
            )),
        ]);
        let mock_server_url = mock_server.url();

        let mut args = base_args(mock_server_url.as_str());
        let idx = args.iter().position(|&a| a == "https://webhook").unwrap();
        args.remove(idx);
        let matches = add_create_webhook_subcommand()
            .get_matches_from(args.iter().map(|s| *s).collect::<Vec<_>>());

        let result = create_webhook(&matches);

        assert!(result.is_err());
        let err = format!("{:?}", result.err());
        assert!(err.contains("400") || err.contains("Some error"));
    }

    #[test]
    fn create_webhook_consumer_only() {
        let request_body = json!({
            "description": "a webhook",
            "events": [ { "name": "contract_content_changed" } ],
            "request": {
                "url": "https://webhook",
                "method": "POST",
                "headers": ["Foo:bar", "Bar:foo"],
                "body": "{\"some\":\"body\"}",
                "username": "username",
                "password": "password"
            },
            "consumer": { "name": "Condor" }
        });

        let response_body = json_pattern!({
            "description": "a webhook",
            "request": {
                "body": json!({ "some": "body" }).to_string()
            },
            "events": [ { "name": "contract_content_changed" } ],
            "_links": {
                "self": {
                    "href": term!(r"http://.*","http://localhost:1234/some-url"),
                    "title": like!("A title")
                }
            }
        });

        let interaction = |mut i: InteractionBuilder| {
            i.given("the 'Pricing Service' and 'Condor' already exist in the pact-broker");
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
        };

        let mock_server = setup_mock_server(vec![
            index_interaction()(InteractionBuilder::new(
                "a request for the index resource",
                "",
            )),
            interaction(InteractionBuilder::new(
                "a request to create a webhook with a JSON body for a consumer",
                "",
            )),
        ]);
        let mock_server_url = mock_server.url();
        let mut args = base_args(mock_server_url.as_str());
        let idx = args.iter().position(|&a| a == "--provider").unwrap();
        args.remove(idx);
        args.remove(idx);
        let matches = add_create_webhook_subcommand()
            .get_matches_from(args.iter().map(|s| *s).collect::<Vec<_>>());

        let result = create_webhook(&matches);

        assert!(result.is_ok());
        let json = result.unwrap();
        assert!(json.contains("a webhook"));
    }

    #[test]
    fn create_webhook_provider_only() {
        let request_body = json!({
            "description": "a webhook",
            "events": [ { "name": "contract_content_changed" } ],
            "request": {
                "url": "https://webhook",
                "method": "POST",
                "headers": ["Foo:bar", "Bar:foo"],
                "body": "{\"some\":\"body\"}",
                "username": "username",
                "password": "password"
            },
            "provider": { "name": "Pricing Service" }
        });

        let response_body = json_pattern!({
            "description": "a webhook",
            "request": {
                "body": json!({ "some": "body" }).to_string()
            },
            "events": [ { "name": "contract_content_changed" } ],
            "_links": {
                "self": {
                    "href": term!(r"http://.*","http://localhost:1234/some-url"),
                    "title": like!("A title")
                }
            }
        });

        let interaction = |mut i: InteractionBuilder| {
            i.given("the 'Pricing Service' and 'Condor' already exist in the pact-broker");
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
        };

        let mock_server = setup_mock_server(vec![
            index_interaction()(InteractionBuilder::new(
                "a request for the index resource",
                "",
            )),
            interaction(InteractionBuilder::new(
                "a request to create a webhook with a JSON body for a provider",
                "",
            )),
        ]);
        let mock_server_url = mock_server.url();
        let mut args = base_args(mock_server_url.as_str());
        let idx = args.iter().position(|&a| a == "--consumer").unwrap();
        args.remove(idx);
        args.remove(idx);

        let matches = add_create_webhook_subcommand()
            .get_matches_from(args.iter().map(|s| *s).collect::<Vec<_>>());

        let result = create_webhook(&matches);

        assert!(result.is_ok());
        let json = result.unwrap();
        assert!(json.contains("a webhook"));
    }

    #[test]
    fn create_webhook_with_specified_uuid() {
        let uuid = "696c5f93-1b7f-44bc-8d03-59440fcaa9a0";

        let request_body = json!({
            "description": "a webhook",
            "events": [ { "name": "contract_content_changed" } ],
            "request": {
                "url": "https://webhook",
                "method": "POST",
                "headers": ["Foo:bar", "Bar:foo"],
                "body": "{\"some\":\"body\"}",
                "username": "username",
                "password": "password"
            },
            "consumer": { "name": "Condor" },
            "provider": { "name": "Pricing Service" }
        });

        let response_body = json_pattern!({
            "description": "a webhook",
            "request": {
                "body": json!({ "some": "body" }).to_string()
            },
            "events": [ { "name": "contract_content_changed" } ],
            "_links": {
                "self": {
                    "href": term!(r"http://.*","http://localhost:1234/some-url"),
                    "title": like!("A title")
                }
            }
        });

        let interaction_get = |mut i: InteractionBuilder| {
            i.given(format!("a webhook with the uuid {} exists", uuid));
            i.request
                .get()
                .path(format!("/webhooks/{}", uuid))
                .header("Accept", "application/hal+json")
                .header("Accept", "application/json");
            i.response
                .status(200)
                .header("Content-Type", "application/hal+json;charset=utf-8")
                .json_body(json_pattern!({
                    "_links": {
                        "self": {
                            "href": term!("http:\\/\\/.*\\/webhooks\\/.*", format!("http://localhost/webhooks/{}", uuid)),
                        },
                    }}));
            i
        };
        let interaction_post = |mut i: InteractionBuilder| {
            i.given("the 'Pricing Service' and 'Condor' already exist in the pact-broker");
            i.request
                .put()
                .path(format!("/webhooks/{}", uuid))
                .header("Accept", "application/hal+json")
                .header("Content-Type", "application/json")
                .json_body(request_body.clone());
            i.response
                .status(201)
                .header("Content-Type", "application/hal+json;charset=utf-8")
                .json_body(response_body);
            i
        };

        let mock_server = setup_mock_server(vec![
            index_interaction_with_webhook_relation(&uuid.to_string())(InteractionBuilder::new(
                "a request for the index resource with the webhook relation",
                "",
            )),
            interaction_get(InteractionBuilder::new(
                "a request to get a webhook with a uuid",
                "",
            )),
            interaction_post(InteractionBuilder::new(
                "a request to create a webhook with a JSON body and a uuid",
                "",
            )),
        ]);
        let mock_server_url = mock_server.url();
        let mut args = base_args(mock_server_url.as_str());
        args.push("--uuid");
        args.push(uuid);
        let matches = add_create_or_update_webhook_subcommand()
            .get_matches_from(args.iter().map(|s| *s).collect::<Vec<_>>());

        let result = create_webhook(&matches);

        assert!(result.is_ok());
        let json = result.unwrap();
        assert!(json.contains("a webhook"));
    }
    #[test]
    fn create_webhook_with_specified_existing_uuid() {
        let uuid = "696c5f93-1b7f-44bc-8d03-59440fcaa9a0";

        let request_body = json!({
            "description": "a webhook",
            "events": [ { "name": "contract_content_changed" } ],
            "request": {
                "url": "https://webhook",
                "method": "POST",
                "headers": ["Foo:bar", "Bar:foo"],
                "body": "{\"some\":\"body\"}",
                "username": "username",
                "password": "password"
            },
            "consumer": { "name": "Condor" },
            "provider": { "name": "Pricing Service" }
        });

        let response_body = json_pattern!({
            "description": "a webhook",
            "request": {
                "body": json!({ "some": "body" }).to_string()
            },
            "events": [ { "name": "contract_content_changed" } ],
            "_links": {
                "self": {
                    "href": term!(r"http://.*","http://localhost:1234/some-url"),
                    "title": like!("A title")
                }
            }
        });

        let interaction_get = |mut i: InteractionBuilder| {
            i.given(format!("a webhook with the uuid {} exists", uuid));
            i.request
                .get()
                .path(format!("/webhooks/{}", uuid))
                .header("Accept", "application/hal+json")
                .header("Accept", "application/json");
            i.response
                .status(200)
                .header("Content-Type", "application/hal+json;charset=utf-8")
                .json_body(json_pattern!({
                    "_links": {
                        "self": {
                            "href": term!("http:\\/\\/.*\\/webhooks\\/.*", format!("http://localhost/webhooks/{}", uuid)),
                        },
                    }}));
            i
        };
        let interaction_post = |mut i: InteractionBuilder| {
            i.given(format!("a webhook with the uuid {} exists", uuid));
            i.request
                .put()
                .path(format!("/webhooks/{}", uuid))
                .header("Accept", "application/hal+json")
                .header("Content-Type", "application/json")
                .json_body(request_body.clone());
            i.response
                .status(200)
                .header("Content-Type", "application/hal+json;charset=utf-8")
                .json_body(response_body);
            i
        };

        let mock_server = setup_mock_server(vec![
            index_interaction_with_webhook_relation(&uuid.to_string())(InteractionBuilder::new(
                "a request for the index resource with the webhook relation",
                "",
            )),
            interaction_get(InteractionBuilder::new(
                "a request to get a webhook with a uuid",
                "",
            )),
            interaction_post(InteractionBuilder::new(
                "a request to create a webhook with a JSON body and a uuid",
                "",
            )),
        ]);
        let mock_server_url = mock_server.url();
        let mut args = base_args(mock_server_url.as_str());
        args.push("--uuid");
        args.push(uuid);
        let matches = add_create_or_update_webhook_subcommand()
            .get_matches_from(args.iter().map(|s| *s).collect::<Vec<_>>());

        let result = create_webhook(&matches);

        assert!(result.is_ok());
        let json = result.unwrap();
        assert!(json.contains("a webhook"));
    }

    #[test]
    fn create_webhook_with_uuid_when_webhook_does_not_exist() {
        // This test verifies the fix for the issue where create-or-update-webhook
        // fails with 404 when the webhook doesn't exist yet
        let uuid = "new-webhook-uuid-12345";

        let request_body = json!({
            "description": "a webhook",
            "events": [ { "name": "contract_content_changed" } ],
            "request": {
                "url": "https://webhook",
                "method": "POST",
                "headers": ["Foo:bar", "Bar:foo"],
                "body": "{\"some\":\"body\"}",
                "username": "username",
                "password": "password"
            },
            "consumer": { "name": "Condor" },
            "provider": { "name": "Pricing Service" }
        });

        let response_body = json_pattern!({
            "description": "a webhook",
            "request": {
                "body": json!({ "some": "body" }).to_string()
            },
            "events": [ { "name": "contract_content_changed" } ],
            "_links": {
                "self": {
                    "href": term!(r"http://.*","http://localhost:1234/some-url"),
                    "title": like!("A title")
                }
            }
        });

        let interaction_get_404 = |mut i: InteractionBuilder| {
            i.given(format!("a webhook with the uuid {} does not exist", uuid));
            i.request
                .get()
                .path(format!("/webhooks/{}", uuid))
                .header("Accept", "application/hal+json")
                .header("Accept", "application/json");
            i.response
                .status(404)
                .header("Content-Type", "application/hal+json;charset=utf-8");
            i
        };

        let interaction_post = |mut i: InteractionBuilder| {
            i.given("the 'Pricing Service' and 'Condor' already exist in the pact-broker");
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
        };

        let mock_server = setup_mock_server(vec![
            index_interaction_with_webhook_relation(&uuid.to_string())(InteractionBuilder::new(
                "a request for the index resource with the webhook relation",
                "",
            )),
            interaction_get_404(InteractionBuilder::new(
                "a request to get a webhook with a uuid that does not exist",
                "",
            )),
            interaction_post(InteractionBuilder::new(
                "a request to create a new webhook with a JSON body",
                "",
            )),
        ]);
        let mock_server_url = mock_server.url();
        let mut args = base_args(mock_server_url.as_str());
        args.push("--uuid");
        args.push(uuid);
        let matches = add_create_or_update_webhook_subcommand()
            .get_matches_from(args.iter().map(|s| *s).collect::<Vec<_>>());

        let result = create_webhook(&matches);

        assert!(result.is_ok());
        let json = result.unwrap();
        assert!(json.contains("a webhook"));
    }
}
