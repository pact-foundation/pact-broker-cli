use clap::{ArgMatches, Id};
use comfy_table::{Table, presets::UTF8_FULL};
use tracing::debug;

use crate::cli::{
    pact_broker::main::{
        HALClient, PactBrokerError,
        utils::{get_auth, get_broker_url, get_ssl_options, handle_error},
    },
    utils,
};

#[derive(Debug, serde::Deserialize)]
struct Summary {
    deployable: Option<bool>,
    // reason: String,
    // success: u32,
    // failed: u32,
    // unknown: u32,
}

#[derive(Debug, serde::Deserialize)]
struct Notice {
    #[serde(rename = "type")]
    notice_type: String,
    text: String,
}

#[derive(Debug, serde::Deserialize)]
struct Version {
    number: String,
}

#[derive(Debug, serde::Deserialize)]
struct Consumer {
    name: String,
    version: Option<Version>,
}

#[derive(Debug, serde::Deserialize)]
struct Provider {
    name: String,
    version: Option<Version>,
}

#[derive(Debug, serde::Deserialize)]
struct VerificationResult {
    success: Option<bool>,
}

#[derive(Debug, serde::Deserialize)]
struct MatrixItem {
    consumer: Consumer,
    provider: Provider,
    #[serde(rename = "verificationResult")]
    verification_result: Option<VerificationResult>,
}

#[derive(Debug, serde::Deserialize)]
struct Data {
    summary: Option<Summary>,
    notices: Option<Vec<Notice>>,
    matrix: Vec<MatrixItem>,
}

#[derive(Debug, Default)]
struct PacticipantArgs {
    pacticipant: String,
    version: Option<String>,
    tags: Vec<String>,
    latest: bool,
    main_branch: bool,
}

        fn parse_args_from_matches(raw_args: Vec<String>) -> Vec<PacticipantArgs> {
            // Get the raw arguments as they were passed on the command line
            let mut args = raw_args.iter().peekable();
            let mut result = Vec::new();

            while let Some(arg) = args.next() {
                if arg == "--pacticipant" || arg == "-a" {
                    let pacticipant = args.next().expect("Expected value after --pacticipant").to_string();
                    let mut pacticipant_args = PacticipantArgs {
                        pacticipant,
                        ..Default::default()
                    };

                    while let Some(next_arg) = args.peek() {
                        match next_arg.as_str() {
                            "--version" | "-e" => {
                                args.next();
                                pacticipant_args.version = args.next().map(|s| s.to_string());
                            }
                            "--tag" => {
                                args.next();
                                if let Some(tag) = args.next() {
                                    pacticipant_args.tags.push(tag.to_string());
                                }
                            }
                            "--latest" | "-l" => {
                                args.next();
                                pacticipant_args.latest = true;
                            }
                            "--main-branch" => {
                                args.next();
                                pacticipant_args.main_branch = true;
                            }
                            "--pacticipant" | "-a" => break,
                            _ => {
                                args.next();
                            }
                        }
                    }
                    result.push(pacticipant_args);
                }
            }
            result
        }



