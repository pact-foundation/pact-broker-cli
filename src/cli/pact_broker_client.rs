use crate::cli::pact_broker::main::branches::delete_branch::{self};
use crate::cli::pact_broker::main::deployments::record_deployment::record_deployment;
use crate::cli::pact_broker::main::deployments::record_release::record_release;
use crate::cli::pact_broker::main::deployments::record_support_ended::record_support_ended;
use crate::cli::pact_broker::main::deployments::record_undeployment::record_undeployment;
use crate::cli::pact_broker::main::environments::create::create_environment;
use crate::cli::pact_broker::main::environments::delete::delete_environment;
use crate::cli::pact_broker::main::environments::describe::describe_environment;
use crate::cli::pact_broker::main::environments::list::list_environments;
use crate::cli::pact_broker::main::environments::update::update_environment;
use crate::cli::pact_broker::main::pacticipants::create::create_or_update_pacticipant;
use crate::cli::pact_broker::main::pacticipants::describe::describe_pacticipant;
use crate::cli::pact_broker::main::pacticipants::list::list_pacticipants;
use crate::cli::pact_broker::main::pacts::get_pacts::get_pacts;
use crate::cli::pact_broker::main::pacts::list_latest_pact_versions::list_latest_pact_versions;
use crate::cli::pact_broker::main::subcommands::{
    add_can_i_deploy_subcommand, add_can_i_merge_subcommand, add_create_environment_subcommand,
    add_create_or_update_pacticipant_subcommand, add_create_or_update_version_subcommand,
    add_create_or_update_webhook_subcommand, add_create_version_tag_subcommand,
    add_create_webhook_subcommand, add_delete_branch_subcommand, add_delete_environment_subcommand,
    add_delete_version_tag_subcommand, add_delete_webhook_subcommand, add_describe_environment_subcommand, add_describe_pacticipant_subcommand,
    add_describe_version_subcommand, add_generate_uuid_subcommand, add_get_pacts_subcommand,
    add_list_environments_subcommand, add_list_latest_pact_versions_subcommand,
    add_list_pacticipants_subcommand, add_provider_states_subcommand, add_publish_pacts_subcommand,
    add_record_deployment_subcommand, add_record_release_subcommand,
    add_record_support_ended_subcommand, add_record_undeployment_subcommand,
    add_test_webhook_subcommand, add_update_environment_subcommand,
};
use crate::cli::pact_broker::main::tags::create_version_tag;
use crate::cli::pact_broker::main::tags::delete_tag::delete_version_tag;
use crate::cli::pact_broker::main::types::{BrokerDetails, OutputType};
use crate::cli::pact_broker::main::utils::{
    get_auth, get_broker_url, get_ssl_options, handle_error,
};
use crate::cli::pact_broker::main::versions::create::create_or_update_version;
use crate::cli::pact_broker::main::versions::describe::describe_version;
use crate::cli::pact_broker::main::webhooks::create::create_webhook;
use crate::cli::pact_broker::main::webhooks::delete::delete_webhook;
use crate::cli::pact_broker::main::webhooks::test::test_webhook;
use crate::cli::pact_broker::main::{can_i_deploy, pact_publish};
use clap::{ArgMatches, Command, command};
use tracing::error;
pub fn add_pact_broker_client_command() -> Command {
    command!()
        .arg_required_else_help(true)
        .args(crate::cli::add_output_arguments(
            ["json", "text", "table", "pretty"].to_vec(),
            "text",
        ))
        .subcommand(add_publish_pacts_subcommand())
        .subcommand(add_list_latest_pact_versions_subcommand())
        .subcommand(add_get_pacts_subcommand())
        .subcommand(add_create_environment_subcommand())
        .subcommand(add_update_environment_subcommand())
        .subcommand(add_delete_environment_subcommand())
        .subcommand(add_describe_environment_subcommand())
        .subcommand(add_list_environments_subcommand())
        .subcommand(add_record_deployment_subcommand())
        .subcommand(add_record_undeployment_subcommand())
        .subcommand(add_record_release_subcommand())
        .subcommand(add_record_support_ended_subcommand())
        .subcommand(add_can_i_deploy_subcommand())
        .subcommand(add_can_i_merge_subcommand())
        .subcommand(add_create_or_update_pacticipant_subcommand())
        .subcommand(add_describe_pacticipant_subcommand())
        .subcommand(add_list_pacticipants_subcommand())
        .subcommand(add_create_webhook_subcommand())
        .subcommand(add_create_or_update_webhook_subcommand())
        .subcommand(add_delete_webhook_subcommand())
        .subcommand(add_test_webhook_subcommand())
        .subcommand(add_delete_branch_subcommand())
        .subcommand(add_create_version_tag_subcommand())
        .subcommand(add_delete_version_tag_subcommand())
        .subcommand(add_describe_version_subcommand())
        .subcommand(add_create_or_update_version_subcommand())
        .subcommand(add_generate_uuid_subcommand())
        .subcommand(add_provider_states_subcommand().arg_required_else_help(true))
}

