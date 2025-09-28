pub mod cli;

use clap::ArgMatches;
use clap::error::ErrorKind;
use clap_complete::{Shell, generate_to};

use std::process::ExitCode;
use std::str::FromStr;

pub fn handle_matches(
    matches: &Result<ArgMatches, clap::Error>,
    raw_args: Option<Vec<String>>,
) -> Result<(), ExitCode> {
    let raw_args = if let Some(args) = raw_args {
        args
    } else {
        std::env::args().skip(1).collect()
    };
    match matches {
        Ok(results) => match results.subcommand() {
            _ => {
                let log_level = results
                    .get_one::<String>("log-level")
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "off".to_string());
                cli::utils::setup_loggers(&log_level);

                match results.subcommand() {
                    Some(("pact-broker", args)) => Ok(cli::pact_broker_client::run(args, raw_args)),
                    Some(("pactflow", args)) => Ok(cli::pactflow_client::run(args, raw_args)),
                    Some(("completions", args)) => Ok(generate_completions(args)),
                    _ => Ok(cli::build_cli().print_help().unwrap()),
                }
            }
        },
        Err(err) => match err.kind() {
            ErrorKind::DisplayHelp => {
                let _ = err.print();
                Ok(())
            }
            ErrorKind::DisplayVersion => {
                let _ = err.print();
                Ok(())
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
