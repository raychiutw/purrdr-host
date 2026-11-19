//! 更新流程的安裝位置：家目錄裡安裝好的主機服務(raychiutw/purrdr.host#6)。

use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::system::HostSystem;
use super::{HostEnvironment, InstallPlatform, InstallRecord, ServiceLayout, autostart, read_install_record, service_arguments};
use crate::update::{InstallLocation, UpdateError, Version};

/// 替換時留下的舊執行檔副檔名。
const PREVIOUS_SUFFIX: &str = ".previous";
/// Developer ID Application 憑證(Apple 的 OID)。
const DEVELOPER_ID_REQUIREMENT: &str = "anchor apple generic and certificate leaf[field.1.2.840.113635.100.6.1.13] exists";

/// 已安裝的主機服務：依安裝紀錄替換執行檔、重啟背景服務(沿用埠與 session,不動設定與已配對裝置)。
pub struct InstalledService<S: HostSystem> {
    environment: HostEnvironment,
    system: S,
    start_timeout: Duration,
}

impl<S: HostSystem> InstalledService<S> {
    pub fn new(environment: HostEnvironment, system: S) -> Self {
        InstalledService {
            environment,
            system,
            start_timeout: Duration::from_secs(30),
        }
    }

    /// 重啟後等新版開始聽埠的時限。
    pub fn with_start_timeout(self, start_timeout: Duration) -> Self {
        InstalledService { start_timeout, ..self }
    }

    /// 測試檢查假系統狀態。
    pub fn system(&self) -> &S {
        &self.system
    }

    fn layout(&self) -> ServiceLayout {
        ServiceLayout::of(&self.environment)
    }

    fn record(&self) -> Result<InstallRecord, UpdateError> {
        read_install_record(&self.layout())
            .map_err(|error| UpdateError::Io(error.to_string()))?
            .ok_or(UpdateError::NotInstalled)
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

    /// 已安裝執行檔的 Team ID(沒簽章時 None)。
    fn installed_team(&mut self) -> Option<String> {
        let executable = self.layout().executable().display().to_string();
        let output = self.system.run("codesign", &["-dv".to_owned(), executable]).ok()?;
        let team = output.stderr.lines().find_map(|line| line.strip_prefix("TeamIdentifier="))?.trim().to_owned();
        (team != "not set").then_some(team)
    }

    fn stop(&mut self) -> Result<(), UpdateError> {
        let layout = self.layout();
        autostart::unregister(&mut self.system, &layout, self.environment.user_name.as_deref()).map_err(|error| UpdateError::Io(error.to_string()))
    }

    fn previous(executable: &Path) -> PathBuf {
        let mut name = executable.as_os_str().to_owned();
        name.push(PREVIOUS_SUFFIX);
        PathBuf::from(name)
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
        self.layout().directory().join("update")
    }

    /// macOS:與已安裝的執行檔同一個 Team 的 Developer ID 簽章。
    fn verify(&mut self, staged: &Path) -> Result<(), UpdateError> {
        if self.environment.platform != InstallPlatform::MacOs {
            return Ok(());
        }
        let requirement = match self.installed_team() {
            Some(team) => format!("{DEVELOPER_ID_REQUIREMENT} and certificate leaf[subject.OU] = \"{team}\""),
            None => DEVELOPER_ID_REQUIREMENT.to_owned(),
        };
        for name in self.layout().executable_names() {
            let file = staged.join(name).display().to_string();
            let arguments = ["--verify".to_owned(), "--strict".to_owned(), format!("-R={requirement}"), file];
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

    /// 執行中的執行檔先改名(Windows 不能覆蓋執行中的檔案，但可以改名),再放新檔。
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
            let moved = std::fs::rename(&installed, &previous).and_then(|()| std::fs::rename(staged.join(name), &installed));
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

    fn restore(&mut self) -> Result<(), UpdateError> {
        self.stop()?;
        let layout = self.layout();
        for name in layout.executable_names() {
            let installed = layout.bin_directory().join(name);
            let previous = Self::previous(&installed);
            if previous.is_file() {
                std::fs::rename(&previous, &installed).map_err(|error| UpdateError::Io(format!("cannot restore {}: {error}", installed.display())))?;
            }
        }
        Ok(())
    }

    fn restart(&mut self, expected: &Version) -> Result<(), UpdateError> {
        let record = self.record()?;
        let address = SocketAddr::from(([127, 0, 0, 1], record.port));
        self.stop()?;
        // 舊行程放開埠之後才啟動，確認聽埠的是新版。
        wait_until(Instant::now() + self.start_timeout, || !is_listening(&address));
        let layout = self.layout();
        autostart::register(&mut self.system, &layout, &service_arguments(&record)).map_err(|error| UpdateError::Io(error.to_string()))?;
        let reported = self.executable_version(&layout.executable()).map_err(UpdateError::Io)?;
        if reported != *expected {
            return Err(UpdateError::Io(format!("the installed host service reports {reported}, expected {expected}")));
        }
        if !wait_until(Instant::now() + self.start_timeout, || is_listening(&address)) {
            return Err(UpdateError::Io(format!(
                "not listening on {address} after {} seconds",
                self.start_timeout.as_secs()
            )));
        }
        Ok(())
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
