//! 從 app 管理主機服務(raychiutw/purrdr.host#18):狀態查詢、立即更新與開關自動更新，都走既有的 `Updater`。

use std::sync::Arc;
use std::time::SystemTime;

use crate::connection::error;
use crate::http::blocking;
use crate::install::system::{HostSystem, ProcessHostSystem};
use crate::install::{HostEnvironment, InstallError, InstalledService, Installer, ServiceLayout};
use crate::protocol::{
    ErrorCode, HostServiceSetAutoUpdateResult, HostServiceStatusResult, HostServiceUpdateOutcome, HostServiceUpdateRecord, HostServiceUpdateResult,
    ServerMessage,
};
use crate::text_format::rfc3339;
use crate::update::{CachedReleaseSource, Clock, HttpReleaseSource, UpdateCheck, UpdateError, UpdateLaunch, UpdateLauncher, Updater, Version, system_clock};

/// 狀態查詢快取「最新版本」的檔案(狀態資料夾裡)。
const LATEST_VERSION_CACHE_FILE: &str = "latest-release.json";

/// 主機服務的更新狀態。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostServiceState {
    pub check: UpdateCheck,
    pub rolled_back: Option<Version>,
}

/// 從 app 能做的主機服務管理(在 blocking 執行緒呼叫)。
pub trait HostServiceManagement: Send + Sync {
    fn status(&self) -> Result<HostServiceState, UpdateError>;
    /// 有新版就以獨立程序更新(回傳時還沒重啟)。
    fn update_now(&self) -> Result<UpdateLaunch, UpdateError>;
    /// 改安裝紀錄的 `auto_update`(同主機上的 `auto-update on|off`)。
    fn set_auto_update(&self, enabled: bool) -> Result<(), UpdateError>;
}

/// 連線共用的主機服務管理。
#[derive(Clone)]
pub struct ServiceManagement(Arc<dyn HostServiceManagement>);

impl ServiceManagement {
    pub fn new(management: impl HostServiceManagement + 'static) -> Self {
        ServiceManagement(Arc::new(management))
    }
}

impl std::fmt::Debug for ServiceManagement {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ServiceManagement")
    }
}

type Factory<T> = Box<dyn Fn() -> T + Send + Sync>;
type LauncherFactory<S> = Box<dyn Fn(&InstalledService<S>) -> Box<dyn UpdateLauncher> + Send + Sync>;

/// 家目錄裡安裝好的主機服務(系統、Release 來源與更新程序的啟動器可替換，測試用假的)。
pub struct InstalledServiceManagement<S: HostSystem> {
    environment: HostEnvironment,
    system: Factory<S>,
    release_source: Factory<HttpReleaseSource>,
    launcher: LauncherFactory<S>,
    clock: Clock,
}

impl<S: HostSystem> InstalledServiceManagement<S> {
    pub fn new(
        environment: HostEnvironment,
        system: impl Fn() -> S + Send + Sync + 'static,
        release_source: impl Fn() -> HttpReleaseSource + Send + Sync + 'static,
        launcher: impl Fn(&InstalledService<S>) -> Box<dyn UpdateLauncher> + Send + Sync + 'static,
    ) -> Self {
        InstalledServiceManagement {
            environment,
            system: Box::new(system),
            release_source: Box::new(release_source),
            launcher: Box::new(launcher),
            clock: system_clock(),
        }
    }

    /// 換掉時鐘(測試推進時間)。
    pub fn with_clock(self, clock: impl Fn() -> SystemTime + Send + Sync + 'static) -> Self {
        InstalledServiceManagement {
            clock: Arc::new(clock),
            ..self
        }
    }

    fn location(&self) -> InstalledService<S> {
        InstalledService::new(self.environment.clone(), (self.system)()).with_clock(self.clock.clone())
    }
}

impl<S: HostSystem> HostServiceManagement for InstalledServiceManagement<S> {
    fn status(&self) -> Result<HostServiceState, UpdateError> {
        let cache_file = ServiceLayout::of(&self.environment).directory().join(LATEST_VERSION_CACHE_FILE);
        let release_source = CachedReleaseSource::new((self.release_source)(), cache_file, self.clock.clone());
        let mut updater = Updater::new(release_source, self.location());
        let check = updater.check()?;
        Ok(HostServiceState {
            check,
            rolled_back: updater.rolled_back_version(),
        })
    }

    fn update_now(&self) -> Result<UpdateLaunch, UpdateError> {
        let mut launcher = DeferredLauncher(|| (self.launcher)(&self.location()));
        Updater::new((self.release_source)(), self.location()).launch(&mut launcher)
    }

