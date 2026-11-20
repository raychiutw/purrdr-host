//! 主機服務的更新流程(raychiutw/purrdr.host#6):「檢查」與「套用」。
//!
//! 往外只接兩個介面：Release 來源(`ReleaseSource`)與安裝位置(`InstallLocation`);
//! 背景自動更新另外問 `AgentActivity`,有 Agent 工作中就延後(raychiutw/purrdr.host#20)。

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use sha2::{Digest, Sha256};

use crate::install::InstallPlatform;

/// Release tag 的前綴(`host-service-v<版本>`)。
const TAG_PREFIX: &str = "host-service-v";
/// 正式的 Release 來源。
pub const GITHUB_RELEASES: &str = "https://github.com/raychiutw/purrdr.host";
const GITHUB_LATEST_RELEASE: &str = "https://api.github.com/repos/raychiutw/purrdr.host/releases/latest";
/// 換成 `<base>/releases/latest` 格式來源的環境變數(測試與實機驗證用)。
pub const RELEASE_URL_VARIABLE: &str = "PURRDR_HOST_SERVICE_RELEASE_URL";
/// 壓縮檔大小上限(防止下載無止境)。
const MAXIMUM_DOWNLOAD: u64 = 256 * 1024 * 1024;
/// 背景自動更新因 Agent 工作中最多連續延後幾次，之後照常更新。
pub const MAXIMUM_DEFERRALS: u32 = 3;

/// 語意化版本(只接受 `主.次.修`)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    major: u64,
    minor: u64,
    patch: u64,
}

impl Version {
    /// 由 Release tag(`host-service-v<版本>`)取版本。
    pub fn from_tag(tag: &str) -> Option<Version> {
        tag.strip_prefix(TAG_PREFIX)?.parse().ok()
    }

    /// 這個版本的 Release tag。
    pub fn tag(&self) -> String {
        format!("{TAG_PREFIX}{self}")
    }

    /// 由 `purrdr-host-service --version` 的輸出取版本。
    pub fn from_version_output(output: &str) -> Option<Version> {
        output.trim().strip_prefix("purrdr-host-service ")?.parse().ok()
    }
}

impl std::str::FromStr for Version {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let invalid = || format!("invalid version: {text}");
        let numbers: Vec<u64> = text
            .split('.')
            .map(|part| {
                if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
                    return Err(invalid());
                }
                part.parse().map_err(|_| invalid())
            })
            .collect::<Result<_, _>>()?;
        match numbers.as_slice() {
            [major, minor, patch] => Ok(Version {
                major: *major,
                minor: *minor,
                patch: *patch,
            }),
            _ => Err(invalid()),
        }
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// 更新失敗。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateError {
    NotInstalled,
    /// 連不到 Release 來源，或檔案不存在。
    Network(String),
    /// 下載的壓縮檔與 `SHA256SUMS` 不符(或沒有它的那一行)。
    ChecksumMismatch {
        file: String,
    },
    /// macOS 的程式碼簽章不符。
    Signature(String),
    /// 新版起不來，已換回原版本。
    RolledBack {
        failed: Version,
        restored: Version,
        reason: String,
    },
    /// 已有另一個更新在進行。
    InProgress,
    /// 新版起不來，換回原版本也失敗。
    RollbackFailed {
        failed: Version,
        reason: String,
    },
    Io(String),
}

