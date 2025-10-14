use crate::cli::utils;
use clap::{Arg, ArgGroup, Command};

pub fn add_broker_auth_arguments() -> Vec<Arg> {
    vec![
        Arg::new("broker-base-url")
            .short('b')
            .long("broker-base-url")
            .num_args(1)
            .help("The base URL of the Pact Broker")
            .required(true)
            .value_name("PACT_BROKER_BASE_URL")
            .env("PACT_BROKER_BASE_URL"),
        Arg::new("broker-username")
            .short('u')
            .long("broker-username")
            .num_args(1)
            .help("Pact Broker basic auth username")
            .value_name("PACT_BROKER_USERNAME")
            .env("PACT_BROKER_USERNAME"),
        Arg::new("broker-password")
            .short('p')
            .long("broker-password")
            .num_args(1)
            .help("Pact Broker basic auth password")
            .value_name("PACT_BROKER_PASSWORD")
            .env("PACT_BROKER_PASSWORD"),
        Arg::new("broker-token")
            .short('k')
            .long("broker-token")
            .num_args(1)
            .help("Pact Broker bearer token")
            .value_name("PACT_BROKER_TOKEN")
            .env("PACT_BROKER_TOKEN"),
    ]
}
pub fn add_publish_pacts_subcommand() -> Command {
    Command::new("publish")
    .args(add_broker_auth_arguments())
    .about("Publishes pacts to the Pact Broker")
    .arg(Arg::new("pact-files-dirs-or-globs")
        .value_name("PACT_FILES_DIRS_OR_GLOBS")
        .help("Pact files, directories or glob patterns containing pact files to publish (can be repeated)")
        .long_help("
Glob pattern to match pact files to publish

?      matches any single character.
*      matches any (possibly empty) sequence of characters.
**     matches the current directory and arbitrary subdirectories. This sequence must form
         a single path component, so both **a and b** are invalid and will result in an
         error. A sequence of more than two consecutive * characters is also invalid.
[...]  matches any character inside the brackets. Character sequences can also specify
         ranges of characters, as ordered by Unicode, so e.g. [0-9] specifies any character
         between 0 and 9 inclusive. An unclosed bracket is invalid.
[!...] is the negation of [...], i.e. it matches any characters not in the brackets.

The metacharacters ?, *, [, ] can be matched by using brackets (e.g. [?]). When a ]
occurs immediately following [ or [! then it is interpreted as being part of, rather
then ending, the character set, so ] and NOT ] can be matched by []] and [!]] respectively.
The - character can be specified inside a character sequence pattern by placing it at
the start or the end, e.g. [abc-].

See https://docs.rs/glob/0.3.0/glob/struct.Pattern.html")
        .required(true)
        .num_args(1..)
        .action(clap::ArgAction::Append))
.arg(Arg::new("validate")
.long("validate")
// .short('v')
.num_args(0)
.action(clap::ArgAction::SetTrue)
.help("Validate the Pact files before publishing."))
.arg(Arg::new("strict")
.long("strict")
// .short('v')
.num_args(0)
.action(clap::ArgAction::SetTrue)
.help("Require strict validation."))
.arg(Arg::new("consumer-app-version")
   .short('a')
   .long("consumer-app-version")
   .value_parser(clap::builder::NonEmptyStringValueParser::new())
   .help("The consumer application version")
   .required_unless_present("auto-detect-version-properties"))
.arg(Arg::new("branch")
   // .short('h')
   .long("branch")
   .value_parser(clap::builder::NonEmptyStringValueParser::new())
   .help("Repository branch of the consumer version"))
.arg(Arg::new("auto-detect-version-properties")
   .short('r')
   .long("auto-detect-version-properties")
   .num_args(0)
   .action(clap::ArgAction::SetTrue)
   .help("Automatically detect the repository commit, branch and build URL from known CI environment variables or git CLI. Supports Buildkite, Circle CI, Travis CI, GitHub Actions, Jenkins, Hudson, AppVeyor, GitLab, CodeShip, Bitbucket and Azure DevOps."))
.arg(Arg::new("tag")
   .short('t')
   .long("tag")
   .value_delimiter(',')
   .num_args(0..)
   .value_parser(clap::builder::NonEmptyStringValueParser::new())
   .help("Tag name for consumer version. Can be specified multiple times (delimiter ,)."))
.arg(Arg::new("tag-with-git-branch")
   // .short('g')
   .long("tag-with-git-branch")
   .num_args(0)
   .action(clap::ArgAction::SetTrue)
   .help("Tag consumer version with the name of the current git branch. Supports Buildkite, Circle CI, Travis CI, GitHub Actions, Jenkins, Hudson, AppVeyor, GitLab, CodeShip, Bitbucket and Azure DevOps."))
