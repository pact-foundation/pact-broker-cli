use clap::{ArgMatches, Command};

use crate::cli::pactflow::main::{
    provider_contracts, subcommands::add_publish_provider_contract_subcommand,
};
pub fn add_pactflow_client_command() -> Command {
    Command::new("pactflow")
        .about("PactFlow specific commands")
        .subcommand(add_publish_provider_contract_subcommand()
        .args(crate::cli::add_ssl_arguments()))
}

pub fn run(args: &ArgMatches, raw_args: Vec<String>) {
    match args.subcommand() {
        Some(("publish-provider-contract", args)) => {
            let res = provider_contracts::publish::publish(args);
            match res {
                Ok(_res) => {
                    std::process::exit(0);
                }
                Err(err) => {
                    std::process::exit(err);
                }
            }
        }
        _ => {
            println!("⚠️  No option provided, try running pactflow --help");
        }
    }
}