impl UpdateError {
    /// 給使用者的說明，最後一段 `Next:` 是下一步(`rerun` 是重跑更新的指令)。
    pub fn explain(&self, rerun: &str) -> String {
        const ISSUES: &str = "https://github.com/raychiutw/purrdr.host/issues";
        match self {
            UpdateError::NotInstalled => format!(
                "The Purrdr host service is not installed, so there is nothing to update.\n\
                 Next: install it (see {GITHUB_RELEASES}#install-the-host-service):\n  {}",
                install_command()
            ),
            UpdateError::Network(message) => format!(
                "Could not get the release from GitHub: {message}\nNothing was changed.\n\
                 Next: check your network, then run again:\n  {rerun}"
            ),
            UpdateError::ChecksumMismatch { file } => format!(
                "{file} does not match SHA256SUMS: the download is incomplete or was altered, so nothing was replaced.\n\
                 Next: run again:\n  {rerun}\n(if it keeps failing, open an issue at {ISSUES})"
            ),
            UpdateError::Signature(message) => format!(
                "The downloaded host service is not signed by the Purrdr developer ({message}), so nothing was replaced.\n\
                 Next: run again:\n  {rerun}\n(if it keeps failing, open an issue at {ISSUES})"
            ),
            UpdateError::RolledBack { failed, restored, reason } => format!(
                "Host service {failed} did not start ({reason}), so {restored} was put back and is running again.\n\
                 Next: open an issue at {ISSUES} with the log in ~/.purrdr/host-service/logs; to try again run:\n  {rerun}"
            ),
            UpdateError::RollbackFailed { failed, reason } => format!(
                "Host service {failed} did not start, and putting the previous version back failed: {reason}\n\
                 Next: reinstall with:\n  {}",
                install_command()
            ),
            UpdateError::Io(message) => format!("{message}\nNext: fix the problem above, then run again:\n  {rerun}"),
            UpdateError::InProgress => format!(
                "Another update of the host service is already in progress, so this one did not start.\n\
                 Next: wait a minute, then run again:\n  {rerun}"
            ),
        }
    }
}

impl UpdateError {
    /// 不含下一步的一句話(放進別的訊息裡用)。
    pub fn summary(&self) -> String {
        match self {
            UpdateError::Io(message) | UpdateError::Network(message) | UpdateError::Signature(message) => message.clone(),
            other => other.explain("").lines().next().unwrap_or_default().to_owned(),
        }
    }
}

impl std::fmt::Display for UpdateError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.explain("purrdr-host-service update"))
    }
}

/// 這個平台的一行安裝指令。
fn install_command() -> String {
    if cfg!(windows) {
        format!("irm {GITHUB_RELEASES}/releases/latest/download/install.ps1 | iex")
    } else {
        format!("curl -fsSL {GITHUB_RELEASES}/releases/latest/download/install.sh | sh")
    }
}

fn io_error(error: impl std::fmt::Display) -> UpdateError {
    UpdateError::Io(error.to_string())
}

/// 上一次檢查或更新的時間與結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateRecord {
    pub checked_at: SystemTime,
    pub result: String,
}

/// 「檢查」的結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateCheck {
    /// 沒有安裝時 None。
    pub installed: Option<Version>,
    /// 取不到(例如沒有網路)時是錯誤。
    pub latest: Result<Version, UpdateError>,
    /// 自動更新是否開啟;沒有安裝時 None。
    pub auto_update: Option<bool>,
    pub last_result: Option<UpdateRecord>,
}

impl UpdateCheck {
    /// 只往更大的版本更新。
    pub fn update_available(&self) -> bool {
        matches!((&self.installed, &self.latest), (Some(installed), Ok(latest)) if latest > installed)
    }

    /// 已安裝與最新的版本都要有。
    pub fn versions(&self) -> Result<(Version, Version), UpdateError> {
        let installed = self.installed.ok_or(UpdateError::NotInstalled)?;
        Ok((installed, self.latest.clone()?))
    }
}

/// 「套用」的結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateOutcome {
    Updated { from: Version, to: Version },
    UpToDate { installed: Version, latest: Version },
}

/// 背景自動檢查的結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutomaticOutcome {
    /// 自動更新關閉，沒有檢查。
    Disabled,
    UpToDate(Version),
    /// 這個版本上次更新後退回過，不再自動重試(手動 `update` 仍可)。
    Skipped(Version),
    /// 有 Agent 正在工作，延後到下次排程(`deferred` 是連續延後的次數)。
    Deferred {
        to: Version,
        deferred: u32,
    },
    /// 已啟動更新程序(由它套用並記錄結果)。
    Started {
        from: Version,
        to: Version,
    },
    Failed(UpdateError),
}

