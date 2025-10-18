use clap::{ArgMatches, Command};
use tracing::error;

use crate::cli::pactflow::main::{
    provider_contracts, subcommands::add_publish_provider_contract_subcommand,
};
pub fn add_pactflow_client_command() -> Command {
    Command::new("pactflow")
        .about("PactFlow specific commands")
        .arg_required_else_help(true)
        .subcommand(add_publish_provider_contract_subcommand())
}

pub fn run(args: &ArgMatches, raw_args: Vec<String>) -> Result<serde_json::Value, i32> {
    match args.subcommand() {
        Some(("publish-provider-contract", args)) => {
            let res = provider_contracts::publish::publish(args);
            match res {
                Ok(res) => Ok(serde_json::to_value(res).unwrap()),
                Err(err) => Err(err),
            }
        }
        _ => {
            error!("⚠️ No option provided, try running --help");
            Err(1)
        }
    }
}