    fn set_auto_update(&self, enabled: bool) -> Result<(), UpdateError> {
        Installer::new(self.environment.clone(), (self.system)())
            .set_auto_update(enabled)
            .map_err(|error| match error {
                InstallError::NotInstalled => UpdateError::NotInstalled,
                other => UpdateError::Io(other.to_string().lines().next().unwrap_or_default().to_owned()),
            })
    }
}

/// 執行中的是安裝好的那份時才能從 app 管理(與背景自動更新相同)。
pub fn for_installed_copy(environment: HostEnvironment) -> Option<ServiceManagement> {
    if !InstalledService::new(environment.clone(), ProcessHostSystem).runs_installed_copy() {
        return None;
    }
    Some(ServiceManagement::new(InstalledServiceManagement::new(
        environment,
        || ProcessHostSystem,
        HttpReleaseSource::from_environment,
        |location: &InstalledService<ProcessHostSystem>| Box::new(location.update_launcher()) as Box<dyn UpdateLauncher>,
    )))
}

/// 真的要更新時才建立啟動器。
struct DeferredLauncher<F: FnMut() -> Box<dyn UpdateLauncher>>(F);

impl<F: FnMut() -> Box<dyn UpdateLauncher>> UpdateLauncher for DeferredLauncher<F> {
    fn launch(&mut self) -> Result<(), String> {
        (self.0)().launch()
    }
}

/// 在 blocking 執行緒執行(檢查會連網路、執行已安裝的執行檔);不是安裝好的那份時回 `NotInstalled`。
async fn run<T: Send + 'static>(
    management: Option<ServiceManagement>,
    operation: impl FnOnce(&dyn HostServiceManagement) -> Result<T, UpdateError> + Send + 'static,
) -> Result<T, UpdateError> {
    let Some(ServiceManagement(management)) = management else {
        return Err(UpdateError::NotInstalled);
    };
    blocking(move || operation(management.as_ref())).await
}

/// `host_service.status`。
pub(crate) async fn status(management: Option<ServiceManagement>, request_id: String) -> ServerMessage {
    match run(management, |management| management.status()).await {
        Ok(state) => status_result(request_id, state),
        Err(update_error) => failure(request_id, update_error),
    }
}

/// `host_service.update`。
pub(crate) async fn update(management: Option<ServiceManagement>, request_id: String) -> ServerMessage {
    match run(management, |management| management.update_now()).await {
        Ok(launch) => update_result(request_id, launch),
        Err(update_error) => failure(request_id, update_error),
    }
}

/// `host_service.set_auto_update`。
pub(crate) async fn set_auto_update(management: Option<ServiceManagement>, request_id: String, enabled: bool) -> ServerMessage {
    match run(management, move |management| management.set_auto_update(enabled)).await {
        Ok(()) => ServerMessage::HostServiceSetAutoUpdateResult(HostServiceSetAutoUpdateResult {
            request_id,
            auto_update: enabled,
        }),
        Err(update_error) => failure(request_id, update_error),
    }
}

fn update_result(request_id: String, launch: UpdateLaunch) -> ServerMessage {
    let (outcome, from, to) = match launch {
        UpdateLaunch::Started { from, to } => (HostServiceUpdateOutcome::Started, from, to),
        UpdateLaunch::UpToDate { installed, latest } => (HostServiceUpdateOutcome::UpToDate, installed, latest),
    };
    ServerMessage::HostServiceUpdateResult(HostServiceUpdateResult {
        request_id,
        outcome,
        from_version: from.to_string(),
        to_version: to.to_string(),
    })
}

fn status_result(request_id: String, state: HostServiceState) -> ServerMessage {
    let check = state.check;
    let update_available = check.update_available();
    let (Some(current), Some(auto_update)) = (check.installed, check.auto_update) else {
        return failure(request_id, UpdateError::NotInstalled);
    };
    let (latest_version, latest_error) = match check.latest {
        Ok(latest) => (Some(latest.to_string()), None),
        Err(error) => (None, Some(error.summary())),
    };
    ServerMessage::HostServiceStatusResult(HostServiceStatusResult {
        request_id,
        installed_version: current.to_string(),
        latest_version,
        latest_error,
        update_available,
        auto_update,
        last_update: check.last_result.map(|record| HostServiceUpdateRecord {
            checked_at: rfc3339(record.checked_at),
            result: record.result,
        }),
        rolled_back: state.rolled_back.map(|version| version.to_string()),
    })
}

fn failure(request_id: String, update_error: UpdateError) -> ServerMessage {
    let (code, message) = match update_error {
        UpdateError::NotInstalled => (
            ErrorCode::HostServiceNotInstalled,
            "this host service is not the installed copy, so it cannot be managed from the app".to_owned(),
        ),
        UpdateError::InProgress => (
            ErrorCode::UpdateInProgress,
            "another update of the host service is already in progress".to_owned(),
        ),
        other => (ErrorCode::UpdateFailed, other.summary()),
    };
    error(Some(request_id), code, &message)
}