/// 推播給配對裝置的更新事件(後台的 `event` 與 `version`)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateEvent {
    Available(Version),
    Updated(Version),
    /// 帶的是起不來、已退回的那個版本。
    RolledBack(Version),
}

impl UpdateEvent {
    /// 後台 `POST /v1/push` 的 `event`。
    pub fn name(&self) -> &'static str {
        match self {
            UpdateEvent::Available(_) => "host_service_update_available",
            UpdateEvent::Updated(_) => "host_service_updated",
            UpdateEvent::RolledBack(_) => "host_service_update_rolled_back",
        }
    }

    pub fn version(&self) -> Version {
        match self {
            UpdateEvent::Available(version) | UpdateEvent::Updated(version) | UpdateEvent::RolledBack(version) => *version,
        }
    }
}

/// 啟動獨立的更新程序(`update --yes`):服務重啟時它不會跟著被結束。
pub trait UpdateLauncher {
    fn launch(&mut self) -> Result<(), String>;
}

/// 這台主機現在有沒有 `working` 的 Agent(自動更新據此延後)。
pub trait AgentActivity {
    fn has_working_agent(&mut self) -> bool;
}

/// 更新鎖;drop 時放開。
pub struct UpdateLock {
    _file: Option<std::fs::File>,
}

impl UpdateLock {
    /// 不鎖任何東西(測試替身用)。
    pub fn none() -> Self {
        UpdateLock { _file: None }
    }

    pub(crate) fn holding(file: std::fs::File) -> Self {
        UpdateLock { _file: Some(file) }
    }
}

/// Release 來源：最新版本與該版本的檔案。
pub trait ReleaseSource {
    fn latest_version(&mut self) -> Result<Version, UpdateError>;
    fn download(&mut self, version: &Version, file_name: &str) -> Result<Vec<u8>, UpdateError>;
}

/// 安裝位置：已安裝的版本、替換執行檔與重啟背景服務。
pub trait InstallLocation {
    fn platform(&self) -> InstallPlatform;
    fn installed_version(&mut self) -> Result<Version, UpdateError>;
    /// 解壓縮用的暫存資料夾(與執行檔同一個檔案系統)。
    fn staging_directory(&self) -> PathBuf;
    /// 驗證解開的執行檔(macOS 的 codesign);其他平台不檢查。
    fn verify(&mut self, staged: &Path) -> Result<(), UpdateError>;
    /// 以 `staged` 的執行檔替換已安裝的，舊的留一份。
    fn replace(&mut self, staged: &Path) -> Result<(), UpdateError>;
    /// 換回留下的舊執行檔。
    fn restore(&mut self) -> Result<(), UpdateError>;
    /// 重啟背景服務，限時內確認它在聽且回報 `expected`。
    fn restart(&mut self, expected: &Version) -> Result<(), UpdateError>;
    /// 安裝紀錄的 `auto_update`。
    fn auto_update(&mut self) -> Result<bool, UpdateError>;
    /// 記下這次檢查或更新的結果(狀態檔與紀錄檔)。
    fn record_result(&mut self, record: &UpdateRecord);
    fn last_result(&mut self) -> Option<UpdateRecord>;
    /// 拿更新鎖(手動與自動互斥);已有更新在進行時回 `InProgress`。
    fn lock(&mut self) -> Result<UpdateLock, UpdateError>;
    /// 上次更新後退回的版本(不再自動重試)。
    fn rolled_back_version(&mut self) -> Option<Version>;
    fn set_rolled_back_version(&mut self, version: Option<Version>);
    /// 背景自動更新因 Agent 工作中連續延後的次數。
    fn deferred_updates(&mut self) -> u32;
    fn set_deferred_updates(&mut self, count: u32);
    /// 更新成功後刪掉留下的舊執行檔。
    fn discard_previous(&mut self);
    /// 通知配對裝置(推播);失敗不影響更新結果。
    fn notify(&mut self, _event: &UpdateEvent) {}
}

