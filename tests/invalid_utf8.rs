//! Clap parses these arguments as `String`, so invalid UTF-8 never reaches URL building.

#[cfg(unix)]
#[test]
fn rejects_non_utf8_argument() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    use std::process::Command;

    let output = Command::new(env!("CARGO_BIN_EXE_pact-broker-cli"))
        .args([
            "create-or-update-version",
            "-b",
            "http://127.0.0.1:9",
            "--pacticipant",
            "c",
            "--version",
            "v",
            "--branch",
        ])
        .arg(OsStr::from_bytes(b"fix/\xFF"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("invalid UTF-8"), "{stderr}");
}
