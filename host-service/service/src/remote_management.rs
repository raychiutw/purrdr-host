//! 從 app 管理主機服務(raychiutw/purrdr.host#18):狀態查詢、立即更新與開關自動更新，都走既有的 `Updater`。

use std::sync::Arc;

use crate::http::blocking;
use crate::install::system::{HostSystem, ProcessHostSystem};
use crate::install::{HostEnvironment, InstallError, InstalledService, Installer};
use crate::protocol::{
    ErrorCode, ErrorMessage, HostServiceSetAutoUpdateResult, HostServiceStatusResult, HostServiceUpdateOutcome, HostServiceUpdateRecord,
    HostServiceUpdateResult, ServerMessage,
};
use crate::update::{HttpReleaseSource, UpdateCheck, UpdateError, UpdateLaunch, UpdateLauncher, Updater, Version};

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
        }
    }

    fn updater(&self) -> Updater<HttpReleaseSource, InstalledService<S>> {
        Updater::new((self.release_source)(), InstalledService::new(self.environment.clone(), (self.system)()))
    }
}

impl<S: HostSystem> HostServiceManagement for InstalledServiceManagement<S> {
    fn status(&self) -> Result<HostServiceState, UpdateError> {
        let mut updater = self.updater();
        let check = updater.check()?;
        Ok(HostServiceState {
            check,
            rolled_back: updater.rolled_back_version(),
        })
    }

    fn update_now(&self) -> Result<UpdateLaunch, UpdateError> {
        let mut launcher = DeferredLauncher(|| (self.launcher)(&InstalledService::new(self.environment.clone(), (self.system)())));
        self.updater().launch(&mut launcher)
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

/// 連線收到的管理請求。
pub(crate) enum ManagementRequest {
    Status(String),
    Update(String),
    SetAutoUpdate { request_id: String, enabled: bool },
}

impl ManagementRequest {
    pub fn request_id(&self) -> &str {
        match self {
            ManagementRequest::Status(request_id) | ManagementRequest::Update(request_id) | ManagementRequest::SetAutoUpdate { request_id, .. } => request_id,
        }
    }
}

/// 在 blocking 執行緒處理請求(檢查會連網路、執行已安裝的執行檔)並轉成回應。
pub(crate) async fn reply(management: Option<ServiceManagement>, request: ManagementRequest) -> ServerMessage {
    let request_id = request.request_id().to_owned();
    let Some(ServiceManagement(management)) = management else {
        return failure(request_id, UpdateError::NotInstalled);
    };
    match request {
        ManagementRequest::Status(_) => match blocking(move || management.status()).await {
            Ok(state) => status_result(request_id, state),
            Err(error) => failure(request_id, error),
        },
        ManagementRequest::Update(_) => match blocking(move || management.update_now()).await {
            Ok(launch) => update_result(request_id, launch),
            Err(error) => failure(request_id, error),
        },
        ManagementRequest::SetAutoUpdate { enabled, .. } => match blocking(move || management.set_auto_update(enabled)).await {
            Ok(()) => ServerMessage::HostServiceSetAutoUpdateResult(HostServiceSetAutoUpdateResult {
                request_id,
                auto_update: enabled,
            }),
            Err(error) => failure(request_id, error),
        },
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
            checked_at: humantime::format_rfc3339_seconds(record.checked_at).to_string(),
            result: record.result,
        }),
        rolled_back: state.rolled_back.map(|version| version.to_string()),
    })
}

fn failure(request_id: String, error: UpdateError) -> ServerMessage {
    let (code, message) = match error {
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
    ServerMessage::ErrorMessage(ErrorMessage {
        request_id: Some(request_id),
        code,
        message,
        herdr_code: None,
        supported_protocol_versions: None,
    })
}
