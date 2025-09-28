use std::process::ExitCode;

mod cli;
pub fn main() -> Result<(), ExitCode> {
    let app = cli::build_cli();
    let matches = app.clone().try_get_matches();
    pact_broker_cli::handle_matches(&matches, None)
}
