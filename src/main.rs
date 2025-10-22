pub mod cli;

use clap::ArgMatches;
use clap::error::ErrorKind;
use clap_complete::{Shell, generate_to};
use tracing::span;

use std::process::ExitCode;
use std::str::FromStr;

use crate::cli::otel::OtelConfig;
use crate::cli::otel::init_logging;
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
                let (
                    enable_otel,
                    enable_otel_logs,
                    enable_otel_traces,
                    otel_exporter,
                    otel_exporter_endpoint,
                    otel_exporter_protocol,
                    log_level,
                ) = match &matches {
                    Ok(m) => (
                        m.get_flag("enable-otel"),
                        m.get_flag("enable-otel-logs"),
                        m.get_flag("enable-otel-traces"),
                        m.get_one::<String>("otel-exporter").map(|s| {
                            s.split(',')
                                .map(|v| v.trim().to_string())
                                .collect::<Vec<String>>()
                        }),
                        m.get_one::<String>("otel-exporter-endpoint"),
                        m.get_one::<String>("otel-exporter-protocol"),
                        m.get_one::<String>("log-level")
                            .and_then(|lvl| lvl.parse::<tracing::Level>().ok()),
                    ),
                    Err(_) => (false, false, false, None, None, None, None),
                };
                let otel_config = Some(OtelConfig {
                    enable_otel: Some(enable_otel),
                    enable_logs: Some(enable_otel_logs),
                    enable_traces: Some(enable_otel_traces),
                    exporter: otel_exporter.map(|v| v.clone()),
                    endpoint: otel_exporter_endpoint.cloned(),
                    protocol: otel_exporter_protocol.cloned(),
                    log_level,
                });
                let tracer_provider = init_logging(otel_config.unwrap());
                let _tracer_provider_dropper;
                if tracer_provider.is_some() {
                    let tracer_provider = tracer_provider.unwrap().clone();
                    _tracer_provider_dropper =
                        crate::cli::otel::TracerProviderDropper(tracer_provider);
                }

                let span = if tracing::Span::current().is_none() {
                    span!(tracing::Level::INFO, "pact-broker-cli")
                } else {
                    tracing::Span::current()
                };
                let _enter = span.enter();

                match results.subcommand() {
                    Some(("pactflow", args)) => match cli::pactflow_client::run(args, raw_args) {
                        Ok(_) => Ok(()),
                        Err(error) => Err(ExitCode::from(error as u8)),
                    },
                    Some(("completions", args)) => Ok(generate_completions(args)),
                    _ => match cli::pact_broker_client::run(results, raw_args) {
                        Ok(_) => Ok(()),
                        Err(error) => Err(ExitCode::from(error as u8)),
                    },
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
            ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand => {
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
