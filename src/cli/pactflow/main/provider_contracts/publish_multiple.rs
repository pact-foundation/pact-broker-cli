use base64::Engine;
use base64::engine::general_purpose::STANDARD as Base64;
use clap::ArgMatches;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

use super::verification::{SelfVerificationInputs, missing_self_verification_keys};
use crate::cli::{
    pact_broker::main::{
        HALClient, Notice, PactBrokerError, process_notices,
        utils::{
            get_auth, get_broker_relation, get_broker_url, get_custom_headers, get_retries,
            get_ssl_options,
        },
    },
    utils::git_info,
};

/// One contract, assembled from a `--contract FILE` and the per-contract options following it.
#[derive(Debug, Clone, PartialEq)]
pub struct ContractSpec {
    pub name: String,
    pub file: String,
    pub specification: String,
    pub content_type: String,
    pub verification_results: Option<String>,
    pub verification_success: bool,
    pub verifier: Option<String>,
    pub verifier_version: Option<String>,
    pub verification_results_content_type: Option<String>,
    pub verification_results_format: Option<String>,
}

/// The per-contract options that take a value. Each is declared `ArgAction::Append`, so clap keeps
/// every occurrence along with its position in argv.
const PER_CONTRACT_OPTIONS: &[&str] = &[
    "name",
    "specification",
    "content-type",
    "verification-exit-code",
    "verification-results",
    "verification-results-content-type",
    "verification-results-format",
    "verifier",
    "verifier-version",
];

/// Trim a value and discard it if nothing is left, so a blank flag or a blank git result is
/// treated as absent rather than published as an empty string.
fn non_blank(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

/// Trim `value` and reject what is left if it is blank.
fn require_non_empty(value: &str, option: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(format!("--{option} must not be empty"));
    }
    Ok(trimmed.to_string())
}

impl ContractSpec {
    /// Build one spec per `--contract`, binding each per-contract option to the `--contract` it
    /// follows.
    ///
    /// clap records the argv position of every occurrence of a repeated option, so a value belongs
    /// to the last `--contract` appearing before it. That ordering is the whole mechanism: there is
    /// no grammar inside the argument, so ordinary shell quoting applies and a value may contain
    /// commas, `=` or anything else without escaping.
    pub fn from_args(args: &ArgMatches) -> Result<Vec<Self>, String> {
        let starts: Vec<usize> = args
            .indices_of("contract")
            .map(Iterator::collect)
            .unwrap_or_default();
        let files: Vec<String> = args
            .get_many::<String>("contract")
            .map(|values| values.cloned().collect())
            .unwrap_or_default();
        if starts.is_empty() {
            return Ok(Vec::new());
        }

        // The contract an option at `idx` describes: the last --contract before it.
        let block_of = |idx: usize, option: &str| -> Result<usize, String> {
            starts
                .iter()
                .rposition(|&start| start < idx)
                .ok_or_else(|| {
                    format!(
                        "--{option} describes a single contract, so it must come after a --contract"
                    )
                })
        };

        let mut fields: Vec<HashMap<&str, String>> = vec![HashMap::new(); starts.len()];
        for option in PER_CONTRACT_OPTIONS {
            let indices: Vec<usize> = args
                .indices_of(option)
                .map(Iterator::collect)
                .unwrap_or_default();
            let values: Vec<String> = args
                .get_many::<String>(option)
                .map(|values| values.cloned().collect())
                .unwrap_or_default();
            for (idx, value) in indices.into_iter().zip(values) {
                let block = block_of(idx, option)?;
                if fields[block].insert(option, value).is_some() {
                    return Err(format!(
                        "--{option} was given more than once for the contract '{}'",
                        files[block]
                    ));
                }
            }
        }

        // Outcome flags, kept with the spelling that set them so a clash can name both.
        let mut outcomes: Vec<Option<(bool, &str)>> = vec![None; starts.len()];
        for (option, positive) in [
            ("verification-success", true),
            ("no-verification-success", false),
        ] {
            let indices: Vec<usize> = args
                .indices_of(option)
                .map(Iterator::collect)
                .unwrap_or_default();
            let values: Vec<bool> = args
                .get_many::<bool>(option)
                .map(|values| values.copied().collect())
                .unwrap_or_default();
            for (idx, value) in indices.into_iter().zip(values) {
                let block = block_of(idx, option)?;
                // --no-verification-success is the negation, so bare use means a failed outcome.
                let outcome = if positive { value } else { !value };
                if let Some((_, previous)) = outcomes[block] {
                    return Err(format!(
                        "--{previous} and --{option} cannot both describe the contract '{}'",
                        files[block]
                    ));
                }
                outcomes[block] = Some((outcome, option));
            }
        }

        let mut specs = Vec::with_capacity(files.len());
        for (block, file) in files.iter().enumerate() {
            let mut fields = std::mem::take(&mut fields[block]);
            let file = require_non_empty(file, "contract")?;
            let name = match fields.remove("name") {
                Some(name) => require_non_empty(&name, "name")?,
                // The domain model needs a name, so fall back to the contract's file name.
                None => std::path::Path::new(&file)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| file.clone()),
            };

            let exit_code = fields.remove("verification-exit-code");
            let verification_success = match (outcomes[block], exit_code) {
                (Some((_, flag)), Some(_)) => {
                    return Err(format!(
                        "--{flag} and --verification-exit-code cannot both describe the contract '{file}'"
                    ));
                }
                (Some((outcome, _)), None) => outcome,
                // A non-numeric exit code is treated as a failure rather than rejected: the shell
                // substitution that produced it may legitimately have been empty.
                (None, Some(code)) => code.trim().parse::<i32>().map(|c| c == 0).unwrap_or(false),
                (None, None) => false,
            };

            let spec = ContractSpec {
                name,
                file,
                specification: fields
                    .remove("specification")
                    .map(|v| v.trim().to_string())
                    .unwrap_or_else(|| "oas".to_string()),
                content_type: fields
                    .remove("content-type")
                    .map(|v| v.trim().to_string())
                    .unwrap_or_else(|| "application/yaml".to_string()),
                verification_results: fields
                    .remove("verification-results")
                    .map(|v| v.trim().to_string()),
                verification_success,
                verifier: fields.remove("verifier").map(|v| v.trim().to_string()),
                verifier_version: fields
                    .remove("verifier-version")
                    .map(|v| v.trim().to_string()),
                verification_results_content_type: fields
                    .remove("verification-results-content-type")
                    .map(|v| v.trim().to_string()),
                verification_results_format: fields
                    .remove("verification-results-format")
                    .map(|v| v.trim().to_string()),
            };
            spec.validate_self_verification()?;
            specs.push(spec);
        }
        Ok(specs)
    }

    /// Reject incomplete self-verification evidence here, naming the contract that needs fixing,
    /// so a PactFlow 400 becomes a local error before anything is published.
    fn validate_self_verification(&self) -> Result<(), String> {
        let missing = missing_self_verification_keys(&SelfVerificationInputs {
            results: self.verification_results.is_some(),
            results_content_type: self.verification_results_content_type.is_some(),
            verifier: self.verifier.is_some(),
            verifier_version: self.verifier_version.is_some(),
        });
        if missing.is_empty() {
            return Ok(());
        }
        Err(format!(
            "contract '{}' has self-verification evidence but is missing: {} \
             (--verification-results, --verification-results-content-type and --verifier are all required)",
            self.name,
            missing.join(", ")
        ))
    }
}

