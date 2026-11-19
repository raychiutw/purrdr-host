//! `update`、`update --check`、`version` 指令(raychiutw/purrdr.host#6):暫存家目錄加本機假 Release 伺服器，不改系統。

mod install_support;

use std::process::{Command, Output, Stdio};

use install_support::release::FakeReleaseServer;
use install_support::{TempDirectory, snapshot};
use purrdr_host_service::install::InstallPlatform;

const BUILT: &str = env!("CARGO_PKG_VERSION");
const NEWER: &str = "99.0.0";

/// 這次建置的主機服務;交叉編譯到別台執行時以 `PURRDR_HOST_SERVICE_EXE` 指定。
fn host_service_executable() -> std::path::PathBuf {
    std::env::var_os("PURRDR_HOST_SERVICE_EXE").map_or_else(|| env!("CARGO_BIN_EXE_purrdr-host-service").into(), Into::into)
}

/// 以暫存家目錄、指向假 Release 伺服器執行主機服務的執行檔(stdin 是空的)。
fn run(home: &TempDirectory, release_url: &str, arguments: &[&str]) -> Output {
    Command::new(host_service_executable())
        .args(arguments)
        .env("HOME", &home.0)
        .env("USERPROFILE", &home.0)
        .env_remove("XDG_CONFIG_HOME")
        .env("PURRDR_HOST_SERVICE_RELEASE_URL", release_url)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).replace('\r', "")
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).replace('\r', "")
}

/// 家目錄裡裝好這次建置的主機服務(只放執行檔與安裝紀錄，沒有登記自動啟動)。
fn installed_home() -> TempDirectory {
    let home = TempDirectory::new("update-cli");
    let bin = home.0.join(".purrdr/host-service/bin");
    std::fs::create_dir_all(&bin).unwrap();
    let name = if cfg!(windows) { "purrdr-host-service.exe" } else { "purrdr-host-service" };
    std::fs::copy(host_service_executable(), bin.join(name)).unwrap();
    let record = r#"{"port":47999,"herdr_session":null,"https_port":443,"https_address":"https://studio.tail1234.ts.net","serve_configured":true,"created_directories":[]}"#;
    std::fs::write(home.0.join(".purrdr/host-service/install.json"), record).unwrap();
    home
}

fn release(version: &str) -> FakeReleaseServer {
    let server = FakeReleaseServer::start();
    server.publish(InstallPlatform::current(), version, &[("purrdr-host-service", "unused".to_owned())]);
    server
}

fn unreachable_url() -> String {
    let server = FakeReleaseServer::start();
    let url = server.url();
    drop(server);
    url
}

#[test]
fn version_shows_installed_and_newer_latest_version() {
    let home = installed_home();
    let server = release(NEWER);

    let output = run(&home, &server.url(), &["version"]);

    assert!(output.status.success(), "{output:?}");
    let text = stdout(&output);
    assert!(text.contains(&format!("installed: {BUILT}\n")), "{text}");
    assert!(text.contains(&format!("latest: {NEWER} (update available, run: ")), "{text}");
    assert!(text.contains(" update)"), "{text}");
}

#[test]
fn version_says_up_to_date_and_not_installed() {
    let server = release(BUILT);
    let installed = installed_home();
    let empty = TempDirectory::new("update-cli-empty");

    let up_to_date = stdout(&run(&installed, &server.url(), &["version"]));
    let not_installed = run(&empty, &server.url(), &["version"]);

    assert!(up_to_date.contains(&format!("latest: {BUILT} (up to date)")), "{up_to_date}");
    assert!(not_installed.status.success(), "{not_installed:?}");
    assert!(stdout(&not_installed).contains("installed: not installed\n"), "{not_installed:?}");
    assert_eq!(std::fs::read_dir(&empty.0).unwrap().count(), 0);
}

#[test]
fn version_without_network_still_shows_installed_version() {
    let home = installed_home();

    let output = run(&home, &unreachable_url(), &["version"]);

    assert!(output.status.success(), "{output:?}");
    let text = stdout(&output);
    assert!(text.contains(&format!("installed: {BUILT}\n")), "{text}");
    assert!(text.contains("latest: unknown ("), "{text}");
}

#[test]
fn update_check_reports_newer_version_without_changing_anything() {
    let home = installed_home();
    let server = release(NEWER);
    let before = snapshot(&home.0);

    let output = run(&home, &server.url(), &["update", "--check"]);

    assert!(output.status.success(), "{output:?}");
    let text = stdout(&output);
    assert!(text.contains(&format!("Update available: {BUILT} -> {NEWER}\n")), "{text}");
    assert!(text.contains("To update, run: "), "{text}");
    assert_eq!(snapshot(&home.0), before);
    assert_eq!(server.requests(), ["/releases/latest"]);
}