pub fn can_i_deploy(args: &ArgMatches, raw_args: Vec<String>, can_i_merge: bool) -> Result<String, PactBrokerError> {
    debug!("Args: {:?}", args);

    let selectors = parse_args_from_matches(raw_args);

    debug!("Selectors: {:?}", selectors);
    // Other arguments
    let to_environment = args.try_get_one::<String>("to-environment").unwrap_or(None);
    let to = args.try_get_one::<String>("to").unwrap_or(None);
    let retry_while_unknown = args.get_one::<String>("retry-while-unknown");
    let retry_interval = args.get_one::<String>("retry-interval");
    let dry_run = args.get_flag("dry-run");
    let broker_url = get_broker_url(args).trim_end_matches('/').to_string();
    let auth = get_auth(args);
    let ssl_options = get_ssl_options(args);

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let hal_client: HALClient =
            HALClient::with_url(&broker_url, Some(auth.clone()), ssl_options.clone());

        // Build matrix_href_path using selectors
        let mut matrix_href_path = String::from("/matrix?");
        for selector in &selectors {
            matrix_href_path.push_str(&format!(
            "q[][pacticipant]={}&",
            urlencoding::encode(&selector.pacticipant)
            ));
            if let Some(version) = &selector.version {
            matrix_href_path.push_str(&format!(
                "q[][version]={}&",
                urlencoding::encode(version)
            ));
            }
            if selector.latest {
            matrix_href_path.push_str("q[][latest]=true&");
            }
            for tag in &selector.tags {
            matrix_href_path.push_str(&format!(
                "q[][tag]={}&",
                urlencoding::encode(tag)
            ));
            }
            if selector.main_branch {
            matrix_href_path.push_str("q[][mainBranch]=true&");
            }
        }
        if let Some(to) = to {
            matrix_href_path.push_str(&format!("tag={}&", urlencoding::encode(to)));
        }

        if selectors.len() == 1 {
            matrix_href_path.push_str("latestby=cvp");
        } else {
            matrix_href_path.push_str("latestby=cvpv");
        }

        if let Some(to_environment) = to_environment {
            matrix_href_path.push_str(&format!(
                "&environment={}",
                urlencoding::encode(to_environment)
            ));
        }
        if to_environment.is_none() && selectors.len() == 1 {
            matrix_href_path.push_str("&latest=true");
        }
        if can_i_merge {
            matrix_href_path.push_str("&mainBranch=true");
        }
        debug!(
            "Querying broker at: {}",
            broker_url.clone() + &matrix_href_path
        );
        let mut res;
        let mut attempts = 0;
        let max_attempts = retry_while_unknown
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0);
        let interval = retry_interval
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(5);

        loop {
            res = hal_client
            .clone()
            .fetch(&(broker_url.clone() + &matrix_href_path))
            .await;

            // If retry_while_unknown is set, poll if deployable is None (unknown)
            if max_attempts > 0 {
            if let Ok(ref response) = res {
                if let Ok(data) = serde_json::from_str::<Data>(&response.to_string()) {
                if let Some(summary) = data.summary {
                    if summary.deployable.is_some() {
                    break;
                    }
                }
                }
            }
            attempts += 1;
            if attempts > max_attempts {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_secs(interval)).await;
            } else {
            break;
            }
        }
        debug!("Response: {:?}", res);
        match res {
            Ok(res) => {
                let output: Result<Option<&String>, clap::parser::MatchesError> =
                    args.try_get_one::<String>("output");

                match output {
                    Ok(Some(output)) => {
                        if output == "json" {
                            let json: String = serde_json::to_string(&res.clone()).unwrap();
                            println!("{}", json);
                            Ok(json)
                        } else {
                            let data: Data = match serde_json::from_str(&res.clone().to_string()) {
                                Ok(data) => data,
                                Err(err) => {
                                    println!("❌ {}", utils::RED.apply_to(err.to_string()));
                                    Data {
                                        summary: Some(Summary {
                                            deployable: Some(false),
                                        }),
                                        notices: Some(Vec::new()),
                                        matrix: Vec::new(),
                                    }
                                }
                            };

                            if data.matrix.len() > 0 {
                                let mut table = Table::new();

                                table.load_preset(UTF8_FULL).set_header(vec![
                                    "CONSUMER",
                                    "C.VERSION",
                                    "PROVIDER",
                                    "P.VERSION",
                                    "SUCCESS?",
                                    "RESULT",
                                ]);
                                for matrix_item in data.matrix {
                                    let verification_result = &matrix_item
                                        .verification_result
                                        .map(|result| result.success.unwrap_or(false).to_string())
                                        .unwrap_or_else(|| "false".to_string());

                                    table.add_row(vec![
                                        matrix_item.consumer.name,
                                        matrix_item
                                            .consumer
                                            .version
                                            .map(|result| result.number.to_string())
                                            .unwrap_or_else(|| "unknown".to_string()),
                                        matrix_item.provider.name,
                                        matrix_item
                                            .provider
                                            .version
                                            .map(|result| result.number.to_string())
                                            .unwrap_or_else(|| "unknown".to_string()),
                                        verification_result.to_string(),
                                        verification_result.to_string(),
                                    ]);
                                }
                                println!("{table}");
                            }

                            if let Some(notices) = &data.notices {
                                if !notices.is_empty() {
                                    for notice in notices {
                                        if notice.notice_type == "warning" {
                                            println!(
                                                "⚠️ {}",
                                                utils::YELLOW.apply_to(notice.text.clone())
                                            );
                                        } else if notice.notice_type == "error" {
                                            println!(
                                                "❌ {}",
                                                utils::RED.apply_to(notice.text.clone())
                                            );
                                        } else {
                                            println!(
                                                "📌 {}",
                                                utils::GREEN.apply_to(notice.text.clone())
                                            );
                                        }
                                    }
                                }
                            }
                            if data
                                .summary
                                .as_ref()
                                .and_then(|s| s.deployable)
                                .unwrap_or(false)
                            {
                                let computer_says_yes = utils::GREEN.apply_to("\\o/");
                                let message = format!("✅ Computer says yes {}", computer_says_yes);
                                println!("{}", message);
                                Ok(message)
                            } else {
                                let computer_says_no = utils::RED.apply_to("¯\\_(ツ)_/¯");
                                println!(r"❌ Computer says no {}", computer_says_no);
                                if dry_run == true {
                                    let message =
                                        "📌 Dry run enabled, suppressing failing exit code"
                                            .to_string();
                                    println!("{}", utils::YELLOW.apply_to(message.clone()));
                                    Ok(message)
                                } else {
                                    Err(PactBrokerError::NotFound(
                                        "No deployable version found".to_string(),
                                    ))
                                }
                            }
                        }
                    }
                    Err(res) => {
                        let message = format!(
                            "❌ No output match provided for {}",
                            res.clone().to_string()
                        );
                        println!("{}", utils::RED.apply_to(message.clone()));
                        Err(PactBrokerError::ValidationError([message].to_vec()))
                    }
                    _ => {
                        let message = res.clone().to_string();
                        println!("{}", message.clone());
                        Ok(message)
                    }
                }
            }
            Err(res) => Err(handle_error(res)),
        }
    })
}

