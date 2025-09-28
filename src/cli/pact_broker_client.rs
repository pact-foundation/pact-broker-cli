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
use crate::cli::pact_broker::main::pacts::list_latest_pact_versions::list_latest_pact_versions;
use crate::cli::pact_broker::main::subcommands::{
    add_can_i_deploy_subcommand, add_can_i_merge_subcommand, add_create_environment_subcommand,
    add_create_or_update_pacticipant_subcommand, add_create_or_update_version_subcommand,
    add_create_or_update_webhook_subcommand, add_create_version_tag_subcommand,
    add_create_webhook_subcommand, add_delete_branch_subcommand, add_delete_environment_subcommand,
    add_describe_environment_subcommand, add_describe_pacticipant_subcommand,
    add_describe_version_subcommand, add_generate_uuid_subcommand,
    add_list_environments_subcommand, add_list_latest_pact_versions_subcommand,
    add_list_pacticipants_subcommand, add_publish_pacts_subcommand,
    add_record_deployment_subcommand, add_record_release_subcommand,
    add_record_support_ended_subcommand, add_record_undeployment_subcommand,
    add_test_webhook_subcommand, add_update_environment_subcommand,
};
use crate::cli::pact_broker::main::tags::create_version_tag;
use crate::cli::pact_broker::main::types::{BrokerDetails, OutputType};
use crate::cli::pact_broker::main::utils::{
    get_auth, get_broker_url, get_ssl_options, handle_error,
};
use crate::cli::pact_broker::main::versions::create::create_or_update_version;
use crate::cli::pact_broker::main::versions::describe::describe_version;
use crate::cli::pact_broker::main::webhooks::create::create_webhook;
use crate::cli::pact_broker::main::webhooks::test::test_webhook;
use crate::cli::pact_broker::main::{can_i_deploy, pact_publish};
use clap::{ArgMatches, Command};
pub fn add_pact_broker_client_command() -> Command {
    Command::new("pact-broker")
        .args(crate::cli::add_output_arguments(
            ["json", "text", "table", "pretty"].to_vec(),
            "text",
        ))
        .subcommand(add_publish_pacts_subcommand().args(crate::cli::add_ssl_arguments()))
        .subcommand(
            add_list_latest_pact_versions_subcommand().args(crate::cli::add_ssl_arguments()),
        )
        .subcommand(add_create_environment_subcommand().args(crate::cli::add_ssl_arguments()))
        .subcommand(add_update_environment_subcommand().args(crate::cli::add_ssl_arguments()))
        .subcommand(add_delete_environment_subcommand().args(crate::cli::add_ssl_arguments()))
        .subcommand(add_describe_environment_subcommand().args(crate::cli::add_ssl_arguments()))
        .subcommand(add_list_environments_subcommand().args(crate::cli::add_ssl_arguments()))
        .subcommand(add_record_deployment_subcommand().args(crate::cli::add_ssl_arguments()))
        .subcommand(add_record_undeployment_subcommand().args(crate::cli::add_ssl_arguments()))
        .subcommand(add_record_release_subcommand().args(crate::cli::add_ssl_arguments()))
        .subcommand(add_record_support_ended_subcommand().args(crate::cli::add_ssl_arguments()))
        .subcommand(add_can_i_deploy_subcommand().args(crate::cli::add_ssl_arguments()))
        .subcommand(add_can_i_merge_subcommand().args(crate::cli::add_ssl_arguments()))
        .subcommand(
            add_create_or_update_pacticipant_subcommand().args(crate::cli::add_ssl_arguments()),
        )
        .subcommand(add_describe_pacticipant_subcommand().args(crate::cli::add_ssl_arguments()))
        .subcommand(add_list_pacticipants_subcommand().args(crate::cli::add_ssl_arguments()))
        .subcommand(add_create_webhook_subcommand().args(crate::cli::add_ssl_arguments()))
        .subcommand(add_create_or_update_webhook_subcommand().args(crate::cli::add_ssl_arguments()))
        .subcommand(add_test_webhook_subcommand().args(crate::cli::add_ssl_arguments()))
        .subcommand(add_delete_branch_subcommand().args(crate::cli::add_ssl_arguments()))
        .subcommand(add_create_version_tag_subcommand().args(crate::cli::add_ssl_arguments()))
        .subcommand(add_describe_version_subcommand().args(crate::cli::add_ssl_arguments()))
        .subcommand(add_create_or_update_version_subcommand().args(crate::cli::add_ssl_arguments()))
        .subcommand(add_generate_uuid_subcommand().args(crate::cli::add_ssl_arguments()))
}