#[test]
fn update_when_up_to_date_changes_nothing() {
    let home = installed_home();
    let server = release(BUILT);
    let before = snapshot(&home.0);

    for arguments in [&["update", "--check"][..], &["update", "--yes"]] {
        let output = run(&home, &server.url(), arguments);

        assert!(output.status.success(), "{output:?}");
        assert!(stdout(&output).contains(&format!("Up to date: {BUILT}")), "{output:?}");
    }
    let mut after = snapshot(&home.0);
    let recorded = [
        ".purrdr/host-service/update.json",
        ".purrdr/host-service/logs/",
        ".purrdr/host-service/logs/update.log",
    ];
    after.retain(|path, _| !recorded.contains(&path.as_str()));
    assert_eq!(after, before, "只多了更新結果的紀錄");
}

#[test]
fn update_asks_first_and_cancels_without_an_answer() {
    let home = installed_home();
    let server = release(NEWER);
    let before = snapshot(&home.0);

    let output = run(&home, &server.url(), &["update"]);

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        stdout(&output).contains(&format!("Update the host service from {BUILT} to {NEWER}?")),
        "{output:?}"
    );
    assert!(stderr(&output).contains("Update cancelled; nothing was changed."), "{output:?}");
    assert_eq!(snapshot(&home.0), before);
    assert_eq!(server.requests(), ["/releases/latest"], "沒同意就不下載");
}

#[test]
fn update_errors_end_with_next_step() {
    let empty = TempDirectory::new("update-cli-empty");
    let server = release(NEWER);
    let installed = installed_home();

    let not_installed = run(&empty, &server.url(), &["update", "--yes"]);
    let offline = run(&installed, &unreachable_url(), &["update", "--check"]);

    for output in [&not_installed, &offline] {
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(stderr(output).contains("\nNext: "), "{output:?}");
    }
    assert!(stderr(&not_installed).contains("not installed"), "{not_installed:?}");
    assert!(stderr(&offline).contains("update --check"), "重跑指令要帶原本的選項：{offline:?}");
    assert_eq!(std::fs::read_dir(&empty.0).unwrap().count(), 0);
}

#[test]
fn version_shows_auto_update_and_the_last_check() {
    let home = installed_home();
    let server = release(BUILT);

    let before_any_check = stdout(&run(&home, &server.url(), &["version"]));
    run(&home, &server.url(), &["update", "--yes"]);
    let after_update_command = stdout(&run(&home, &server.url(), &["version"]));

    assert!(before_any_check.contains("auto update: on\n"), "{before_any_check}");
    assert!(before_any_check.contains("last check: never\n"), "{before_any_check}");
    assert!(after_update_command.contains(&format!(", up to date ({BUILT})\n")), "{after_update_command}");
    assert!(after_update_command.contains("last check: 20"), "時間是 RFC 3339:{after_update_command}");
}

#[test]
fn auto_update_off_and_on_change_the_setting() {
    let home = installed_home();
    let server = release(BUILT);

    let off = run(&home, &server.url(), &["auto-update", "off"]);
    let version_when_off = stdout(&run(&home, &server.url(), &["version"]));
    let on = run(&home, &server.url(), &["auto-update", "on"]);

    assert!(off.status.success(), "{off:?}");
    assert!(stdout(&off).contains("Auto update is off."), "{off:?}");
    assert!(version_when_off.contains("auto update: off\n"), "{version_when_off}");
    assert!(stdout(&on).contains("Auto update is on."), "{on:?}");
    let record = std::fs::read_to_string(home.0.join(".purrdr/host-service/install.json")).unwrap();
    assert!(record.contains("\"auto_update\": true"), "{record}");
}

#[test]
fn auto_update_without_install_says_how_to_install() {
    let empty = TempDirectory::new("update-cli-empty");

    let output = run(&empty, &unreachable_url(), &["auto-update", "off"]);

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(stderr(&output).contains("not installed"), "{output:?}");
    assert!(stderr(&output).contains("\nNext: "), "{output:?}");
    assert_eq!(std::fs::read_dir(&empty.0).unwrap().count(), 0);
}

#[test]
fn running_service_tells_the_updater_its_version() {
    let state = TempDirectory::new("running-marker");
    let mut service = Command::new(host_service_executable())
        .args(["--port", "0", "--state-dir"])
        .arg(&state.0)
        .env("HERDR_SOCKET_PATH", "purrdr-running-marker-missing-socket")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let marker_file = state.0.join("running.json");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while !marker_file.is_file() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let marker = std::fs::read_to_string(&marker_file);
    let _ = service.kill();
    let _ = service.wait();

    let marker: serde_json::Value = serde_json::from_str(&marker.expect("服務啟動後要寫 running.json")).unwrap();
    assert_eq!(marker["version"], BUILT);
    assert_eq!(marker["pid"], service.id());
    let port = marker["port"].as_u64().unwrap();
    assert!(port > 0, "{marker}");
    assert!(marker["started_at_ms"].as_u64().unwrap() > 0, "{marker}");
}
