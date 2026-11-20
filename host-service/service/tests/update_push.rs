//! 更新事件推播(raychiutw/purrdr.host#22):更新流程的結果經 `~/.purrdr/push-<裝置>/pairing` 送到後台。
//! 假 Release 伺服器、假後台、暫存家目錄與假系統。

mod install_support;

use std::path::PathBuf;
use std::time::Duration;

use install_support::push_backend::FakePushBackend;
use install_support::release::FakeReleaseServer;
use install_support::{FakeSystem, TestHost, executable_names, fake_host_service, snapshot};
use purrdr_host_service::install::{InstallPlatform, InstalledService, Installer};
use purrdr_host_service::update::{AgentActivity, AutomaticOutcome, HttpReleaseSource, UpdateError, UpdateLauncher, UpdateOutcome, Updater};
use serde_json::json;

const INSTALLED: &str = "0.5.0";
const PHONE_KEY: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const TABLET_KEY: &str = "2222222222222222222222222222222222222222222222222222222222222222";

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

/// 已安裝 0.5.0、背景服務執行中的測試主機。
struct InstalledHost {
    host: TestHost,
    system: Option<FakeSystem>,
}

impl InstalledHost {
    fn new(platform: InstallPlatform) -> InstalledHost {
        let host = TestHost::new(platform);
        for name in executable_names(platform) {
            std::fs::write(host.downloads.0.join(name), fake_host_service(INSTALLED, name)).unwrap();
        }
        let port = free_port();
        let mut system = FakeSystem::ready(&host.home.0);
        system.service_port = Some(port);
        let mut installer = Installer::new(host.environment(), system);
        installer.install(&host.options(port), |_| true).unwrap();
        let mut system = installer.into_system();
        system.commands.clear();
        InstalledHost { host, system: Some(system) }
    }

    fn updater(&mut self, server: &FakeReleaseServer) -> Updater<HttpReleaseSource, InstalledService<FakeSystem>> {
        let location = InstalledService::new(self.host.environment(), self.system.take().unwrap()).with_start_timeout(Duration::from_secs(3));
        Updater::new(HttpReleaseSource::at(&server.url()), location)
    }

    fn push_directory(&self, device: &str) -> PathBuf {
        self.host.home.0.join(".purrdr").join(format!("push-{device}"))
    }

    /// 與 app／推播外掛代裝寫的格式相同。
    fn pair_device(&self, device: &str, endpoint: &str, key: &str) {
        let directory = self.push_directory(device);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("journal.json"), r#"{"entries":[],"version":1}"#).unwrap();
        std::fs::write(directory.join("pairing"), format!("purrdr-push 1\nendpoint={endpoint}\nkey={key}\n")).unwrap();
    }
}

#[derive(Default)]
struct FakeLauncher {
    launches: usize,
}

impl UpdateLauncher for FakeLauncher {
    fn launch(&mut self) -> Result<(), String> {
        self.launches += 1;
        Ok(())
    }
}

/// 固定回報有沒有 `working` Agent。
struct FakeAgents {
    working: bool,
}

impl AgentActivity for FakeAgents {
    fn has_working_agent(&mut self) -> bool {
        self.working
    }
}

fn publish(server: &FakeReleaseServer, platform: InstallPlatform, version: &str, traits: &str) {
    let executables: Vec<(&str, String)> = executable_names(platform)
        .into_iter()
        .map(|name| (name, fake_host_service(version, &format!("{name} {traits}"))))
        .collect();
    server.publish(platform, version, &executables);
}

#[test]
fn manual_update_sends_updated_event_with_the_new_version_to_every_paired_device() {
    let mut installed = InstalledHost::new(InstallPlatform::Linux);
    let backend = FakePushBackend::start();
    installed.pair_device("phone", &backend.endpoint(), PHONE_KEY);
    installed.pair_device("tablet", &backend.endpoint(), TABLET_KEY);
    let server = FakeReleaseServer::start();
    publish(&server, InstallPlatform::Linux, "0.5.1", "");

    let outcome = installed.updater(&server).apply();

    assert!(matches!(outcome, Ok(UpdateOutcome::Updated { .. })), "{outcome:?}");
    let mut received = backend.received();
    received.sort_by(|left, right| left.1["key"].as_str().cmp(&right.1["key"].as_str()));
    assert_eq!(
        received,
        vec![
            (
                "/v1/push".to_owned(),
                json!({"key": PHONE_KEY, "event": "host_service_updated", "version": "0.5.1"})
            ),
            (
                "/v1/push".to_owned(),
                json!({"key": TABLET_KEY, "event": "host_service_updated", "version": "0.5.1"})
            ),
        ]
    );
}

