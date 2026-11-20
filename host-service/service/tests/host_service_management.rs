//! 從 app 管理主機服務(raychiutw/purrdr.host#18):`host_service.status`／`host_service.update`／`host_service.set_auto_update`。
//! 以真正的 WebSocket client 從協定這一端驗證，背後是假 Release 伺服器、暫存家目錄與假系統(同 `tests/update.rs`)。

mod install_support;
mod support;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, SystemTime};

use install_support::release::FakeReleaseServer;
use install_support::{FakeSystem, TestHost, executable_names, fake_host_service, reserved_port};
use purrdr_host_service::install::{InstallPlatform, InstalledService, Installer};
use purrdr_host_service::remote_management::{self, InstalledServiceManagement, ServiceManagement};
use purrdr_host_service::update::{HttpReleaseSource, InstallLocation, UpdateLauncher, Updater};
use serde_json::{Value, json};
use support::{FakeHerdr, TestClient, TestHostService, recorded, start_service_with_management};

const INSTALLED: &str = "0.5.0";
const PLATFORM: InstallPlatform = InstallPlatform::Linux;

fn publish(server: &FakeReleaseServer, version: &str, traits: &str) {
    let executables: Vec<(&str, String)> = executable_names(PLATFORM)
        .into_iter()
        .map(|name| (name, fake_host_service(version, &format!("{name} {traits}"))))
        .collect();
    server.publish(PLATFORM, version, &executables);
}

/// 被要求啟動更新程序時，像真的 `update --yes` 一樣在另一條執行緒套用;收到放行訊號才開始。
#[derive(Clone)]
struct HelperProcess {
    launches: Arc<AtomicUsize>,
    release: Arc<Mutex<Option<mpsc::Receiver<()>>>>,
    finished: Arc<Mutex<Option<std::thread::JoinHandle<()>>>>,
    /// 安裝時啟動、還在聽埠的假系統(重啟時先停掉它)。
    running_system: Arc<Mutex<Option<FakeSystem>>>,
    host: Arc<TestHost>,
    port: u16,
    release_url: String,
}

impl UpdateLauncher for HelperProcess {
    fn launch(&mut self) -> Result<(), String> {
        self.launches.fetch_add(1, Ordering::SeqCst);
        let Some(release) = self.release.lock().unwrap().take() else {
            return Ok(());
        };
        let host = self.host.clone();
        let running_system = self.running_system.lock().unwrap().take();
        let (port, release_url) = (self.port, self.release_url.clone());
        let helper = std::thread::spawn(move || {
            let _ = release.recv();
            let mut system = running_system.unwrap_or_else(|| FakeSystem::ready(&host.home.0));
            system.service_port = Some(port);
            let location = InstalledService::new(host.environment(), system).with_start_timeout(Duration::from_secs(3));
            // 套用時已確認重啟後的服務在聽埠且回報新版。
            let applied = Updater::new(HttpReleaseSource::at(&release_url), location).apply();
            assert!(applied.is_ok(), "{applied:?}");
        });
        *self.finished.lock().unwrap() = Some(helper);
        Ok(())
    }
}

/// 已安裝 0.5.0、背景服務執行中的主機，加上連著它的主機服務(時鐘由測試推進)。
struct ManagedHost {
    host: Arc<TestHost>,
    server: FakeReleaseServer,
    helper: HelperProcess,
    service: TestHostService,
    clock: Arc<Mutex<SystemTime>>,
    _herdr: FakeHerdr,
}

impl ManagedHost {
    async fn start() -> ManagedHost {
        let host = Arc::new(TestHost::new(PLATFORM));
        for name in executable_names(PLATFORM) {
            std::fs::write(host.downloads.0.join(name), fake_host_service(INSTALLED, name)).unwrap();
        }
        let port = reserved_port();
        let mut system = FakeSystem::ready(&host.home.0);
        system.service_port = Some(port);
        let mut installer = Installer::new(host.environment(), system);
        installer.install(&host.options(port), |_| true).unwrap();
        let running_system = installer.into_system();

        let server = FakeReleaseServer::start();
        let helper = HelperProcess {
            launches: Arc::default(),
            release: Arc::default(),
            finished: Arc::default(),
            running_system: Arc::new(Mutex::new(Some(running_system))),
            host: host.clone(),
            port,
            release_url: server.url(),
        };
        let clock = Arc::new(Mutex::new(SystemTime::now()));
        let management = {
            let (home, release_url, launcher, now) = (host.home.0.clone(), server.url(), helper.clone(), clock.clone());
            InstalledServiceManagement::new(
                host.environment(),
                move || FakeSystem::ready(&home),
                move || HttpReleaseSource::at(&release_url),
                move |_: &InstalledService<FakeSystem>| Box::new(launcher.clone()) as Box<dyn UpdateLauncher>,
            )
            .with_clock(move || *now.lock().unwrap())
        };
        let herdr = FakeHerdr::start(vec![("ping", recorded("ping.ndjson"))]).await;
        let service = start_service_with_management(herdr.socket.clone(), Some(ServiceManagement::new(management))).await;
        ManagedHost {
            host,
            server,
            helper,
            service,
            clock,
            _herdr: herdr,
        }
    }