/// 以 HTTP 取 Release(正式是 GitHub)。
pub struct HttpReleaseSource {
    latest_url: String,
    download_url: String,
    agent: ureq::Agent,
}

impl HttpReleaseSource {
    /// `PURRDR_HOST_SERVICE_RELEASE_URL`(測試與實機驗證用)有設定時用它，否則 GitHub。
    /// 正式版一律 GitHub。
    pub fn from_environment() -> Self {
        #[cfg(any(debug_assertions, feature = "release-url-override"))]
        if let Ok(base) = std::env::var(RELEASE_URL_VARIABLE)
            && !base.trim().is_empty()
        {
            return HttpReleaseSource::at(&base);
        }
        HttpReleaseSource::github()
    }

    /// `raychiutw/purrdr.host` 的 GitHub Release。
    pub fn github() -> Self {
        HttpReleaseSource {
            latest_url: GITHUB_LATEST_RELEASE.to_owned(),
            download_url: format!("{GITHUB_RELEASES}/releases/download"),
            agent: agent(),
        }
    }

    /// `<base>/releases/latest` 與 `<base>/releases/download/<tag>/<檔名>`(測試的假 Release 伺服器)。
    pub fn at(base: &str) -> Self {
        let base = base.trim_end_matches('/');
        HttpReleaseSource {
            latest_url: format!("{base}/releases/latest"),
            download_url: format!("{base}/releases/download"),
            agent: agent(),
        }
    }