#[cfg(test)]
mod contract_spec_from_args_tests {
    use super::ContractSpec;
    use crate::cli::pactflow::main::subcommands::add_publish_provider_contract_subcommand;

    /// Parse `argv` through the real command, then bucket it into specs.
    fn parse(extra: &[&str]) -> Result<Vec<ContractSpec>, String> {
        let mut argv = vec![
            "publish-provider-contract",
            "-b",
            "http://localhost:9999",
            "--provider",
            "p",
            "-a",
            "1.0",
        ];
        argv.extend_from_slice(extra);
        let matches = add_publish_provider_contract_subcommand()
            .try_get_matches_from(argv)
            .map_err(|e| e.to_string())?;
        ContractSpec::from_args(&matches)
    }

    fn specs(extra: &[&str]) -> Vec<ContractSpec> {
        parse(extra).expect("expected the contracts to parse")
    }

    fn err(extra: &[&str]) -> String {
        parse(extra).expect_err("expected a parse error")
    }

    #[test]
    fn builds_one_spec_with_defaults() {
        let s = specs(&["--contract", "./pay.yaml", "--name", "payments-api"]);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].name, "payments-api");
        assert_eq!(s[0].file, "./pay.yaml");
        assert_eq!(s[0].specification, "oas");
        assert_eq!(s[0].content_type, "application/yaml");
        assert!(!s[0].verification_success);
        assert_eq!(s[0].verifier, None);
    }

    #[test]
    fn binds_each_option_to_the_contract_it_follows() {
        let s = specs(&[
            "--contract",
            "./oas/payments.yaml",
            "--name",
            "payments-api",
            "--specification",
            "oas",
            "--verifier",
            "schemathesis",
            "--verifier-version",
            "3.30",
            "--verification-results",
            "./reports/schemathesis.txt",
            "--verification-results-content-type",
            "text/plain",
            "--verification-success",
            "--contract",
            "./asyncapi/fraud.yaml",
            "--name",
            "fraud-events",
            "--specification",
            "asyncapi",
            "--no-verification-success",
        ]);
        assert_eq!(s.len(), 2);

        assert_eq!(s[0].name, "payments-api");
        assert_eq!(s[0].file, "./oas/payments.yaml");
        assert_eq!(s[0].specification, "oas");
        assert_eq!(s[0].verifier.as_deref(), Some("schemathesis"));
        assert_eq!(s[0].verifier_version.as_deref(), Some("3.30"));
        assert!(s[0].verification_success);

        assert_eq!(s[1].name, "fraud-events");
        assert_eq!(s[1].file, "./asyncapi/fraud.yaml");
        assert_eq!(s[1].specification, "asyncapi");
        // Options from the first contract must not leak into the second.
        assert_eq!(s[1].verifier, None);
        assert_eq!(s[1].verifier_version, None);
        assert!(!s[1].verification_success);
    }

    #[test]
    fn keeps_values_that_would_have_needed_escaping_in_the_old_grammar() {
        let s = specs(&[
            "--contract",
            "./specs/v1,v2/api.yaml",
            "--name",
            "Acme, Inc. payments",
            "--content-type",
            "text/plain;charset=utf-8",
        ]);
        assert_eq!(s[0].file, "./specs/v1,v2/api.yaml");
        assert_eq!(s[0].name, "Acme, Inc. payments");
        assert_eq!(s[0].content_type, "text/plain;charset=utf-8");
    }

    #[test]
    fn a_zero_exit_code_is_a_success_and_anything_else_is_not() {
        for (code, expected) in [("0", true), ("1", false), ("", false), ("boom", false)] {
            let s = specs(&[
                "--contract",
                "./pay.yaml",
                "--name",
                "a",
                "--verification-exit-code",
                code,
            ]);
            assert_eq!(
                s[0].verification_success, expected,
                "exit code {code:?} should give {expected}"
            );
        }
    }

    /// The outcome flags take no value, so neither spelling may be given one.
    #[test]
    fn the_outcome_flags_are_bare() {
        assert!(
            specs(&[
                "--contract",
                "./a.yaml",
                "--name",
                "a",
                "--verification-success"
            ])[0]
                .verification_success
        );
        assert!(
            !specs(&[
                "--contract",
                "./a.yaml",
                "--name",
                "a",
                "--no-verification-success"
            ])[0]
                .verification_success
        );
        for argument in [
            "--verification-success=false",
            "--no-verification-success=x",
        ] {
            assert!(
                parse(&["--contract", "./a.yaml", "--name", "a", argument]).is_err(),
                "{argument} should be rejected: the flag takes no value"
            );
        }
    }

    #[test]
    fn each_contract_keeps_its_own_outcome() {
        let s = specs(&[
            "--contract",
            "./a.yaml",
            "--name",
            "a",
            "--verification-success",
            "--contract",
            "./b.yaml",
            "--name",
            "b",
            "--no-verification-success",
            "--contract",
            "./c.yaml",
            "--name",
            "c",
            "--verification-exit-code",
            "0",
        ]);
        assert!(s[0].verification_success);
        assert!(!s[1].verification_success);
        assert!(s[2].verification_success);
    }

    #[test]
    fn rejects_a_per_contract_option_before_any_contract() {
        let e = err(&["--name", "orphan", "--contract", "./a.yaml"]);
        assert!(
            e.contains("must come after a --contract"),
            "message was: {e}"
        );
    }

    #[test]
    fn defaults_a_missing_name_to_the_file_name() {
        let s = specs(&["--contract", "./oas/a.yaml", "--contract", "./b.yaml"]);
        assert_eq!(s[0].name, "a.yaml");
        assert_eq!(s[1].name, "b.yaml");
    }

    #[test]
    fn rejects_an_empty_name() {
        let e = err(&["--contract", "./a.yaml", "--name", "   "]);
        assert!(e.contains("name"), "message was: {e}");
    }

    #[test]
    fn rejects_an_option_repeated_within_one_contract() {
        let e = err(&[
            "--contract",
            "./a.yaml",
            "--name",
            "a",
            "--verifier",
            "x",
            "--verifier",
            "y",
        ]);
        assert!(e.contains("more than once"), "message was: {e}");
    }

    #[test]
    fn rejects_two_outcomes_for_one_contract() {
        let e = err(&[
            "--contract",
            "./a.yaml",
            "--name",
            "a",
            "--verification-success",
            "--no-verification-success",
        ]);
        assert!(e.contains("cannot both describe"), "message was: {e}");

        let e = err(&[
            "--contract",
            "./a.yaml",
            "--name",
            "a",
            "--verification-success",
            "--verification-exit-code",
            "0",
        ]);
        assert!(e.contains("cannot both describe"), "message was: {e}");
    }

    #[test]
    fn rejects_incomplete_self_verification_evidence() {
        let e = err(&[
            "--contract",
            "./a.yaml",
            "--name",
            "a",
            "--verifier",
            "schemathesis",
        ]);
        assert!(e.contains("self-verification evidence"), "message was: {e}");
        assert!(e.contains("verification-results"), "message was: {e}");
    }

    #[test]
    fn no_contract_flags_gives_no_specs() {
        let matches = add_publish_provider_contract_subcommand()
            .try_get_matches_from(vec![
                "publish-provider-contract",
                "-b",
                "http://x",
                "--provider",
                "p",
                "-a",
                "1.0",
                "./single.yaml",
            ])
            .expect("single-contract mode should parse");
        assert!(ContractSpec::from_args(&matches).unwrap().is_empty());
    }
}

