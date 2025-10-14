use clap::{Arg, Command, command};

pub mod pact_broker;
pub mod pact_broker_client;
pub mod pactflow;
pub mod pactflow_client;
pub mod utils;

pub fn build_cli() -> Command {
    let app = pact_broker_client::add_pact_broker_client_command()
        .version(env!("CARGO_PKG_VERSION"))
        .about("A pact cli tool")
        .args(add_logging_arguments())
        .subcommand(
            pactflow_client::add_pactflow_client_command().version(env!("CARGO_PKG_VERSION")),
        )
        .subcommand(add_completions_subcommand());
    app
}

pub fn add_logging_arguments() -> Vec<Arg> {
    vec![
        Arg::new("log-level")
            .long("log-level")
            .global(true)
            .value_name("LEVEL")
            .help("Set the log level (none, off, error, warn, info, debug, trace)")
            .value_parser(clap::builder::PossibleValuesParser::new([
                "off", "none", "error", "warn", "info", "debug", "trace",
            ]))
            .default_value("off"),
    ]
}
pub fn add_output_arguments(
    value_parser_args: Vec<&'static str>,
    default_value: &'static str,
) -> Vec<Arg> {
    vec![
        Arg::new("output")
            .short('o')
            .long("output")
            .value_name("OUTPUT")
            .value_parser(clap::builder::PossibleValuesParser::new(&value_parser_args))
            .default_value(default_value)
            .value_name("OUTPUT")
            .help(format!("Value must be one of {:?}", value_parser_args)),
    ]
}

pub fn add_ssl_arguments() -> Vec<Arg> {
    vec![
        Arg::new("ssl-certificate")
            .short('c')
            .long("ssl-certificate")
            .num_args(1)
            .help("The path to a valid SSL certificate file")
            .required(false)
            .value_name("SSL_CERT_FILE")
            .env("SSL_CERT_FILE"),
        Arg::new("skip-ssl-verification")
            .long("skip-ssl-verification")
            .num_args(0)
            .help("Skip SSL certificate verification")
            .required(false)
            .value_name("SSL_SKIP_VERIFICATION")
            .env("SSL_SKIP_VERIFICATION"),
        Arg::new("ssl-trust-store")
            .long("ssl-trust-store")
            .num_args(1)
            .default_value("true")
            .value_parser(clap::builder::BoolValueParser::new())
            .help("Use the system's root trust store for SSL verification")
            .required(false)
            .value_name("SSL_TRUST_STORE")
            .env("SSL_TRUST_STORE"),
    ]
}

pub fn add_verbose_arguments() -> Vec<Arg> {
    vec![
        Arg::new("verbose")
            .short('v')
            .long("verbose")
            .num_args(0)
            .help("Verbose output."),
    ]
}

fn add_completions_subcommand() -> Command {
    Command::new("completions") 
    .about("Generates completion scripts for your shell")
    .arg(Arg::new("shell")
        .value_name("SHELL")
        .required(true)
        .value_parser(clap::builder::PossibleValuesParser::new(&["bash", "fish", "zsh", "powershell", "elvish"]))
        .help("The shell to generate the script for"))
    .arg(Arg::new("dir")
        .short('d')
        .long("dir")
        .value_name("DIRECTORY")
        .required(false)
        .default_value(".")
        .num_args(1)
        .value_parser(clap::builder::NonEmptyStringValueParser::new())
        .help("The directory to write the shell completions to, default is the current directory"))
}