.arg(Arg::new("build-url")
   .long("build-url")
   .num_args(1)
   .help("The build URL that created the pact"))
.arg(Arg::new("merge")
   .long("merge")
   .num_args(0)
   .action(clap::ArgAction::SetTrue)
   .help("If a pact already exists for this consumer version and provider, merge the contents. Useful when running Pact tests concurrently on different build nodes."))
.args(crate::cli::add_output_arguments(["json", "text", "pretty"].to_vec(),"text"))
.args(crate::cli::add_verbose_arguments())
}
pub fn add_list_latest_pact_versions_subcommand() -> Command {
    Command::new("list-latest-pact-versions")
        .about("List the latest pact for each integration")
        .args(add_broker_auth_arguments())
        .args(crate::cli::add_verbose_arguments())
        .args(crate::cli::add_output_arguments(
            ["json", "table"].to_vec(),
            "table",
        ))
}
pub fn add_create_environment_subcommand() -> Command {
    Command::new("create-environment")
    .about("Create an environment resource in the Pact Broker to represent a real world deployment or release environment")
    .arg(Arg::new("name")
        .long("name")
        .value_name("NAME")
        .required(true)
        .help("The uniquely identifying name of the environment as used in deployment code"))
    .arg(Arg::new("display-name")
        .long("display-name")
        .value_name("DISPLAY_NAME")
        .help("The display name of the environment"))
    .arg(Arg::new("production")
        .long("production")
        .action(clap::ArgAction::SetTrue)
        .help("Whether or not this environment is a production environment. This is currently informational only."))
    .arg(Arg::new("contact-name")
        .long("contact-name")
        .value_name("CONTACT_NAME")
        .help("The name of the team/person responsible for this environment"))
    .arg(Arg::new("contact-email-address")
        .long("contact-email-address")
        .value_name("CONTACT_EMAIL_ADDRESS")
        .help("The email address of the team/person responsible for this environment"))
        .args(crate::cli::add_output_arguments(["json", "text", "id"].to_vec(), "text"))

.args(add_broker_auth_arguments())
.args(crate::cli::add_verbose_arguments())
}
pub fn add_update_environment_subcommand() -> Command {
    Command::new("update-environment")
    .about("Update an environment resource in the Pact Broker")
    .arg(Arg::new("uuid")
        .long("uuid")
        .value_name("UUID")
        .required(true)
        .help("The UUID of the environment to update"))
    .arg(Arg::new("name")
        .long("name")
        .value_name("NAME")
        .help("The uniquely identifying name of the environment as used in deployment code"))
    .arg(Arg::new("display-name")
        .long("display-name")
        .value_name("DISPLAY_NAME")
        .help("The display name of the environment"))
    .arg(Arg::new("production")
        .long("production")
        .action(clap::ArgAction::SetTrue)
        .help("Whether or not this environment is a production environment. This is currently informational only."))
    .arg(Arg::new("contact-name")
        .long("contact-name")
        .value_name("CONTACT_NAME")
        .help("The name of the team/person responsible for this environment"))
    .arg(Arg::new("contact-email-address")
        .long("contact-email-address")
        .value_name("CONTACT_EMAIL_ADDRESS")
        .help("The email address of the team/person responsible for this environment"))
        .args(crate::cli::add_output_arguments(["json", "text", "id"].to_vec(), "text"))
.args(add_broker_auth_arguments())
.args(crate::cli::add_verbose_arguments())
}
pub fn add_describe_environment_subcommand() -> Command {
    Command::new("describe-environment")
        .about("Describe an environment")
        .arg(
            Arg::new("uuid")
                .long("uuid")
                .value_name("UUID")
                .required(true)
                .help("The UUID of the environment to describe"),
        )
        .args(crate::cli::add_output_arguments(
            ["json", "text"].to_vec(),
            "text",
        ))
        .args(add_broker_auth_arguments())
        .args(crate::cli::add_verbose_arguments())
}
pub fn add_delete_environment_subcommand() -> Command {
    Command::new("delete-environment")
        .about("Delete an environment")
        .arg(
            Arg::new("uuid")
                .long("uuid")
                .value_name("UUID")
                .required(true)
                .help("The UUID of the environment to delete"),
        )
        .args(add_broker_auth_arguments())
        .args(crate::cli::add_verbose_arguments())
}
pub fn add_list_environments_subcommand() -> Command {
    Command::new("list-environments")
        .about("List environments")
        .args(crate::cli::add_output_arguments(
            ["json", "text", "pretty"].to_vec(),
            "text",
        ))
        .args(add_broker_auth_arguments())
        .args(crate::cli::add_verbose_arguments())
}
pub fn add_record_deployment_subcommand() -> Command {
    Command::new("record-deployment")
    .about("Record deployment of a pacticipant version to an environment")
    .arg(Arg::new("pacticipant")
        .short('a')
        .long("pacticipant")
        .value_name("PACTICIPANT")
        .value_parser(clap::builder::NonEmptyStringValueParser::new())
        .required(true)
        .help("The name of the pacticipant that was deployed"))
    .arg(Arg::new("version")
        .short('e')
        .long("version")
        .value_name("VERSION")
        .value_parser(clap::builder::NonEmptyStringValueParser::new())
        .required(true)
        .help("The pacticipant version number that was deployed"))
    .arg(Arg::new("environment")
        .long("environment")
        .value_name("ENVIRONMENT")
        .value_parser(clap::builder::NonEmptyStringValueParser::new())
        .required(true)
        .help("The name of the environment that the pacticipant version was deployed to"))
    .arg(Arg::new("application-instance")
        .long("application-instance")
        .value_name("APPLICATION_INSTANCE")
        .alias("target")
        .value_parser(clap::builder::NonEmptyStringValueParser::new())
        .help("Optional. The application instance to which the deployment has occurred - a logical identifer required to differentiate deployments when there are multiple instances of the same application in an environment. This field was called 'target' in a beta release"))
    .args(crate::cli::add_output_arguments(
        ["json", "text", "pretty"].to_vec(),
        "text",
    ))

.args(add_broker_auth_arguments())
.args(crate::cli::add_verbose_arguments())
}
pub fn add_record_undeployment_subcommand() -> Command {
    Command::new("record-undeployment")
    .about("Record undeployment of a pacticipant version from an environment")
    .long_about("Record undeployment of a pacticipant version from an environment.\n\nNote that use of this command is only required if you are permanently removing an application instance from an environment. It is not required if you are deploying over a previous version, as record-deployment will automatically mark the previously deployed version as undeployed for you. See https://docs.pact.io/go/record-undeployment for more information.")
    .arg(Arg::new("pacticipant")
        .short('a')
        .long("pacticipant")
        .value_name("PACTICIPANT")
        .value_parser(clap::builder::NonEmptyStringValueParser::new())
        .required(true)
        .help("The name of the pacticipant that was undeployed"))
    .arg(Arg::new("environment")
        .long("environment")
       .value_name("ENVIRONMENT")
        .value_parser(clap::builder::NonEmptyStringValueParser::new())
        .required(true)
        .help("The name of the environment that the pacticipant version was undeployed from"))
    .arg(Arg::new("application-instance")
        .long("application-instance")
        .alias("target")
        .value_name("APPLICATION_INSTANCE")
        .value_parser(clap::builder::NonEmptyStringValueParser::new())
        .help("Optional. The application instance from which the application is being undeployed - a logical identifer required to differentiate deployments when there are multiple instances of the same application in an environment. This field was called 'target' in a beta release"))

    .args(add_broker_auth_arguments())
    .args(crate::cli::add_verbose_arguments())
    .args(crate::cli::add_output_arguments(
        ["json", "text", "pretty"].to_vec(),
        "text",
    ))
}
pub fn add_record_release_subcommand() -> Command {
    Command::new("record-release")
        .about("Record release of a pacticipant version to an environment.")
        .arg(
            Arg::new("pacticipant")
                .short('a')
                .long("pacticipant")
                .value_name("PACTICIPANT")
                .required(true)
                .help("The name of the pacticipant that was released."),
        )
        .arg(
            Arg::new("version")
                .short('e')
                .long("version")
                .value_name("VERSION")
                .required(true)
                .help("The pacticipant version number that was released."),
        )
        .arg(
            Arg::new("environment")
                .long("environment")
                .value_name("ENVIRONMENT")
                .required(true)
                .help("The name of the environment that the pacticipant version was released to."),
        )
        .args(crate::cli::add_output_arguments(
            ["json", "text", "pretty"].to_vec(),
            "text",
        ))
        .args(add_broker_auth_arguments())
        .args(crate::cli::add_verbose_arguments())
}
pub fn add_record_support_ended_subcommand() -> Command {
    Command::new("record-support-ended")
        .about("Record the end of support for a pacticipant version in an environment.")
        .arg(
            Arg::new("pacticipant")
                .short('a')
                .long("pacticipant")
                .value_name("PACTICIPANT")
                .required(true)
                .help("The name of the pacticipant."),
        )
        .arg(
            Arg::new("version")
                .short('e')
                .long("version")
                .value_name("VERSION")
                .required(true)
                .help("The pacticipant version number for which support is ended."),
        )
        .arg(
            Arg::new("environment")
                .long("environment")
                .value_name("ENVIRONMENT")
                .required(true)
                .help("The name of the environment in which the support is ended."),
        )
        .args(crate::cli::add_output_arguments(
            ["json", "text", "pretty"].to_vec(),
            "text",
        ))
        .args(add_broker_auth_arguments())
        .args(crate::cli::add_verbose_arguments())
}
pub fn add_can_i_deploy_subcommand() -> Command {
    Command::new("can-i-deploy")
    .about("Check if a pacticipant can be deployed.")
    .long_about(
    r"
    Check if a pacticipant can be deployed.

    Description:
    Returns exit code 0 or 1, indicating whether or not the specified application (pacticipant) has a successful verification result with
    each of the application versions that are already deployed to a particular environment. Prints out the relevant pact/verification
    details, indicating any missing or failed verification results.
  
    The can-i-deploy tool was originally written to support specifying versions and dependencies using tags. This usage has now been
    superseded by first class support for environments, deployments and releases. For documentation on how to use can-i-deploy with tags,
    please see https://docs.pact.io/pact_broker/client_cli/can_i_deploy_usage_with_tags/
  
    Before `can-i-deploy` can be used, the relevant environment resources must first be created in the Pact Broker using the
    `create-environment` command. The 'test' and 'production' environments will have been seeded for you. You can check the existing
    environments by running `pact-broker-cli list-environments`. See https://docs.pact.io/pact_broker/client_cli/readme#environments for more
    information.

    $ pact-broker-cli create-environment --name 'uat' --display-name 'UAT' --no-production

    After an application is deployed or released, its deployment must be recorded using the `record-deployment` or `record-release`
    commands. See https://docs.pact.io/pact_broker/recording_deployments_and_releases/ for more information.
  
    $ pact-broker-cli record-deployment --pacticipant Foo --version 173153ae0 --environment uat
  
    Before an application is deployed or released to an environment, the can-i-deploy command must be run to check that the application
    version is safe to deploy with the versions of each integrated application that are already in that environment.
  
    $ pact-broker-cli can-i-deploy --pacticipant PACTICIPANT --version VERSION --to-environment ENVIRONMENT
  
    Example: can I deploy version 173153ae0 of application Foo to the test environment?
  
    $ pact-broker-cli can-i-deploy --pacticipant Foo --version 173153ae0 --to-environment test
  
    Can-i-deploy can also be used to check if arbitrary versions have a successful verification. When asking 'Can I deploy this
    application version with the latest version from the main branch of another application' it functions as a 'can I merge' check.
  
    $ pact-broker-cli can-i-deploy --pacticipant Foo 173153ae0 \\ --pacticipant Bar --latest main
  
    ##### Polling
  
    If the verification process takes a long time and there are results missing when the can-i-deploy command runs in your CI/CD pipeline,
    you can configure the command to poll and wait for the missing results to arrive. The arguments to specify are `--retry-while-unknown
    TIMES` and `--retry-interval SECONDS`, set to appropriate values for your pipeline.
    "
    )
    .arg(Arg::new("pacticipant")
        .short('a')
        .long("pacticipant")
        .value_name("PACTICIPANT")
        .required(true)
        .num_args(1)
        .action(clap::ArgAction::Append)
        .help("The pacticipant name. Use once for each pacticipant being checked. The following options (--version, --latest, --tag, --branch, --main-branch, --no-main-branch, --skip-main-branch) must come after each --pacticipant."))
    .arg(Arg::new("version")
        .short('e')
        .long("version")
        .value_name("VERSION")
        .num_args(1)
        .action(clap::ArgAction::Append)
        .help("The pacticipant version. Must be entered after the --pacticipant that it relates to."))
    .arg(Arg::new("latest")
        .short('l')
        .long("latest")
        .num_args(0)
        .action(clap::ArgAction::SetTrue)
        .action(clap::ArgAction::Append)
        .help("Use the latest pacticipant version. Optionally specify a TAG to use the latest version with the specified tag. Must be entered after the --pacticipant that it relates to."))
    .arg(Arg::new("tag")
        .long("tag")
        .value_name("TAG")
        .num_args(1)
        .action(clap::ArgAction::Append)
        .help("The tag of the version for which you want to check the verification results. Must be entered after the --pacticipant that it relates to."))
    .arg(Arg::new("branch")
        .long("branch")
        .value_name("BRANCH")
        .num_args(1)
        .action(clap::ArgAction::Append)
        .help("The branch of the version for which you want to check the verification results. Must be entered after the --pacticipant that it relates to."))
    .arg(Arg::new("main-branch")
        .long("main-branch")
        .num_args(0)
        .action(clap::ArgAction::SetTrue)
        .action(clap::ArgAction::Append)
        .conflicts_with_all(&["no-main-branch", "skip-main-branch"])
        .help("Use the latest version of the configured main branch of the pacticipant as the version for which you want to check the verification results. Must be entered after the --pacticipant that it relates to."))
      .group(ArgGroup::new("pacticipants")
        .args(["pacticipant", "version", "latest", "tag", "branch", "main-branch", "no-main-branch", "skip-main-branch"].to_vec())
        .multiple(true))
    .arg(Arg::new("no-main-branch")
        .long("no-main-branch")
        .action(clap::ArgAction::Append)
        .conflicts_with_all(&["main-branch", "skip-main-branch"])
        .help("Do not use the main branch of the pacticipant as the version for which you want to check the verification results. Must be entered after the --pacticipant that it relates to."))
    .arg(Arg::new("skip-main-branch")
        .long("skip-main-branch")
        .action(clap::ArgAction::Append)
        .conflicts_with_all(&["main-branch", "no-main-branch"])
        .help("Skip the configured main branch of the pacticipant as the version for which you want to check the verification results. Must be entered after the --pacticipant that it relates to."))
    .arg(Arg::new("ignore")
        .long("ignore")
        .num_args(1)
        .action(clap::ArgAction::Append)
        .help("The pacticipant name to ignore. Use once for each pacticipant being ignored. A specific version can be ignored by also specifying a --version after the pacticipant name option. The environment variable PACT_BROKER_CAN_I_DEPLOY_IGNORE may also be used to specify a pacticipant name to ignore, with commas to separate multiple pacticipant names if necessary."))
    .arg(Arg::new("to-environment")
        .long("to-environment")
        .value_name("ENVIRONMENT")
        .help("The environment into which the pacticipant(s) are to be deployed"))
    .arg(Arg::new("to")
        .long("to")
        .value_name("TO")
        .help("The tag that represents the branch or environment of the integrated applications for which you want to check the verification result status."))
    .args(crate::cli::add_output_arguments(["json", "table"].to_vec(), "table"))
    .arg(Arg::new("retry-while-unknown")
        .long("retry-while-unknown")
        .value_name("TIMES")
        .help("The number of times to retry while there is an unknown verification result (ie. the provider verification is likely still running)"))
    .arg(Arg::new("retry-interval")
        .long("retry-interval")
        .value_name("SECONDS")
        .help("The time between retries in seconds. Use in conjuction with --retry-while-unknown"))
    .arg(Arg::new("dry-run")
        .long("dry-run")
        .num_args(0)
        .action(clap::ArgAction::SetTrue)
        .help("When dry-run is enabled, always exit process with a success code. Can also be enabled by setting the environment variable PACT_BROKER_CAN_I_DEPLOY_DRY_RUN=true. This mode is useful when setting up your CI/CD pipeline for the first time, or in a 'break glass' situation where you need to knowingly deploy what Pact considers a breaking change. For the second scenario, it is recommended to use the environment variable and just set it for the build required to deploy that particular version, so you don't accidentally leave the dry run mode enabled."))

.args(add_broker_auth_arguments())
.args(crate::cli::add_verbose_arguments())
}
pub fn add_can_i_merge_subcommand() -> Command {
    Command::new("can-i-merge")
    .about("Checks if the specified pacticipant version is compatible with the configured main branch of each of the pacticipants with which it is integrated.")
    .args(add_broker_auth_arguments())
    .arg(Arg::new("pacticipant")
        .short('a')
        .long("pacticipant")
        .value_name("PACTICIPANT")
        .required(true)
        .num_args(1)
        .action(clap::ArgAction::Append)
        .help("The pacticipant name. Use once for each pacticipant being checked. The following options (--version, --latest, --tag, --branch) must come after each --pacticipant."))
    .arg(Arg::new("version")
        .short('e')
        .long("version")
        .value_name("VERSION")
        .num_args(1)
        .action(clap::ArgAction::Append)
        .help("The pacticipant version. Must be entered after the --pacticipant that it relates to."))
        .args(crate::cli::add_output_arguments(["json", "table"].to_vec(), "table"))
    .arg(Arg::new("retry-while-unknown")
        .long("retry-while-unknown")
        .value_name("TIMES")
        .help("The number of times to retry while there is an unknown verification result (ie. the provider verification is likely still running)"))
    .arg(Arg::new("retry-interval")
        .long("retry-interval")
        .value_name("SECONDS")
        .help("The time between retries in seconds. Use in conjuction with --retry-while-unknown"))
    .arg(Arg::new("dry-run")
        .long("dry-run")
        .num_args(0)
        .action(clap::ArgAction::SetTrue)
        .help("When dry-run is enabled, always exit process with a success code. Can also be enabled by setting the environment variable PACT_BROKER_CAN_I_DEPLOY_DRY_RUN=true. This mode is useful when setting up your CI/CD pipeline for the first time, or in a 'break glass' situation where you need to knowingly deploy what Pact considers a breaking change. For the second scenario, it is recommended to use the environment variable and just set it for the build required to deploy that particular version, so you don't accidentally leave the dry run mode enabled."))

.args(crate::cli::add_verbose_arguments())
}
pub fn add_create_or_update_pacticipant_subcommand() -> Command {
    Command::new("create-or-update-pacticipant")
        .about("Create or update pacticipant by name")
        .args(add_broker_auth_arguments())
        .arg(
            Arg::new("name")
                .long("name")
                .value_name("NAME")
                .required(true)
                .help("Pacticipant name"),
        )
        .arg(
            Arg::new("display-name")
                .long("display-name")
                .value_name("DISPLAY_NAME")
                .help("Display name"),
        )
        .arg(
            Arg::new("main-branch")
                .long("main-branch")
                .value_name("MAIN_BRANCH")
                .help("The main development branch of the pacticipant repository"),
        )
        .arg(
            Arg::new("repository-url")
                .long("repository-url")
                .value_name("REPOSITORY_URL")
                .help("The repository URL of the pacticipant"),
        )
        .args(crate::cli::add_output_arguments(
            ["json", "text"].to_vec(),
            "text",
        ))
        .args(crate::cli::add_verbose_arguments())
}
pub fn add_describe_pacticipant_subcommand() -> Command {
    Command::new("describe-pacticipant")
        .about("Describe a pacticipant")
        .args(add_broker_auth_arguments())
        .arg(
            Arg::new("name")
                .long("name")
                .value_name("NAME")
                .required(true)
                .help("Pacticipant name"),
        )
        .args(crate::cli::add_output_arguments(
            ["json", "text", "table"].to_vec(),
            "text",
        ))
        .args(crate::cli::add_verbose_arguments())
}
pub fn add_list_pacticipants_subcommand() -> Command {
    Command::new("list-pacticipants")
        .about("List pacticipants")
        .args(add_broker_auth_arguments())
        .args(crate::cli::add_output_arguments(
            ["json", "table"].to_vec(),
            "table",
        ))
        .args(crate::cli::add_verbose_arguments())
}
pub fn add_create_webhook_subcommand() -> Command {
    Command::new("create-webhook")
    .about("Create a webhook")
    .arg(Arg::new("url")
        .value_name("URL")
        .required(true)
        .help("Webhook URL"))
    .arg(Arg::new("request")
        .short('X')
        .long("request")
        .value_name("METHOD")
        .help("Webhook HTTP method"))
    .arg(Arg::new("header")
        .short('H')
        .long("header")
        .value_name("one two three")
        .num_args(0..=1)
        .value_delimiter(' ')
        .help("Webhook Header"))
    .arg(Arg::new("data")
        .short('d')
        .long("data")
        .value_name("DATA")
        .help("Webhook payload"))
    .arg(Arg::new("user")
        // .short('u')
        .long("user")
        .value_name("USER")
        .help("Webhook basic auth username and password eg. username:password"))
    .arg(Arg::new("consumer")
        .long("consumer")
        .value_name("CONSUMER")
        .help("Consumer name"))
    .arg(Arg::new("consumer-label")
        .long("consumer-label")
        .value_name("CONSUMER_LABEL")
        .help("Consumer label, mutually exclusive with consumer name"))
    .arg(Arg::new("provider")
        .long("provider")
        .value_name("PROVIDER")
        .help("Provider name"))
    .arg(Arg::new("provider-label")
        .long("provider-label")
        .value_name("PROVIDER_LABEL")
        .help("Provider label, mutually exclusive with provider name"))
    .arg(Arg::new("description")
        .long("description")
        .value_name("DESCRIPTION")
        .help("Webhook description"))
    .arg(Arg::new("contract-content-changed")
        .long("contract-content-changed")
        .num_args(0)
        .default_value("false")
        .action(clap::ArgAction::SetTrue)
        .help("Trigger this webhook when the pact content changes"))
    .arg(Arg::new("contract-published")
        .long("contract-published")
        .num_args(0)
        .default_value("false")
        .action(clap::ArgAction::SetTrue)
        .help("Trigger this webhook when a pact is published"))
    .arg(Arg::new("provider-verification-published")
        .long("provider-verification-published")
        .num_args(0)
        .default_value("false")
        .action(clap::ArgAction::SetTrue)
        .help("Trigger this webhook when a provider verification result is published"))
    .arg(Arg::new("provider-verification-failed")
        .long("provider-verification-failed")
        .num_args(0)
        .default_value("false")
        .action(clap::ArgAction::SetTrue)
        .help("Trigger this webhook when a failed provider verification result is published"))
    .arg(Arg::new("provider-verification-succeeded")
        .long("provider-verification-succeeded")
        .num_args(0)
        .default_value("false")
        .action(clap::ArgAction::SetTrue)
        .help("Trigger this webhook when a successful provider verification result is published"))
    .arg(Arg::new("contract-requiring-verification-published")
        .long("contract-requiring-verification-published")
        .num_args(0)
        .default_value("false")
        .action(clap::ArgAction::SetTrue)
        .help("Trigger this webhook when a contract is published that requires verification"))
    .arg(Arg::new("team-uuid")
        .long("team-uuid")
        .value_name("UUID")
        .help("UUID of the PactFlow team to which the webhook should be assigned (PactFlow only)"))

.args(add_broker_auth_arguments())
.args(crate::cli::add_verbose_arguments())
}
pub fn add_create_or_update_webhook_subcommand() -> Command {
    Command::new("create-or-update-webhook")
    .about("Create or update a webhook")
    .args(add_broker_auth_arguments())
    .arg(Arg::new("url")
        .value_name("URL")
        .required(true)
        .help("Webhook URL"))
    .arg(Arg::new("uuid")
        .long("uuid")
        .value_name("UUID")
        .required(true)
        .help("Specify the uuid for the webhook"))
    .arg(Arg::new("request")
        .short('X')
        .long("request")
        .value_name("METHOD")
        .help("Webhook HTTP method"))
   .arg(Arg::new("header")
        .short('H')
        .long("header")
        .value_name("one two three")
        .num_args(0..=1)
        .value_delimiter(' ')
        .help("Webhook Header"))
    .arg(Arg::new("data")
        .short('d')
        .long("data")
        .value_name("DATA")
        .help("Webhook payload"))
    .arg(Arg::new("user")
        // .short('u')
        .long("user")
        .value_name("USER")
        .help("Webhook basic auth username and password eg. username:password"))
    .arg(Arg::new("consumer")
        .long("consumer")
        .value_name("CONSUMER")
        .help("Consumer name"))
    .arg(Arg::new("consumer-label")
        .long("consumer-label")
        .value_name("CONSUMER_LABEL")
        .help("Consumer label, mutually exclusive with consumer name"))
    .arg(Arg::new("provider")
        .long("provider")
        .value_name("PROVIDER")
        .help("Provider name"))
    .arg(Arg::new("provider-label")
        .long("provider-label")
        .value_name("PROVIDER_LABEL")
        .help("Provider label, mutually exclusive with provider name"))
    .arg(Arg::new("description")
        .long("description")
        .value_name("DESCRIPTION")
        .help("Webhook description"))
       .arg(Arg::new("contract-content-changed")
        .long("contract-content-changed")
        .num_args(0)
        .default_value("false")
        .action(clap::ArgAction::SetTrue)
        .help("Trigger this webhook when the pact content changes"))
    .arg(Arg::new("contract-published")
        .long("contract-published")
        .num_args(0)
        .default_value("false")
        .action(clap::ArgAction::SetTrue)
        .help("Trigger this webhook when a pact is published"))
    .arg(Arg::new("provider-verification-published")
        .long("provider-verification-published")
        .num_args(0)
        .default_value("false")
        .action(clap::ArgAction::SetTrue)
        .help("Trigger this webhook when a provider verification result is published"))
    .arg(Arg::new("provider-verification-failed")
        .long("provider-verification-failed")
        .num_args(0)
        .default_value("false")
        .action(clap::ArgAction::SetTrue)
        .help("Trigger this webhook when a failed provider verification result is published"))
    .arg(Arg::new("provider-verification-succeeded")
        .long("provider-verification-succeeded")
        .num_args(0)
        .default_value("false")
        .action(clap::ArgAction::SetTrue)
        .help("Trigger this webhook when a successful provider verification result is published"))
    .arg(Arg::new("contract-requiring-verification-published")
        .long("contract-requiring-verification-published")
        .num_args(0)
        .default_value("false")
        .action(clap::ArgAction::SetTrue)
        .help("Trigger this webhook when a contract is published that requires verification"))
    .arg(Arg::new("team-uuid")
        .long("team-uuid")
        .value_name("UUID")
        .help("UUID of the PactFlow team to which the webhook should be assigned (PactFlow only)"))
        .args(crate::cli::add_verbose_arguments())
}
pub fn add_test_webhook_subcommand() -> Command {
    Command::new("test-webhook")
        .about("Test a webhook")
        .arg(
            Arg::new("uuid")
                .long("uuid")
                .value_name("UUID")
                .num_args(1)
                .required(true)
                .help("Specify the uuid for the webhook"),
        )
        .args(add_broker_auth_arguments())
        .args(crate::cli::add_verbose_arguments())
}
pub fn add_delete_branch_subcommand() -> Command {
    Command::new("delete-branch")
    .about("Deletes a pacticipant branch. Does not delete the versions or pacts/verifications associated with the branch, but does make the pacts inaccessible for verification via consumer versions selectors or WIP pacts.")
    .args(add_broker_auth_arguments())
    .arg(Arg::new("branch")
        .long("branch")
        .value_name("BRANCH")
        .required(true)
        .help("The pacticipant branch name"))
    .arg(Arg::new("pacticipant")
        .short('a')
        .long("pacticipant")
        .value_name("PACTICIPANT")
        .required(true)
        .help("The name of the pacticipant that the branch belongs to"))
    .arg(Arg::new("error-when-not-found")
        .long("error-when-not-found")
        .num_args(1)
        .action(clap::ArgAction::SetTrue)
        .help("Raise an error if the branch that is to be deleted is not found"))
    .args(crate::cli::add_verbose_arguments())
}
pub fn add_create_version_tag_subcommand() -> Command {
    Command::new("create-version-tag")
        .about("Add a tag to a pacticipant version")
        .args(add_broker_auth_arguments())
        .arg(
            Arg::new("pacticipant")
                .short('a')
                .long("pacticipant")
                .value_name("PACTICIPANT")
                .required(true)
                .help("The pacticipant name"),
        )
        .arg(
            Arg::new("version")
                .short('e')
                .long("version")
                .value_name("VERSION")
                .required(true)
                .help("The pacticipant version"),
        )
        .arg(
            Arg::new("tag")
                .short('t')
                .long("tag")
                .value_name("TAG")
                .value_delimiter(',')
                .num_args(1..)
                .required(true)
                .value_parser(clap::builder::NonEmptyStringValueParser::new())
                .help("Tag name for pacticipant version. Can be specified multiple times"),
        )
        .arg(
            Arg::new("auto-create-version")
                .long("auto-create-version")
                .num_args(0)
                .default_value("false")
                .action(clap::ArgAction::SetTrue)
                .help("Automatically create the pacticipant version if it does not exist"),
        )
        .arg(
            Arg::new("tag-with-git-branch")
                .short('g')
                .long("tag-with-git-branch")
                .num_args(0)
                .default_value("false")
                .action(clap::ArgAction::SetTrue)
                .help("Tag pacticipant version with the name of the current git branch"),
        )
}
pub fn add_describe_version_subcommand() -> Command {
    Command::new("describe-version")
    .about("Describes a pacticipant version. If no version or tag is specified, the latest version is described.")
    .args(add_broker_auth_arguments())
    .arg(Arg::new("pacticipant")
        .short('a')
        .long("pacticipant")
        .value_name("PACTICIPANT")
        .required(true)
        .help("The name of the pacticipant that the version belongs to"))
    .arg(Arg::new("version")
        .short('e')
        .long("version")
        .value_name("VERSION")
        .help("The pacticipant version number"))
    .arg(Arg::new("latest")
        .short('l')
        .long("latest")
        .value_name("TAG")
        .num_args(0..=1)
        .help("Describe the latest pacticipant version. Optionally specify a TAG to describe the latest version with the specified tag"))
        .args(crate::cli::add_output_arguments(["json", "table"].to_vec(), "table"))
}
pub fn add_create_or_update_version_subcommand() -> Command {
    Command::new("create-or-update-version")
        .about("Create or update pacticipant version by version number")
        .args(add_broker_auth_arguments())
        .arg(
            Arg::new("pacticipant")
                .short('a')
                .long("pacticipant")
                .value_name("PACTICIPANT")
                .required(true)
                .help("The pacticipant name"),
        )
        .arg(
            Arg::new("version")
                .short('e')
                .long("version")
                .value_name("VERSION")
                .required(true)
                .help("The pacticipant version number"),
        )
        .arg(
            Arg::new("branch")
                .long("branch")
                .value_name("BRANCH")
                .help("The repository branch name"),
        )
        .arg(
            Arg::new("tag")
                .short('t')
                .long("tag")
                .value_name("TAG")
                .num_args(0..=1)
                .help("Tag name for pacticipant version. Can be specified multiple times"),
        )
        .args(crate::cli::add_output_arguments(
            ["json", "text"].to_vec(),
            "text",
        ))
}
pub fn add_generate_uuid_subcommand() -> Command {
    Command::new("generate-uuid")
        .about("Generate a UUID for use when calling create-or-update-webhook")
}
