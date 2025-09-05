mod cli;
use crate::cli::pact_broker::{self};
use clap::ArgMatches;
use clap::error::ErrorKind;
use clap_complete::{Shell, generate_to};

use std::str::FromStr;

pub fn main() {
    if std::env::var("PACT_LOG_LEVEL").is_ok() {
        let _ = cli::utils::setup_loggers(&std::env::var("PACT_LOG_LEVEL").unwrap());
    }
    let app = cli::build_cli();
    let matches = app.clone().try_get_matches();
    let raw_args: Vec<String> = std::env::args().skip(1).collect();
    match matches {
        Ok(results) => match results.subcommand() {
            Some(("pact-broker", args)) => cli::pact_broker_client::run(args, raw_args),
            Some(("pactflow", args)) => cli::pactflow_client::run(args, raw_args),
            Some(("completions", args)) => generate_completions(args),
            _ => cli::build_cli().print_help().unwrap(),
        },
        Err(ref err) => match err.kind() {
            ErrorKind::DisplayHelp => {
                let _ = err.print();
            }
            ErrorKind::DisplayVersion => {
                let _ = err.print();
            }
            _ => err.exit(),
        },
    }
}

fn generate_completions(args: &ArgMatches) {
    let shell = args
        .get_one::<String>("shell")
        .expect("a shell is required");
    let out_dir = args
        .get_one::<String>("dir")
        .expect("a directory is expected")
        .to_string();
    let mut cmd = cli::build_cli();
    let shell_enum = Shell::from_str(&shell).unwrap();
    let _ = generate_to(
        shell_enum,
        &mut cmd,
        "pact-broker-cli".to_string(),
        &out_dir,
    );
    println!(
        "ℹ️  {} shell completions for pact-broker-cli written to {}",
        &shell_enum, &out_dir
    );
}
