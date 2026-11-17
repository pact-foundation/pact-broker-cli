use clap::{ArgMatches, Command};
use tracing::error;

use crate::cli::{
    pact_broker::main::utils::handle_error,
    pactflow::main::{
        provider_contracts,
        subcommands::{
            add_delete_provider_contract_subcommand, add_list_provider_contracts_subcommand,
            add_publish_provider_contract_subcommand,
        },
    },
};
pub fn add_pactflow_client_command() -> Command {
    Command::new("pactflow")
        .about("PactFlow specific commands")
        .arg_required_else_help(true)
        .subcommand(add_publish_provider_contract_subcommand())
        .subcommand(add_delete_provider_contract_subcommand())
        .subcommand(add_list_provider_contracts_subcommand())
}

pub fn run(args: &ArgMatches) -> Result<serde_json::Value, i32> {
    match args.subcommand() {
        Some(("publish-provider-contract", args)) => {
            let res = provider_contracts::publish::publish(args);
            if let Err(err) = res {
                handle_error(err);
                Err(1)
            } else {
                Ok(serde_json::to_value(res.unwrap()).unwrap())
            }
        }
        Some(("delete-provider-contract", args)) => {
            if let Err(err) = provider_contracts::delete::delete(args) {
                handle_error(err);
                Err(1)
            } else {
                Ok(serde_json::json!({}))
            }
        }
        Some(("list-provider-contracts", args)) => {
            if let Err(err) = provider_contracts::list::list(args) {
                handle_error(err);
                Err(1)
            } else {
                Ok(serde_json::json!({}))
            }
        }
        _ => {
            error!("⚠️ No option provided, try running --help");
            Err(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use axum::{
        Router,
        http::{StatusCode, Uri},
    };
    use serde_json::{Value, json};

    use super::*;

    /// A PactFlow server that advertises `pf:provider-contracts` at `/` and
    /// answers every other request with `status` and `body`. Returns its URL.
    fn spawn_server(status: u16, body: Value) -> String {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            tokio::runtime::Runtime::new().unwrap().block_on(async move {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                tx.send(format!("http://{}", listener.local_addr().unwrap()))
                    .unwrap();
                let router = Router::new().fallback(move |uri: Uri| {
                    let body = body.clone();
                    async move {
                        let (status, response) = if uri.path() == "/" {
                            (
                                200,
                                json!({"_links": {"pf:provider-contracts": {
                                    "href": "/provider-contracts/provider/{provider}/version/{version}",
                                    "templated": true
                                }}}),
                            )
                        } else {
                            (status, body)
                        };
                        (
                            StatusCode::from_u16(status).unwrap(),
                            [("content-type", "application/hal+json")],
                            response.to_string(),
                        )
                    }
                });
                axum::serve(listener, router).await.unwrap();
            });
        });
        rx.recv().unwrap()
    }

    /// Runs `pactflow <subcommand> -b <base> <extra>`.
    fn run_pactflow(subcommand: &str, base: &str, extra: &[&str]) -> Result<Value, i32> {
        let mut argv = vec!["pactflow", subcommand, "-b", base];
        argv.extend_from_slice(extra);
        run(&add_pactflow_client_command().get_matches_from(argv))
    }

    #[test]
    fn delete_exits_zero_when_the_contract_is_deleted() {
        let base = spawn_server(200, json!({"notices": []}));
        let result = run_pactflow(
            "delete-provider-contract",
            &base,
            &["--provider", "Foo", "-a", "1.0"],
        );
        assert!(result.is_ok(), "{result:?}");
    }

    #[test]
    fn delete_exits_one_when_the_delete_fails() {
        let base = spawn_server(
            409,
            json!({"notices": [{"text": "still needed", "type": "danger"}]}),
        );
        let result = run_pactflow(
            "delete-provider-contract",
            &base,
            &["--provider", "Foo", "-a", "1.0"],
        );
        assert_eq!(result, Err(1));
    }

    #[test]
    fn list_exits_zero_when_the_list_is_fetched() {
        let base = spawn_server(200, json!({"_embedded": {"contracts": []}}));
        let result = run_pactflow(
            "list-provider-contracts",
            &base,
            &["--provider", "Foo", "-a", "1.0"],
        );
        assert!(result.is_ok(), "{result:?}");
    }

    #[test]
    fn list_exits_one_when_the_list_fails() {
        let base = spawn_server(404, json!({}));
        let result = run_pactflow(
            "list-provider-contracts",
            &base,
            &["--provider", "Foo", "-a", "1.0"],
        );
        assert_eq!(result, Err(1));
    }
}
