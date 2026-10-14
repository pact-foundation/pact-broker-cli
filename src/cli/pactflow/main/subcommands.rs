use crate::cli::add_output_arguments;
use clap::{Arg, ArgAction, ArgGroup, Command};

/// A per-contract option: repeatable, and bound to the `--contract` that precedes it.
///
/// `Append` is what makes the ordered form work — every occurrence keeps its own argv index, which
/// `ContractSpec::from_args` uses to decide which `--contract` block the value belongs to.
fn per_contract(name: &'static str, value_name: &'static str, help: &'static str) -> Arg {
    Arg::new(name)
        .long(name)
        .value_name(value_name)
        .num_args(1)
        .action(ArgAction::Append)
        .help(format!("{help} {PER_CONTRACT_NOTE}"))
}

/// Appended to every per-contract option, so reading any one of them on its own says which
/// contract it applies to — the ordering rule is the easiest part of this form to get wrong.
const PER_CONTRACT_NOTE: &str =
    "Describes the contract given by the preceding --contract, or the single CONTRACT_FILE.";

/// A per-contract boolean: a bare flag that takes no value, like any other.
///
/// `Append` rather than `SetTrue` only because `SetTrue` refuses to be repeated and `Count` keeps
/// just the last argv index, so neither can say which `--contract` the flag belongs to. Pairing
/// `Append` with `num_args(0)` keeps the per-occurrence index while `default_missing_value`
/// supplies the `true` each occurrence stands for, so nothing leaks into the help text.
fn per_contract_flag(name: &'static str, help: &'static str) -> Arg {
    Arg::new(name)
        .long(name)
        .num_args(0)
        .action(ArgAction::Append)
        .default_missing_value("true")
        .value_parser(clap::value_parser!(bool))
        .help(format!("{help} {PER_CONTRACT_NOTE}"))
}

pub fn add_publish_provider_contract_subcommand() -> Command {
    Command::new("publish-provider-contract")
    .about("Publish provider contract(s) to PactFlow")
    .args(crate::cli::pact_broker::main::subcommands::add_broker_auth_arguments())
    .arg(Arg::new("contract-file")
        .num_args(1)
        .value_name("CONTRACT_FILE")
        .help("The contract file to publish. Use --contract instead to publish several contracts in one request."))
    .arg(Arg::new("contract")
        .long("contract")
        .num_args(1)
        .action(ArgAction::Append)
        .value_name("CONTRACT_FILE")
        .help(
            "A contract file to publish, starting a new contract. Repeat --contract once per \
             contract to publish several in a single request. The options that follow a \
             --contract describe that contract and may be repeated for each one: --name \
             (required), --specification, --content-type, --verification-success, \
             --no-verification-success, --verification-exit-code, --verification-results, \
             --verification-results-content-type, --verification-results-format, --verifier, \
             --verifier-version. Every other option is shared by all the contracts in the \
             request, wherever it appears: --provider, --provider-app-version, --branch, --tag, \
             --build-url, --auto-detect-version-properties and --tag-with-git-branch. \
             Cannot be combined with the positional CONTRACT_FILE.",
        ))
    .group(ArgGroup::new("contract-source")
        .args(["contract-file", "contract"])
        .required(true)
        .multiple(false))
    .arg(per_contract("name", "NAME", "The contract name, unique within the request. Required once per --contract; not used with the positional CONTRACT_FILE.")
        .conflicts_with("contract-file"))
    .arg(Arg::new("provider")
        .long("provider")
        .value_name("PROVIDER")
        .required(true)
        .value_parser(clap::builder::NonEmptyStringValueParser::new())
        .help("The provider name. Shared by every contract in the request."))
    .arg(Arg::new("provider-app-version")
        .short('a')
        .long("provider-app-version")
        .value_name("PROVIDER_APP_VERSION")
        .required_unless_present("auto-detect-version-properties")
        .value_parser(clap::builder::NonEmptyStringValueParser::new())
        .help("The provider application version. Shared by every contract in the request."))
    .arg(Arg::new("branch")
        .long("branch")
        .value_name("BRANCH")
        .help("Repository branch of the provider version. Shared by every contract in the request."))
    .arg(Arg::new("tag")
        .short('t')
        .long("tag")
        .value_delimiter(',')
        .num_args(0..)
        .value_parser(clap::builder::NonEmptyStringValueParser::new())
        .help("Tag name for provider version. Can be specified multiple times (delimiter ,). Shared by every contract in the request."))
    // No clap default_value: a defaulted value carries no argv index, which would desync the
    // value/index pairing the ordered form relies on. "oas" is applied in code instead.
    .arg(per_contract("specification", "SPECIFICATION", "The contract specification [default: oas]"))
    .arg(per_contract("content-type", "CONTENT_TYPE", "The content type. eg. application/yml [default: application/yaml]"))
    .arg(per_contract_flag("verification-success", "Whether or not the self verification passed successfully."))
    .arg(per_contract_flag("no-verification-success", "Whether or not the self verification failed."))
    .arg(per_contract("verification-exit-code", "N", "The exit code of the verification process. Can be used instead of --verification-success|--no-verification-success for a simpler build script."))
    .arg(per_contract("verification-results", "VERIFICATION_RESULTS", "The path to the file containing the output from the verification process."))
    .arg(per_contract("verification-results-content-type", "VERIFICATION_RESULTS_CONTENT_TYPE", "The content type of the verification output eg. text/plain, application/yaml."))
    .arg(per_contract("verification-results-format", "VERIFICATION_RESULTS_FORMAT", "The format of the verification output eg. junit, text."))
    .arg(per_contract("verifier", "VERIFIER", "The tool used to verify the provider contract."))
    .arg(per_contract("verifier-version", "VERIFIER_VERSION", "The version of the tool used to verify the provider contract."))
    .arg(Arg::new("build-url")
        .long("build-url")
        .value_name("BUILD_URL")
        .help("The build URL that created the provider contract. Shared by every contract in the request."))
    .arg(Arg::new("auto-detect-version-properties")
        .short('r')
        .long("auto-detect-version-properties")
        .num_args(0)
        .action(clap::ArgAction::SetTrue)
        .help("Automatically detect the repository commit, branch and build URL from known CI environment variables or git CLI. Supports Buildkite, Circle CI, Travis CI, GitHub Actions, Jenkins, Hudson, AppVeyor, GitLab, CodeShip, Bitbucket and Azure DevOps."))
    .arg(Arg::new("tag-with-git-branch")
        // .short('g')
        .long("tag-with-git-branch")
        .num_args(0)
        .action(clap::ArgAction::SetTrue)
        .help("Tag provider version with the name of the current git branch. Supports Buildkite, Circle CI, Travis CI, GitHub Actions, Jenkins, Hudson, AppVeyor, GitLab, CodeShip, Bitbucket and Azure DevOps."))
    .args(add_output_arguments(["json", "text"].to_vec(), "text"))
    .args(crate::cli::add_ssl_arguments())
}
