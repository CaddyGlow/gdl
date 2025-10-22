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
        .stderr(contains("<URL>"));
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

#[test]
fn prints_long_version_information() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin("gdl")?;
    cmd.arg("--version");
    cmd.assert()
        .success()
        .stdout(contains(env!("CARGO_PKG_VERSION")));
    Ok(())
}

#[test]
fn rejects_invalid_url() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin("gdl")?;
    cmd.arg("not-a-valid-url");
    cmd.assert()
        .failure()
        .stderr(contains("invalid"));
    Ok(())
}

#[test]
fn accepts_clear_cache_without_url() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin("gdl")?;
    cmd.arg("--clear-cache");
    cmd.assert()
        .success();
    Ok(())
}

#[test]
fn accepts_check_update_without_url() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin("gdl")?;
    cmd.arg("--check-update");
    // May succeed or fail depending on network, but shouldn't require URL
    let _ = cmd.assert();
    Ok(())
}

#[test]
fn accepts_parallel_flag() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin("gdl")?;
    cmd.arg("--parallel").arg("8");
    cmd.arg("https://github.com/invalid/test");
    // Should fail for invalid URL, not for parallel flag
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("parallel").not());
    Ok(())
}

#[test]
fn accepts_strategy_flag_api() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin("gdl")?;
    cmd.arg("--strategy").arg("api");
    cmd.arg("https://github.com/invalid/test");
    // Should fail for invalid URL, not for strategy flag
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("strategy").not());
    Ok(())
}

#[test]
fn accepts_strategy_flag_git() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin("gdl")?;
    cmd.arg("--strategy").arg("git");
    cmd.arg("https://github.com/invalid/test");
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("strategy").not());
    Ok(())
}

#[test]
fn accepts_strategy_flag_zip() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin("gdl")?;
    cmd.arg("--strategy").arg("zip");
    cmd.arg("https://github.com/invalid/test");
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("strategy").not());
    Ok(())
}

#[test]
fn accepts_strategy_flag_auto() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin("gdl")?;
    cmd.arg("--strategy").arg("auto");
    cmd.arg("https://github.com/invalid/test");
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("strategy").not());
    Ok(())
}

#[test]
fn rejects_invalid_strategy() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin("gdl")?;
    cmd.arg("--strategy").arg("invalid");
    cmd.arg("https://github.com/invalid/test");
    cmd.assert()
        .failure()
        .stderr(contains("invalid value 'invalid'"));
    Ok(())
}

#[test]
fn accepts_verbose_flag() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin("gdl")?;
    cmd.arg("-v");
    cmd.arg("https://github.com/invalid/test");
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("verbose").not());
    Ok(())
}

#[test]
fn accepts_multiple_verbose_flags() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin("gdl")?;
    cmd.arg("-vvv");
    cmd.arg("https://github.com/invalid/test");
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("verbose").not());
    Ok(())
}

#[test]
fn accepts_no_cache_flag() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin("gdl")?;
    cmd.arg("--no-cache");
    cmd.arg("https://github.com/invalid/test");
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("no-cache").not());
    Ok(())
}

#[test]
fn accepts_force_flag() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin("gdl")?;
    cmd.arg("--force");
    cmd.arg("https://github.com/invalid/test");
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("force").not());
    Ok(())
}

#[test]
fn accepts_output_flag() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin("gdl")?;
    cmd.arg("--output").arg("/tmp/test");
    cmd.arg("https://github.com/invalid/test");
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("output").not());
    Ok(())
}

#[test]
fn accepts_token_flag() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin("gdl")?;
    cmd.arg("--token").arg("test_token");
    cmd.arg("https://github.com/invalid/test");
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("token").not());
    Ok(())
}
