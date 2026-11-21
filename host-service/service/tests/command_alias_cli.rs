//! 以指令別名 `purrdr` 執行(raychiutw/purrdr.host#53):只打 `purrdr` 印出可用指令，不在前景啟動服務。

use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

const BINARY: &str = env!("CARGO_BIN_EXE_purrdr-host-service");

/// 跑完(或逾時就結束掉)後的輸出;逾時回 None。
fn run(program: &std::path::Path, arguments: &[&str]) -> Option<Output> {
    let mut child = Command::new(program)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Some(child.wait_with_output().unwrap())
}

#[cfg(unix)]
fn alias_in_temp_directory(label: &str) -> std::path::PathBuf {
    let directory = std::env::temp_dir().join(format!("purrdr-alias-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).unwrap();
    let alias = directory.join("purrdr");
    std::os::unix::fs::symlink(BINARY, &alias).unwrap();
    alias
}

fn all_text(output: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr))
}

#[test]
fn help_lists_the_commands_with_the_short_name() {
    let output = run(BINARY.as_ref(), &["help"]).expect("help 不該一直執行");

    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8_lossy(&output.stdout);
    for command in [
        "purrdr pair",
        "purrdr devices",
        "purrdr revoke",
        "purrdr install",
        "purrdr update",
        "purrdr status",
        "purrdr uninstall",
    ] {
        assert!(text.contains(command), "{command}: {text}");
    }
}

#[cfg(unix)]
#[test]
fn purrdr_without_arguments_shows_help_instead_of_starting_the_service() {
    let alias = alias_in_temp_directory("bare");

    let output = run(&alias, &[]).expect("只打 purrdr 不該在前景啟動服務");

    assert!(!output.status.success(), "{output:?}");
    assert!(all_text(&output).contains("purrdr pair"), "{}", all_text(&output));
    let _ = std::fs::remove_dir_all(alias.parent().unwrap());
}

#[cfg(unix)]
#[test]
fn purrdr_runs_the_same_commands_as_the_long_name() {
    let alias = alias_in_temp_directory("version");

    let short = run(&alias, &["--version"]).unwrap();
    let long = run(BINARY.as_ref(), &["--version"]).unwrap();

    assert!(short.status.success());
    assert_eq!(short.stdout, long.stdout);
    let _ = std::fs::remove_dir_all(alias.parent().unwrap());
}