#[test]
fn rolled_back_update_sends_rolled_back_event_with_the_failed_version() {
    let mut installed = InstalledHost::new(InstallPlatform::Linux);
    let backend = FakePushBackend::start();
    installed.pair_device("phone", &backend.endpoint(), PHONE_KEY);
    let server = FakeReleaseServer::start();
    publish(&server, InstallPlatform::Linux, "0.5.1", "broken");

    let outcome = installed.updater(&server).apply();

    assert!(matches!(outcome, Err(UpdateError::RolledBack { .. })), "{outcome:?}");
    assert_eq!(
        backend.bodies(),
        vec![json!({"key": PHONE_KEY, "event": "host_service_update_rolled_back", "version": "0.5.1"})]
    );
}

#[test]
fn automatic_check_that_finds_a_newer_release_sends_update_available_event() {
    let mut installed = InstalledHost::new(InstallPlatform::MacOs);
    let backend = FakePushBackend::start();
    installed.pair_device("phone", &backend.endpoint(), PHONE_KEY);
    let server = FakeReleaseServer::start();
    publish(&server, InstallPlatform::MacOs, "0.5.1", "");
    let mut launcher = FakeLauncher::default();

    let outcome = installed.updater(&server).automatic(&mut launcher, &mut FakeAgents { working: false });

    assert!(matches!(outcome, AutomaticOutcome::Started { .. }), "{outcome:?}");
    assert_eq!(launcher.launches, 1);
    assert_eq!(
        backend.bodies(),
        vec![json!({"key": PHONE_KEY, "event": "host_service_update_available", "version": "0.5.1"})]
    );
}

#[test]
fn automatic_update_deferred_because_an_agent_is_working_still_sends_update_available_event() {
    let mut installed = InstalledHost::new(InstallPlatform::Linux);
    let backend = FakePushBackend::start();
    installed.pair_device("phone", &backend.endpoint(), PHONE_KEY);
    let server = FakeReleaseServer::start();
    publish(&server, InstallPlatform::Linux, "0.5.1", "");
    let mut launcher = FakeLauncher::default();

    let outcome = installed.updater(&server).automatic(&mut launcher, &mut FakeAgents { working: true });

    assert!(matches!(outcome, AutomaticOutcome::Deferred { .. }), "{outcome:?}");
    assert_eq!(launcher.launches, 0);
    assert_eq!(
        backend.bodies(),
        vec![json!({"key": PHONE_KEY, "event": "host_service_update_available", "version": "0.5.1"})]
    );
}

#[test]
fn the_same_event_and_version_is_sent_only_once_but_a_newer_version_is_sent_again() {
    let mut installed = InstalledHost::new(InstallPlatform::Linux);
    let backend = FakePushBackend::start();
    installed.pair_device("phone", &backend.endpoint(), PHONE_KEY);
    let server = FakeReleaseServer::start();
    publish(&server, InstallPlatform::Linux, "0.5.1", "");
    let mut updater = installed.updater(&server);
    let mut launcher = FakeLauncher::default();

    updater.automatic(&mut launcher, &mut FakeAgents { working: false });
    updater.automatic(&mut launcher, &mut FakeAgents { working: false });
    publish(&server, InstallPlatform::Linux, "0.5.2", "");
    updater.automatic(&mut launcher, &mut FakeAgents { working: false });

    assert_eq!(launcher.launches, 3);
    assert_eq!(
        backend.bodies(),
        vec![
            json!({"key": PHONE_KEY, "event": "host_service_update_available", "version": "0.5.1"}),
            json!({"key": PHONE_KEY, "event": "host_service_update_available", "version": "0.5.2"}),
        ]
    );
}

