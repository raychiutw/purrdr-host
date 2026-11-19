//! 主機服務的手動更新(raychiutw/purrdr.host#6):假 Release 伺服器加暫存家目錄與假系統，三平台流程都在每個 CI 平台跑。

mod install_support;

use std::time::Duration;

use install_support::release::{FakeReleaseServer, sha256_hex};
use install_support::{FakeSystem, TestHost, executable_names, fake_host_service, snapshot};
use purrdr_host_service::install::{InstallPlatform, InstalledService, Installer};
use purrdr_host_service::update::{HttpReleaseSource, UpdateError, UpdateOutcome, Updater, Version};

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
            let home_before = snapshot(&installed.host.home.0);
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
            assert_eq!(snapshot(&installed.host.home.0), home_before, "{platform:?} {latest}");
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
        let home_before = snapshot(&installed.host.home.0);
        let mut updater = installed.updater(&server);

        let error = updater.apply().unwrap_err();

        assert!(matches!(error, UpdateError::ChecksumMismatch { .. }), "{platform:?}: {error:?}");
        assert!(error.explain("purrdr-host-service update").contains("Next:"), "{error}");
        assert_eq!(snapshot(&installed.host.home.0), home_before, "{platform:?}");
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
        let home_before = snapshot(&installed.host.home.0);
        let mut updater = installed.updater(&server);
        drop(server);

        let error = updater.apply().unwrap_err();

        assert!(matches!(error, UpdateError::Network(_)), "{platform:?}: {error:?}");
        assert!(error.explain("purrdr-host-service update").contains("Next:"), "{error}");
        assert_eq!(snapshot(&installed.host.home.0), home_before, "{platform:?}");
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
    let home_before = snapshot(&installed.host.home.0);

    let error = installed.updater(&server).apply().unwrap_err();

    assert!(matches!(error, UpdateError::Network(_)), "{error:?}");
    assert_eq!(snapshot(&installed.host.home.0), home_before);
}

#[test]
fn macos_release_without_matching_signature_is_not_installed() {
    let mut installed = InstalledHost::new(InstallPlatform::MacOs);
    let server = FakeReleaseServer::start();
    publish(&server, InstallPlatform::MacOs, "0.5.1", "unsigned");
    let home_before = snapshot(&installed.host.home.0);
    let mut updater = installed.updater(&server);

    let error = updater.apply().unwrap_err();

    assert!(matches!(error, UpdateError::Signature(_)), "{error:?}");
    assert!(error.explain("purrdr-host-service update").contains("Next:"), "{error}");
    assert_eq!(snapshot(&installed.host.home.0), home_before);
    assert!(is_listening(installed.port));
}

#[test]
fn check_reports_installed_and_latest_versions_without_changes() {
    let mut installed = InstalledHost::new(InstallPlatform::Linux);
    let server = FakeReleaseServer::start();
    publish(&server, InstallPlatform::Linux, "0.10.0", "");
    let home_before = snapshot(&installed.host.home.0);

    let check = installed.updater(&server).check().unwrap();

    assert_eq!((check.installed, check.latest.clone()), (Some(version(INSTALLED)), Ok(version("0.10.0"))));
    assert!(check.update_available());
    assert_eq!(snapshot(&installed.host.home.0), home_before);
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
