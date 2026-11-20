//! 更新流程的安裝位置：家目錄裡安裝好的主機服務(raychiutw/purrdr.host#6)。

use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime};

use super::system::{HostSystem, spawn_update_helper};
use super::{HostEnvironment, InstallError, InstallPlatform, InstallRecord, ServiceLayout, autostart, read_install_record, service_arguments};
use crate::running_service;
use crate::update::{InstallLocation, UpdateError, UpdateEvent, UpdateLauncher, UpdateLock, UpdateRecord, Version};
use crate::update_push::{HttpPushTransport, PushTransport, UpdateEventPush};

/// 替換時留下的舊執行檔副檔名。
const PREVIOUS_SUFFIX: &str = ".previous";
/// 只接受 Purrdr 的 Developer ID Application 憑證(Apple 的 OID 與 Team ID)。
const PURRDR_TEAM_ID: &str = "8Z6WVFJ574";
const SIGNATURE_REQUIREMENT: &str = concat!(
    "anchor apple generic and certificate leaf[field.1.2.840.113635.100.6.1.13] exists",
    " and certificate leaf[subject.OU] = \"8Z6WVFJ574\""
);
/// Windows 退回時，換下來的新版先改成這個名字(執行中的檔案不能被覆蓋)。
const FAILED_SUFFIX: &str = ".failed";
/// 暫存資料夾的序號(同一個程序裡也不重複)。
static NEXT_STAGING: AtomicU64 = AtomicU64::new(0);

/// 已安裝的主機服務：依安裝紀錄替換執行檔、重啟背景服務(沿用埠與 session,不動設定與已配對裝置)。
pub struct InstalledService<S: HostSystem> {
    environment: HostEnvironment,
    system: S,
    start_timeout: Duration,
    push_transport: Box<dyn PushTransport>,
}

impl<S: HostSystem> InstalledService<S> {
    pub fn new(environment: HostEnvironment, system: S) -> Self {
        InstalledService {
            environment,
            system,
            start_timeout: Duration::from_secs(30),
            push_transport: Box::new(HttpPushTransport::default()),
        }
    }

    /// 換掉送更新事件推播的 HTTP client。
    pub fn with_push_transport(self, push_transport: Box<dyn PushTransport>) -> Self {
        InstalledService { push_transport, ..self }
    }

    /// 重啟後等新版開始聽埠的時限。
    pub fn with_start_timeout(self, start_timeout: Duration) -> Self {
        InstalledService { start_timeout, ..self }
    }

    /// 測試檢查假系統狀態。
    pub fn system(&self) -> &S {
        &self.system
    }

    /// 正在執行的是安裝好的那份(只有它做背景自動更新)。
    pub fn runs_installed_copy(&self) -> bool {
        let bin = self.layout().bin_directory();
        let running_from = self.environment.current_executable.parent().and_then(|parent| parent.canonicalize().ok());
        self.record().is_ok() && running_from.is_some() && running_from == bin.canonicalize().ok()
    }

    /// 執行安裝好的 `update --yes` 的啟動器。
    pub fn update_launcher(&self) -> HelperLauncher {
        HelperLauncher::new(self.layout().executable(), "systemd-run".to_owned())
    }

    fn update_status_file(&self) -> PathBuf {
        self.layout().directory().join("update.json")
    }

    fn update_status(&self) -> serde_json::Map<String, serde_json::Value> {
        std::fs::read_to_string(self.update_status_file())
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    fn write_update_status(&self, status: &serde_json::Map<String, serde_json::Value>) {
        let _ = std::fs::write(self.update_status_file(), serde_json::to_string_pretty(status).expect("可序列化"));
    }

    fn layout(&self) -> ServiceLayout {
        ServiceLayout::of(&self.environment)
    }

    fn record(&self) -> Result<InstallRecord, UpdateError> {
        read_install_record(&self.layout()).map_err(install_failure)?.ok_or(UpdateError::NotInstalled)
    }

    fn executable_version(&mut self, executable: &Path) -> Result<Version, String> {
        let output = self
            .system
            .run(&executable.display().to_string(), &["--version".to_owned()])
            .map_err(|error| format!("cannot run {}: {error}", executable.display()))?;
        if !output.success {
            return Err(format!("{} --version failed: {}", executable.display(), output.stderr.trim()));
        }
        Version::from_version_output(&output.stdout).ok_or_else(|| format!("unexpected version output: {}", output.stdout.trim()))
    }

    fn with_suffix(executable: &Path, suffix: &str) -> PathBuf {
        let mut name = executable.as_os_str().to_owned();
        name.push(suffix);
        PathBuf::from(name)
    }

    fn previous(executable: &Path) -> PathBuf {
        Self::with_suffix(executable, PREVIOUS_SUFFIX)
    }

    /// 執行中的服務在 `since` 之後啟動、回報 `expected` 且在聽埠。
    fn runs_version_since(&self, expected: &Version, since: SystemTime, address: &SocketAddr) -> bool {
        let Some(running) = running_service::read(&self.layout().directory()) else {
            return false;
        };
        running.started_at_ms >= running_service::unix_millis(since)
            && running.version.parse::<Version>().ok().as_ref() == Some(expected)
            && is_listening(address)
    }
}

impl<S: HostSystem> InstallLocation for InstalledService<S> {
    fn platform(&self) -> InstallPlatform {
        self.environment.platform
    }