#[test]
fn revoked_pairing_answered_with_410_removes_only_that_device_folder() {
    let mut installed = InstalledHost::new(InstallPlatform::Linux);
    let backend = FakePushBackend::start();
    backend.respond_to_key(PHONE_KEY, 410);
    installed.pair_device("phone", &backend.endpoint(), PHONE_KEY);
    installed.pair_device("tablet", &backend.endpoint(), TABLET_KEY);
    let server = FakeReleaseServer::start();
    publish(&server, InstallPlatform::Linux, "0.5.1", "");

    let outcome = installed.updater(&server).apply();

    assert!(matches!(outcome, Ok(UpdateOutcome::Updated { .. })), "{outcome:?}");
    assert!(!installed.push_directory("phone").exists(), "410 的裝置要刪掉配對資料夾");
    assert!(installed.push_directory("tablet").join("pairing").is_file());
    assert_eq!(backend.bodies().len(), 2);
}

#[test]
fn unreachable_backend_does_not_change_the_update_result_and_is_retried_on_the_next_trigger() {
    let mut installed = InstalledHost::new(InstallPlatform::Linux);
    let backend = FakePushBackend::start();
    // 特權埠 1 不會有人聽;不用 free_port(),以免同時跑的測試拿到同一個埠。
    installed.pair_device("phone", "http://127.0.0.1:1/v1/push", PHONE_KEY);
    installed.pair_device("tablet", &backend.endpoint(), TABLET_KEY);
    let server = FakeReleaseServer::start();
    publish(&server, InstallPlatform::Linux, "0.5.1", "");
    let mut updater = installed.updater(&server);
    let mut launcher = FakeLauncher::default();

    let while_unreachable = updater.automatic(&mut launcher, &mut FakeAgents { working: false });
    installed.pair_device("phone", &backend.endpoint(), PHONE_KEY);
    updater.automatic(&mut launcher, &mut FakeAgents { working: false });

    assert!(matches!(while_unreachable, AutomaticOutcome::Started { .. }), "{while_unreachable:?}");
    assert_eq!(
        backend.bodies(),
        vec![
            json!({"key": TABLET_KEY, "event": "host_service_update_available", "version": "0.5.1"}),
            json!({"key": PHONE_KEY, "event": "host_service_update_available", "version": "0.5.1"}),
        ]
    );
}

#[test]
fn pairing_key_appears_in_no_command_argument_and_no_file_other_than_the_pairing() {
    let mut installed = InstalledHost::new(InstallPlatform::Linux);
    let backend = FakePushBackend::start();
    installed.pair_device("phone", &backend.endpoint(), PHONE_KEY);
    let server = FakeReleaseServer::start();
    publish(&server, InstallPlatform::Linux, "0.5.1", "broken");
    let mut updater = installed.updater(&server);

    let _ = updater.apply();
    updater.automatic(&mut FakeLauncher::default(), &mut FakeAgents { working: false });

    assert_eq!(backend.bodies().len(), 1, "{:?}", backend.bodies());
    let commands = &updater.location().system().commands;
    assert!(!commands.iter().any(|command| command.contains(PHONE_KEY)), "{commands:?}");
    let leaks: Vec<String> = snapshot(&installed.host.home.0)
        .into_iter()
        .filter(|(path, contents)| !path.ends_with("push-phone/pairing") && contents.as_deref().is_some_and(|text| text.contains(PHONE_KEY)))
        .map(|(path, _)| path)
        .collect();
    assert!(leaks.is_empty(), "{leaks:?}");
}

#[test]
fn automatic_check_that_finds_nothing_new_sends_nothing() {
    let mut installed = InstalledHost::new(InstallPlatform::Windows);
    let backend = FakePushBackend::start();
    installed.pair_device("phone", &backend.endpoint(), PHONE_KEY);
    let server = FakeReleaseServer::start();
    publish(&server, InstallPlatform::Windows, INSTALLED, "");

    let outcome = installed
        .updater(&server)
        .automatic(&mut FakeLauncher::default(), &mut FakeAgents { working: false });

    assert!(matches!(outcome, AutomaticOutcome::UpToDate(_)), "{outcome:?}");
    assert_eq!(backend.bodies(), Vec::<serde_json::Value>::new());
}