#[derive(Debug, Deserialize)]
struct PublishContractsResponse {
    notices: Vec<Notice>,
    contracts: Vec<String>,
}

pub fn publish_multiple(args: &ArgMatches) -> Result<Value, PactBrokerError> {
    let specs =
        ContractSpec::from_args(args).map_err(|e| PactBrokerError::ValidationError(vec![e]))?;

    // Validate: no duplicate contract names
    let mut seen: HashSet<&str> = HashSet::new();
    for name in specs.iter().map(|spec| spec.name.as_str()) {
        if !seen.insert(name) {
            return Err(PactBrokerError::ValidationError(vec![format!(
                "Duplicate contract name: '{}'",
                name
            )]));
        }
    }

    // Read all files eagerly — fail before any HTTP call
    let contract_data: Vec<(ContractSpec, String, Option<String>)> = specs
        .into_iter()
        .map(|spec| {
            let content = std::fs::read_to_string(&spec.file).map_err(|e| {
                eprintln!("❌ Failed to read contract file '{}': {}", spec.file, e);
                PactBrokerError::IoError(e.to_string())
            })?;
            if content.is_empty() {
                return Err(PactBrokerError::ValidationError(vec![format!(
                    "Contract file '{}' is empty",
                    spec.file
                )]));
            }
            let verif_content = if let Some(ref path) = spec.verification_results {
                Some(std::fs::read_to_string(path).map_err(|e| {
                    eprintln!(
                        "❌ Failed to read verification results file '{}': {}",
                        path, e
                    );
                    PactBrokerError::IoError(e.to_string())
                })?)
            } else {
                None
            };
            Ok::<_, PactBrokerError>((spec, content, verif_content))
        })
        .collect::<Result<Vec<_>, _>>()?;

    // Resolve version metadata before any network call, so a misconfigured run fails fast.
    let provider_name = non_blank(args.get_one::<String>("provider").map(String::as_str))
        .ok_or_else(|| {
            PactBrokerError::ValidationError(vec![
                "The provider name must not be blank.".to_string(),
            ])
        })?;
    let auto_detect = args.get_flag("auto-detect-version-properties");
    let tag_with_git_branch = args.get_flag("tag-with-git-branch");

    // At most one branch lookup, shared by --auto-detect-version-properties and
    // --tag-with-git-branch.
    let git_branch = if auto_detect || tag_with_git_branch {
        non_blank(git_info::branch(false).as_deref())
    } else {
        None
    };

    let mut provider_app_version = non_blank(
        args.get_one::<String>("provider-app-version")
            .map(String::as_str),
    );
    let mut branch = non_blank(args.get_one::<String>("branch").map(String::as_str));
    let mut build_url = non_blank(args.get_one::<String>("build-url").map(String::as_str));

    if auto_detect {
        if provider_app_version.is_none() {
            provider_app_version = non_blank(git_info::commit(false).as_deref());
            if let Some(v) = &provider_app_version {
                println!("🔍 Auto detected git commit: {}", v);
            }
        }
        if branch.is_none() {
            branch = git_branch.clone();
            if let Some(b) = &branch {
                println!("🔍 Auto detected git branch: {}", b);
            }
        }
        if build_url.is_none() {
            build_url = non_blank(git_info::build_url().as_deref());
            if let Some(u) = &build_url {
                println!("🔍 Auto detected build URL: {}", u);
            }
        }
    }

    // Past this point the version is a known non-blank string, so the payload can never carry
    // a null or empty pacticipantVersionNumber.
    let Some(provider_app_version) = provider_app_version else {
        return Err(PactBrokerError::ValidationError(vec![
            if auto_detect {
                "Could not determine the provider application version: \
                 --auto-detect-version-properties was set but no git commit could be detected. \
                 Pass --provider-app-version explicitly."
            } else {
                "The provider application version must not be blank."
            }
            .to_string(),
        ]));
    };

    let mut tags: Vec<String> = args
        .get_many::<String>("tag")
        .unwrap_or_default()
        .map(|t| t.to_string())
        .collect();
    if tag_with_git_branch {
        let Some(detected) = git_branch.clone() else {
            return Err(PactBrokerError::ValidationError(vec![
                "--tag-with-git-branch was set but no git branch could be detected. \
                 Pass --tag explicitly instead."
                    .to_string(),
            ]));
        };
        tags.push(detected);
    }

    let broker_url = get_broker_url(args).trim_end_matches('/').to_string();
    let hal_client: HALClient = HALClient::with_url(
        &broker_url,
        Some(get_auth(args)),
        get_ssl_options(args),
        get_custom_headers(args),
    )
    .with_retry_count(get_retries(args));

    let publish_href_result = tokio::runtime::Runtime::new().unwrap().block_on(async {
        get_broker_relation(
            hal_client.clone(),
            "pf:publish-provider-contracts".to_string(),
            broker_url.to_string(),
        )
        .await
    });

    match publish_href_result {
        Ok(publish_href) => {
            let publish_href =
                publish_href.replace("{provider}", &urlencoding::encode(&provider_name));

            // Build the contracts array
            let contracts_array: Vec<Value> = contract_data
                .iter()
                .map(|(spec, content, verif_content)| {
                    let mut entry = json!({
                        "name": spec.name,
                        "content": Base64.encode(content),
                        "contentType": spec.content_type,
                        "specification": spec.specification,
                    });

                    // Gate matches publish.rs:187-190: the outcome is always resolved but only
                    // travels when there is verification evidence to attach it to.
                    if verif_content.is_some()
                        || spec.verifier.is_some()
                        || spec.verifier_version.is_some()
                    {
                        let mut svr = serde_json::Map::new();
                        svr.insert(
                            "success".to_string(),
                            Value::Bool(spec.verification_success),
                        );
                        if let Some(vc) = verif_content {
                            svr.insert("content".to_string(), Value::String(Base64.encode(vc)));
                        }
                        if let Some(ct) = &spec.verification_results_content_type {
                            svr.insert("contentType".to_string(), Value::String(ct.clone()));
                        }
                        if let Some(fmt) = &spec.verification_results_format {
                            svr.insert("format".to_string(), Value::String(fmt.clone()));
                        }
                        if let Some(v) = &spec.verifier {
                            svr.insert("verifier".to_string(), Value::String(v.clone()));
                        }
                        if let Some(vv) = &spec.verifier_version {
                            svr.insert("verifierVersion".to_string(), Value::String(vv.clone()));
                        }
                        entry["selfVerificationResults"] = Value::Object(svr);
                    }

                    entry
                })
                .collect();

            let mut payload = json!({
                "pacticipantVersionNumber": provider_app_version,
                "contracts": contracts_array,
            });

            if !tags.is_empty() {
                payload["tags"] =
                    Value::Array(tags.iter().map(|t| Value::String(t.clone())).collect());
            }
            if let Some(b) = &branch {
                payload["branch"] = Value::String(b.to_string());
            }
            if let Some(u) = &build_url {
                payload["buildUrl"] = Value::String(u.to_string());
            }

            // clap constrains --output to these two values, so anything else is unreachable.
            let output = args
                .get_one::<String>("output")
                .map_or("text", String::as_str);

            println!(
                "📨 Attempting to publish {} provider contracts for provider: {} version: {}",
                contract_data.len(),
                provider_name,
                provider_app_version
            );

            let res = tokio::runtime::Runtime::new().unwrap().block_on(async {
                hal_client
                    .clone()
                    .post_json(
                        &publish_href,
                        &payload.to_string(),
                        Some({
                            let mut headers = std::collections::HashMap::new();
                            headers.insert(
                                "Accept".to_string(),
                                "application/problem+json".to_string(),
                            );
                            headers
                        }),
                    )
                    .await
            });

            match res {
                Ok(res) => {
                    if output == "json" {
                        return Ok(res);
                    }
                    match serde_json::from_value::<PublishContractsResponse>(res) {
                        Ok(parsed) => {
                            print!("✅ ");
                            process_notices(&parsed.notices);
                            println!("Published contracts: {}", parsed.contracts.join(", "));
                        }
                        // The server accepted the contracts; only the summary is unreadable, so
                        // warn rather than reporting a failed publish.
                        Err(err) => {
                            println!(
                                "✅ Provider contracts published successfully for: {} version: {}",
                                provider_name, provider_app_version
                            );
                            println!("⚠️ Warning: Failed to process response - Error: {:?}", err);
                        }
                    }
                }
                Err(err) => {
                    match &err {
                        PactBrokerError::ValidationErrorWithNotices(messages, notices) => {
                            println!("❌ Provider contract publication failed:");
                            for message in messages {
                                println!("   {}", message);
                            }
                            if !notices.is_empty() {
                                println!("\nDetails:");
                                process_notices(notices);
                            }
                        }
                        _ => {
                            println!("❌ {}", err);
                        }
                    }
                    return Err(err);
                }
            }
            Ok(json!({}))
        }
        Err(err) => Err(err),
    }
}