pub fn run(args: &ArgMatches, raw_args: Vec<String>) {
    match args.subcommand() {
        Some(("publish", args)) => {
            let res = pact_publish::handle_matches(args);
            match res {
                Ok(_) => {
                    let res = pact_publish::publish_pacts(args);
                    match res {
                        Ok(_res) => {
                            std::process::exit(0);
                        }
                        Err(err) => {
                            std::process::exit(err);
                        }
                    }
                }
                Err(err) => {
                    std::process::exit(err);
                }
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

            let verbose = args.get_flag("verbose");
            let res = list_latest_pact_versions(&broker_details, output, verbose);
            if let Err(err) = res {
                handle_error(err);
                std::process::exit(1);
            }
        }
        Some(("create-environment", args)) => {
            let res = create_environment(args);
            if let Err(err) = res {
                handle_error(err);
                std::process::exit(1);
            }
        }
        Some(("update-environment", args)) => {
            let res = update_environment(args);
            if let Err(err) = res {
                handle_error(err);
                std::process::exit(1);
            }
        }
        Some(("describe-environment", args)) => {
            let res = describe_environment(args);
            if let Err(err) = res {
                handle_error(err);
                std::process::exit(1);
            }
        }
        Some(("delete-environment", args)) => {
            let res = delete_environment(args);
            if let Err(err) = res {
                handle_error(err);
                std::process::exit(1);
            }
        }
        Some(("list-environments", args)) => {
            let res = list_environments(args);
            if let Err(err) = res {
                handle_error(err);
                std::process::exit(1);
            }
        }
        Some(("record-deployment", args)) => {
            let res = record_deployment(args);
            if let Err(err) = res {
                handle_error(err);
                std::process::exit(1);
            }
        }
        Some(("record-undeployment", args)) => {
            let res = record_undeployment(args);
            if let Err(err) = res {
                handle_error(err);
                std::process::exit(1);
            }
        }
        Some(("record-release", args)) => {
            let res = record_release(args);
            if let Err(err) = res {
                handle_error(err);
                std::process::exit(1);
            }
        }
        Some(("record-support-ended", args)) => {
            let res = record_support_ended(args);
            if let Err(err) = res {
                handle_error(err);
                std::process::exit(1);
            }
        }
        Some(("can-i-deploy", args)) => {
            let res = can_i_deploy::can_i_deploy(args, raw_args, false);
            if let Err(err) = res {
                handle_error(err);
                std::process::exit(1);
            }
        }
        Some(("can-i-merge", args)) => {
            let res = can_i_deploy::can_i_deploy(args, raw_args, true);
            if let Err(err) = res {
                handle_error(err);
                std::process::exit(1);
            }
        }
        Some(("create-or-update-pacticipant", args)) => {
            let res = create_or_update_pacticipant(args);
            if let Err(err) = res {
                handle_error(err);
                std::process::exit(1);
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

            let verbose = args.get_flag("verbose");
            let res = describe_pacticipant(
                pacticipant_name.to_string(),
                &broker_details,
                output,
                verbose,
            );
            if let Err(err) = res {
                handle_error(err);
                std::process::exit(1);
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

            let verbose = args.get_flag("verbose");
            let res = list_pacticipants(&broker_details, output, verbose);
            if let Err(err) = res {
                handle_error(err);
                std::process::exit(1);
            }
        }
        Some(("create-webhook", args)) => {
            let res = create_webhook(args);
            if let Err(err) = res {
                handle_error(err);
                std::process::exit(1);
            }
        }
        Some(("create-or-update-webhook", args)) => {
            let res = create_webhook(args);
            if let Err(err) = res {
                handle_error(err);
                std::process::exit(1);
            }
        }
        Some(("test-webhook", args)) => {
            let res = test_webhook(args);
            if let Err(err) = res {
                handle_error(err);
                std::process::exit(1);
            }
        }
        Some(("delete-branch", args)) => {
            let res = delete_branch::delete_branch(args);
            if let Err(err) = res {
                handle_error(err);
                std::process::exit(1);
            }
        }
        Some(("create-version-tag", args)) => {
            let res = create_version_tag::create_version_tag(args);
            if let Err(err) = res {
                handle_error(err);
                std::process::exit(1);
            }
        }
        Some(("describe-version", args)) => {
            let res = describe_version(args);
            if let Err(err) = res {
                handle_error(err);
                std::process::exit(1);
            }
        }
        Some(("create-or-update-version", args)) => {
            let res = create_or_update_version(args);
            if let Err(err) = res {
                handle_error(err);
                std::process::exit(1);
            }
        }
        Some(("generate-uuid", _args)) => {
            println!("{}", uuid::Uuid::new_v4());
        }
        _ => {
            println!("⚠️  No option provided, try running pact-broker --help");
        }
    }
}
