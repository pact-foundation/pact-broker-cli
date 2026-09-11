use base64::Engine;
use base64::engine::general_purpose::STANDARD as Base64;
use clap::ArgMatches;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashSet;

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

/// One entry from a `--contract name=X,file=Y,...` flag.
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

/// Every key accepted inside a single `--contract` value. Drives both field splitting and
/// unknown-key rejection, so the plural command is as strict about typos as clap is for the
/// singular `publish-provider-contract`.
const KNOWN_KEYS: &[&str] = &[
    "name",
    "file",
    "specification",
    "content-type",
    "verification-results",
    "verification-success",
    "verification-exit-code",
    "verifier",
    "verifier-version",
    "verification-results-content-type",
    "verification-results-format",
];

/// True when `fragment` opens a new field, i.e. it begins with a known key followed by `=`.
/// Tested against every key rather than stopping at the first, so `verifier-version=1` is not
/// mistaken for the shorter `verifier` key.
fn starts_new_field(fragment: &str) -> bool {
    let trimmed = fragment.trim_start();
    KNOWN_KEYS.iter().any(|key| {
        trimmed
            .strip_prefix(key)
            .is_some_and(|rest| rest.trim_start().starts_with('='))
    })
}

/// True when `fragment` reads as an attempted `key=value` assignment — the text before its first
/// `=` is a bare identifier. Such a fragment opens a field even when the key is unknown, so a
/// typo like `content-tpye=x` reaches the unknown-key check instead of being silently absorbed
/// into the preceding value.
fn looks_like_assignment(fragment: &str) -> bool {
    let trimmed = fragment.trim();
    trimmed.find('=').is_some_and(|eq| {
        let candidate = &trimmed[..eq];
        !candidate.is_empty()
            && candidate
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    })
}

/// Split a `--contract` value on commas, treating a comma as a separator only when what follows
/// opens a new field. Anything else is a literal comma belonging to the preceding value, so
/// `verifier=Acme, Inc.` and `file=./specs/v1,v2/api.yaml` survive intact.
fn split_fields(input: &str) -> Vec<String> {
    let mut fields: Vec<String> = Vec::new();
    for fragment in input.split(',') {
        let opens_field = starts_new_field(fragment) || looks_like_assignment(fragment);
        match fields.last_mut() {
            Some(previous) if !opens_field => {
                previous.push(',');
                previous.push_str(fragment);
            }
            _ => fields.push(fragment.to_string()),
        }
    }
    fields
}