#[cfg(test)]
mod publish_multiple_provider_contracts_tests {
    use super::*;
    use crate::cli::pactflow::main::subcommands::add_publish_provider_contract_subcommand;
    use pact_consumer::prelude::*;
    use pact_models::PactSpecification;
    use serde_json::json;

    const PROVIDER_NAME: &str = "my-provider";
    const PROVIDER_VERSION: &str = "1.4.2";
    const BRANCH: &str = "main";
    const TAG: &str = "dev";
    const BUILD_URL: &str = "http://ci/build/42";
    const PAYMENTS_FIXTURE: &str = "tests/fixtures/payments-api.yaml";
    const FRAUD_FIXTURE: &str = "tests/fixtures/fraud-events.yaml";
    const PROTO_FIXTURE: &str = "tests/fixtures/service.proto";
    const VERIF_RESULTS: &str = "tests/fixtures/verification-results.txt";

    fn mock_server_config() -> MockServerConfig {
        MockServerConfig {
            pact_specification: PactSpecification::V2,
            ..MockServerConfig::default()
        }
    }

    // Test 1: happy path — two contracts (OAS + AsyncAPI), no verification results
    #[test]
    fn publish_two_contracts_succeeds() {
        let payments_content = std::fs::read_to_string(PAYMENTS_FIXTURE).unwrap();
        let fraud_content = std::fs::read_to_string(FRAUD_FIXTURE).unwrap();
        let payments_b64 = Base64.encode(&payments_content);
        let fraud_b64 = Base64.encode(&fraud_content);

        let request_body = json!({
            "pacticipantVersionNumber": PROVIDER_VERSION,
            "branch": BRANCH,
            "tags": [TAG],
            "buildUrl": BUILD_URL,
            "contracts": [
                {
                    "name": "payments-api",
                    "content": payments_b64,
                    "contentType": "application/yaml",
                    "specification": "oas"
                },
                {
                    "name": "fraud-events",
                    "content": fraud_b64,
                    "contentType": "application/yaml",
                    "specification": "asyncapi"
                }
            ]
        });

        let response_body = json!({
            "notices": [{ "text": "Contracts published successfully", "type": "success" }],
            "contracts": ["payments-api", "fraud-events"]
        });

        let pactflow_service = PactBuilder::new("pact-broker-cli", "PactFlow")
            .interaction("GET / returns HAL index with pf:publish-provider-contracts", "", |mut i| {
                i.given("pf:publish-provider-contracts relation exists in index");
                i.request
                    .get()
                    .path("/")
                    .header("Accept", "application/hal+json")
                    .header("Accept", "application/json");
                i.response
                    .status(200)
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(json_pattern!({
                        "_links": {
                            "pf:publish-provider-contracts": {
                                "href": term!(
                                    format!(".*\\/provider-contracts\\/provider\\/{}\\/publish-contracts", PROVIDER_NAME),
                                    format!("http://localhost:1234/provider-contracts/provider/{}/publish-contracts", PROVIDER_NAME)
                                )
                            }
                        }
                    }));
                i
            })
            .interaction("POST publish-contracts publishes two contracts", "", |mut i| {
                i.request
                    .post()
                    .path(format!(
                        "/provider-contracts/provider/{}/publish-contracts",
                        PROVIDER_NAME
                    ))
                    .header("Content-Type", "application/json")
                    .header("Accept", "application/hal+json,application/problem+json")
                    .json_body(request_body.clone());
                i.response
                    .status(200)
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(response_body.clone());
                i
            })
            .start_mock_server(None, Some(mock_server_config()));

        let url = pactflow_service.url();

        let matches = add_publish_provider_contract_subcommand().get_matches_from(vec![
            "publish-provider-contract",
            "-b",
            url.as_str(),
            "--provider",
            PROVIDER_NAME,
            "--provider-app-version",
            PROVIDER_VERSION,
            "--branch",
            BRANCH,
            "--tag",
            TAG,
            "--build-url",
            BUILD_URL,
            "--contract",
            PAYMENTS_FIXTURE,
            "--name",
            "payments-api",
            "--specification",
            "oas",
            "--content-type",
            "application/yaml",
            // verification-success is set but no results/verifier/verifier-version accompany it,
            // so the expected request body above carries no selfVerificationResults for this
            // contract. That gate is inherited from publish.rs — asserting it here keeps the
            // behaviour deliberate. The mock server fails on any body mismatch.
            "--contract",
            FRAUD_FIXTURE,
            "--name",
            "fraud-events",
            "--specification",
            "asyncapi",
            "--content-type",
            "application/yaml",
            "--verification-success",
            "--output",
            "json",
        ]);

        let result = publish_multiple(&matches);

        assert!(result.is_ok());
        let val = result.unwrap();
        let notices = val.get("notices").unwrap().as_array().unwrap();
        assert!(!notices.is_empty());
        let contracts = val.get("contracts").unwrap().as_array().unwrap();
        assert_eq!(contracts.len(), 2);
        assert!(contracts.iter().any(|c| c.as_str() == Some("payments-api")));
        assert!(contracts.iter().any(|c| c.as_str() == Some("fraud-events")));
    }

