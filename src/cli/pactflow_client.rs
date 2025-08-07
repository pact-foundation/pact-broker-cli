use clap::{Arg, ArgMatches, Command};

use super::{add_output_arguments, add_verbose_arguments};

pub fn add_pactflow_client_command() -> Command {
    Command::new("pactflow").subcommand(add_publish_provider_contract_subcommand())
}

fn add_publish_provider_contract_subcommand() -> Command {
    Command::new("publish-provider-contract")
    .about("Publish provider contract to PactFlow")
    .args(crate::pact_broker::main::subcommands::add_broker_auth_arguments())
    .arg(Arg::new("contract-file")
        .num_args(1)
        .value_name("CONTRACT_FILE")
        .required(true)
        .help("The contract file(s)"))
    .arg(Arg::new("provider")
        .long("provider")
        .value_name("PROVIDER")
        .help("The provider name"))
    .arg(Arg::new("provider-app-version")
        .short('a')
        .long("provider-app-version")
        .value_name("PROVIDER_APP_VERSION")
        .required(true)
        .help("The provider application version"))
    .arg(Arg::new("branch")
        .long("branch")
        .value_name("BRANCH")
        .help("Repository branch of the provider version"))
    .arg(Arg::new("tag")
        .short('t')
        .long("tag")
        .value_name("TAG")
        .num_args(0..=1)
        .help("Tag name for provider version. Can be specified multiple times."))
    .arg(Arg::new("specification")
        .long("specification")
        .value_name("SPECIFICATION")
        .default_value("oas")
        .help("The contract specification"))
    .arg(Arg::new("content-type")
        .long("content-type")
        .value_name("CONTENT_TYPE")
        .help("The content type. eg. application/yml"))
    .arg(Arg::new("verification-success")
        .long("verification-success")
        .help("Whether or not the self verification passed successfully."))
    .arg(Arg::new("verification-exit-code")
        .long("verification-exit-code")
        .value_name("N")
        .help("The exit code of the verification process. Can be used instead of --verification-success|--no-verification-success for a simpler build script."))
    .arg(Arg::new("verification-results")
        .long("verification-results")
        .value_name("VERIFICATION_RESULTS")
        .help("The path to the file containing the output from the verification process"))
    .arg(Arg::new("verification-results-content-type")
        .long("verification-results-content-type")
        .value_name("VERIFICATION_RESULTS_CONTENT_TYPE")
        .help("The content type of the verification output eg. text/plain, application/yaml"))
    .arg(Arg::new("verification-results-format")
        .long("verification-results-format")
        .value_name("VERIFICATION_RESULTS_FORMAT")
        .help("The format of the verification output eg. junit, text"))
    .arg(Arg::new("verifier")
        .long("verifier")
        .value_name("VERIFIER")
        .help("The tool used to verify the provider contract"))
    .arg(Arg::new("verifier-version")
        .long("verifier-version")
        .value_name("VERIFIER_VERSION")
        .help("The version of the tool used to verify the provider contract"))
    .arg(Arg::new("build-url")
        .long("build-url")
        .value_name("BUILD_URL")
        .help("The build URL that created the provider contract"))
        .args(add_output_arguments(["json", "text"].to_vec(), "text"))
.args(add_verbose_arguments())
}

pub fn run(args: &ArgMatches) {
    match args.subcommand() {
        Some(("publish-provider-contract", args)) => {
            println!("{:?}", args);

            println!("Unimplemented");
            std::process::exit(1);
        }
        _ => {
            println!("⚠️  No option provided, try running pactflow --help");
        }
    }
}