    fn advance_clock(&self, elapsed: Duration) {
        *self.clock.lock().unwrap() += elapsed;
    }

    async fn client(&self, read_only: bool) -> TestClient {
        let mut client = if read_only {
            let device = self.service.pair_device("唯讀裝置", true);
            TestClient::connect_with_token(&self.service, Some(&device.device_token)).await
        } else {
            TestClient::connect(&self.service).await
        };
        let welcome = client.request(json!({"type": "hello", "request_id": "h", "protocol_version": 1})).await;
        assert_eq!(welcome["type"], "welcome", "{welcome}");
        client
    }

    fn location(&self) -> InstalledService<FakeSystem> {
        InstalledService::new(self.host.environment(), FakeSystem::ready(&self.host.home.0))
    }

    fn install_record(&self) -> Value {
        serde_json::from_str(&std::fs::read_to_string(self.host.service_directory().join("install.json")).unwrap()).unwrap()
    }
}

fn status(request_id: &str) -> Value {
    json!({"type": "host_service.status", "request_id": request_id})
}

#[tokio::test]
async fn status_reports_installed_and_latest_versions_and_auto_update() {
    let managed = ManagedHost::start().await;
    publish(&managed.server, "0.5.1", "");
    let mut client = managed.client(true).await;

    let reply = client.request(status("s1")).await;

    assert_eq!(
        reply,
        json!({
            "type": "host_service.status.result",
            "request_id": "s1",
            "installed_version": INSTALLED,
            "latest_version": "0.5.1",
            "update_available": true,
            "auto_update": true
        })
    );
}

#[tokio::test]
async fn status_after_a_rolled_back_update_shows_the_failure_and_the_rolled_back_version() {
    let managed = ManagedHost::start().await;
    publish(&managed.server, "0.5.1", "broken");
    let running_system = managed.helper.running_system.lock().unwrap().take().unwrap();
    let location = InstalledService::new(managed.host.environment(), running_system).with_start_timeout(Duration::from_secs(3));
    assert!(Updater::new(HttpReleaseSource::at(&managed.server.url()), location).apply().is_err());
    let mut client = managed.client(false).await;

    let reply = client.request(status("s2")).await;

    assert_eq!(reply["installed_version"], INSTALLED, "{reply}");
    assert_eq!(reply["rolled_back"], "0.5.1", "{reply}");
    let last_update = reply["last_update"]["result"].as_str().unwrap();
    assert!(last_update.starts_with("update failed: ") && last_update.contains("0.5.1"), "{last_update}");
    assert!(reply["last_update"]["checked_at"].as_str().unwrap().ends_with('Z'), "{reply}");
}

#[tokio::test]
async fn status_without_release_source_still_reports_the_installed_version() {
    let managed = ManagedHost::start().await;
    let mut client = managed.client(false).await;

    let reply = client.request(status("s3")).await;

    assert_eq!(reply["type"], "host_service.status.result", "{reply}");
    assert_eq!(reply["installed_version"], INSTALLED);
    assert_eq!(reply["update_available"], false);
    assert!(reply.get("latest_version").is_none(), "{reply}");
    assert!(reply["latest_error"].as_str().is_some_and(|message| !message.is_empty()), "{reply}");
}

#[tokio::test]
async fn status_reuses_the_latest_version_for_fifteen_minutes() {
    let managed = ManagedHost::start().await;
    publish(&managed.server, "0.5.1", "");
    let mut client = managed.client(false).await;

    let first = client.request(status("c1")).await;
    publish(&managed.server, "0.5.2", "");
    managed.advance_clock(Duration::from_secs(14 * 60));
    let cached = client.request(status("c2")).await;
    managed.advance_clock(Duration::from_secs(2 * 60));
    let refreshed = client.request(status("c3")).await;

    assert_eq!(first["latest_version"], "0.5.1", "{first}");
    assert_eq!(cached["latest_version"], "0.5.1", "15 分鐘內不重查：{cached}");
    assert_eq!(refreshed["latest_version"], "0.5.2", "過期後重查：{refreshed}");
}