/// Trim a value and discard it if nothing is left, so a blank flag or a blank git result is
/// treated as absent rather than published as an empty string.
fn non_blank(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

fn take_required(
    map: &mut std::collections::HashMap<String, String>,
    key: &str,
) -> Result<String, String> {
    match map.remove(key) {
        None => Err(format!("--contract requires a '{key}' key")),
        Some(value) if value.is_empty() => Err(format!("--contract '{key}' must not be empty")),
        Some(value) => Ok(value),
    }
}

impl ContractSpec {
    /// Parse `"name=payments-api,file=./pay.yaml,specification=oas,content-type=application/yaml"`
    /// into a `ContractSpec`. Required keys: `name`, `file`.
    pub fn parse(input: &str) -> Result<Self, String> {
        let mut map: std::collections::HashMap<String, String> = std::collections::HashMap::new();

        for field in split_fields(input) {
            let field = field.trim();
            if field.is_empty() {
                continue;
            }
            let Some(eq_pos) = field.find('=') else {
                return Err(format!(
                    "--contract fragment '{field}' is not in key=value form"
                ));
            };
            let key = field[..eq_pos].trim();
            let value = field[eq_pos + 1..].trim();

            if !KNOWN_KEYS.contains(&key) {
                return Err(format!(
                    "--contract has unknown key '{}'. Valid keys: {}",
                    key,
                    KNOWN_KEYS.join(", ")
                ));
            }
            if map.insert(key.to_string(), value.to_string()).is_some() {
                return Err(format!("--contract has a duplicate '{key}' key"));
            }
        }

        let name = take_required(&mut map, "name")?;
        let file = take_required(&mut map, "file")?;
        let specification = map
            .remove("specification")
            .unwrap_or_else(|| "oas".to_string());
        let content_type = map
            .remove("content-type")
            .unwrap_or_else(|| "application/yaml".to_string());
        let verification_results = map.remove("verification-results");
        // Mirrors publish.rs:145-160 — verification-success wins, then verification-exit-code,
        // then false. The singular's --no-verification-success has no key here because
        // verification-success takes a value and `=false` already says it.
        let explicit_success = map
            .remove("verification-success")
            .map(|raw| match raw.to_lowercase().as_str() {
                "true" | "1" => Ok(true),
                "false" | "0" => Ok(false),
                _ => Err(format!(
                    "--contract verification-success must be true, false, 1 or 0 (got '{raw}')"
                )),
            })
            .transpose()?;
        let exit_code = map.remove("verification-exit-code");
        if explicit_success.is_some() && exit_code.is_some() {
            return Err(
                "--contract sets both 'verification-success' and 'verification-exit-code'; \
                 use one or the other"
                    .to_string(),
            );
        }
        // An unparseable exit code becomes false rather than an error, matching publish.rs:157.
        let verification_success = match (explicit_success, exit_code) {
            (Some(success), _) => success,
            (None, Some(raw)) => raw.parse::<i32>().is_ok_and(|code| code == 0),
            (None, None) => false,
        };
        let verifier = map.remove("verifier");
        let verifier_version = map.remove("verifier-version");
        let verification_results_content_type = map.remove("verification-results-content-type");
        let verification_results_format = map.remove("verification-results-format");

        Ok(ContractSpec {
            name,
            file,
            specification,
            content_type,
            verification_results,
            verification_success,
            verifier,
            verifier_version,
            verification_results_content_type,
            verification_results_format,
        })
    }
}

#[cfg(test)]
mod contract_spec_parse_tests {
    use super::ContractSpec;

    fn spec(input: &str) -> ContractSpec {
        ContractSpec::parse(input).expect("expected a valid spec")
    }

    fn err(input: &str) -> String {
        ContractSpec::parse(input).expect_err("expected a parse error")
    }

    #[test]
    fn parses_a_minimal_spec_and_applies_defaults() {
        let s = spec("name=payments-api,file=./pay.yaml");
        assert_eq!(s.name, "payments-api");
        assert_eq!(s.file, "./pay.yaml");
        assert_eq!(s.specification, "oas");
        assert_eq!(s.content_type, "application/yaml");
        assert!(!s.verification_success);
        assert_eq!(s.verifier, None);
    }

    #[test]
    fn parses_every_known_key() {
        let s = spec(
            "name=a,file=f.yaml,specification=asyncapi,content-type=application/json,\
             verification-results=r.txt,verification-success=true,verifier=spectral,\
             verifier-version=1.2.3,verification-results-content-type=text/plain,\
             verification-results-format=junit",
        );
        assert_eq!(s.specification, "asyncapi");
        assert_eq!(s.content_type, "application/json");
        assert_eq!(s.verification_results.as_deref(), Some("r.txt"));
        assert!(s.verification_success);
        assert_eq!(s.verifier.as_deref(), Some("spectral"));
        assert_eq!(s.verifier_version.as_deref(), Some("1.2.3"));
        assert_eq!(
            s.verification_results_content_type.as_deref(),
            Some("text/plain")
        );
        assert_eq!(s.verification_results_format.as_deref(), Some("junit"));
    }

    #[test]
    fn trims_whitespace_around_keys_and_values() {
        let s = spec("  name = a , file = f.yaml , specification = oas ");
        assert_eq!(s.name, "a");
        assert_eq!(s.file, "f.yaml");
        assert_eq!(s.specification, "oas");
    }

    #[test]
    fn keeps_everything_after_the_first_equals_in_the_value() {
        let s = spec("name=a,file=f.yaml,verification-success=true,verifier-version=1.0=rc1");
        assert_eq!(s.verifier_version.as_deref(), Some("1.0=rc1"));
    }

    // --- the 5 regressions vs the singular command ---

    #[test]
    fn keeps_a_comma_inside_a_value() {
        let s = spec("name=a,file=f.yaml,verification-success=true,verifier=Acme, Inc.");
        assert_eq!(s.verifier.as_deref(), Some("Acme, Inc."));
        assert_eq!(s.name, "a");
        assert_eq!(s.file, "f.yaml");
    }

    #[test]
    fn keeps_a_comma_inside_a_file_path() {
        let s = spec("name=a,file=./specs/v1,v2/api.yaml");
        assert_eq!(s.file, "./specs/v1,v2/api.yaml");
    }

    #[test]
    fn keeps_a_semicolon_bearing_content_type_whole() {
        let s = spec("name=a,file=f.yaml,content-type=text/plain;charset=utf-8");
        assert_eq!(s.content_type, "text/plain;charset=utf-8");
    }

    #[test]
    fn rejects_an_unknown_key() {
        let e = err("name=a,file=f.yaml,content-tpye=application/json");
        assert!(e.contains("content-tpye"), "message was: {e}");
    }

    #[test]
    fn rejects_a_duplicate_key() {
        let e = err("name=a,file=f.yaml,name=b");
        assert!(e.contains("name"), "message was: {e}");
    }

    #[test]
    fn rejects_an_empty_name_or_file() {
        assert!(err("name=,file=f.yaml").contains("name"));
        assert!(err("name=a,file=").contains("file"));
    }

    #[test]
    fn rejects_a_fragment_with_no_equals() {
        let e = err("just-a-file.yaml");
        assert!(!e.is_empty(), "expected a descriptive error");
    }

    #[test]
    fn accepts_all_valid_boolean_spellings() {
        for (raw, expected) in [
            ("true", true),
            ("TRUE", true),
            ("1", true),
            ("false", false),
            ("False", false),
            ("0", false),
        ] {
            let s = spec(&format!("name=a,file=f.yaml,verification-success={raw}"));
            assert_eq!(
                s.verification_success, expected,
                "verification-success={raw} should parse as {expected}"
            );
        }
    }

    #[test]
    fn rejects_an_invalid_boolean_instead_of_defaulting_to_false() {
        for raw in ["yes", "ture", "maybe", ""] {
            let e = err(&format!("name=a,file=f.yaml,verification-success={raw}"));
            assert!(
                e.contains("verification-success"),
                "verification-success={raw} should be rejected, message was: {e}"
            );
        }
    }

    #[test]
    fn accepts_verification_success_on_its_own() {
        let s = spec("name=a,file=f.yaml,verification-success=false");
        assert!(!s.verification_success);
        assert_eq!(s.verifier, None);
    }

    // --- verification-exit-code, mirroring publish.rs:145-160 ---

    #[test]
    fn treats_a_zero_exit_code_as_success() {
        let s = spec("name=a,file=f.yaml,verification-exit-code=0");
        assert!(s.verification_success);
    }

    #[test]
    fn treats_a_non_zero_exit_code_as_failure() {
        for raw in ["1", "2", "127", "-1"] {
            let s = spec(&format!("name=a,file=f.yaml,verification-exit-code={raw}"));
            assert!(
                !s.verification_success,
                "exit code {raw} should be a failed verification"
            );
        }
    }

    // publish.rs:157 coerces an unparseable exit code to false rather than erroring; the plural
    // matches it so a pipeline behaves identically whichever mode it uses.
    #[test]
    fn treats_an_unparseable_exit_code_as_failure() {
        for raw in ["abc", "", "0.0"] {
            let s = spec(&format!("name=a,file=f.yaml,verification-exit-code={raw}"));
            assert!(
                !s.verification_success,
                "exit code '{raw}' should be a failed verification"
            );
        }
    }

    #[test]
    fn rejects_both_outcome_keys_in_one_contract() {
        let e = err("name=a,file=f.yaml,verification-success=true,verification-exit-code=0");
        assert!(e.contains("verification-exit-code"), "message was: {e}");
    }

    // --- the singular's looseness, adopted deliberately ---

    #[test]
    fn defaults_to_a_failed_verification_when_no_outcome_is_given() {
        let s = spec("name=a,file=f.yaml,verifier=spectral");
        assert!(!s.verification_success);
        assert_eq!(s.verifier.as_deref(), Some("spectral"));
    }

    // --- required keys ---

    #[test]
    fn rejects_a_missing_name() {
        assert!(err("file=f.yaml").contains("name"));
    }

    #[test]
    fn rejects_a_missing_file() {
        assert!(err("name=a").contains("file"));
    }
}

#[derive(Debug, Deserialize)]
struct PublishContractsResponse {
    notices: Vec<Notice>,
    contracts: Vec<String>,
}

pub fn publish_multiple(args: &ArgMatches) -> Result<Value, PactBrokerError> {
    // Parse --contract flags
    let raw_contracts: Vec<String> = args
        .get_many::<String>("contract")
        .unwrap_or_default()
        .cloned()
        .collect();

    let specs: Vec<ContractSpec> = raw_contracts
        .iter()
        .map(|s| ContractSpec::parse(s))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| PactBrokerError::ValidationError(vec![e]))?;

    // Validate: no duplicate contract names
    let mut seen: HashSet<&str> = HashSet::new();
    for spec in &specs {
        if !seen.insert(spec.name.as_str()) {
            return Err(PactBrokerError::ValidationError(vec![format!(
                "Duplicate contract name: '{}'",
                spec.name
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
            &format!(
                "name=payments-api,file={},specification=oas,content-type=application/yaml",
                PAYMENTS_FIXTURE
            ),
            // verification-success is set but no results/verifier/verifier-version accompany it,
            // so the expected request body above carries no selfVerificationResults for this
            // contract. That gate is inherited from publish.rs:187-190 — asserting it here keeps
            // the behaviour deliberate. The mock server fails on any body mismatch.
            "--contract",
            &format!(
                "name=fraud-events,file={},specification=asyncapi,content-type=application/yaml,\
                 verification-success=true",
                FRAUD_FIXTURE
            ),
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
            &format!(
                "name=payments-api,file={},specification=oas,content-type=application/yaml,\
                 verification-results={},verification-success=true,verifier=spectral,\
                 verifier-version=1.0.0,verification-results-content-type=text/plain,\
                 verification-results-format=text",
                PAYMENTS_FIXTURE, VERIF_RESULTS
            ),
            "--contract",
            &format!(
                "name=fraud-events,file={},specification=asyncapi,content-type=application/yaml",
                FRAUD_FIXTURE
            ),
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
            "name=payments-api,file=tests/fixtures/nonexistent.yaml,specification=oas,content-type=application/yaml",
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
            &format!(
                "name=payments-api,file={},specification=oas,content-type=application/yaml",
                PAYMENTS_FIXTURE
            ),
            "--contract",
            &format!(
                "name=fraud-events,file={},specification=asyncapi,content-type=application/yaml",
                FRAUD_FIXTURE
            ),
            "--contract",
            &format!(
                "name=payments-grpc,file={},specification=protobuf,content-type=application/x-protobuf",
                PROTO_FIXTURE
            ),
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
            &format!(
                "name=payments-api,file={},specification=oas,content-type=application/yaml",
                PAYMENTS_FIXTURE
            ),
            "--contract",
            &format!(
                "name=payments-api,file={},specification=asyncapi,content-type=application/yaml",
                FRAUD_FIXTURE
            ),
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
            &format!("name=payments-api,file={}", empty.display()),
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
            &format!("name=payments-api,file={}", PAYMENTS_FIXTURE),
        ]);

        assert!(
            result.is_err(),
            "an empty --provider-app-version should be rejected"
        );
    }
}