    fn installed_version(&mut self) -> Result<Version, UpdateError> {
        self.record()?;
        let executable = self.layout().executable();
        self.executable_version(&executable).map_err(UpdateError::Io)
    }

    fn staging_directory(&self) -> PathBuf {
        let sequence = NEXT_STAGING.fetch_add(1, Ordering::SeqCst);
        let nanos = SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_nanos());
        self.layout().directory().join(format!("update-{}-{nanos}-{sequence}", std::process::id()))
    }

    /// macOS:Purrdr(`PURRDR_TEAM_ID`)的 Developer ID 簽章;不看已安裝的那份。
    fn verify(&mut self, staged: &Path) -> Result<(), UpdateError> {
        if self.environment.platform != InstallPlatform::MacOs {
            return Ok(());
        }
        debug_assert!(SIGNATURE_REQUIREMENT.contains(PURRDR_TEAM_ID));
        for name in self.layout().executable_names() {
            let file = staged.join(name).display().to_string();
            let arguments = ["--verify".to_owned(), "--strict".to_owned(), format!("-R={SIGNATURE_REQUIREMENT}"), file];
            let output = self
                .system
                .run("codesign", &arguments)
                .map_err(|error| UpdateError::Signature(format!("codesign: {error}")))?;
            if !output.success {
                return Err(UpdateError::Signature(output.stderr.trim().to_owned()));
            }
        }
        Ok(())
    }

