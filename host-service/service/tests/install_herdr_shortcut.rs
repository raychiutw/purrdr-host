//! 安裝時在 herdr 的 config.toml 加一個快捷鍵，在 herdr 裡用 popup 顯示配對 QR code(raychiutw/purrdr.host#47);
//! 解除安裝時移除，維持完整還原(ADR-0010)。

mod install_support;

use install_support::{FakeSystem, TestHost};

use purrdr_host_service::install::{HostEnvironment, InstallPlatform, Installer};

const PORT: u16 = 47_820;
const BLOCK_START: &str = "# >>> purrdr-host-service >>>";
const BLOCK_END: &str = "# <<< purrdr-host-service <<<";

/// TOML basic string 內的反斜線與雙引號要跳脫。
fn toml_escaped(text: &str) -> String {
    text.replace('\\', r"\\").replace('"', r#"\""#)
}

fn installed_executable(host: &TestHost) -> String {
    let name = if host.platform == InstallPlatform::Windows {
        "purrdr-host-service.exe"
    } else {
        "purrdr-host-service"
    };
    host.service_directory().join("bin").join(name).display().to_string()
}

fn expected_block(command: &str) -> String {
    format!(
        "{BLOCK_START}\n[[keys.command]]\nkey = \"prefix+alt+p\"\ntype = \"popup\"\ncommand = \"{}\"\ndescription = \"Purrdr: pair a device\"\nwidth = \"80%\"\nheight = \"80%\"\n{BLOCK_END}\n",
        toml_escaped(command)
    )
}

fn install(environment: HostEnvironment, host: &TestHost) -> Installer<FakeSystem> {
    let mut installer = Installer::new(environment, FakeSystem::ready(&host.home.0));
    installer.install(&host.options(PORT), |_| true).unwrap();
    installer
}

#[test]
fn macos_and_linux_bind_a_popup_running_pair_wait_in_the_herdr_config() {
    for platform in [InstallPlatform::MacOs, InstallPlatform::Linux] {
        let host = TestHost::new(platform);
        let config = host.home.0.join(".config/herdr/config.toml");

        install(host.environment(), &host);

        let command = format!("'{}' pair --wait", installed_executable(&host));
        assert_eq!(std::fs::read_to_string(&config).unwrap(), expected_block(&command), "{platform:?}");
    }
}

#[test]
fn windows_binds_the_popup_in_appdata_with_a_cmd_quoted_executable() {
    let host = TestHost::new(InstallPlatform::Windows);
    let app_data = host.home.0.join("AppData").join("Roaming");
    let mut environment = host.environment();
    environment.app_data = Some(app_data.clone());

    install(environment, &host);

    let command = format!("\"{}\" pair --wait", installed_executable(&host));
    let config = app_data.join("herdr").join("config.toml");
    assert_eq!(std::fs::read_to_string(config).unwrap(), expected_block(&command));
}

#[test]
fn herdr_config_location_follows_herdr_config_path_then_xdg_config_home() {
    let host = TestHost::new(InstallPlatform::MacOs);
    let custom = host.home.0.join("dotfiles/herdr.toml");
    std::fs::create_dir_all(custom.parent().unwrap()).unwrap();
    let mut environment = host.environment();
    environment.herdr_config_file = Some(custom.clone());
    environment.xdg_config_home = Some(host.home.0.join("xdg"));
    install(environment, &host);
    assert!(std::fs::read_to_string(&custom).unwrap().contains("prefix+alt+p"));

    let host = TestHost::new(InstallPlatform::Windows);
    let mut environment = host.environment();
    environment.xdg_config_home = Some(host.home.0.join("xdg"));
    environment.app_data = Some(host.home.0.join("AppData").join("Roaming"));
    install(environment, &host);
    assert!(host.home.0.join("xdg/herdr/config.toml").exists(), "herdr 在 Windows 也先看 XDG_CONFIG_HOME");
}

#[test]
fn existing_herdr_config_is_kept_installing_again_does_not_duplicate_and_uninstall_restores_it() {
    let host = TestHost::new(InstallPlatform::MacOs);
    let config = host.home.0.join(".config/herdr/config.toml");
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    let before = "onboarding = false\n\n[ui.toast]\ndelivery = \"terminal\"";
    std::fs::write(&config, before).unwrap();

    let mut installer = install(host.environment(), &host);
    installer.install(&host.options(PORT), |_| true).unwrap();

    let command = format!("'{}' pair --wait", installed_executable(&host));
    assert_eq!(std::fs::read_to_string(&config).unwrap(), format!("{before}\n{}", expected_block(&command)));
    installer.uninstall().unwrap();
    assert_eq!(std::fs::read_to_string(&config).unwrap(), format!("{before}\n"));
}

#[test]
fn uninstall_removes_the_herdr_config_and_folders_it_created() {
    let host = TestHost::new(InstallPlatform::Linux);
    let mut installer = install(host.environment(), &host);
    assert!(host.home.0.join(".config/herdr/config.toml").exists());

    installer.uninstall().unwrap();

    assert!(!host.home.0.join(".config/herdr").exists());
    assert!(!host.home.0.join(".config").exists());
}

#[test]
fn shortcut_already_used_by_the_user_is_left_alone_with_a_warning() {
    let host = TestHost::new(InstallPlatform::MacOs);
    let config = host.home.0.join(".config/herdr/config.toml");
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    let before = "[[keys.command]]\nkey = \"prefix+alt+p\"\ntype = \"popup\"\ncommand = \"htop\"\n";
    std::fs::write(&config, before).unwrap();
    let mut installer = Installer::new(host.environment(), FakeSystem::ready(&host.home.0));

    let installed = installer.install(&host.options(PORT), |_| true).unwrap();

    assert_eq!(std::fs::read_to_string(&config).unwrap(), before);
    assert_eq!(installed.herdr_shortcut, None);
    assert!(
        installed.warnings.iter().any(|warning| warning.contains("prefix+alt+p")),
        "{:?}",
        installed.warnings
    );
    installer.uninstall().unwrap();
    assert_eq!(std::fs::read_to_string(&config).unwrap(), before);
}

#[test]
fn install_message_tells_the_herdr_shortcut() {
    let host = TestHost::new(InstallPlatform::Linux);
    let mut installer = Installer::new(host.environment(), FakeSystem::ready(&host.home.0));

    let installed = installer.install(&host.options(PORT), |_| true).unwrap();

    assert_eq!(installed.herdr_shortcut.as_deref(), Some("prefix+alt+p"));
    assert!(installed.to_string().contains("In herdr, press prefix+alt+p"), "{installed}");
}

fn install_with_config(host: &TestHost, before: &str) -> (purrdr_host_service::install::Installed, String) {
    let config = host.home.0.join(".config/herdr/config.toml");
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    std::fs::write(&config, before).unwrap();
    let mut installer = Installer::new(host.environment(), FakeSystem::ready(&host.home.0));
    let installed = installer.install(&host.options(PORT), |_| true).unwrap();
    (installed, std::fs::read_to_string(&config).unwrap())
}

#[test]
fn key_written_in_another_case_counts_as_already_bound() {
    let host = TestHost::new(InstallPlatform::MacOs);
    let before = "[[keys.command]]\nkey = \"Prefix+Alt+P\"\ntype = \"popup\"\ncommand = \"htop\"\n";

    let (installed, after) = install_with_config(&host, before);

    assert_eq!(after, before, "herdr 解析按鍵不分大小寫");
    assert_eq!(installed.herdr_shortcut, None);
}

#[test]
fn key_mentioned_only_in_a_comment_or_description_is_not_a_conflict() {
    let host = TestHost::new(InstallPlatform::MacOs);
    let before =
        "# 以前用過 prefix+alt+p\n[[keys.command]]\nkey = \"prefix+alt+g\"\ntype = \"popup\"\ncommand = \"lazygit\"\ndescription = \"not prefix+alt+p\"\n";

    let (installed, after) = install_with_config(&host, before);

    assert_eq!(installed.herdr_shortcut.as_deref(), Some("prefix+alt+p"));
    assert!(after.starts_with(before), "{after}");
}

#[test]
fn config_where_the_block_would_break_toml_is_left_alone_with_a_warning() {
    let host = TestHost::new(InstallPlatform::MacOs);
    // 以 inline array 定義 keys.command 時再接 [[keys.command]] 是重複定義，整份 config 會讀不了。
    let before = "[keys]\ncommand = [{ key = \"prefix+alt+g\", type = \"popup\", command = \"lazygit\" }]\n";

    let (installed, after) = install_with_config(&host, before);

    assert_eq!(after, before);
    assert_eq!(installed.herdr_shortcut, None);
    assert!(installed.warnings.iter().any(|warning| warning.contains("herdr")), "{:?}", installed.warnings);
}

#[test]
fn herdr_config_that_cannot_be_written_does_not_fail_the_install() {
    let host = TestHost::new(InstallPlatform::MacOs);
    let blocker = host.home.0.join("not-a-folder");
    std::fs::write(&blocker, "file").unwrap();
    let mut environment = host.environment();
    environment.herdr_config_file = Some(blocker.join("config.toml"));
    let mut installer = Installer::new(environment, FakeSystem::ready(&host.home.0));

    let installed = installer.install(&host.options(PORT), |_| true).unwrap();

    assert_eq!(installed.herdr_shortcut, None);
    assert!(installed.warnings.iter().any(|warning| warning.contains("herdr")), "{:?}", installed.warnings);
    installer.uninstall().unwrap();
}