#[tokio::test]
async fn update_queries_the_latest_version_instead_of_the_cached_one() {
    let managed = ManagedHost::start().await;
    publish(&managed.server, "0.5.1", "");
    let mut client = managed.client(false).await;
    client.request(status("c4")).await;
    publish(&managed.server, "0.5.2", "");

    let reply = client.request(update("c5")).await;

    assert_eq!(reply["to_version"], "0.5.2", "{reply}");
}

fn update(request_id: &str) -> Value {
    json!({"type": "host_service.update", "request_id": request_id})
}

fn installed_executable(managed: &ManagedHost) -> String {
    std::fs::read_to_string(managed.host.service_directory().join("bin").join(executable_names(PLATFORM)[0])).unwrap()
}

#[tokio::test]
async fn update_replies_before_the_restart_and_the_helper_installs_the_new_version() {
    let managed = ManagedHost::start().await;
    publish(&managed.server, "0.5.1", "");
    let (allow_helper, helper_allowed) = mpsc::channel();
    *managed.helper.release.lock().unwrap() = Some(helper_allowed);
    let mut client = managed.client(false).await;

    let reply = client.request(update("u1")).await;

    assert_eq!(
        reply,
        json!({"type": "host_service.update.result", "request_id": "u1", "outcome": "started", "from_version": INSTALLED, "to_version": "0.5.1"})
    );
    assert_eq!(managed.helper.launches.load(Ordering::SeqCst), 1);
    assert!(installed_executable(&managed).starts_with("fake-host-service 0.5.0 "), "回應時還沒替換、沒重啟");
    allow_helper.send(()).unwrap();
    let helper = managed.helper.finished.lock().unwrap().take().unwrap();
    tokio::task::spawn_blocking(move || helper.join().unwrap()).await.unwrap();
    let after = client.request(status("u2")).await;
    assert_eq!(after["installed_version"], "0.5.1", "{after}");
    assert_eq!(after["last_update"]["result"], "updated 0.5.0 -> 0.5.1", "{after}");
}