    /// `accept` 只給 API 呼叫;下載 Release 檔案不帶。
    fn get(&self, url: &str, accept: Option<&str>) -> Result<Vec<u8>, UpdateError> {
        let network = |error: ureq::Error| UpdateError::Network(format!("{url}: {error}"));
        let mut request = self.agent.get(url);
        if let Some(accept) = accept {
            request = request.header("Accept", accept);
        }
        let mut response = request.call().map_err(network)?;
        let mut body = Vec::new();
        response
            .body_mut()
            .as_reader()
            .take(MAXIMUM_DOWNLOAD)
            .read_to_end(&mut body)
            .map_err(|error| UpdateError::Network(format!("{url}: {error}")))?;
        Ok(body)
    }
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(120)))
        .user_agent(concat!("purrdr-host-service/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

#[derive(serde::Deserialize)]
struct LatestRelease {
    tag_name: String,
}

impl ReleaseSource for HttpReleaseSource {
    fn latest_version(&mut self) -> Result<Version, UpdateError> {
        let body = self.get(&self.latest_url, Some("application/json"))?;
        let release: LatestRelease =
            serde_json::from_slice(&body).map_err(|error| UpdateError::Network(format!("{}: unexpected response: {error}", self.latest_url)))?;
        Version::from_tag(&release.tag_name).ok_or_else(|| UpdateError::Network(format!("unexpected release tag {}", release.tag_name)))
    }

    fn download(&mut self, version: &Version, file_name: &str) -> Result<Vec<u8>, UpdateError> {
        self.get(&format!("{}/{}/{file_name}", self.download_url, version.tag()), None)
    }
}

/// 這個平台的 Release 壓縮檔名(與 Release workflow 的 target 對應)。
pub fn release_asset_name(platform: InstallPlatform) -> String {
    let architecture = std::env::consts::ARCH;
    match platform {
        InstallPlatform::MacOs => format!("purrdr-host-service-{architecture}-apple-darwin.tar.gz"),
        InstallPlatform::Linux => format!("purrdr-host-service-{architecture}-unknown-linux-musl.tar.gz"),
        InstallPlatform::Windows => "purrdr-host-service-x86_64-pc-windows-msvc.zip".to_owned(),
    }
}

/// 更新流程：只對外「檢查」與「套用」。
pub struct Updater<R: ReleaseSource, L: InstallLocation> {
    source: R,
    location: L,
}

impl<R: ReleaseSource, L: InstallLocation> Updater<R, L> {
    pub fn new(source: R, location: L) -> Self {
        Updater { source, location }
    }

    /// 安裝位置(測試檢查假系統)。
    pub fn location(&self) -> &L {
        &self.location
    }

    fn record(&mut self, result: String) {
        self.location.record_result(&UpdateRecord {
            checked_at: SystemTime::now(),
            result,
        });
    }

    /// 已安裝與最新的版本。
    pub fn check(&mut self) -> Result<UpdateCheck, UpdateError> {
        let installed = match self.location.installed_version() {
            Err(UpdateError::NotInstalled) => None,
            installed => Some(installed?),
        };
        let (auto_update, last_result) = match installed {
            Some(_) => (Some(self.location.auto_update()?), self.location.last_result()),
            None => (None, None),
        };
        Ok(UpdateCheck {
            installed,
            latest: self.source.latest_version(),
            auto_update,
            last_result,
        })
    }

    /// 背景自動檢查：開啟且有新版時啟動更新程序(由它套用);結果都記下來。
    pub fn automatic(&mut self, launcher: &mut impl UpdateLauncher, agents: &mut impl AgentActivity) -> AutomaticOutcome {
        match self.location.auto_update() {
            Ok(true) => {}
            Ok(false) => return AutomaticOutcome::Disabled,
            Err(error) => return AutomaticOutcome::Failed(error),
        }
        let outcome = match self.check().and_then(|check| Ok((check.update_available(), check.versions()?))) {
            Err(error) => AutomaticOutcome::Failed(error),
            Ok((false, (installed, _))) => {
                self.location.set_deferred_updates(0);
                AutomaticOutcome::UpToDate(installed)
            }
            Ok((true, (_, to))) if self.location.rolled_back_version() == Some(to) => AutomaticOutcome::Skipped(to),
            Ok((true, (_, to))) if self.location.deferred_updates() < MAXIMUM_DEFERRALS && agents.has_working_agent() => {
                let deferred = self.location.deferred_updates() + 1;
                self.location.set_deferred_updates(deferred);
                AutomaticOutcome::Deferred { to, deferred }
            }
            Ok((true, (from, to))) => {
                self.location.set_deferred_updates(0);
                match launcher.launch() {
                    Ok(()) => AutomaticOutcome::Started { from, to },
                    Err(message) => AutomaticOutcome::Failed(UpdateError::Io(format!("cannot start the update: {message}"))),
                }
            }
        };
        let result = match &outcome {
            AutomaticOutcome::UpToDate(installed) => format!("up to date ({installed})"),
            AutomaticOutcome::Started { to, .. } => format!("update to {to} started"),
            AutomaticOutcome::Failed(error) => format!("check failed: {}", error.summary()),
            AutomaticOutcome::Skipped(version) => format!("skipped {version}: it was rolled back before (run update to try again)"),
            AutomaticOutcome::Deferred { to, deferred } => format!("deferred update to {to}: an agent is working ({deferred} in a row)"),
            AutomaticOutcome::Disabled => unreachable!("前面已回傳"),
        };
        self.record(result);
        if let AutomaticOutcome::Started { to, .. } | AutomaticOutcome::Deferred { to, .. } = &outcome {
            self.location.notify(&UpdateEvent::Available(*to));
        }
        outcome
    }

    /// 有更大的版本就下載、核對、替換、重啟;新版起不來就退回。結果記下來。
    pub fn apply(&mut self) -> Result<UpdateOutcome, UpdateError> {
        let applied = self.apply_unrecorded();
        match &applied {
            Ok(UpdateOutcome::Updated { to, .. }) => self.location.notify(&UpdateEvent::Updated(*to)),
            Err(UpdateError::RolledBack { failed, .. }) => self.location.notify(&UpdateEvent::RolledBack(*failed)),
            _ => {}
        }
        let result = match &applied {
            Ok(UpdateOutcome::Updated { from, to }) => format!("updated {from} -> {to}"),
            Ok(UpdateOutcome::UpToDate { installed, .. }) => format!("up to date ({installed})"),
            Err(UpdateError::NotInstalled) => return applied,
            Err(error) => format!("update failed: {}", error.summary()),
        };
        self.record(result);
        applied
    }

    fn apply_unrecorded(&mut self) -> Result<UpdateOutcome, UpdateError> {
        let check = self.check()?;
        let (installed, latest) = check.versions()?;
        if !check.update_available() {
            return Ok(UpdateOutcome::UpToDate { installed, latest });
        }
        let _lock = self.location.lock()?;
        let asset = release_asset_name(self.location.platform());
        let checksums = self.source.download(&latest, "SHA256SUMS")?;
        let archive = self.source.download(&latest, &asset)?;
        verify_checksum(&checksums, &asset, &archive)?;

        let staging = self.location.staging_directory();
        let staged = self.stage(&staging, &archive);
        let replaced = staged
            .and_then(|()| self.location.verify(&staging))
            .and_then(|()| self.location.replace(&staging));
        let _ = std::fs::remove_dir_all(&staging);
        replaced?;

        if let Err(error) = self.location.restart(&latest) {
            let reason = error.summary();
            self.location.set_rolled_back_version(Some(latest));
            return match self.location.restore().and_then(|()| self.location.restart(&installed)) {
                Ok(()) => Err(UpdateError::RolledBack {
                    failed: latest,
                    restored: installed,
                    reason,
                }),
                Err(rollback) => Err(UpdateError::RollbackFailed {
                    failed: latest,
                    reason: rollback.summary(),
                }),
            };
        }
        self.location.discard_previous();
        self.location.set_rolled_back_version(None);
        Ok(UpdateOutcome::Updated { from: installed, to: latest })
    }

    /// 解開壓縮檔到新的暫存資料夾;只接受一般檔案與資料夾(連結項目整個拒絕)。
    fn stage(&self, staging: &Path, archive: &[u8]) -> Result<(), UpdateError> {
        let extract_failed = |error: &dyn std::fmt::Display| io_error(format!("cannot extract the downloaded archive: {error}"));
        std::fs::create_dir_all(staging).map_err(io_error)?;
        if self.location.platform() == InstallPlatform::Windows {
            let mut zip = zip::ZipArchive::new(std::io::Cursor::new(archive)).map_err(|error| extract_failed(&error))?;
            for index in 0..zip.len() {
                let file = zip.by_index(index).map_err(|error| extract_failed(&error))?;
                if file.is_symlink() {
                    return Err(link_refused(&file.name().map(|name| name.into_owned()).unwrap_or_default()));
                }
            }
            zip.extract(staging).map_err(|error| extract_failed(&error))
        } else {
            let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(archive));
            for entry in tar.entries().map_err(|error| extract_failed(&error))? {
                let mut entry = entry.map_err(|error| extract_failed(&error))?;
                let kind = entry.header().entry_type();
                if !(kind.is_file() || kind.is_dir()) {
                    return Err(link_refused(&entry.path().map(|path| path.display().to_string()).unwrap_or_default()));
                }
                entry.unpack_in(staging).map_err(|error| extract_failed(&error))?;
            }
            Ok(())
        }
    }
}

fn link_refused(name: &str) -> UpdateError {
    UpdateError::Io(format!("the downloaded archive contains a link or special entry ({name}), so it was not used"))
}

/// `SHA256SUMS` 裡 `asset` 那一行要與壓縮檔相符。
fn verify_checksum(checksums: &[u8], asset: &str, archive: &[u8]) -> Result<(), UpdateError> {
    let actual: String = Sha256::digest(archive).iter().map(|byte| format!("{byte:02x}")).collect();
    let expected = String::from_utf8_lossy(checksums).lines().find_map(|line| {
        let (digest, name) = line.split_once(char::is_whitespace)?;
        let name = name.trim_start();
        (name == asset || name.strip_prefix('*') == Some(asset)).then(|| digest.to_ascii_lowercase())
    });
    if expected.as_deref() == Some(actual.as_str()) {
        Ok(())
    } else {
        Err(UpdateError::ChecksumMismatch { file: asset.to_owned() })
    }
}