    /// Unix:舊檔先 hard link 成 `.previous`,新檔再以 rename 原子覆蓋。
    /// Windows:執行中的檔案不能覆蓋但可以改名，所以先改名成 `.previous` 再放新檔。失敗時換回舊檔。
    fn replace(&mut self, staged: &Path) -> Result<(), UpdateError> {
        let layout = self.layout();
        let names = layout.executable_names();
        for name in &names {
            if !staged.join(name).is_file() {
                return Err(UpdateError::Io(format!("the downloaded archive has no {name}")));
            }
        }
        for name in &names {
            let installed = layout.bin_directory().join(name);
            let previous = Self::previous(&installed);
            let _ = std::fs::remove_file(&previous);
            #[cfg(unix)]
            let moved = std::fs::hard_link(&installed, &previous).and_then(|()| std::fs::rename(staged.join(name), &installed));
            #[cfg(not(unix))]
            let moved = std::fs::rename(&installed, &previous).and_then(|()| {
                std::fs::rename(staged.join(name), &installed).inspect_err(|_| {
                    let _ = std::fs::rename(&previous, &installed);
                })
            });
            if let Err(error) = moved {
                let _ = self.restore();
                return Err(UpdateError::Io(format!("cannot replace {}: {error}", installed.display())));
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&installed, std::fs::Permissions::from_mode(0o755));
            }
        }
        Ok(())
    }

    /// 只換回檔案，不停止也不取消登記服務(由呼叫端重啟)。
    fn restore(&mut self) -> Result<(), UpdateError> {
        let layout = self.layout();
        for name in layout.executable_names() {
            let installed = layout.bin_directory().join(name);
            let previous = Self::previous(&installed);
            if !previous.is_file() {
                continue;
            }
            if cfg!(windows) {
                // 新版可能還在跑：先改名讓出位置。
                let failed = Self::with_suffix(&installed, FAILED_SUFFIX);
                let _ = std::fs::remove_file(&failed);
                let _ = std::fs::rename(&installed, &failed);
            }
            std::fs::rename(&previous, &installed).map_err(|error| UpdateError::Io(format!("cannot restore {}: {error}", installed.display())))?;
        }
        Ok(())
    }

    /// Windows 上更新程序自己就是改名後的舊檔，刪不掉時留到下次更新再刪。
    fn discard_previous(&mut self) {
        let layout = self.layout();
        for name in layout.executable_names() {
            let installed = layout.bin_directory().join(name);
            let _ = std::fs::remove_file(Self::previous(&installed));
            let _ = std::fs::remove_file(Self::with_suffix(&installed, FAILED_SUFFIX));
        }
    }

    fn notify(&mut self, event: &UpdateEvent) {
        UpdateEventPush::new(&self.environment.home, self.push_transport.as_mut()).send(event);
    }

    fn auto_update(&mut self) -> Result<bool, UpdateError> {
        Ok(self.record()?.auto_update)
    }

    /// `update.json` 只留最後一次，`logs/update.log` 逐行累積;寫不進去不影響更新。
    fn record_result(&mut self, record: &UpdateRecord) {
        let checked_at = humantime::format_rfc3339_seconds(record.checked_at).to_string();
        let mut status = self.update_status();
        status.insert("checked_at".into(), checked_at.clone().into());
        status.insert("result".into(), record.result.clone().into());
        self.write_update_status(&status);
        let log = self.layout().log_file().with_file_name("update.log");
        if let Some(directory) = log.parent() {
            let _ = std::fs::create_dir_all(directory);
        }
        if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(log) {
            use std::io::Write;
            let _ = writeln!(file, "{checked_at} {}", record.result);
        }
    }

    fn last_result(&mut self) -> Option<UpdateRecord> {
        let status: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(self.update_status_file()).ok()?).ok()?;
        Some(UpdateRecord {
            checked_at: humantime::parse_rfc3339(status["checked_at"].as_str()?).ok()?,
            result: status["result"].as_str()?.to_owned(),
        })
    }

    /// 狀態資料夾的 `update.lock`(檔案鎖，程序結束時系統會放開)。
    fn lock(&mut self) -> Result<UpdateLock, UpdateError> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(self.layout().directory().join("update.lock"))
            .map_err(|error| UpdateError::Io(format!("cannot open update.lock: {error}")))?;
        match file.try_lock() {
            Ok(()) => Ok(UpdateLock::holding(file)),
            Err(std::fs::TryLockError::WouldBlock) => Err(UpdateError::InProgress),
            Err(std::fs::TryLockError::Error(error)) => Err(UpdateError::Io(format!("cannot lock update.lock: {error}"))),
        }
    }

    fn rolled_back_version(&mut self) -> Option<Version> {
        self.update_status().get("rolled_back")?.as_str()?.parse().ok()
    }

    fn set_rolled_back_version(&mut self, version: Option<Version>) {
        let mut status = self.update_status();
        match version {
            Some(version) => status.insert("rolled_back".into(), version.to_string().into()),
            None => status.remove("rolled_back"),
        };
        self.write_update_status(&status);
    }

    fn deferred_updates(&mut self) -> u32 {
        self.update_status()
            .get("deferred_count")
            .and_then(serde_json::Value::as_u64)
            .map_or(0, |count| count.try_into().unwrap_or(u32::MAX))
    }

    /// 0 時移除欄位。
    fn set_deferred_updates(&mut self, count: u32) {
        let mut status = self.update_status();
        match count {
            0 => status.remove("deferred_count"),
            count => status.insert("deferred_count".into(), count.into()),
        };
        self.write_update_status(&status);
    }

    /// 就地重啟(不取消登記)。執行中的服務寫的 `running.json` 回報 `expected`、且在這次重啟之後啟動，才算成功。
    fn restart(&mut self, expected: &Version) -> Result<(), UpdateError> {
        let record = self.record()?;
        let address = SocketAddr::from(([127, 0, 0, 1], record.port));
        let since = SystemTime::now();
        let layout = self.layout();
        autostart::restart(&mut self.system, &layout, &service_arguments(&record), self.environment.user_name.as_deref()).map_err(install_failure)?;
        if !wait_until(Instant::now() + self.start_timeout, || self.runs_version_since(expected, since, &address)) {
            return Err(UpdateError::Io(format!(
                "the host service on {address} did not report {expected} within {} seconds",
                self.start_timeout.as_secs()
            )));
        }
        Ok(())
    }
}

/// 以獨立程序執行安裝好的 `update --yes`,服務重啟時不會跟著被結束。
pub struct HelperLauncher {
    executable: PathBuf,
    systemd_run: String,
}

impl HelperLauncher {
    /// `systemd_run` 是 Linux 用來把更新程序放進另一個 unit 的程式(測試換成假的)。
    pub fn new(executable: PathBuf, systemd_run: String) -> Self {
        HelperLauncher { executable, systemd_run }
    }
}

impl UpdateLauncher for HelperLauncher {
    fn launch(&mut self) -> Result<(), String> {
        spawn_update_helper(&self.executable, &self.systemd_run).map_err(|error| format!("{}: {error}", self.executable.display()))
    }
}

/// 安裝流程的錯誤只取「發生什麼」;下一步由更新流程的訊息給。
fn install_failure(error: InstallError) -> UpdateError {
    match error {
        InstallError::Autostart(message) | InstallError::Io(message) | InstallError::Tailscale(message) => UpdateError::Io(message),
        other => UpdateError::Io(other.to_string().lines().next().unwrap_or_default().to_owned()),
    }
}

fn is_listening(address: &SocketAddr) -> bool {
    TcpStream::connect_timeout(address, Duration::from_millis(500)).is_ok()
}

/// 每 200ms 檢查一次，直到成立或超過 `deadline`。
fn wait_until(deadline: Instant, mut condition: impl FnMut() -> bool) -> bool {
    loop {
        if condition() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}
