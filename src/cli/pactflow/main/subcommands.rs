use crate::cli::add_output_arguments;
use crate::cli::pact_broker::main::broker_path::STRIPPED_EXTENSIONS;
use clap::{Arg, Command};

pub fn add_publish_provider_contract_subcommand() -> Command {
    Command::new("publish-provider-contract")
    .about("Publish provider contract to PactFlow")
    .args(crate::cli::pact_broker::main::subcommands::add_broker_auth_arguments())
    .arg(Arg::new("contract-file")
        .num_args(1)
        .value_name("CONTRACT_FILE")
        .required(true)
        .help("The contract file to publish, or an https://*.swaggerhub.com URL"))
    .arg(Arg::new("provider")
        .long("provider")
        .value_name("PROVIDER")
        .required(true)
        .help("The provider name"))
    .arg(Arg::new("provider-app-version")
        .short('a')
        .long("provider-app-version")
        .value_name("PROVIDER_APP_VERSION")
        .required_unless_present("auto-detect-version-properties")
        .help("The provider application version"))
    .arg(Arg::new("branch")
        .long("branch")
        .value_name("BRANCH")
        .help("Repository branch of the provider version"))
    .arg(Arg::new("tag")
        .short('t')
        .long("tag")
        .value_delimiter(',')
        .num_args(0..)
        .value_parser(clap::builder::NonEmptyStringValueParser::new())
        .help("Tag name for provider version. Can be specified multiple times (delimiter ,)."))
    .arg(Arg::new("specification")
        .long("specification")
        .value_name("SPECIFICATION")
        .default_value("oas")
        .help("The contract specification"))
    .arg(Arg::new("contract-name")
        .long("contract-name")
        .value_name("NAME")
        .allow_hyphen_values(true)
        .value_parser(parse_contract_name)
        .help("Name that tells this document apart from others of the same specification at this provider version. Needed only when a provider version has more than one document of a specification."))
    .arg(Arg::new("content-type")
        .long("content-type")
        .value_name("CONTENT_TYPE")
        .help("The content type. eg. application/yml"))
    .arg(Arg::new("verification-success")
        .long("verification-success")
        .action(clap::ArgAction::SetTrue)
        .conflicts_with("verification-exit-code")
        .help("Whether or not the self verification passed successfully."))
    .arg(Arg::new("no-verification-success")
        .long("no-verification-success")
        .action(clap::ArgAction::SetTrue)
        .conflicts_with("verification-exit-code")
        .help("Whether or not the self verification failed."))
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

pub fn add_delete_provider_contract_subcommand() -> Command {
    Command::new("delete-provider-contract")
        .about("Delete one provider contract of a provider version from PactFlow")
        .args(crate::cli::pact_broker::main::subcommands::add_broker_auth_arguments())
        .arg(Arg::new("provider")
            .long("provider")
            .value_name("PROVIDER")
            .required(true)
            .help("The provider name"))
        .arg(Arg::new("provider-app-version")
            .short('a')
            .long("provider-app-version")
            .value_name("PROVIDER_APP_VERSION")
            .required(true)
            .help("The provider application version"))
        .arg(Arg::new("specification")
            .long("specification")
            .value_name("SPECIFICATION")
            .default_value("oas")
            .help("The contract specification"))
        .arg(Arg::new("contract-name")
            .long("contract-name")
            .value_name("NAME")
            .allow_hyphen_values(true)
            .value_parser(parse_contract_name)
            .help("Name of the document to delete. Omit it to delete the unnamed document of the specification."))
        .args(crate::cli::add_ssl_arguments())
}

/// A contract name starts with a letter or digit, then allows `.`, `_` and `-`, up to 64 characters.
/// It may not end in a suffix PactFlow reads as a file-format request, since a document name ending
/// in one could never be addressed.
fn parse_contract_name(value: &str) -> Result<String, String> {
    static PATTERN: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$").unwrap()
    });
    if !PATTERN.is_match(value) {
        return Err("must match ^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$".to_string());
    }
    if let Some(suffix) = STRIPPED_EXTENSIONS
        .iter()
        .find(|suffix| value.ends_with(**suffix))
    {
        return Err(format!(
            "may not end in {suffix} because PactFlow reads it as a file-format request"
        ));
    }
    Ok(value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_a_plain_name() {
        assert_eq!(parse_contract_name("admin"), Ok("admin".to_string()));
        assert_eq!(
            parse_contract_name("orders.v2"),
            Ok("orders.v2".to_string())
        );
    }

    #[test]
    fn rejects_a_name_outside_the_pattern() {
        assert!(parse_contract_name("-admin").is_err());
        assert!(parse_contract_name("a b").is_err());
    }

    #[test]
    fn rejects_each_stripped_suffix() {
        for suffix in STRIPPED_EXTENSIONS {
            let err = parse_contract_name(&format!("orders{suffix}")).unwrap_err();
            assert_eq!(
                err,
                format!(
                    "may not end in {suffix} because PactFlow reads it as a file-format request"
                )
            );
        }
    }

    #[test]
    fn accepts_a_near_miss_of_a_stripped_suffix() {
        for name in [
            "json-api",
            "orders.json2",
            "orders-json",
            "yaml",
            "orders.JSON",
        ] {
            assert_eq!(parse_contract_name(name), Ok(name.to_string()), "{name}");
        }
    }

    /// `delete-provider-contract` with a broker URL, then `extra`.
    fn parse_delete(extra: &[&str]) -> Result<clap::ArgMatches, clap::Error> {
        let mut argv = vec!["delete-provider-contract", "-b", "http://localhost:1"];
        argv.extend_from_slice(extra);
        add_delete_provider_contract_subcommand().try_get_matches_from(argv)
    }

    #[test]
    fn delete_defaults_the_specification_to_oas_and_needs_no_name() {
        let matches = parse_delete(&["--provider", "Bar", "-a", "1"]).unwrap();
        assert_eq!(
            matches
                .get_one::<String>("specification")
                .map(String::as_str),
            Some("oas")
        );
        assert_eq!(matches.get_one::<String>("contract-name"), None);
    }

    #[test]
    fn delete_requires_the_provider_and_its_version() {
        for argv in [&["-a", "1"][..], &["--provider", "Bar"][..]] {
            let error = parse_delete(argv).unwrap_err();
            assert_eq!(
                error.kind(),
                clap::error::ErrorKind::MissingRequiredArgument,
                "{argv:?}"
            );
        }
    }

    #[test]
    fn delete_validates_the_contract_name() {
        let matches = parse_delete(&[
            "--provider",
            "Bar",
            "-a",
            "1",
            "--specification",
            "asyncapi",
            "--contract-name",
            "events",
        ])
        .unwrap();
        assert_eq!(
            matches
                .get_one::<String>("contract-name")
                .map(String::as_str),
            Some("events")
        );
        let error = parse_delete(&["--provider", "Bar", "-a", "1", "--contract-name", "e.zip"])
            .unwrap_err();
        assert_eq!(error.kind(), clap::error::ErrorKind::ValueValidation);
    }
}
