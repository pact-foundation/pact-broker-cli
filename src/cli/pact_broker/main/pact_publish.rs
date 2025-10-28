//! CLI to publish Pact files to a Pact broker.

#![warn(missing_docs)]

use std::fs::File;

use anyhow::{Context, anyhow};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as Base64;
use clap::ArgMatches;
use log::*;

use glob::glob;
use pact_models::http_utils::HttpAuth;
use pact_models::{http_utils, pact};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::cli::pact_broker::main::utils::{
    get_auth, get_broker_relation, get_broker_url, get_custom_headers,
};
use crate::cli::pact_broker::main::utils::{get_ssl_options, handle_error};
use crate::cli::pact_broker::main::{HALClient, Notice, process_notices};
use crate::cli::utils::git_info;
use std::collections::HashMap;

use super::verification::{VerificationResult, display_results, verify_json};

/// Error type for pact merging conflicts
#[derive(Debug)]
pub struct PactMergeError {
    /// The error message describing the merge conflict
    pub message: String,
}

impl std::fmt::Display for PactMergeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PactBroker::Client::PactMergeError - {}", self.message)
    }
}

impl std::error::Error for PactMergeError {}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Root {
    #[serde(rename = "_embedded")]
    pub embedded: Embedded,
    #[serde(rename = "_links")]
    pub links: Links3,
    pub logs: Vec<Log>,
    pub notices: Option<Vec<Notice>>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Embedded {
    pub pacticipant: Pacticipant,
    pub version: Version,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Pacticipant {
    #[serde(rename = "_links")]
    pub links: Links,
    pub name: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Links {
    #[serde(rename = "self")]
    pub self_field: SelfField,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SelfField {
    pub href: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Version {
    #[serde(rename = "_links")]
    pub links: Links2,
    pub number: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Links2 {
    #[serde(rename = "self")]
    pub self_field: SelfField2,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SelfField2 {
    pub href: String,
    pub name: String,
    pub title: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Links3 {
    #[serde(rename = "pb:contracts")]
    pub pb_contracts: Vec<Contract>,
    #[serde(rename = "pb:pacticipant")]
    pub pb_pacticipant: PbPacticipant,
    #[serde(rename = "pb:pacticipant-version")]
    pub pb_pacticipant_version: PbPacticipantVersion,
    #[serde(rename = "pb:pacticipant-version-tags")]
    pub pb_pacticipant_version_tags: Vec<Value>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Contract {
    pub href: String,
    pub name: String,
    pub title: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PbPacticipant {
    pub href: String,
    pub name: String,
    pub title: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PbPacticipantVersion {
    pub href: String,
    pub name: String,
    pub title: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Log {
    pub deprecation_warning: String,
    pub level: String,
    pub message: String,
}

/// Check if two interactions have the same description and provider state
fn same_description_and_state(original: &Value, additional: &Value) -> bool {
    let same_description = original.get("description") == additional.get("description");

    let same_state = match (
        original.get("providerState"),
        additional.get("providerState"),
    ) {
        (Some(orig_state), Some(add_state)) => orig_state == add_state,
        (None, None) => true,
        _ => {
            // Check providerStates array as well
            match (
                original.get("providerStates"),
                additional.get("providerStates"),
            ) {
                (Some(orig_states), Some(add_states)) => orig_states == add_states,
                (None, None) => true,
                _ => false,
            }
        }
    };

    same_description && same_state
}

fn almost_duplicate_message(original: &Value, new_interaction: &Value) -> String {
    let description = new_interaction
        .get("description")
        .and_then(|d| d.as_str())
        .unwrap_or("unknown");

    let provider_state = new_interaction
        .get("providerState")
        .or_else(|| new_interaction.get("providerStates"))
        .map(|s| serde_json::to_string(s).unwrap_or_else(|_| "unknown".to_string()))
        .unwrap_or_else(|| "null".to_string());

    let original_json =
        serde_json::to_string_pretty(original).unwrap_or_else(|_| "<invalid json>".to_string());
    let new_json = serde_json::to_string_pretty(new_interaction)
        .unwrap_or_else(|_| "<invalid json>".to_string());

    format!(
        "Two interactions have been found with same description ({:?}) and provider state ({}) but a different request or response. Please use a different description or provider state, or hard-code any random data.\n\n{}\n\n{}",
        description, provider_state, original_json, new_json
    )
}

fn merge_interactions_or_messages(
    existing_interactions: &mut Vec<Value>,
    additional_interactions: &[Value],
) -> Result<(), PactMergeError> {
    for new_interaction in additional_interactions {
        if let Some(existing_index) = existing_interactions
            .iter()
            .position(|existing| same_description_and_state(existing, new_interaction))
        {
            if existing_interactions[existing_index] == *new_interaction {
                existing_interactions[existing_index] = new_interaction.clone();
            } else {
                return Err(PactMergeError {
                    message: almost_duplicate_message(
                        &existing_interactions[existing_index],
                        new_interaction,
                    ),
                });
            }
        } else {
            existing_interactions.push(new_interaction.clone());
        }
    }
    Ok(())
}

pub fn handle_matches(args: &ArgMatches) -> Result<Vec<VerificationResult>, i32> {
    if args.get_flag("validate") == false {
        return Ok(vec![]);
    }
    let files = load_files(args).map_err(|_| 1)?;
    let results: Vec<VerificationResult> = files
        .iter()
        .map(|(source, pact_json)| {
            let spec_version =
                pact::determine_spec_version(source, &pact::parse_meta_data(pact_json));
            let results = verify_json(pact_json, spec_version, source, args.get_flag("strict"));

            let verification_results = VerificationResult::new(source, results);
            verification_results
        })
        .collect();

    if results.is_empty() {
        println!("❌ No pact files found to publish");
        return Err(1);
    }
    let display_result = display_results(&results, "json");

    if display_result.is_err() {
        return Err(3);
    } else if results.iter().any(|res| res.has_errors()) {
        return Err(2);
    } else {
        return Ok(results);
    }
}

pub fn publish_pacts(args: &ArgMatches) -> Result<Value, i32> {
    let files: Result<Vec<(String, Value)>, anyhow::Error> = load_files(args);
    if files.is_err() {
        let error = files.err().unwrap();
        println!("{}", error);

        return Err(1);
    }
    let files = files.map_err(|_| 1)?;

    let broker_url = get_broker_url(args).trim_end_matches('/').to_string();
    let auth = get_auth(args);
    let custom_headers = get_custom_headers(args);
    let ssl_options = get_ssl_options(args);
    let hal_client: HALClient = HALClient::with_url(
        &broker_url,
        Some(auth.clone()),
        ssl_options.clone(),
        custom_headers.clone(),
    );

    let publish_pact_href_path = tokio::runtime::Runtime::new().unwrap().block_on(async {
        get_broker_relation(
            hal_client.clone(),
            "pb:publish-contracts".to_string(),
            broker_url.to_string(),
        )
        .await
    });

    match publish_pact_href_path {
        Ok(publish_pact_href) => {
            let mut consumer_app_version = args.get_one::<String>("consumer-app-version");
            let mut branch = args.get_one::<String>("branch");
            let auto_detect_version_properties: bool =
                args.get_flag("auto-detect-version-properties");
            let tag_with_git_branch = args.get_flag("tag-with-git-branch");
            let build_url = args.get_one::<String>("build-url");
            let (git_commit, git_branch);
            if auto_detect_version_properties {
                git_commit = git_info::commit(false);
                git_branch = git_info::branch(false);
            } else {
                git_commit = Some("".to_string());
                git_branch = Some("".to_string());
            }
            if auto_detect_version_properties {
                if consumer_app_version == None {
                    consumer_app_version = git_commit.as_ref();
                    println!(
                        "🔍 Auto detected git commit: {}",
                        consumer_app_version.unwrap().to_string()
                    );
                } else {
                    println!(
                        "🔍 auto_detect_version_properties set to {}, but consumer_app_version provided {}",
                        auto_detect_version_properties,
                        consumer_app_version.unwrap().to_string()
                    );
                }
                if branch == None {
                    branch = git_branch.as_ref();
                    println!(
                        "🔍 Auto detected git branch: {}",
                        branch.unwrap().to_string()
                    );
                } else {
                    println!(
                        "🔍 auto_detect_version_properties set to {}, but branch provided {}",
                        auto_detect_version_properties,
                        branch.unwrap().to_string()
                    );
                }
            }

            let on_conflict = if args.get_flag("merge") {
                "merge"
            } else {
                "overwrite"
            };
            let output: Result<Option<&String>, clap::parser::MatchesError> =
                args.try_get_one::<String>("output");
            // publish the pacts
            // Group pacts by (consumer, provider) pair and merge their interactions
            let mut merged_pacts: HashMap<(String, String), Value> = HashMap::new();
            for (source, pact_json) in files.iter() {
                tracing::debug!("Processing pact file: {}", source);

                // Load pact and extract consumer/provider names
                let pact_res = pact::load_pact_from_json(source, pact_json);
                if let Ok(pact) = &pact_res {
                    let consumer_name = pact.consumer().name.clone();
                    let provider_name = pact.provider().name.clone();
                    let key = (consumer_name.clone(), provider_name.clone());

                    tracing::debug!(
                        "Loaded pact for consumer: '{}' and provider: '{}'",
                        consumer_name,
                        provider_name
                    );

                    // If already present, merge interactions
                    if let Some(existing_json) = merged_pacts.get_mut(&key) {
                        tracing::debug!(
                            "Merging interactions for consumer: '{}' and provider: '{}'",
                            consumer_name,
                            provider_name
                        );
                        // Merge interactions arrays with duplicate detection
                        if let (Some(existing_interactions), Some(new_interactions)) = (
                            existing_json.get_mut("interactions"),
                            pact_json.get("interactions"),
                        ) {
                            if let (Some(existing_arr), Some(new_arr)) = (
                                existing_interactions.as_array_mut(),
                                new_interactions.as_array(),
                            ) {
                                tracing::debug!(
                                    "Existing interactions: {}, New interactions: {}",
                                    existing_arr.len(),
                                    new_arr.len()
                                );

                                match merge_interactions_or_messages(existing_arr, new_arr) {
                                    Ok(()) => {
                                        tracing::debug!(
                                            "Total interactions after merge: {}",
                                            existing_arr.len()
                                        );
                                    }
                                    Err(merge_error) => {
                                        println!("❌ {}", merge_error);
                                        error!("Pact merge error: {}", merge_error);
                                        return Err(1);
                                    }
                                }
                            }
                        }
                    } else {
                        tracing::debug!(
                            "Inserting new pact for consumer: '{}' and provider: '{}'",
                            consumer_name,
                            provider_name
                        );
                        // Insert new pact
                        merged_pacts.insert(key, pact_json.clone());
                    }
                } else {
                    println!("❌ Failed to load pact from JSON: {:?}", pact_res);
                    error!("Failed to load pact from JSON: {:?}", pact_res);
                    return Err(1);
                }
            }

            // Publish merged pacts
            for ((consumer_name, provider_name), pact_json) in merged_pacts.iter() {
                let pact_res = pact::load_pact_from_json(
                    &format!("{}-{}", consumer_name, provider_name),
                    pact_json,
                );
                match pact_res {
                    Ok(pact) => {
                        let consumer_name = pact.consumer().name.clone();
                        let provider_name = pact.provider().name.clone();
                        let pact_spec = pact.specification_version();
                        let pact_json_data = pact.to_json(pact_spec).unwrap();
                        let mut payload = json!({});
                        payload["pacticipantName"] = Value::String(consumer_name.clone());
                        if consumer_app_version != None {
                            payload["pacticipantVersionNumber"] =
                                Value::String(consumer_app_version.unwrap().to_string());
                        } else {
                            println!("❌ Error: Consumer app version is required to publish pact");
                            return Err(1);
                        }
                        if branch != None {
                            payload["branch"] = Value::String(branch.unwrap().to_string());
                        }
                        if build_url != None {
                            payload["buildUrl"] = Value::String(build_url.unwrap().to_string());
                        }
                        if let Some(tags) = args.get_many::<String>("tag") {
                            payload["tags"] = serde_json::Value::Array(vec![]);
                            for tag in tags {
                                payload["tags"]
                                    .as_array_mut()
                                    .unwrap()
                                    .push(serde_json::Value::String(tag.to_string()));
                            }
                        };
                        if tag_with_git_branch {
                            if !payload.get("tags").map_or(false, |v| v.is_array()) {
                                payload["tags"] = serde_json::Value::Array(vec![]);
                            }
                            payload["tags"].as_array_mut().unwrap().push(
                                serde_json::Value::String(
                                    git_info::commit(false).unwrap_or_default(),
                                ),
                            );
                        }

                        payload["contracts"] = serde_json::Value::Array(vec![json!({
                          "consumerName": consumer_name,
                          "providerName": provider_name,
                          "specification": "pact",
                          "contentType": "application/json",
                          "content": Base64.encode(pact_json_data.to_string()),
                          "onConflict": on_conflict
                        })]);
                        println!();
                        println!(
                            "📨 Attempting to publish pact for consumer: {} against provider: {}",
                            consumer_name, provider_name
                        );
                        let res = tokio::runtime::Runtime::new().unwrap().block_on(async {
                            hal_client
                                .clone()
                                .post_json(&(publish_pact_href), &payload.to_string(), None)
                                .await
                        });
                        match res {
                            Ok(res) => match output {
                                Ok(Some(output)) => {
                                    if output == "pretty" {
                                        let json = serde_json::to_string_pretty(&res).unwrap();
                                        println!("{}", json);
                                    } else if output == "json" {
                                        let json: String =
                                            serde_json::to_string(&res.clone()).unwrap();
                                        println!("{}", json);
                                    } else {
                                        let parsed_res = serde_json::from_value::<Root>(res);
                                        match parsed_res {
                                            Ok(parsed_res) => {
                                                print!("✅ ");
                                                if let Some(notices) = parsed_res.notices {
                                                    process_notices(&notices);
                                                } else {
                                                    println!(
                                                        "Pact published successfully for consumer: {} against provider: {}",
                                                        consumer_name, provider_name
                                                    );
                                                }
                                            }
                                            Err(err) => {
                                                println!(
                                                    "✅ Pact published successfully for consumer: {} against provider: {}",
                                                    consumer_name, provider_name
                                                );
                                                println!(
                                                    "⚠️ Warning: Failed to process response notices - Error: {:?}",
                                                    err
                                                );
                                            }
                                        }
                                    }
                                }
                                _ => {
                                    println!("{:?}", res.clone());
                                }
                            },
                            Err(err) => {
                                match &err {
                                    crate::cli::pact_broker::main::PactBrokerError::ValidationErrorWithNotices(messages, notices) => {
                                        println!("❌ Pact publication failed:");
                                        for message in messages {
                                            println!("   {}", message);
                                        }
                                        if !notices.is_empty() {
                                            println!("\nDetails:");
                                            process_notices(notices);
                                        }
                                    },
                                    _ => {
                                        println!("❌ {}", err.to_string());
                                    }
                                }
                                return Err(1);
                            }
                        }
                    }
                    _ => {
                        println!("❌ Failed to load pact from JSON: {:?}", pact_res);
                        return Err(1);
                    }
                }
            }
            Ok(json!({}))
        }
        Err(err) => {
            handle_error(err);
            return Err(1);
        }
    }
}

pub fn load_files(args: &ArgMatches) -> anyhow::Result<Vec<(String, Value)>> {
    let mut collected: Vec<(String, anyhow::Result<Value>)> = Vec::new();

    if let Some(inputs) = args.get_many::<String>("pact-files-dirs-or-globs") {
        for input in inputs {
            let path = std::path::Path::new(input);

            tracing::info!("Processing input: '{}'", input);

            match (path.exists(), path.is_file(), path.is_dir()) {
                (true, true, _) => {
                    tracing::debug!("Loading pact file: '{}'", input);
                    collected.push((input.to_string(), load_file(input)));
                }
                (true, false, true) => {
                    tracing::debug!("Loading pact files from directory: '{}'", input);
                    match load_files_from_dir(input) {
                        Ok(files) => {
                            for (name, pact) in files {
                                tracing::debug!("Loaded pact file from dir: '{}'", name);
                                collected.push((name, Ok(pact)));
                            }
                        }
                        Err(e) => {
                            tracing::error!(
                                "Failed to load pact files from directory '{}': {}",
                                input,
                                e
                            );
                            collected.push((input.to_string(), Err(e)));
                        }
                    }
                }
                (false, _, _) => {
                    tracing::error!("File or directory does not exist: '{}'", input);
                    error!("❌ File or directory does not exist: '{}'", input);
                    return Err(anyhow!("❌ File or directory does not exist: '{}'", input));
                }
                _ => {
                    // Treat as glob pattern
                    tracing::debug!("Treating input as glob pattern: '{}'", input);
                    match glob(input) {
                        Ok(paths) => {
                            let mut found = false;
                            for entry in paths {
                                match entry {
                                    Ok(pathbuf) => {
                                        if let Some(fname) = pathbuf.to_str() {
                                            tracing::debug!(
                                                "Loading pact file from glob match: '{}'",
                                                fname
                                            );
                                            collected.push((fname.to_string(), load_file(fname)));
                                            found = true;
                                        }
                                    }
                                    Err(e) => {
                                        tracing::error!(
                                            "Error processing glob entry for '{}': {}",
                                            input,
                                            e
                                        );
                                        collected.push((input.to_string(), Err(anyhow!(e))));
                                    }
                                }
                            }
                            if !found {
                                tracing::error!("No files matched glob pattern: '{}'", input);
                                error!("No files matched glob pattern: '{}'", input);
                                return Err(anyhow!(
                                    "❌ No files matched glob pattern: '{}'",
                                    input
                                ));
                            }
                        }
                        Err(e) => {
                            tracing::error!("Invalid glob pattern: '{}': {}", input, e);
                            error!("❌ Invalid glob pattern: '{}'", input);
                            return Err(anyhow!(e));
                        }
                    }
                }
            }
        }
    }

    if collected.is_empty() {
        tracing::error!("No pact files found to load");
        error!("No pact files found to load");
        return Err(anyhow!("No pact files found to load"));
    }

    let failures: Vec<_> = collected.iter().filter(|(_, res)| res.is_err()).collect();
    if !failures.is_empty() {
        let errors: Vec<(String, String, String)> = failures
            .iter()
            .filter_map(|(src, err)| {
                let error_msg = err.as_ref().err().map(|e| e.to_string());
                let source_type = if std::path::Path::new(src).is_file() {
                    "file"
                } else if std::path::Path::new(src).is_dir() {
                    "directory"
                } else if src.contains('*') || src.contains('?') || src.contains('[') {
                    "glob"
                } else {
                    "unknown"
                };
                error_msg.map(|msg| (src.clone(), source_type.to_string(), msg))
            })
            .collect();

        error!("Failed to load the following pact files:");
        for (source, source_type, err_msg) in &errors {
            tracing::error!("    '{}' [{}] - {}", source, source_type, err_msg);
        }

        let pretty_errors = errors
            .iter()
            .map(|(source, source_type, err_msg)| {
                format!(
                    "\n  Source: {}\n  Type: {}\n  Error: {}\n",
                    source, source_type, err_msg
                )
            })
            .collect::<String>();

        return Err(anyhow!(format!(
            "Failed to load one or more pact files:{}",
            pretty_errors
        )));
    } else {
        tracing::info!("Successfully loaded all pact files.");
        Ok(collected
            .into_iter()
            .map(|(src, res)| (src, res.unwrap()))
            .collect())
    }
}

fn fetch_pact(url: &str, args: &ArgMatches) -> anyhow::Result<(String, Value)> {
    let auth = if args.contains_id("user") {
        args.get_one::<String>("password").map(|user| {
            HttpAuth::User(
                user.to_string(),
                args.get_one::<String>("password").map(|p| p.to_string()),
            )
        })
    } else if args.contains_id("token") {
        args.get_one::<String>("token")
            .map(|token| HttpAuth::Token(token.to_string()))
    } else {
        None
    };
    http_utils::fetch_json_from_url(&url.to_string(), &auth)
}

fn load_file(file_name: &str) -> anyhow::Result<Value> {
    let file = File::open(file_name)?;
    serde_json::from_reader(file).context("file is not JSON")
}

pub fn load_files_from_dir(dir: &str) -> anyhow::Result<Vec<(String, Value)>> {
    let mut sources: Vec<(String, anyhow::Result<Value>)> = vec![];

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let file_path = entry.path();
        if file_path.is_file()
            && file_path
                .extension()
                .map(|ext| ext == "json")
                .unwrap_or(false)
        {
            let file_name = file_path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or(anyhow!("Invalid file name"))?;
            sources.push((
                file_name.to_string(),
                load_file(file_path.to_str().unwrap()),
            ));
        }
    }

    if sources.iter().any(|(_, res)| res.is_err()) {
        let errors: Vec<(String, String)> = sources
            .iter()
            .filter_map(|(source, result)| {
                result
                    .as_ref()
                    .err()
                    .map(|e| (source.clone(), e.to_string()))
            })
            .collect();

        error!("Failed to load the following pact files:");
        for (source, err_msg) in &errors {
            tracing::error!("    '{}' - {}", source, err_msg);
        }
        Err(anyhow!(format!(
            "Failed to load one or more pact files: {:?}",
            errors
                .iter()
                .map(|(source, err_msg)| format!("{}: {}", source, err_msg))
                .collect::<Vec<_>>()
        )))
    } else {
        Ok(sources
            .iter()
            .map(|(source, result)| (source.clone(), result.as_ref().unwrap().clone()))
            .collect())
    }
}

#[cfg(test)]
mod publish_contracts_tests {
    use crate::cli::pact_broker::main::pact_publish::publish_pacts;
    use crate::cli::pact_broker::main::subcommands::add_publish_pacts_subcommand;
    use base64::{Engine, engine::general_purpose::STANDARD as Base64};
    use pact_consumer::prelude::*;
    use pact_models::prelude::Generator;
    use pact_models::{PactSpecification, generators};
    use serde_json::{Value, json};
    use std::fs::File;
    use std::io::Read;

    #[test]
    fn publish_contracts_success() {
        // Arrange - set up the pact mock server (as v2 for compatibility with pact-ruby)
        let config = MockServerConfig {
            pact_specification: PactSpecification::V2,
            ..MockServerConfig::default()
        };
        let pacticipant_name = "Foo";
        let provider_name = "Bar";
        let version_number = "5556b8149bf8bac76bc30f50a8a2dd4c22c85f30";
        let branch = "main";
        let tag = "dev";
        let build_url = "http://build";
        let pact_file_path = "tests/fixtures/foo-bar.json";

        // Load pact file and encode content
        let mut pact_file = File::open(pact_file_path).expect("Fixture pact file missing");
        let mut pact_json_str = String::new();
        pact_file.read_to_string(&mut pact_json_str).unwrap();
        let mut pact_json: serde_json::Value = serde_json::from_str(&pact_json_str).unwrap();
        // Merge with existing keys in metadata if present
        let mut metadata = pact_json
            .get("metadata")
            .cloned()
            .unwrap_or_else(|| json!({}));
        if let Some(obj) = metadata.as_object_mut() {
            obj.insert(
                "pactRust".to_string(),
                json!({ "models": pact_models::PACT_RUST_VERSION }),
            );
            pact_json["metadata"] = Value::Object(obj.clone());
        } else {
            pact_json["metadata"] = json!({
            "pactRust": { "models": pact_models::PACT_RUST_VERSION },
            });
        }
        let expected_content = Base64.encode(pact_json.to_string());

        let request_body = json!({
            "pacticipantName": pacticipant_name,
            "pacticipantVersionNumber": version_number,
            "branch": branch,
            "tags": [tag],
            "buildUrl": build_url,
            "contracts": [
                {
                    "consumerName": pacticipant_name,
                    "providerName": provider_name,
                    "specification": "pact",
                    "contentType": "application/json",
                    "content": expected_content,
                    "onConflict": "merge"
                }
            ]
        });

        let contract_path_generator = generators! {
            "BODY" => {
            "$._links.pb:pb:publish-contracts.href" => Generator::MockServerURL(
                            "/contracts/publish".to_string(),
                            ".*\\/contracts\\/publish".to_string()
            )
            }
        };

        let pact_broker_service = PactBuilder::new("pact-broker-cli", "Pact Broker")
            .interaction("a request for the index resource", "", |mut i| {
                i.given("the pb:publish-contracts relations exists in the index resource");
                i.request
                    .path("/")
                    .header("Accept", "application/hal+json")
                    .header("Accept", "application/json");
                i.response
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(json_pattern!({
                        "_links": {
                            "pb:publish-contracts": {
                                "href": term!("http:\\/\\/.*\\/contracts\\/publish", "http://localhost:1234/contracts/publish"),
                                "title": "Publish contracts",
                                "templated": false
                            }
                        }
                    }))
                    .generators()
                    .add_generators(contract_path_generator);
                i
            })
            .interaction("a request to publish contracts", "", |mut i| {
                i.request
                    .post()
                    .path("/contracts/publish")
                    .header("Content-Type", "application/json")
                    .json_body(request_body.clone());
                i.response
                    .status(200)
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(json_pattern!({
                        "_embedded": {
                            "pacticipant": {
                                "name": pacticipant_name
                            },
                            "version": {
                                "number": version_number
                            }
                        },
                        "logs": each_like!({
                            "level": "info",
                            "message": "some message"
                        }),
                        "_links": {
                            "pb:pacticipant-version-tags": each_like!({ "name": tag }),
                            "pb:contracts": each_like!({ "href": like!("http://some-pact") })
                        }
                    }));
                i
            })
            .start_mock_server(None, Some(config));

        let mock_server_url = pact_broker_service.url();

        // Arrange - set up the command line arguments
        let matches = add_publish_pacts_subcommand().get_matches_from(vec![
            "publish",
            pact_file_path,
            "-b",
            mock_server_url.as_str(),
            "--consumer-app-version",
            version_number,
            "--branch",
            branch,
            "--tag",
            tag,
            "--build-url",
            build_url,
            "--merge",
        ]);

        // Act
        let result = publish_pacts(&matches);

        // Assert
        assert!(result.is_ok());
        let value = result.unwrap();

        assert!(value.is_object());
    }

    #[test]
    fn test_error_handling_with_notices() {
        // Test the handle_validation_errors function with a response containing notices
        use crate::cli::pact_broker::main::handle_validation_errors;
        use serde_json::json;

        let error_response = json!({
            "errors": ["Consumer version not found"],
            "notices": [
                {
                    "text": "Please create the consumer version first",
                    "type": "error"
                },
                {
                    "text": "Visit https://docs.pact.io for more information",
                    "type": "info"
                }
            ]
        });

        let result = handle_validation_errors(error_response);

        match result {
            crate::cli::pact_broker::main::PactBrokerError::ValidationErrorWithNotices(
                messages,
                notices,
            ) => {
                assert_eq!(messages.len(), 1);
                assert_eq!(messages[0], "Consumer version not found");
                assert_eq!(notices.len(), 2);
                assert_eq!(notices[0].text, "Please create the consumer version first");
                assert_eq!(notices[0].type_field, "error");
                assert_eq!(
                    notices[1].text,
                    "Visit https://docs.pact.io for more information"
                );
                assert_eq!(notices[1].type_field, "info");
            }
            _ => panic!("Expected ValidationErrorWithNotices variant"),
        }
    }

    #[test]
    fn test_error_handling_without_notices() {
        // Test the handle_validation_errors function with a response without notices
        use crate::cli::pact_broker::main::handle_validation_errors;
        use serde_json::json;

        let error_response = json!({
            "errors": ["Invalid pact file format"]
        });

        let result = handle_validation_errors(error_response);

        match result {
            crate::cli::pact_broker::main::PactBrokerError::ValidationError(messages) => {
                assert_eq!(messages.len(), 1);
                assert_eq!(messages[0], "Invalid pact file format");
            }
            _ => panic!("Expected ValidationError variant"),
        }
    }

    #[test]
    fn test_error_handling_notices_only() {
        // Test the handle_validation_errors function with notices but no explicit errors
        use crate::cli::pact_broker::main::handle_validation_errors;
        use serde_json::json;

        let error_response = json!({
            "notices": [
                {
                    "text": "Pact could not be published because version already exists",
                    "type": "error"
                },
                {
                    "text": "Use --overwrite flag to replace existing pact",
                    "type": "warning"
                }
            ]
        });

        let result = handle_validation_errors(error_response);

        match result {
            crate::cli::pact_broker::main::PactBrokerError::ValidationErrorWithNotices(
                messages,
                notices,
            ) => {
                assert_eq!(messages.len(), 2);
                assert_eq!(
                    messages[0],
                    "Pact could not be published because version already exists"
                );
                assert_eq!(messages[1], "Use --overwrite flag to replace existing pact");
                assert_eq!(notices.len(), 2);
                assert_eq!(
                    notices[0].text,
                    "Pact could not be published because version already exists"
                );
                assert_eq!(notices[0].type_field, "error");
                assert_eq!(
                    notices[1].text,
                    "Use --overwrite flag to replace existing pact"
                );
                assert_eq!(notices[1].type_field, "warning");
            }
            _ => panic!("Expected ValidationErrorWithNotices variant"),
        }
    }

    #[test]
    fn test_duplicate_interaction_detection() {
        use crate::cli::pact_broker::main::pact_publish::merge_interactions_or_messages;
        use serde_json::json;

        // Create two interactions with same description and state but different content
        let interaction1 = json!({
            "description": "test interaction",
            "providerState": "test state",
            "request": {
                "method": "POST",
                "path": "/",
                "body": {"complete": {"certificateUri": "http://..."}}
            },
            "response": {
                "status": 200,
                "body": {"_id": "1234", "desc": "Response 1"}
            }
        });

        let interaction2 = json!({
            "description": "test interaction",
            "providerState": "test state",
            "request": {
                "method": "GET",
                "path": "/",
                "headers": {"TEST-X": "X, Y"}
            },
            "response": {
                "status": 200,
                "body": {"_id": "5678", "desc": "Response 2"}
            }
        });

        let mut existing_interactions = vec![interaction1];
        let additional_interactions = vec![interaction2];

        // This should fail with a PactMergeError
        let result =
            merge_interactions_or_messages(&mut existing_interactions, &additional_interactions);

        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(
            error
                .message
                .contains("Two interactions have been found with same description")
        );
        assert!(error.message.contains("test interaction"));
        assert!(error.message.contains("test state"));
    }

    #[test]
    fn test_identical_interactions_allowed() {
        use crate::cli::pact_broker::main::pact_publish::merge_interactions_or_messages;
        use serde_json::json;

        // Create two identical interactions
        let interaction = json!({
            "description": "test interaction",
            "providerState": "test state",
            "request": {
                "method": "GET",
                "path": "/"
            },
            "response": {
                "status": 200,
                "body": {"message": "success"}
            }
        });

        let mut existing_interactions = vec![interaction.clone()];
        let additional_interactions = vec![interaction];

        // This should succeed - identical interactions are allowed
        let result =
            merge_interactions_or_messages(&mut existing_interactions, &additional_interactions);

        assert!(result.is_ok());
        assert_eq!(existing_interactions.len(), 1); // Still only one interaction
    }

    #[test]
    fn test_different_interactions_allowed() {
        use crate::cli::pact_broker::main::pact_publish::merge_interactions_or_messages;
        use serde_json::json;

        // Create two different interactions (different descriptions)
        let interaction1 = json!({
            "description": "test interaction 1",
            "providerState": "test state",
            "request": {"method": "GET", "path": "/"},
            "response": {"status": 200}
        });

        let interaction2 = json!({
            "description": "test interaction 2",
            "providerState": "test state",
            "request": {"method": "POST", "path": "/"},
            "response": {"status": 201}
        });

        let mut existing_interactions = vec![interaction1];
        let additional_interactions = vec![interaction2];

        // This should succeed - different descriptions are allowed
        let result =
            merge_interactions_or_messages(&mut existing_interactions, &additional_interactions);

        assert!(result.is_ok());
        assert_eq!(existing_interactions.len(), 2); // Now we have both interactions
    }
}
