use crate::cli::pact_broker::main::{
    HALClient, Link, PactBrokerError,
    utils::{get_auth, get_broker_relation, get_broker_url, get_ssl_options},
};
use maplit::hashmap;
use std::result::Result::Ok;

pub fn create_or_update_pacticipant(args: &clap::ArgMatches) -> Result<String, PactBrokerError> {
    let broker_url = get_broker_url(args);
    let auth = get_auth(args);
    let ssl_options = get_ssl_options(args);

    let pacticipant_name = args.get_one::<String>("name").unwrap();
    let display_name = args.try_get_one::<String>("display-name").unwrap();
    let main_branch = args.try_get_one::<String>("main-branch").unwrap();
    let repository_url = args.try_get_one::<String>("repository-url").unwrap();

    let res = tokio::runtime::Runtime::new().unwrap().block_on(async {
        let hal_client: HALClient =
            HALClient::with_url(&broker_url, Some(auth.clone()), ssl_options.clone());

        let template_values = hashmap! {
            "pacticipant".to_string() => pacticipant_name.to_string(),
        };

        let pacticipant_href = get_broker_relation(
            hal_client.clone(),
            "pb:pacticipant".to_string(),
            broker_url.to_string(),
        )
        .await;
        let pacticipant_entity = match pacticipant_href {
            Ok(pacticipant_href) => {
                let link = Link {
                    name: "pb:pacticipant".to_string(),
                    href: Some(pacticipant_href),
                    templated: true,
                    title: None,
                };
                hal_client.clone().fetch_url(&link, &template_values).await
            }
            Err(err) => return Err(err),
        };

        match &pacticipant_entity {
            Ok(entity) => {
                let pacticipant_href = entity
                    .get("_links")
                    .and_then(|links| links.get("self"))
                    .and_then(|link| link.get("href"))
                    .and_then(|href| href.as_str())
                    .unwrap_or_default()
                    .to_string();
                let mut pacticipant_data = serde_json::json!({
                    "name": pacticipant_name,
                });

                if let Some(display_name) = display_name {
                    pacticipant_data["displayName"] =
                        serde_json::Value::String(display_name.to_string());
                }
                if let Some(main_branch) = main_branch {
                    pacticipant_data["mainBranch"] =
                        serde_json::Value::String(main_branch.to_string());
                }
                if let Some(repository_url) = repository_url {
                    pacticipant_data["repositoryUrl"] =
                        serde_json::Value::String(repository_url.to_string());
                }

                let pacticipant_data_str = pacticipant_data.to_string();
                hal_client
                    .patch_json(&pacticipant_href, &pacticipant_data_str)
                    .await
                    .map_err(|e| {
                        PactBrokerError::IoError(format!(
                            "Failed to update pacticipant '{}': {}",
                            pacticipant_name, e
                        ))
                    })?;
                Ok(format!(
                    "Pacticipant '{}' updated successfully",
                    pacticipant_name
                ))
            }
            Err(PactBrokerError::NotFound(_)) => {
                println!("Pacticipant does not exist, creating it at: {}", broker_url);
                let pacticipants_href = get_broker_relation(
                    hal_client.clone(),
                    "pb:pacticipants".to_string(),
                    broker_url.to_string(),
                )
                .await
                .unwrap();
                let mut pacticipant_data = serde_json::json!({
                    "name": pacticipant_name,
                });
                if let Some(display_name) = display_name {
                    println!("Creating pacticipant with display name: {}", display_name);
                    pacticipant_data["displayName"] =
                        serde_json::Value::String(display_name.to_string());
                }
                if let Some(main_branch) = main_branch {
                    println!("Creating pacticipant with main branch: {}", main_branch);
                    pacticipant_data["mainBranch"] =
                        serde_json::Value::String(main_branch.to_string());
                }
                if let Some(repository_url) = repository_url {
                    println!(
                        "Creating pacticipant with repository URL: {}",
                        repository_url
                    );
                    pacticipant_data["repositoryUrl"] =
                        serde_json::Value::String(repository_url.to_string());
                }

                let pacticipant_data_str = pacticipant_data.to_string();
                hal_client
                    .post_json(&pacticipants_href, &pacticipant_data_str)
                    .await
                    .map_err(|e| {
                        PactBrokerError::IoError(format!(
                            "Failed to create pacticipant '{}': {}",
                            pacticipant_name, e
                        ))
                    })?;
                Ok(format!(
                    "Pacticipant '{}' created successfully",
                    pacticipant_name
                ))
            }
            Err(err) => return Err(err.clone()),
        }
    });

    match res {
        Ok(message) => {
            println!("{}", message);
            Ok(message)
        }
        Err(err) => Err(err),
    }
}