    // Test 2: verification results on one contract (OAS), not the other (AsyncAPI)
    #[test]
    fn publish_contracts_with_per_contract_verification_results() {
        let payments_content = std::fs::read_to_string(PAYMENTS_FIXTURE).unwrap();
        let fraud_content = std::fs::read_to_string(FRAUD_FIXTURE).unwrap();
        let verif_content = std::fs::read_to_string(VERIF_RESULTS).unwrap();
        let payments_b64 = Base64.encode(&payments_content);
        let fraud_b64 = Base64.encode(&fraud_content);
        let verif_b64 = Base64.encode(&verif_content);

        let request_body = json!({
            "pacticipantVersionNumber": PROVIDER_VERSION,
            "contracts": [
                {
                    "name": "payments-api",
                    "content": payments_b64,
                    "contentType": "application/yaml",
                    "specification": "oas",
                    "selfVerificationResults": {
                        "success": true,
                        "content": verif_b64,
                        "contentType": "text/plain",
                        "format": "text",
                        "verifier": "spectral",
                        "verifierVersion": "1.0.0"
                    }
                },
                {
                    "name": "fraud-events",
                    "content": fraud_b64,
                    "contentType": "application/yaml",
                    "specification": "asyncapi"
                }
            ]
        });

        let response_body = json!({
            "notices": [{ "text": "Contracts published", "type": "success" }],
            "contracts": ["payments-api", "fraud-events"]
        });

        let pactflow_service = PactBuilder::new("pact-broker-cli", "PactFlow")
            .interaction(
                "GET / returns HAL index with pf:publish-provider-contracts (verif test)",
                "",
                |mut i| {
                    i.given("pf:publish-provider-contracts relation exists in index");
                    i.request
                        .get()
                        .path("/")
                        .header("Accept", "application/hal+json")
                        .header("Accept", "application/json");
                    i.response
                        .status(200)
                        .header("Content-Type", "application/hal+json;charset=utf-8")
                        .json_body(json_pattern!({
                            "_links": {
                                "pf:publish-provider-contracts": {
                                    "href": term!(
                                        format!(".*\\/provider-contracts\\/provider\\/{}\\/publish-contracts", PROVIDER_NAME),
                                        format!("http://localhost:1234/provider-contracts/provider/{}/publish-contracts", PROVIDER_NAME)
                                    )
                                }
                            }
                        }));
                    i
                },
            )
            .interaction(
                "POST publish-contracts with per-contract verification results",
                "",
                |mut i| {
                    i.request
                        .post()
                        .path(format!(
                            "/provider-contracts/provider/{}/publish-contracts",
                            PROVIDER_NAME
                        ))
                        .header("Content-Type", "application/json")
                        .header("Accept", "application/hal+json,application/problem+json")
                        .json_body(request_body.clone());
                    i.response
                        .status(200)
                        .header("Content-Type", "application/hal+json;charset=utf-8")
                        .json_body(response_body.clone());
                    i
                },
            )
            .start_mock_server(None, Some(mock_server_config()));

        let url = pactflow_service.url();

        let matches = add_publish_provider_contract_subcommand().get_matches_from(vec![
            "publish-provider-contract",
            "-b",
            url.as_str(),
            "--provider",
            PROVIDER_NAME,
            "--provider-app-version",
            PROVIDER_VERSION,
            "--contract",
            PAYMENTS_FIXTURE,
            "--name",
            "payments-api",
            "--specification",
            "oas",
            "--content-type",
            "application/yaml",
            "--verification-results",
            VERIF_RESULTS,
            "--verification-success",
            "--verifier",
            "spectral",
            "--verifier-version",
            "1.0.0",
            "--verification-results-content-type",
            "text/plain",
            "--verification-results-format",
            "text",
            "--contract",
            FRAUD_FIXTURE,
            "--name",
            "fraud-events",
            "--specification",
            "asyncapi",
            "--content-type",
            "application/yaml",
            "--output",
            "json",
        ]);

        let result = publish_multiple(&matches);

        assert!(result.is_ok());
        let val = result.unwrap();
        let contracts = val.get("contracts").unwrap().as_array().unwrap();
        assert_eq!(contracts.len(), 2);
    }