#[tokio::test]
async fn update_when_up_to_date_starts_nothing() {
    let managed = ManagedHost::start().await;
    publish(&managed.server, INSTALLED, "");
    let mut client = managed.client(false).await;

    let reply = client.request(update("u3")).await;

    assert_eq!(
        reply,
        json!({"type": "host_service.update.result", "request_id": "u3", "outcome": "up_to_date", "from_version": INSTALLED, "to_version": INSTALLED})
    );
    assert_eq!(managed.helper.launches.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn update_while_another_update_runs_is_in_progress() {
    let managed = ManagedHost::start().await;
    publish(&managed.server, "0.5.1", "");
    let mut other_update = managed.location();
    let held = other_update.lock().unwrap();
    let mut client = managed.client(false).await;

    let reply = client.request(update("u4")).await;

    assert_eq!(reply["code"], "update_in_progress", "{reply}");
    assert_eq!(managed.helper.launches.load(Ordering::SeqCst), 0);
    drop(held);
    let retried = client.request(update("u5")).await;
    assert_eq!(retried["outcome"], "started", "鎖放開後可以更新：{retried}");
}

#[tokio::test]
async fn second_update_before_the_helper_takes_the_lock_is_in_progress() {
    let managed = ManagedHost::start().await;
    publish(&managed.server, "0.5.1", "");
    let (allow_helper, helper_allowed) = mpsc::channel();
    *managed.helper.release.lock().unwrap() = Some(helper_allowed);
    let mut client = managed.client(false).await;

    let first = client.request(update("d1")).await;
    let second = client.request(update("d2")).await;

    assert_eq!(first["outcome"], "started", "{first}");
    assert_eq!(second["code"], "update_in_progress", "{second}");
    assert_eq!(managed.helper.launches.load(Ordering::SeqCst), 1);
    allow_helper.send(()).unwrap();
    let helper = managed.helper.finished.lock().unwrap().take().unwrap();
    tokio::task::spawn_blocking(move || helper.join().unwrap()).await.unwrap();
    publish(&managed.server, "0.5.2", "");
    let after_helper = client.request(update("d3")).await;
    assert_eq!(after_helper["outcome"], "started", "更新程序接手後不再擋：{after_helper}");
}

#[tokio::test]
async fn update_reservation_expires_when_the_helper_never_takes_over() {
    let managed = ManagedHost::start().await;
    publish(&managed.server, "0.5.1", "");
    let mut client = managed.client(false).await;

    let first = client.request(update("e1")).await;
    let blocked = client.request(update("e2")).await;
    managed.advance_clock(Duration::from_secs(3 * 60));
    let after_expiry = client.request(update("e3")).await;

    assert_eq!(first["outcome"], "started", "{first}");
    assert_eq!(blocked["code"], "update_in_progress", "{blocked}");
    assert_eq!(after_expiry["outcome"], "started", "更新程序一直沒接手，標記過期後可再試：{after_expiry}");
    assert_eq!(managed.helper.launches.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn update_without_release_source_fails_and_starts_nothing() {
    let managed = ManagedHost::start().await;
    let mut client = managed.client(false).await;

    let reply = client.request(update("u6")).await;

    assert_eq!(reply["code"], "update_failed", "{reply}");
    assert_eq!(managed.helper.launches.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn read_only_device_cannot_update_and_the_attempt_is_audited() {
    let managed = ManagedHost::start().await;
    publish(&managed.server, "0.5.1", "");
    let mut client = managed.client(true).await;

    let reply = client.request(update("u7")).await;

    assert_eq!(reply["code"], "read_only_device", "{reply}");
    assert_eq!(managed.helper.launches.load(Ordering::SeqCst), 0);
    let audit = managed.service.audit_log();
    assert!(
        audit
            .lines()
            .any(|line| line.contains("host_service.update") && line.contains("\"allowed\":false")),
        "{audit}"
    );
}

fn set_auto_update(request_id: &str, enabled: bool) -> Value {
    json!({"type": "host_service.set_auto_update", "request_id": request_id, "enabled": enabled})
}

#[tokio::test]
async fn auto_update_can_be_turned_off_and_on_and_the_status_follows() {
    let managed = ManagedHost::start().await;
    publish(&managed.server, INSTALLED, "");
    let mut client = managed.client(false).await;

    let turned_off = client.request(set_auto_update("a1", false)).await;
    let status_when_off = client.request(status("a2")).await;
    let record_when_off = managed.install_record();
    let turned_on = client.request(set_auto_update("a3", true)).await;

    assert_eq!(
        turned_off,
        json!({"type": "host_service.set_auto_update.result", "request_id": "a1", "auto_update": false})
    );
    assert_eq!(status_when_off["auto_update"], false, "{status_when_off}");
    assert_eq!(record_when_off["auto_update"], false, "{record_when_off}");
    assert_eq!(turned_on["auto_update"], true, "{turned_on}");
    assert_eq!(managed.install_record()["auto_update"], true);
}

#[tokio::test]
async fn read_only_device_cannot_change_auto_update() {
    let managed = ManagedHost::start().await;
    let mut client = managed.client(true).await;

    let reply = client.request(set_auto_update("a4", false)).await;

    assert_eq!(reply["code"], "read_only_device", "{reply}");
    assert_eq!(managed.install_record()["auto_update"], true);
}

#[tokio::test]
async fn only_the_installed_copy_offers_management_to_the_app() {
    let managed = ManagedHost::start().await;
    let mut from_installed_copy = managed.host.environment();
    from_installed_copy.current_executable = managed.host.service_directory().join("bin").join(executable_names(PLATFORM)[0]);
    let not_installed_host = TestHost::new(PLATFORM);

    assert!(remote_management::for_installed_copy(from_installed_copy).is_some());
    assert!(
        remote_management::for_installed_copy(managed.host.environment()).is_none(),
        "下載資料夾裡的那份"
    );
    assert!(remote_management::for_installed_copy(not_installed_host.environment()).is_none());
}

#[tokio::test]
async fn host_service_that_is_not_the_installed_copy_cannot_be_managed() {
    let herdr = FakeHerdr::start(vec![("ping", recorded("ping.ndjson"))]).await;
    let not_installed_host = TestHost::new(PLATFORM);
    let home = not_installed_host.home.0.clone();
    let not_installed = InstalledServiceManagement::new(
        not_installed_host.environment(),
        move || FakeSystem::ready(&home),
        || HttpReleaseSource::at("http://127.0.0.1:9"),
        |_: &InstalledService<FakeSystem>| -> Box<dyn UpdateLauncher> { panic!("沒有安裝時不啟動更新程序") },
    );
    let services = [
        start_service_with_management(herdr.socket.clone(), None).await,
        start_service_with_management(herdr.socket.clone(), Some(ServiceManagement::new(not_installed))).await,
    ];

    for service in &services {
        let mut client = TestClient::connect(service).await;
        client.request(json!({"type": "hello", "request_id": "h", "protocol_version": 1})).await;
        for request in [
            status("n1"),
            json!({"type": "host_service.update", "request_id": "n1"}),
            json!({"type": "host_service.set_auto_update", "request_id": "n1", "enabled": false}),
        ] {
            let reply = client.request(request.clone()).await;
            assert_eq!(reply["code"], "host_service_not_installed", "{request}: {reply}");
        }
    }
}
