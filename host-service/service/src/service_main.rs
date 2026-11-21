//! 以命令列參數執行主機服務(`purrdr-host-service` 與 Windows 的 `purrdr-host-service-background` 共用)。

use std::process::ExitCode;

use crate::herdr_socket::{Platform, SocketEnvironment};
use crate::{HostServiceConfig, WEBSOCKET_PATH};

/// `[--port <埠>] [--session <herdr session>] [--state-dir <資料夾>]`:啟動並一直執行。
pub async fn run_service(arguments: &[String]) -> ExitCode {
    let config = match HostServiceConfig::from_arguments(arguments, Platform::current(), &SocketEnvironment::from_process()) {
        Ok(config) => config,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };
    let herdr_socket = config.herdr_socket.clone();
    let state_directory = config.state_directory.clone();
    let service = match crate::start(config).await {
        Ok(service) => service,
        Err(error) => {
            crate::service_log::ServiceLog::under(&state_directory).record(&format!("cannot listen: {error}"));
            return ExitCode::FAILURE;
        }
    };
    println!(
        "purrdr-host-service {} listening on http://{} (WebSocket {WEBSOCKET_PATH}, herdr socket {})",
        env!("CARGO_PKG_VERSION"),
        service.local_addr(),
        herdr_socket.native_name()
    );
    if let Err(error) = crate::running_service::write(&state_directory, service.local_addr().port()) {
        crate::service_log::ServiceLog::under(&state_directory).record(&format!("cannot write running.json: {error}"));
    }
    // 安裝好的那份才會自動更新(開發中直接執行的不會)。
    let _auto_update = crate::auto_update::spawn_for_installed_service(herdr_socket);
    service.wait().await;
    ExitCode::SUCCESS
}