    // Test 3: missing contract file returns IoError before any HTTP call
    #[test]
    fn publish_contracts_missing_file_returns_io_error() {
        let matches = add_publish_provider_contract_subcommand().get_matches_from(vec![
            "publish-provider-contract",
            "-b",
            "http://localhost:9999",
            "--provider",
            PROVIDER_NAME,
            "--provider-app-version",
            PROVIDER_VERSION,
            "--contract",
            "tests/fixtures/nonexistent.yaml",
            "--name",
            "payments-api",
        ]);

        let result = publish_multiple(&matches);

        assert!(result.is_err());
        match result.unwrap_err() {
            PactBrokerError::IoError(_) => {}
            other => panic!("Expected IoError, got: {:?}", other),
        }
    }

    // Test 4: arbitrary specification type (protobuf) passes through unchanged — server decides validity
    #[test]
    fn publish_contracts_supports_arbitrary_specification_types() {
        let payments_content = std::fs::read_to_string(PAYMENTS_FIXTURE).unwrap();
        let fraud_content = std::fs::read_to_string(FRAUD_FIXTURE).unwrap();
        let proto_content = std::fs::read_to_string(PROTO_FIXTURE).unwrap();
        let payments_b64 = Base64.encode(&payments_content);
        let fraud_b64 = Base64.encode(&fraud_content);
        let proto_b64 = Base64.encode(&proto_content);

        let request_body = json!({
            "pacticipantVersionNumber": PROVIDER_VERSION,
            "contracts": [
                {
                    "name": "payments-api",
                    "content": payments_b64,
                    "contentType": "application/yaml",
                    "specification": "oas"
                },
                {
                    "name": "fraud-events",
                    "content": fraud_b64,
                    "contentType": "application/yaml",
                    "specification": "asyncapi"
                },
                {
                    "name": "payments-grpc",
                    "content": proto_b64,
                    "contentType": "application/x-protobuf",
                    "specification": "protobuf"
                }
            ]
        });

        let response_body = json!({
            "notices": [{ "text": "Contracts published successfully", "type": "success" }],
            "contracts": ["payments-api", "fraud-events", "payments-grpc"]
        });

        let pactflow_service = PactBuilder::new("pact-broker-cli", "PactFlow")
            .interaction(
                "GET / returns HAL index with pf:publish-provider-contracts (multi-spec test)",
                "",
                |mut i| {
                    i.given("pf:publish-provider-contracts relation exists in index");
                    i.request
                        .get()
                        .path("/")
                        .header("Accept", "application/hal+json")
                        .header("Accept", "application/json");
                    i.response
                        .status(200)
                        .header("Content-Type", "application/hal+json;charset=utf-8")
                        .json_body(json_pattern!({
                            "_links": {
                                "pf:publish-provider-contracts": {
                                    "href": term!(
                                        format!(".*\\/provider-contracts\\/provider\\/{}\\/publish-contracts", PROVIDER_NAME),
                                        format!("http://localhost:1234/provider-contracts/provider/{}/publish-contracts", PROVIDER_NAME)
                                    )
                                }
                            }
                        }));
                    i
                },
            )
            .interaction(
                "POST publish-contracts with oas, asyncapi, and protobuf specifications",
                "",
                |mut i| {
                    i.request
                        .post()
                        .path(format!(
                            "/provider-contracts/provider/{}/publish-contracts",
                            PROVIDER_NAME
                        ))
                        .header("Content-Type", "application/json")
                        .header("Accept", "application/hal+json,application/problem+json")
                        .json_body(request_body.clone());
                    i.response
                        .status(200)
                        .header("Content-Type", "application/hal+json;charset=utf-8")
                        .json_body(response_body.clone());
                    i
                },
            )
            .start_mock_server(None, Some(mock_server_config()));

        let url = pactflow_service.url();

        let matches = add_publish_provider_contract_subcommand().get_matches_from(vec![
            "publish-provider-contract",
            "-b",
            url.as_str(),
            "--provider",
            PROVIDER_NAME,
            "--provider-app-version",
            PROVIDER_VERSION,
            "--contract",
            PAYMENTS_FIXTURE,
            "--name",
            "payments-api",
            "--specification",
            "oas",
            "--content-type",
            "application/yaml",
            "--contract",
            FRAUD_FIXTURE,
            "--name",
            "fraud-events",
            "--specification",
            "asyncapi",
            "--content-type",
            "application/yaml",
            "--contract",
            PROTO_FIXTURE,
            "--name",
            "payments-grpc",
            "--specification",
            "protobuf",
            "--content-type",
            "application/x-protobuf",
            "--output",
            "json",
        ]);

        let result = publish_multiple(&matches);

        assert!(result.is_ok());
        let val = result.unwrap();
        let contracts = val.get("contracts").unwrap().as_array().unwrap();
        assert_eq!(contracts.len(), 3);
        assert!(contracts.iter().any(|c| c.as_str() == Some("payments-api")));
        assert!(contracts.iter().any(|c| c.as_str() == Some("fraud-events")));
        assert!(
            contracts
                .iter()
                .any(|c| c.as_str() == Some("payments-grpc"))
        );
    }