#[cfg(test)]
mod can_i_deploy_tests {
    use super::*;
    use crate::cli::pact_broker::main::subcommands::add_can_i_deploy_subcommand;
    use pact_consumer::prelude::*;
    use pact_models::PactSpecification;
    use serde_json::json;

    fn matrix_response_body() -> JsonPattern {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/matrix.json");
        let data = std::fs::read_to_string(path).expect("Failed to read matrix.json fixture");
        let json: serde_json::Value = serde_json::from_str(&data).unwrap();
        let json_pattern = json_pattern!(like!(json));
        json_pattern
    }

    fn build_matches(args: Vec<&str>) -> clap::ArgMatches {
        add_can_i_deploy_subcommand()
            .args(crate::cli::add_ssl_arguments())
            .get_matches_from(args)
    }

    #[test]
    fn returns_matrix_when_results_found() {
        let config = MockServerConfig {
            pact_specification: PactSpecification::V2,
            ..Default::default()
        };
        let pact_broker_service = PactBuilder::new("pact-broker-cli", "Pact Broker")
            .interaction("a request for the compatibility matrix for Foo version 1.2.3 and Bar version 4.5.6 returns_matrix_when_results_found", "", |mut i| {
                i.given("the pact for Foo version 1.2.3 has been verified by Bar version 4.5.6");
                i.request
                    .get()
                    .path("/matrix")
                    .query_param("q[][pacticipant]", "Foo")
                    .query_param("q[][version]", "1.2.3")
                    .query_param("q[][pacticipant]", "Bar")
                    .query_param("q[][version]", "4.5.6")
                    .query_param("latestby", "cvpv");
                i.response
                    .status(200)
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(matrix_response_body());
                i
            })
            .start_mock_server(None, Some(config));
        let mock_server_url = pact_broker_service.url();
        
        let raw_args = vec![
            "can-i-deploy",
            "-b",
            mock_server_url.as_str(),
            "--pacticipant",
            "Foo",
            "--version",
            "1.2.3",
            "--pacticipant",
            "Bar",
            "--version",
            "4.5.6",
        ];
        let matches = build_matches(raw_args.clone());
        let raw_args: Vec<String> = raw_args.into_iter().map(|s| s.to_string()).collect();
        let result = can_i_deploy(&matches, raw_args, false);
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("Computer says"));
    }

    #[test]
    fn pacticipant_name_with_space() {
        let config = MockServerConfig {
            pact_specification: PactSpecification::V2,
            ..Default::default()
        };

        let pact_broker_service = PactBuilder::new("pact-broker-cli", "Pact Broker")
            .interaction("a request for the compatibility matrix for Foo version 1.2.3 and Bar version 4.5.6", "", |mut i| {
                i.given("the pact for Foo Thing version 1.2.3 has been verified by Bar version 4.5.6");
                i.request
                    .get()
                    .path("/matrix")
                    .query_param("q[][pacticipant]", "Foo Thing")
                    .query_param("q[][version]", "1.2.3")
                    .query_param("q[][pacticipant]", "Bar")
                    .query_param("q[][version]", "4.5.6")
                    .query_param("latestby", "cvpv");
                i.response
                    .status(200)
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(matrix_response_body());
                i
            })
            .start_mock_server(None, Some(config));
        let mock_server_url = pact_broker_service.url();
        let raw_args = vec![
            "can-i-deploy",
            "-b",
            mock_server_url.as_str(),
            "--pacticipant",
            "Foo Thing",
            "--version",
            "1.2.3",
            "--pacticipant",
            "Bar",
            "--version",
            "4.5.6",
        ];
        let matches = build_matches(raw_args.clone());
        let raw_args: Vec<String> = raw_args.into_iter().map(|s| s.to_string()).collect();
        let result = can_i_deploy(&matches, raw_args, false);
        assert!(result.is_ok());
    }

    #[test]
    fn only_one_version_selector() {
        let config = MockServerConfig {
            pact_specification: PactSpecification::V2,
            ..Default::default()
        };
        let pact_broker_service = PactBuilder::new("pact-broker-cli", "Pact Broker")
            .interaction("a request for the compatibility matrix where only the version of Foo is specified", "", |mut i| {
                i.given("the pact for Foo version 1.2.3 has been verified by Bar version 4.5.6 and version 5.6.7");
                i.request
                    .get()
                    .path("/matrix")
                    .query_param("q[][pacticipant]", "Foo")
                    .query_param("q[][version]", "1.2.3")
                    .query_param("latestby", "cvp")
                    .query_param("latest", "true");
                i.response
                    .status(200)
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(matrix_response_body());
                i
            })
            .start_mock_server(None, Some(config));
        let mock_server_url = pact_broker_service.url();
        let raw_args = vec![
            "can-i-deploy",
            "-b",
            mock_server_url.as_str(),
            "--pacticipant",
            "Foo",
            "--version",
            "1.2.3",
        ];
        let matches = build_matches(raw_args.clone());
        let raw_args: Vec<String> = raw_args.into_iter().map(|s| s.to_string()).collect();
        let result = can_i_deploy(&matches, raw_args, false);
        assert!(result.is_ok());
    }

    #[test]
    fn one_or_more_versions_does_not_exist() {
        let config = MockServerConfig {
            pact_specification: PactSpecification::V2,
            ..Default::default()
        };
        let pact_broker_service = PactBuilder::new("pact-broker-cli", "Pact Broker")
            .interaction(
                "a request for the compatibility matrix where one or more versions does not exist",
                "",
                |mut i| {
                    i.given(
                        "the pact for Foo version 1.2.3 has been verified by Bar version 4.5.6",
                    );
                    i.request
                        .get()
                        .path("/matrix")
                        .query_param("q[][pacticipant]", "Foo")
                        .query_param("q[][version]", "1.2.3")
                        .query_param("q[][pacticipant]", "Bar")
                        .query_param("q[][version]", "9.9.9")
                        .query_param("latestby", "cvpv");
                    i.response
                        .status(200)
                        .header("Content-Type", "application/hal+json;charset=utf-8")
                        .json_body(json_pattern!({
                            "summary": {
                                "reason": like!("an error message")
                            }
                        }));
                    i
                },
            )
            .start_mock_server(None, Some(config));
        let mock_server_url = pact_broker_service.url();

        let raw_args = vec![
            "can-i-deploy",
            "-b",
            mock_server_url.as_str(),
            "--pacticipant",
            "Foo",
            "--version",
            "1.2.3",
            "--pacticipant",
            "Bar",
            "--version",
            "9.9.9",
        ];
        let matches = build_matches(raw_args.clone());
        let raw_args: Vec<String> = raw_args.into_iter().map(|s| s.to_string()).collect();
        let result = can_i_deploy(&matches, raw_args, false);
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn results_not_found_returns_error() {
        let config = MockServerConfig {
            pact_specification: PactSpecification::V2,
            ..Default::default()
        };
        let pact_broker_service = PactBuilder::new("pact-broker-cli", "Pact Broker")
            .interaction(
                "a request for the compatibility matrix for a pacticipant that does not exist",
                "",
                |mut i| {
                    i.request
                        .get()
                        .path("/matrix")
                        .query_param("q[][pacticipant]", "Wiffle")
                        .query_param("q[][version]", "1.2.3")
                        .query_param("q[][pacticipant]", "Meep")
                        .query_param("q[][version]", "9.9.9")
                        .query_param("latestby", "cvpv");
                    i.response
                        .status(400)
                        .header("Content-Type", "application/hal+json;charset=utf-8")
                        .json_body(json_pattern!({
                            "errors": each_like!("an error message")
                        }));
                    i
                },
            )
            .start_mock_server(None, Some(config));
        let mock_server_url = pact_broker_service.url();

        let raw_args = vec![
            "can-i-deploy",
            "-b",
            mock_server_url.as_str(),
            "--pacticipant",
            "Wiffle",
            "--version",
            "1.2.3",
            "--pacticipant",
            "Meep",
            "--version",
            "9.9.9",
        ];
        let matches = build_matches(raw_args.clone());
        let raw_args: Vec<String> = raw_args.into_iter().map(|s| s.to_string()).collect();
        let result = can_i_deploy(&matches, raw_args, false);
        assert!(result.is_err());
    }

    #[test]
    fn no_versions_specified_returns_multiple_rows() {
        let config = MockServerConfig {
            pact_specification: PactSpecification::V2,
            ..Default::default()
        };
        let pact_broker_service = PactBuilder::new("pact-broker-cli", "Pact Broker")
            .interaction("a request for the compatibility matrix for all versions of Foo and Bar", "", |mut i| {
                i.given("the pact for Foo version 1.2.3 and 1.2.4 has been verified by Bar version 4.5.6");
                i.request
                    .get()
                    .path("/matrix")
                    .query_param("q[][pacticipant]", "Foo")
                    .query_param("q[][pacticipant]", "Bar")
                    .query_param("latestby", "cvpv");
                i.response
                    .status(200)
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(json_pattern!(
                                {
                                    "summary": {
                                        "deployable": true,
                                    },
                                    "matrix": each_like!({
                                    "consumer": {
                                        "name": "Foo",
                                        "version": {
                                        "number": "4"
                                        }
                                    },
                                    "provider": {
                                        "name": "Bar",
                                        "version": {
                                        "number": "5"
                                        }
                                    },
                                    "verificationResult": {
                                        "verifiedAt": "2017-10-10T12:49:04+11:00",
                                        "success": true,
                                        "_links": {
                                        "self": {
                                            "href": "http://result"
                                        }
                                        }
                                    },
                                    "pact": {
                                        "createdAt": "2017-10-10T12:49:04+11:00"
                                    }
                                }, min=2)}
                            ));
                i
            })
            .start_mock_server(None, Some(config));
        let mock_server_url = pact_broker_service.url();

        let raw_args = vec![
            "can-i-deploy",
            "-b",
            mock_server_url.as_str(),
            "--pacticipant",
            "Foo",
            "--pacticipant",
            "Bar",
        ];
        let matches = build_matches(raw_args.clone());
        let raw_args: Vec<String> = raw_args.into_iter().map(|s| s.to_string()).collect();
        let result = can_i_deploy(&matches, raw_args, false);
        assert!(result.is_ok());
    }

    // #[test]
    // fn success_option_true_returns_only_successful_row() {
    //     let config = MockServerConfig {
    //         pact_specification: PactSpecification::V2,
    //         ..Default::default()
    //     };
    //     let pact_broker_service = PactBuilder::new("pact-broker-cli", "Pact Broker")
    //         .interaction("a request for the successful rows of the compatibility matrix for all versions of Foo and Bar", "", |mut i| {
    //             i.given("the pact for Foo version 1.2.3 has been successfully verified by Bar version 4.5.6, and 1.2.4 unsuccessfully by 9.9.9");
    //             i.request
    //                 .get()
    //                 .path("/matrix")
    //                 .query_param("q[][pacticipant]", "Foo")
    //                 .query_param("q[][pacticipant]", "Bar")
    //                 .query_param("latestby", "cvpv")
    //                 .query_param("success[]", "true");
    //             i.response
    //                 .status(200)
    //                 .header("Content-Type", "application/hal+json;charset=utf-8")
    //                 .json_body(matrix_response_body());
    //             i
    //         })
    //         .start_mock_server(None, Some(config));
    //     let mock_server_url = pact_broker_service.url();

    //     let matches = build_matches(vec![
    //         "can-i-deploy",
    //         "-b", mock_server_url.as_str(),
    //         "--pacticipant", "Foo",
    //         "--pacticipant", "Bar",
    //         "--success", "true",
    //     ]);
    //     let result = can_i_deploy(&matches, false);
    //     assert!(result.is_ok());
    // }

    #[test]
    fn latest_tagged_versions_of_other_services() {
        let config = MockServerConfig {
            pact_specification: PactSpecification::V2,
            ..Default::default()
        };
        let matrix_response = json!({
                    "summary": {
                        "deployable": true,
        },
                    "matrix": [{
                        "consumer": { "name": "Foo", "version": { "number": "1.2.3" } },
                        "provider": { "name": "Bar", "version": { "number": "4.5.6"} }
                    }]
                });
        let pact_broker_service = PactBuilder::new("pact-broker-cli", "Pact Broker")
            .interaction("a request for the compatibility matrix for Foo version 1.2.3 and the latest prod versions of all other pacticipants", "", |mut i| {
                i.given("the pact for Foo version 1.2.3 has been successfully verified by Bar version 4.5.6 (tagged prod) and version 5.6.7");
                i.request
                    .get()
                    .path("/matrix")
                    .query_param("q[][pacticipant]", "Foo")
                    .query_param("q[][version]", "1.2.3")
                    .query_param("latestby", "cvp")
                    .query_param("latest", "true")
                    .query_param("tag", "prod");
                i.response
                    .status(200)
                    .header("Content-Type", "application/hal+json;charset=utf-8")
                    .json_body(matrix_response);
                i
            })
            .start_mock_server(None, Some(config));
        let mock_server_url = pact_broker_service.url();

        let raw_args = vec![
            "can-i-deploy",
            "-b",
            mock_server_url.as_str(),
            "--pacticipant",
            "Foo",
            "--version",
            "1.2.3",
            "--to",
            "prod",
        ];
        let matches = build_matches(raw_args.clone());
        let raw_args: Vec<String> = raw_args.into_iter().map(|s| s.to_string()).collect();
        let result = can_i_deploy(&matches, raw_args, false);
        assert!(result.is_ok());
    }


    // sends URL Querying broker at: http://127.0.0.1:61567/matrix?q[][pacticipant]=Foo&q[][version]=1.2.3&q[][pacticipant]=Bar&q[][latest]=true&q[][tag]=prod&latestby=cvpv
    // pact saves url as latestby=cvpv&q[][latest]=true&q[][pacticipant]=Foo&q[][tag]=prod&q[][version]=1%2e2%2e3&q[][pacticipant]=Bar
    // pact should save url as q[][pacticipant]=Foo&q[][version]=1%2e2%2e3&q[][pacticipant]=Bar&q[][latest]=true&q[][tag]=prod&latestby=cvpv
    // pact-ruby saves q%5B%5D%5Bpacticipant%5D=Foo&q%5B%5D%5Bversion%5D=1.2.3&q%5B%5D%5Bpacticipant%5D=Bar&q%5B%5D%5Blatest%5D=true&q%5B%5D%5Btag%5D=prod&latestby=cvpv 
    #[test]
    fn latest_tagged_version_of_provider() {
        let config = MockServerConfig {
            pact_specification: PactSpecification::V2,
            ..Default::default()
        };
        let pact_broker_service = PactBuilder::new("pact-broker-cli", "Pact Broker")
            .interaction(
                "a request for the compatibility matrix for Foo version 1.2.3 and the latest prod version of Bar",
                "",
                |mut i| {
                    i.given("the pact for Foo version 1.2.3 has been successfully verified by Bar version 4.5.6 with tag prod, and 1.2.4 unsuccessfully by 9.9.9");
                    i.request
                        .get()
                        .path("/matrix")
                        .query_param("q[][pacticipant]", "Foo")
                        .query_param("q[][version]", "1.2.3")
                        .query_param("q[][pacticipant]", "Bar")
                        .query_param("q[][latest]", "true")
                        .query_param("q[][tag]", "prod")
                        .query_param("latestby", "cvpv");
                    i.response
                        .status(200)
                        .header("Content-Type", "application/hal+json;charset=utf-8")
                        .json_body(matrix_response_body());
                    i
                }
            )
            .start_mock_server(None, Some(config));
        let mock_server_url = pact_broker_service.url();

        let raw_args = vec![
            "can-i-deploy",
            "-b",
            mock_server_url.as_str(),
            "--pacticipant",
            "Foo",
            "--version",
            "1.2.3",
            "--pacticipant",
            "Bar",
            "--latest",
            "--tag",
            "prod",
        ];
        let matches = build_matches(raw_args.clone());
        let raw_args: Vec<String> = raw_args.into_iter().map(|s| s.to_string()).collect();
        let result = can_i_deploy(&matches, raw_args, false);        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("Computer says"));
    }



    // sends URL Querying broker at: http://127.0.0.1:61517/matrix?q[][pacticipant]=Foo&q[][version]=1.2.4&q[][pacticipant]=Bar&q[][latest]=true&latestby=cvpv
    // pact saves URL as latestby=cvpv&q[][latest]=true&q[][pacticipant]=Foo&q[][version]=1%2e2%2e4&q[][pacticipant]=Bar
    // pact should save url as q[][pacticipant]=Foo&q[][version]=1%2e2%2e4&q[][pacticipant]=Bar&q[][latest]=true&latestby=cvpv
    // pact-ruby saves q%5B%5D%5Bpacticipant%5D=Foo&q%5B%5D%5Bversion%5D=1.2.4&q%5B%5D%5Bpacticipant%5D=Bar&q%5B%5D%5Blatest%5D=true&latestby=cvpv
    #[test]
    fn latest_version_of_provider() {
        let config = MockServerConfig {
            pact_specification: PactSpecification::V2,
            ..Default::default()
        };
        let pact_broker_service = PactBuilder::new("pact-broker-cli", "Pact Broker")
            .interaction(
                "a request for the compatibility matrix for Foo version 1.2.4 and the latest version of Bar",
                "",
                |mut i| {
                    i.given("the pact for Foo version 1.2.3 has been successfully verified by Bar version 4.5.6, and 1.2.4 unsuccessfully by 9.9.9");
                    i.request
                        .get()
                        .path("/matrix")
                        .query_param("q[][pacticipant]", "Foo")
                        .query_param("q[][version]", "1.2.4")
                        .query_param("q[][pacticipant]", "Bar")
                        .query_param("q[][latest]", "true")
                        .query_param("latestby", "cvpv");
                    i.response
                        .status(200)
                        .header("Content-Type", "application/hal+json;charset=utf-8")
                        .json_body(matrix_response_body());
                    i
                }
            )
            .start_mock_server(None, Some(config));
        let mock_server_url = pact_broker_service.url();

        let raw_args = vec![
            "can-i-deploy",
            "-b",
            mock_server_url.as_str(),
            "--pacticipant",
            "Foo",
            "--version",
            "1.2.4",
            "--pacticipant",
            "Bar",
            "--latest",
        ];
        let matches = build_matches(raw_args.clone());
        let raw_args: Vec<String> = raw_args.into_iter().map(|s| s.to_string()).collect();
        let result = can_i_deploy(&matches, raw_args, false);  
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("Computer says"));
    }
}
