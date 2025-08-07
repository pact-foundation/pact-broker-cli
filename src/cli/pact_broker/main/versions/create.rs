use crate::cli::pact_broker::main::{
    HALClient, PactBrokerError,
    utils::{get_auth, get_broker_url, get_ssl_options},
};

pub fn create_or_update_version(args: &clap::ArgMatches) -> Result<String, PactBrokerError> {
    let broker_url = get_broker_url(args);
    let auth = get_auth(args);
    let ssl_options = get_ssl_options(args);

    let pacticipant_name = args.get_one::<String>("pacticipant").unwrap();
    let version_number = args.get_one::<String>("version").unwrap();
    let branch_name = args.try_get_one::<String>("branch").unwrap();
    let tags = args
        .get_many::<String>("tag")
        .unwrap_or_default()
        .cloned()
        .collect::<Vec<_>>();

    let result = tokio::runtime::Runtime::new().unwrap().block_on(async {
        let hal_client: HALClient =
            HALClient::with_url(&broker_url, Some(auth.clone()), ssl_options.clone());
        let version_href = format!(
            "{}/pacticipants/{}/versions/{}",
            broker_url, pacticipant_name, version_number
        );

        // create branch version
        if let Some(branch) = branch_name {
            let branch_href = format!(
                "{}/pacticipants/{}/versions/{}/branches/{}",
                broker_url, pacticipant_name, version_number, branch
            );
            let branch_data = serde_json::json!({ "name": branch });
            let branch_data_str = branch_data.to_string();
            let res = hal_client.put_json(&branch_href, &branch_data_str).await;
            res?;
            println!(
                "Branch '{}' created for version '{}'",
                branch, version_number
            );
        }

        // create tags

        for tag in &tags {
            let tag_href = format!(
                "{}/pacticipants/{}/versions/{}/tags/{}",
                broker_url, pacticipant_name, version_number, tag
            );
            let tag_data = serde_json::json!({ "name": tag });
            let tag_data_str = tag_data.to_string();
            let res = hal_client.put_json(&tag_href, &tag_data_str).await;
            res?;
            println!("Tag '{}' created for version '{}'", tag, version_number);
        }
        // if no tags or branches, create version
        if tags.is_empty() && branch_name.is_none() {
            let version_data = serde_json::json!({});
            let version_data_str = version_data.to_string();
            let res = hal_client.put_json(&version_href, &version_data_str).await;
            match res {
                Ok(_) => {
                    println!(
                        "Version '{}' created or updated successfully",
                        version_number
                    );
                }
                Err(err) => {
                    return Err(PactBrokerError::IoError(format!(
                        "Failed to create or update version '{}': {}",
                        version_number, err
                    )));
                }
            }
        }
        Ok("Version created or updated successfully".to_string())
    });
    result
}