    // Test 5: duplicate contract names are rejected before any HTTP call
    #[test]
    fn publish_contracts_duplicate_names_rejected() {
        let matches = add_publish_provider_contract_subcommand().get_matches_from(vec![
            "publish-provider-contract",
            "-b",
            "http://localhost:9999",
            "--provider",
            PROVIDER_NAME,
            "--provider-app-version",
            PROVIDER_VERSION,
            "--contract",
            PAYMENTS_FIXTURE,
            "--name",
            "payments-api",
            "--contract",
            FRAUD_FIXTURE,
            "--name",
            "payments-api",
        ]);

        let result = publish_multiple(&matches);

        assert!(result.is_err());
        match result.unwrap_err() {
            PactBrokerError::ValidationError(msgs) => {
                assert!(msgs.iter().any(|m| m.contains("payments-api")));
            }
            other => panic!("Expected ValidationError, got: {:?}", other),
        }
    }

    // Test 6: an empty contract file is caught locally rather than posted as empty content.
    #[test]
    fn publish_contracts_rejects_an_empty_contract_file() {
        let empty = std::env::temp_dir().join("pact-broker-cli-empty-contract.yaml");
        std::fs::write(&empty, b"").unwrap();

        let matches = add_publish_provider_contract_subcommand().get_matches_from(vec![
            "publish-provider-contract",
            "-b",
            "http://localhost:9999",
            "--provider",
            PROVIDER_NAME,
            "--provider-app-version",
            PROVIDER_VERSION,
            "--contract",
            &empty.display().to_string(),
            "--name",
            "payments-api",
        ]);

        let result = publish_multiple(&matches);
        let _ = std::fs::remove_file(&empty);

        match result.unwrap_err() {
            PactBrokerError::ValidationError(msgs) => {
                assert!(
                    msgs.iter().any(|m| m.contains("empty")),
                    "expected an empty-file error, got: {msgs:?}"
                );
            }
            other => panic!("Expected ValidationError, got: {:?}", other),
        }
    }

