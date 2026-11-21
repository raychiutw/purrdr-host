//! 以指令別名 `purrdr` 執行(raychiutw/purrdr.host#53):只打 `purrdr` 印出可用指令，不在前景啟動服務。

mod install_support;

use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

const BINARY: &str = env!("CARGO_BIN_EXE_purrdr-host-service");

/// 跑完(或逾時就結束掉)後的輸出;逾時回 None。家目錄指到暫存資料夾：萬一誤啟動服務也不碰真的家目錄。
fn run(program: &std::path::Path, arguments: &[&str]) -> Option<Output> {
    let home = install_support::TempDirectory::new("alias-home");
    let mut child = Command::new(program)
        .args(arguments)
        .env("HOME", &home.0)
        .env("USERPROFILE", &home.0)
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

/// 暫存資料夾裡指向測試執行檔的 `purrdr`(資料夾在回傳值 drop 時刪除)。
#[cfg(unix)]
fn alias_in_temp_directory(label: &str) -> (install_support::TempDirectory, std::path::PathBuf) {
    let directory = install_support::TempDirectory::new(label);
    let alias = directory.0.join("purrdr");
    std::os::unix::fs::symlink(BINARY, &alias).unwrap();
    (directory, alias)
}

#[cfg(unix)]
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
    let (_directory, alias) = alias_in_temp_directory("alias-bare");

    let output = run(&alias, &[]).expect("只打 purrdr 不該在前景啟動服務");

    assert!(!output.status.success(), "{output:?}");
    assert!(all_text(&output).contains("purrdr pair"), "{}", all_text(&output));
}

#[cfg(unix)]
#[test]
fn purrdr_runs_the_same_commands_as_the_long_name() {
    let (_directory, alias) = alias_in_temp_directory("alias-version");

    let short = run(&alias, &["--version"]).unwrap();
    let long = run(BINARY.as_ref(), &["--version"]).unwrap();

    assert!(short.status.success());
    assert_eq!(short.stdout, long.stdout);
}
