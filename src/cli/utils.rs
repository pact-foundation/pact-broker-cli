use console::Style;
use log::{LevelFilter, SetLoggerError};
use simplelog::{ColorChoice, Config, TermLogger, TerminalMode};
use std::str::FromStr;

pub fn setup_loggers(level: &str) -> Result<(), SetLoggerError> {
    let log_level = match level {
        "none" => LevelFilter::Off,
        _ => LevelFilter::from_str(level).unwrap(),
    };
    TermLogger::init(
        log_level,
        Config::default(),
        TerminalMode::Stderr,
        ColorChoice::Auto,
    )
}

pub fn glob_value(v: String) -> Result<String, String> {
    match glob::Pattern::new(&v) {
        Ok(res) => Ok(res.to_string()),
        Err(err) => Err(format!("'{}' is not a valid glob pattern - {}", v, err)),
    }
}

pub const RED: Style = Style::new().red();
pub const GREEN: Style = Style::new().green();
pub const YELLOW: Style = Style::new().yellow();
pub const CYAN: Style = Style::new().cyan();