pub fn run(args: &ArgMatches, raw_args: Vec<String>) -> Result<serde_json::Value, i32> {
    match args.subcommand() {
        Some(("publish", args)) => {
            let pacts = pact_publish::handle_matches(args);
            match pacts {
                Ok(_) => {
                    // todo: update to return a PactBrokerError rather than an i32 exit code
                    let res = pact_publish::publish_pacts(args);
                    match res {
                        Ok(res) => Ok(serde_json::to_value(res).unwrap()),
                        Err(err) => Err(err),
                    }
                }
                Err(err) => Err(err),
            }
        }
        Some(("list-latest-pact-versions", args)) => {
            // setup client with broker url and credentials
            let broker_url = get_broker_url(args).trim_end_matches('/').to_string();
            let auth = get_auth(args);
            let ssl_options = get_ssl_options(args);

            let broker_details = BrokerDetails {
                url: broker_url.clone(),
                auth: Some(auth),
                ssl_options: ssl_options.clone(),
            };
            let default_output: String = "text".to_string();
            let output_arg: &String = args.get_one::<String>("output").unwrap_or(&default_output);
            let output = match output_arg.as_str() {
                "json" => OutputType::Json,
                "table" => OutputType::Table,
                "pretty" => OutputType::Pretty,
                _ => OutputType::Text,
            };

            let res = list_latest_pact_versions(&broker_details, output);
            if let Err(err) = res {
                handle_error(err);
                Err(1)
            } else {
                Ok(serde_json::to_value(res.unwrap()).unwrap())
            }
        }
        Some(("get-pacts", args)) => {
            // setup client with broker url and credentials
            let broker_url = get_broker_url(args).trim_end_matches('/').to_string();
            let auth = get_auth(args);
            let ssl_options = get_ssl_options(args);

            let broker_details = BrokerDetails {
                url: broker_url.clone(),
                auth: Some(auth),
                ssl_options: ssl_options.clone(),
            };

            let default_output: String = "text".to_string();
            let output_arg: &String = args.get_one::<String>("output").unwrap_or(&default_output);
            let output = match output_arg.as_str() {
                "json" => OutputType::Json,
                "table" => OutputType::Table,
                "pretty" => OutputType::Pretty,
                _ => OutputType::Text,
            };

            let provider = args.get_one::<String>("provider").unwrap();
            let consumer = args.get_one::<String>("consumer");
            let branch = args.get_one::<String>("branch");
            let latest = args.get_flag("latest");
            let download = args.get_flag("download");
            let download_dir = args.get_one::<String>("download-dir").unwrap();

            let res = get_pacts(
                &broker_details,
                provider,
                consumer.map(|s| s.as_str()),
                branch.map(|s| s.as_str()),
                latest,
                output,
                download,
                download_dir,
            );
            if let Err(err) = res {
                handle_error(err);
                Err(1)
            } else {
                Ok(serde_json::to_value(res.unwrap()).unwrap())
            }
        }
        Some(("create-environment", args)) => {
            let res = create_environment(args);
            if let Err(err) = res {
                handle_error(err);
                Err(1)
            } else {
                Ok(serde_json::to_value(res.unwrap()).unwrap())
            }
        }
        Some(("update-environment", args)) => {
            let res = update_environment(args);
            if let Err(err) = res {
                handle_error(err);
                Err(1)
            } else {
                Ok(serde_json::to_value(res.unwrap()).unwrap())
            }
        }
        Some(("describe-environment", args)) => {
            let res = describe_environment(args);
            if let Err(err) = res {
                handle_error(err);
                Err(1)
            } else {
                Ok(serde_json::to_value(res.unwrap()).unwrap())
            }
        }
        Some(("delete-environment", args)) => {
            let res = delete_environment(args);
            if let Err(err) = res {
                handle_error(err);
                Err(1)
            } else {
                Ok(serde_json::to_value(res.unwrap()).unwrap())
            }
        }
        Some(("list-environments", args)) => {
            let res = list_environments(args);
            if let Err(err) = res {
                handle_error(err);
                Err(1)
            } else {
                Ok(serde_json::to_value(res.unwrap()).unwrap())
            }
        }
        Some(("record-deployment", args)) => {
            let res = record_deployment(args);
            if let Err(err) = res {
                handle_error(err);
                Err(1)
            } else {
                Ok(serde_json::to_value(res.unwrap()).unwrap())
            }
        }
        Some(("record-undeployment", args)) => {
            let res = record_undeployment(args);
            if let Err(err) = res {
                handle_error(err);
                Err(1)
            } else {
                Ok(serde_json::to_value(res.unwrap()).unwrap())
            }
        }
        Some(("record-release", args)) => {
            let res = record_release(args);
            if let Err(err) = res {
                handle_error(err);
                Err(1)
            } else {
                Ok(serde_json::to_value(res.unwrap()).unwrap())
            }
        }
        Some(("record-support-ended", args)) => {
            let res = record_support_ended(args);
            if let Err(err) = res {
                handle_error(err);
                Err(1)
            } else {
                Ok(serde_json::to_value(res.unwrap()).unwrap())
            }
        }
        Some(("can-i-deploy", args)) => {
            let res = can_i_deploy::can_i_deploy(args, raw_args, false);
            if let Err(err) = res {
                handle_error(err);
                Err(1)
            } else {
                Ok(serde_json::to_value(res.unwrap()).unwrap())
            }
        }
        Some(("can-i-merge", args)) => {
            let res = can_i_deploy::can_i_deploy(args, raw_args, true);
            if let Err(err) = res {
                handle_error(err);
                Err(1)
            } else {
                Ok(serde_json::to_value(res.unwrap()).unwrap())
            }
        }
        Some(("create-or-update-pacticipant", args)) => {
            let res = create_or_update_pacticipant(args);
            if let Err(err) = res {
                handle_error(err);
                Err(1)
            } else {
                Ok(serde_json::to_value(res.unwrap()).unwrap())
            }
        }
        Some(("describe-pacticipant", args)) => {
            let broker_url = get_broker_url(args).trim_end_matches('/').to_string();
            let auth = get_auth(args);
            let ssl_options = get_ssl_options(args);

            let broker_details = BrokerDetails {
                url: broker_url.clone(),
                auth: Some(auth),
                ssl_options: ssl_options.clone(),
            };
            let default_output: String = "table".to_string();
            let output_arg: &String = args.get_one::<String>("output").unwrap_or(&default_output);
            let pacticipant_name: &String = args.get_one::<String>("name").unwrap();
            let output = match output_arg.as_str() {
                "json" => OutputType::Json,
                "table" => OutputType::Table,
                "pretty" => OutputType::Pretty,
                _ => OutputType::Text,
            };

            let res = describe_pacticipant(pacticipant_name.to_string(), &broker_details, output);
            if let Err(err) = res {
                handle_error(err);
                Err(1)
            } else {
                Ok(serde_json::to_value(res.unwrap()).unwrap())
            }
        }
        Some(("list-pacticipants", args)) => {
            let broker_url = get_broker_url(args).trim_end_matches('/').to_string();
            let auth = get_auth(args);
            let ssl_options = get_ssl_options(args);

            let broker_details = BrokerDetails {
                url: broker_url.clone(),
                auth: Some(auth),
                ssl_options: ssl_options.clone(),
            };
            let default_output: String = "table".to_string();
            let output_arg: &String = args.get_one::<String>("output").unwrap_or(&default_output);
            let output = match output_arg.as_str() {
                "json" => OutputType::Json,
                "table" => OutputType::Table,
                "pretty" => OutputType::Pretty,
                _ => OutputType::Text,
            };

            let res = list_pacticipants(&broker_details, output);
            if let Err(err) = res {
                handle_error(err);
                Err(1)
            } else {
                Ok(serde_json::to_value(res.unwrap()).unwrap())
            }
        }
        Some(("create-webhook", args)) => {
            let res = create_webhook(args);
            if let Err(err) = res {
                handle_error(err);
                Err(1)
            } else {
                Ok(serde_json::to_value(res.unwrap()).unwrap())
            }
        }
        Some(("create-or-update-webhook", args)) => {
            let res = create_webhook(args);
            if let Err(err) = res {
                handle_error(err);
                Err(1)
            } else {
                Ok(serde_json::to_value(res.unwrap()).unwrap())
            }
        }
        Some(("test-webhook", args)) => {
            let res = test_webhook(args);
            if let Err(err) = res {
                handle_error(err);
                Err(1)
            } else {
                Ok(serde_json::to_value(res.unwrap()).unwrap())
            }
        }
        Some(("delete-webhook", args)) => {
            let res = delete_webhook(args);
            if let Err(err) = res {
                handle_error(err);
                Err(1)
            } else {
                Ok(serde_json::to_value(res.unwrap()).unwrap())
            }
        }
        Some(("delete-branch", args)) => {
            let res = delete_branch::delete_branch(args);
            if let Err(err) = res {
                handle_error(err);
                Err(1)
            } else {
                Ok(serde_json::to_value(res.unwrap()).unwrap())
            }
        }
        Some(("create-version-tag", args)) => {
            let res = create_version_tag::create_version_tag(args);
            if let Err(err) = res {
                handle_error(err);
                Err(1)
            } else {
                Ok(serde_json::to_value(res.unwrap()).unwrap())
            }
        }
        Some(("delete-version-tag", args)) => {
            let res = delete_version_tag(args);
            if let Err(err) = res {
                handle_error(err);
                Err(1)
            } else {
                Ok(serde_json::to_value(res.unwrap()).unwrap())
            }
        }
        Some(("describe-version", args)) => {
            let res = describe_version(args);
            if let Err(err) = res {
                handle_error(err);
                Err(1)
            } else {
                Ok(serde_json::to_value(res.unwrap()).unwrap())
            }
        }
        Some(("create-or-update-version", args)) => {
            let res = create_or_update_version(args);
            if let Err(err) = res {
                handle_error(err);
                Err(1)
            } else {
                Ok(serde_json::to_value(res.unwrap()).unwrap())
            }
        }
        Some(("generate-uuid", _args)) => {
            let value = serde_json::json!({
                "uuid": uuid::Uuid::new_v4().to_string()
            });
            println!("{}", value["uuid"].as_str().unwrap());
            Ok(value)
        }
        Some(("provider-states", args)) => {
            match args.subcommand() {
                Some(("list", list_args)) => {
                    use crate::cli::pact_broker::main::provider_states::list::handle_list_provider_states_command;
                    let res = handle_list_provider_states_command(list_args);
                    match res {
                        Ok(output) => {
                            println!("{}", output);
                            Ok(serde_json::Value::String(output))
                        }
                        Err(err) => {
                            handle_error(err);
                            Err(1)
                        }
                    }
                }
                _ => {
                    error!(
                        "⚠️ No provider-states subcommand provided, try running provider-states --help"
                    );
                    Err(1)
                }
            }
        }
        _ => {
            error!("⚠️ No option provided, try running --help");
            Err(1)
        }
    }
}
