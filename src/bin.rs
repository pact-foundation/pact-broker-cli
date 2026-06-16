use std::process::ExitCode;

pub fn main() -> ExitCode {
    let app = pact_broker_cli::build_cli();
    let matches = app.clone().try_get_matches();
    match pact_broker_cli::handle_matches(&matches, None) {
        Ok(_) => ExitCode::SUCCESS,
        Err(code) => code,
    }
}
