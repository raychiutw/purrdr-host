//! 主機服務的更新(raychiutw/purrdr.host#6、#7):假 Release 伺服器加暫存家目錄與假系統，三平台流程都在每個 CI 平台跑。

mod install_support;

use std::time::Duration;

use install_support::release::{FakeReleaseServer, sha256_hex};
use install_support::{FakeSystem, TestHost, executable_names, fake_host_service, snapshot};
use purrdr_host_service::install::{InstallPlatform, InstalledService, Installer};
use purrdr_host_service::update::{AutomaticOutcome, HttpReleaseSource, UpdateError, UpdateLauncher, UpdateOutcome, Updater, Version};

const PLATFORMS: [InstallPlatform; 3] = [InstallPlatform::MacOs, InstallPlatform::Linux, InstallPlatform::Windows];
const INSTALLED: &str = "0.5.0";

fn version(text: &str) -> Version {
    text.parse().unwrap()
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

fn is_listening(port: u16) -> bool {
    std::net::TcpStream::connect(("127.0.0.1", port)).is_ok()
}

/// 已安裝 0.5.0、背景服務執行中的測試主機。
struct InstalledHost {
    host: TestHost,
    port: u16,
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
        std::fs::write(host.service_directory().join("devices.json"), r#"{"devices":["paired"]}"#).unwrap();
        let system = installer.into_system();
        assert!(is_listening(port), "安裝後背景服務要在聽");
        InstalledHost {
            host,
            port,
            system: Some(system),
        }
    }

    fn updater(&mut self, server: &FakeReleaseServer) -> Updater<HttpReleaseSource, InstalledService<FakeSystem>> {
        let location = InstalledService::new(self.host.environment(), self.system.take().unwrap()).with_start_timeout(Duration::from_secs(3));
        Updater::new(HttpReleaseSource::at(&server.url()), location)
    }

    fn installed_executable(&self, name: &str) -> String {
        std::fs::read_to_string(self.host.service_directory().join("bin").join(name)).unwrap()
    }

    /// 執行檔以外的設定與已配對裝置。
    fn settings(&self) -> Vec<String> {
        ["install.json", "config.json", "devices.json"]
            .iter()
            .map(|name| std::fs::read_to_string(self.host.service_directory().join(name)).unwrap())
            .collect()
    }
}

/// 家目錄快照，不含更新結果的紀錄(`update.json`、`logs/update.log`)。
fn home_without_update_record(host: &TestHost) -> std::collections::BTreeMap<String, Option<String>> {
    let mut entries = snapshot(&host.home.0);
    entries.retain(|path, _| !path.ends_with(".purrdr/host-service/update.json") && !path.ends_with(".purrdr/host-service/logs/update.log"));
    entries
}

/// 記下被要求啟動更新程序幾次的假啟動器。
#[derive(Default)]
struct FakeLauncher {
    launches: usize,
    fails: bool,
}

impl UpdateLauncher for FakeLauncher {
    fn launch(&mut self) -> Result<(), String> {
        self.launches += 1;
        if self.fails { Err("spawn failed".into()) } else { Ok(()) }
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
fn newer_release_replaces_executables_restarts_service_and_keeps_settings_and_devices() {
    for platform in PLATFORMS {
        let mut installed = InstalledHost::new(platform);
        let settings_before = installed.settings();
        let server = FakeReleaseServer::start();
        publish(&server, platform, "0.5.1", "");
        let mut updater = installed.updater(&server);

        let outcome = updater.apply().unwrap();

        assert_eq!(
            outcome,
            UpdateOutcome::Updated {
                from: version(INSTALLED),
                to: version("0.5.1")
            },
            "{platform:?}"
        );
        for name in executable_names(platform) {
            assert!(
                installed.installed_executable(name).starts_with("fake-host-service 0.5.1 "),
                "{platform:?} {name}"
            );
        }
        assert_eq!(installed.settings(), settings_before, "{platform:?}");
        assert!(is_listening(installed.port), "{platform:?}:更新後背景服務要重新啟動");
        let check = updater.check().unwrap();
        assert_eq!(
            (check.installed, check.latest.clone()),
            (Some(version("0.5.1")), Ok(version("0.5.1"))),
            "{platform:?}"
        );
        assert!(!check.update_available(), "{platform:?}");
    }
}

#[test]
fn same_or_older_release_changes_nothing() {
    for platform in PLATFORMS {
        for latest in [INSTALLED, "0.4.1"] {
            let mut installed = InstalledHost::new(platform);
            let server = FakeReleaseServer::start();
            publish(&server, platform, latest, "");
            let home_before = home_without_update_record(&installed.host);
            let mut updater = installed.updater(&server);

            let outcome = updater.apply().unwrap();

            assert_eq!(
                outcome,
                UpdateOutcome::UpToDate {
                    installed: version(INSTALLED),
                    latest: version(latest)
                },
                "{platform:?}"
            );
            assert_eq!(home_without_update_record(&installed.host), home_before, "{platform:?} {latest}");
            assert_eq!(server.requests(), ["/releases/latest"], "{platform:?} {latest}:不下載");
        }
    }
}

#[test]
fn checksum_mismatch_replaces_nothing() {
    for platform in PLATFORMS {
        let mut installed = InstalledHost::new(platform);
        let server = FakeReleaseServer::start();
        publish(&server, platform, "0.5.1", "");
        let other_archive = format!("{}  {}\n", sha256_hex(b"something else"), install_support::release::asset_name(platform));
        server.put("releases/download/host-service-v0.5.1/SHA256SUMS", other_archive.into_bytes());
        let home_before = home_without_update_record(&installed.host);
        let mut updater = installed.updater(&server);

        let error = updater.apply().unwrap_err();

        assert!(matches!(error, UpdateError::ChecksumMismatch { .. }), "{platform:?}: {error:?}");
        assert!(error.explain("purrdr-host-service update").contains("Next:"), "{error}");
        assert_eq!(home_without_update_record(&installed.host), home_before, "{platform:?}");
        assert!(is_listening(installed.port), "{platform:?}");
    }
}

#[test]
fn new_version_that_does_not_start_is_rolled_back() {
    for platform in PLATFORMS {
        let mut installed = InstalledHost::new(platform);
        let settings_before = installed.settings();
        let server = FakeReleaseServer::start();
        publish(&server, platform, "0.5.1", "broken");
        let mut updater = installed.updater(&server);

        let error = updater.apply().unwrap_err();

        assert!(
            matches!(&error, UpdateError::RolledBack { failed, restored, .. } if *failed == version("0.5.1") && *restored == version(INSTALLED)),
            "{platform:?}: {error:?}"
        );
        assert!(error.explain("purrdr-host-service update").contains("Next:"), "{error}");
        for name in executable_names(platform) {
            assert!(
                installed.installed_executable(name).starts_with("fake-host-service 0.5.0 "),
                "{platform:?} {name}"
            );
        }
        assert_eq!(installed.settings(), settings_before, "{platform:?}");
        assert!(is_listening(installed.port), "{platform:?}:退回後舊版要在跑");
        assert_eq!(updater.check().unwrap().installed, Some(version(INSTALLED)), "{platform:?}");
    }
}

#[test]
fn unreachable_release_server_is_a_network_error_and_changes_nothing() {
    for platform in PLATFORMS {
        let mut installed = InstalledHost::new(platform);
        let server = FakeReleaseServer::start();
        let home_before = home_without_update_record(&installed.host);
        let mut updater = installed.updater(&server);
        drop(server);

        let error = updater.apply().unwrap_err();

        assert!(matches!(error, UpdateError::Network(_)), "{platform:?}: {error:?}");
        assert!(error.explain("purrdr-host-service update").contains("Next:"), "{error}");
        assert_eq!(home_without_update_record(&installed.host), home_before, "{platform:?}");
    }
}

#[test]
fn check_keeps_installed_version_when_release_server_is_unreachable() {
    let mut installed = InstalledHost::new(InstallPlatform::Linux);
    let server = FakeReleaseServer::start();
    let mut updater = installed.updater(&server);
    drop(server);

    let check = updater.check().unwrap();

    assert_eq!(check.installed, Some(version(INSTALLED)));
    assert!(matches!(check.latest, Err(UpdateError::Network(_))), "{check:?}");
    assert!(!check.update_available());
}

#[test]
fn missing_release_files_are_a_network_error() {
    let mut installed = InstalledHost::new(InstallPlatform::Linux);
    let server = FakeReleaseServer::start();
    server.put("releases/latest", br#"{"tag_name":"host-service-v0.5.1"}"#.to_vec());
    let home_before = home_without_update_record(&installed.host);

    let error = installed.updater(&server).apply().unwrap_err();

    assert!(matches!(error, UpdateError::Network(_)), "{error:?}");
    assert_eq!(home_without_update_record(&installed.host), home_before);
}

#[test]
fn macos_release_without_matching_signature_is_not_installed() {
    let mut installed = InstalledHost::new(InstallPlatform::MacOs);
    let server = FakeReleaseServer::start();
    publish(&server, InstallPlatform::MacOs, "0.5.1", "unsigned");
    let home_before = home_without_update_record(&installed.host);
    let mut updater = installed.updater(&server);

    let error = updater.apply().unwrap_err();

    assert!(matches!(error, UpdateError::Signature(_)), "{error:?}");
    assert!(error.explain("purrdr-host-service update").contains("Next:"), "{error}");
    assert_eq!(home_without_update_record(&installed.host), home_before);
    assert!(is_listening(installed.port));
}

#[test]
fn check_reports_installed_and_latest_versions_without_changes() {
    let mut installed = InstalledHost::new(InstallPlatform::Linux);
    let server = FakeReleaseServer::start();
    publish(&server, InstallPlatform::Linux, "0.10.0", "");
    let home_before = home_without_update_record(&installed.host);

    let check = installed.updater(&server).check().unwrap();

    assert_eq!((check.installed, check.latest.clone()), (Some(version(INSTALLED)), Ok(version("0.10.0"))));
    assert!(check.update_available());
    assert_eq!(home_without_update_record(&installed.host), home_before);
}

#[test]
fn update_without_install_says_how_to_install() {
    let host = TestHost::new(InstallPlatform::Linux);
    let server = FakeReleaseServer::start();
    publish(&server, InstallPlatform::Linux, "0.5.1", "");
    let location = InstalledService::new(host.environment(), FakeSystem::ready(&host.home.0));
    let mut updater = Updater::new(HttpReleaseSource::at(&server.url()), location);

    let check = updater.check().unwrap();
    let error = updater.apply().unwrap_err();

    assert_eq!((check.installed, check.latest), (None, Ok(version("0.5.1"))));
    assert_eq!(error, UpdateError::NotInstalled);
    assert!(error.explain("purrdr-host-service update").contains("Next:"), "{error}");
    assert_eq!(std::fs::read_dir(&host.home.0).unwrap().count(), 0);
}

#[test]
fn versions_compare_numerically_and_parse_release_tags() {
    assert!(version("0.10.0") > version("0.9.9"));
    assert!(version("1.0.0") > version("0.99.99"));
    assert_eq!(version("0.5.0"), version("0.5.0"));
    assert_eq!(Version::from_tag("host-service-v0.5.1"), Some(version("0.5.1")));
    assert_eq!(Version::from_tag("v0.5.1"), None);
    for invalid in ["", "0.5", "0.5.x", "0.5.1-beta", "a.b.c"] {
        assert!(invalid.parse::<Version>().is_err(), "{invalid}");
    }
    assert_eq!(version("0.5.1").to_string(), "0.5.1");
}

#[test]
fn service_that_cannot_be_restarted_at_all_asks_to_reinstall_with_one_next_step() {
    for platform in PLATFORMS {
        let mut installed = InstalledHost::new(platform);
        installed.system.as_mut().unwrap().autostart_fails = true;
        let server = FakeReleaseServer::start();
        publish(&server, platform, "0.5.1", "");
        let mut updater = installed.updater(&server);

        let error = updater.apply().unwrap_err();

        assert!(matches!(error, UpdateError::RollbackFailed { .. }), "{platform:?}: {error:?}");
        let message = error.explain("purrdr-host-service update");
        assert_eq!(message.matches("Next:").count(), 1, "{message}");
        assert!(message.contains("install"), "{message}");
    }
}

#[test]
fn rollback_message_has_one_next_step() {
    let mut installed = InstalledHost::new(InstallPlatform::Linux);
    let server = FakeReleaseServer::start();
    publish(&server, InstallPlatform::Linux, "0.5.1", "broken");
    let mut updater = installed.updater(&server);

    let message = updater.apply().unwrap_err().explain("purrdr-host-service update");

    assert_eq!(message.matches("Next:").count(), 1, "{message}");
}

#[test]
fn applying_records_the_result_and_the_check_reports_it() {
    let mut installed = InstalledHost::new(InstallPlatform::Linux);
    let server = FakeReleaseServer::start();
    publish(&server, InstallPlatform::Linux, "0.5.1", "");
    let mut updater = installed.updater(&server);
    assert_eq!(updater.check().unwrap().last_result, None);

    updater.apply().unwrap();

    let check = updater.check().unwrap();
    assert_eq!(check.auto_update, Some(true));
    assert_eq!(check.last_result.unwrap().result, "updated 0.5.0 -> 0.5.1");
    let log = std::fs::read_to_string(installed.host.service_directory().join("logs/update.log")).unwrap();
    assert!(log.contains("updated 0.5.0 -> 0.5.1"), "{log}");
}

#[test]
fn automatic_check_launches_the_update_when_a_newer_release_exists() {
    for platform in PLATFORMS {
        let mut installed = InstalledHost::new(platform);
        let server = FakeReleaseServer::start();
        publish(&server, platform, "0.5.1", "");
        let mut updater = installed.updater(&server);
        let mut launcher = FakeLauncher::default();

        let outcome = updater.automatic(&mut launcher);

        assert_eq!(
            outcome,
            AutomaticOutcome::Started {
                from: version(INSTALLED),
                to: version("0.5.1")
            },
            "{platform:?}"
        );
        assert_eq!(launcher.launches, 1, "{platform:?}");
        assert!(
            installed
                .installed_executable(executable_names(platform)[0])
                .starts_with("fake-host-service 0.5.0 "),
            "由更新程序替換，不在服務裡換"
        );
        assert_eq!(updater.check().unwrap().last_result.unwrap().result, "update to 0.5.1 started");
    }
}

#[test]
fn automatic_check_does_nothing_when_auto_update_is_off_but_manual_update_still_works() {
    let mut installed = InstalledHost::new(InstallPlatform::Linux);
    let mut installer = Installer::new(installed.host.environment(), FakeSystem::ready(&installed.host.home.0));
    installer.set_auto_update(false).unwrap();
    let server = FakeReleaseServer::start();
    publish(&server, InstallPlatform::Linux, "0.5.1", "");
    let mut updater = installed.updater(&server);
    let mut launcher = FakeLauncher::default();

    let outcome = updater.automatic(&mut launcher);

    assert_eq!(outcome, AutomaticOutcome::Disabled);
    assert_eq!(launcher.launches, 0);
    assert!(server.requests().is_empty(), "關閉時不連網路：{:?}", server.requests());
    assert_eq!(updater.check().unwrap().auto_update, Some(false));
    assert!(matches!(updater.apply(), Ok(UpdateOutcome::Updated { .. })));
}

#[test]
fn automatic_check_when_up_to_date_or_offline_launches_nothing_and_records_the_result() {
    let mut installed = InstalledHost::new(InstallPlatform::Linux);
    let server = FakeReleaseServer::start();
    publish(&server, InstallPlatform::Linux, INSTALLED, "");
    let mut updater = installed.updater(&server);
    let mut launcher = FakeLauncher::default();

    let up_to_date = updater.automatic(&mut launcher);
    assert_eq!(up_to_date, AutomaticOutcome::UpToDate(version(INSTALLED)));
    assert_eq!(updater.check().unwrap().last_result.unwrap().result, "up to date (0.5.0)");
    drop(server);
    let offline = updater.automatic(&mut launcher);

    assert!(matches!(offline, AutomaticOutcome::Failed(UpdateError::Network(_))), "{offline:?}");
    assert_eq!(launcher.launches, 0);
    let last = updater.check().unwrap().last_result.unwrap().result;
    assert!(last.starts_with("check failed: "), "{last}");
}

#[test]
fn automatic_check_reports_a_launcher_that_cannot_start() {
    let mut installed = InstalledHost::new(InstallPlatform::Windows);
    let server = FakeReleaseServer::start();
    publish(&server, InstallPlatform::Windows, "0.5.1", "");
    let mut updater = installed.updater(&server);
    let mut launcher = FakeLauncher {
        fails: true,
        ..FakeLauncher::default()
    };

    let outcome = updater.automatic(&mut launcher);

    assert!(matches!(outcome, AutomaticOutcome::Failed(UpdateError::Io(_))), "{outcome:?}");
}

#[test]
fn only_the_installed_copy_runs_automatic_updates() {
    let installed = InstalledHost::new(InstallPlatform::Linux);
    let mut from_installed_copy = installed.host.environment();
    from_installed_copy.current_executable = installed.host.service_directory().join("bin").join("purrdr-host-service");

    let downloaded = InstalledService::new(installed.host.environment(), FakeSystem::ready(&installed.host.home.0));
    let in_place = InstalledService::new(from_installed_copy, FakeSystem::ready(&installed.host.home.0));
    let not_installed_host = TestHost::new(InstallPlatform::Linux);
    let not_installed = InstalledService::new(not_installed_host.environment(), FakeSystem::ready(&not_installed_host.home.0));

    assert!(!downloaded.runs_installed_copy());
    assert!(in_place.runs_installed_copy());
    assert!(!not_installed.runs_installed_copy());
}
