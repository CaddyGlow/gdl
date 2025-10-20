use assert_cmd::prelude::*;
use predicates::{prelude::*, str::contains};
use std::process::Command;

#[test]
fn displays_help() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin("gdl")?;
    cmd.arg("--help");
    cmd.assert().success();
    Ok(())
}

#[test]
fn requires_url_argument() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin("gdl")?;
    cmd.assert()
        .failure()
        .code(predicate::eq(2))
        .stderr(contains("--url"));
    Ok(())
}

#[test]
fn prints_version_information() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin("gdl")?;
    cmd.arg("-V");
    cmd.assert()
        .success()
        .stdout(contains(env!("CARGO_PKG_VERSION")));
    Ok(())
}
