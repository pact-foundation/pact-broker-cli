//! Runs commands through the CLI against a recording broker and checks that
//! values reach the request path percent-encoded.

use rstest::rstest;

use crate::cli::build_cli;
use crate::cli::pact_broker::main::test_utils::spawn_recording_broker_with_responses;
use crate::cli::pact_broker_client;

const INDEX: &str = r#"{"_links":{
    "pb:pacticipant":{"href":"BASE/pacticipants/{pacticipant}","templated":true},
    "pb:pacticipant-branch":{"href":"BASE/pacticipants/{pacticipant}/branches/{branch}","templated":true},
    "pb:environments":{"href":"BASE/environments"}
}}"#;

const PACTICIPANT: &str = r#"{"_links":{
    "pb:version-tag":{"href":"BASE/pacticipants/{pacticipant}/versions/{version}/tags/{tag}","templated":true}
}}"#;

const ENVIRONMENTS: &str =
    r#"{"_embedded":{"environments":[{"uuid":"env/1","name":"test"}]},"_links":{}}"#;

const ENVIRONMENT: &str = r#"{"_links":{
    "pb:currently-supported-released-versions":{"href":"BASE/released"},
    "pb:currently-deployed-deployed-versions":{"href":"BASE/deployed"}
}}"#;

#[rstest]
#[case::delete_branch(
    &["delete-branch", "--pacticipant", "p", "--branch", "fix/a"],
    "DELETE /pacticipants/p/branches/fix%2Fa"
)]
#[case::delete_version_tag(
    &["delete-version-tag", "--pacticipant", "p", "--version", "1", "--tag", "fix/a"],
    "DELETE /pacticipants/p/versions/1/tags/fix%2Fa"
)]
#[case::delete_environment(
    &["delete-environment", "--uuid", "a/b"],
    "DELETE /environments/a%2Fb"
)]
#[case::update_environment(
    &["update-environment", "--uuid", "a/b"],
    "GET /environments/a%2Fb"
)]
#[case::record_release(
    &["record-release", "--pacticipant", "my app", "--version", "fix/1", "--environment", "test"],
    "GET /pacticipants/my%20app/versions/fix%2F1"
)]
#[case::record_support_ended(
    &["record-support-ended", "--pacticipant", "p", "--version", "1", "--environment", "test"],
    "GET /environments/env%2F1"
)]
#[case::record_undeployment(
    &["record-undeployment", "--pacticipant", "p", "--environment", "test"],
    "GET /environments/env%2F1"
)]
#[case::describe_version_deployed(
    &["describe-version", "--pacticipant", "p", "--environment", "test"],
    "GET /environments/env%2F1/deployed-versions/currently-deployed"
)]
#[case::describe_version_released(
    &["describe-version", "--pacticipant", "p", "--environment", "test"],
    "GET /environments/env%2F1/released-versions/currently-supported"
)]
#[case::create_webhook(
    &[
        "create-webhook", "https://example.com/hook", "--request", "POST",
        "--provider", "my provider", "--consumer", "c/x", "--contract-published",
    ],
    "POST /webhooks/provider/my%20provider/consumer/c%2Fx"
)]
fn command_encodes_path_values(#[case] args: &[&str], #[case] expected: &str) {
    let (broker_url, requests) = spawn_recording_broker_with_responses(&[
        ("/", INDEX),
        ("/pacticipants/p", PACTICIPANT),
        ("/environments", ENVIRONMENTS),
        ("/environments/env%2F1", ENVIRONMENT),
    ]);
    let argv: Vec<&str> = ["pact-broker-cli"]
        .into_iter()
        .chain(args.iter().copied())
        .chain(["-b", broker_url.as_str()])
        .collect();
    let matches = build_cli().get_matches_from(&argv);
    let _ = pact_broker_client::run(
        &matches,
        argv[1..].iter().map(|arg| arg.to_string()).collect(),
    );
    let requests = requests.lock().unwrap().clone();
    assert!(
        requests.iter().any(|request| request == expected),
        "{expected} not in {requests:?}"
    );
}
