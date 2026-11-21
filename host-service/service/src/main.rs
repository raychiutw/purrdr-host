//! `purrdr-host-service [--port <埠>] [--session <herdr session>] [--state-dir <資料夾>]`:執行主機服務。
//! `purrdr-host-service pair | devices | revoke …`:管理連線配對與裝置(見 `management`)。
//! `purrdr-host-service --version`:印出版本(一行安裝腳本與 app 的更新提示用，#198)。
//! `purrdr-host-service install [--port <埠>] [--session <herdr session>] [--yes]`、`uninstall`、`status`:安裝(見 `install`)。
//! `purrdr-host-service update [--check] [--yes]`、`version`:更新到最新 Release、顯示版本(見 `update`)。

use std::process::ExitCode;
use std::time::SystemTime;

use purrdr_host_service::herdr_socket::{Platform, SocketEnvironment};
use purrdr_host_service::install::{self, InstallCommand};
use purrdr_host_service::management;

#[tokio::main]
async fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if matches!(arguments.as_slice(), [flag] if flag == "--version" || flag == "-V") {
        println!("purrdr-host-service {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    if let Some(command) = InstallCommand::parse(&arguments) {
        return match command {
            Ok(command) => install::run_command(command).await,
            Err(message) => {
                eprintln!("{message}");
                ExitCode::from(2)
            }
        };
    }
    match arguments.first().map(String::as_str) {
        Some("pair" | "devices" | "revoke") => run_management(&arguments),
        _ => purrdr_host_service::run_service(&arguments).await,
    }
}

fn run_management(arguments: &[String]) -> ExitCode {
    match management::run(arguments, Platform::current(), &SocketEnvironment::from_process(), SystemTime::now()) {
        Ok(output) => {
            print!("{output}");
            wait_for_enter_if_asked(arguments);
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("{message}");
            wait_for_enter_if_asked(arguments);
            ExitCode::from(2)
        }
    }
}

/// `pair --wait`:herdr 的 popup 在指令結束時就關掉，成功或失敗都等使用者按 Enter,讓 QR code 或錯誤留在畫面上。
fn wait_for_enter_if_asked(arguments: &[String]) {
    use std::io::Write as _;
    if management::waits_for_enter(arguments) {
        let _ = std::io::stdout().flush();
        let _ = std::io::stdin().read_line(&mut String::new());
    }
}