    // Test 7: a blank version is refused by clap, so it can never reach the payload as "".
    #[test]
    fn publish_contracts_rejects_a_blank_provider_app_version() {
        let result = add_publish_provider_contract_subcommand().try_get_matches_from(vec![
            "publish-provider-contract",
            "-b",
            "http://localhost:9999",
            "--provider",
            PROVIDER_NAME,
            "--provider-app-version",
            "",
            "--contract",
            PAYMENTS_FIXTURE,
            "--name",
            "payments-api",
        ]);

        assert!(
            result.is_err(),
            "an empty --provider-app-version should be rejected"
        );
    }

    // Test 8: an outcome with nothing to attach it to sends no selfVerificationResults at all.
    // Inherited from publish.rs:187-190, where the block is gated on verification results, a
    // verifier or a verifier version rather than on the outcome itself. Covers both spellings —
    // the mock server rejects the request if either contract gains the block.
    #[test]
    fn publish_contracts_omits_self_verification_results_without_evidence() {
        let payments_content = std::fs::read_to_string(PAYMENTS_FIXTURE).unwrap();
        let fraud_content = std::fs::read_to_string(FRAUD_FIXTURE).unwrap();
        let payments_b64 = Base64.encode(&payments_content);
        let fraud_b64 = Base64.encode(&fraud_content);

        let request_body = json!({
            "pacticipantVersionNumber": PROVIDER_VERSION,
            "contracts": [
                {
                    "name": "payments-api",
                    "content": payments_b64,
                    "contentType": "application/yaml",
                    "specification": "oas"
                },
                {
                    "name": "fraud-events",
                    "content": fraud_b64,
                    "contentType": "application/yaml",
                    "specification": "oas"
                }
            ]
        });

        let response_body = json!({
            "notices": [{ "text": "Contracts published successfully", "type": "success" }],
            "contracts": ["payments-api", "fraud-events"]
        });

        let pactflow_service = PactBuilder::new("pact-broker-cli", "PactFlow")
            .interaction(
                "GET / returns HAL index with pf:publish-provider-contracts (no-evidence test)",
                "",
                |mut i| {
                    i.given("pf:publish-provider-contracts relation exists in index");
                    i.request
                        .get()
                        .path("/")
                        .header("Accept", "application/hal+json")
                        .header("Accept", "application/json");
                    i.response
                        .status(200)
                        .header("Content-Type", "application/hal+json;charset=utf-8")
                        .json_body(json_pattern!({
                            "_links": {
                                "pf:publish-provider-contracts": {
                                    "href": term!(
                                        format!(".*\\/provider-contracts\\/provider\\/{}\\/publish-contracts", PROVIDER_NAME),
                                        format!("http://localhost:1234/provider-contracts/provider/{}/publish-contracts", PROVIDER_NAME)
                                    )
                                }
                            }
                        }));
                    i
                },
            )
            .interaction(
                "POST publish-contracts with outcomes but no verification evidence",
                "",
                |mut i| {
                    i.request
                        .post()
                        .path(format!(
                            "/provider-contracts/provider/{}/publish-contracts",
                            PROVIDER_NAME
                        ))
                        .header("Content-Type", "application/json")
                        .header("Accept", "application/hal+json,application/problem+json")
                        .json_body(request_body.clone());
                    i.response
                        .status(200)
                        .header("Content-Type", "application/hal+json;charset=utf-8")
                        .json_body(response_body.clone());
                    i
                },
            )
            .start_mock_server(None, Some(mock_server_config()));

        let url = pactflow_service.url();

        let matches = add_publish_provider_contract_subcommand().get_matches_from(vec![
            "publish-provider-contract",
            "-b",
            url.as_str(),
            "--provider",
            PROVIDER_NAME,
            "--provider-app-version",
            PROVIDER_VERSION,
            "--contract",
            PAYMENTS_FIXTURE,
            "--name",
            "payments-api",
            "--verification-success",
            "--contract",
            FRAUD_FIXTURE,
            "--name",
            "fraud-events",
            "--verification-exit-code",
            "0",
            "--output",
            "json",
        ]);

        let result = publish_multiple(&matches);

        assert!(
            result.is_ok(),
            "an outcome without evidence should still publish: {result:?}"
        );
        let val = result.unwrap();
        let contracts = val.get("contracts").unwrap().as_array().unwrap();
        assert_eq!(contracts.len(), 2);
    }
}
