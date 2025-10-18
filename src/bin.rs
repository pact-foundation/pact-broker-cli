use std::process::ExitCode;

mod cli;
pub fn main() -> ExitCode {
    let app = cli::build_cli();
    let matches = app.clone().try_get_matches();
    match pact_broker_cli::handle_matches(&matches, None) {
        Ok(_) => ExitCode::SUCCESS,
        Err(code) => code,
    }
}
