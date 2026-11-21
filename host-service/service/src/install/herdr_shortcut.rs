//! 安裝時在 herdr 的 config.toml 加一個快捷鍵(raychiutw/purrdr.host#47):在 herdr 裡按 `prefix+alt+p`,
//! 用 popup 執行安裝好的 `purrdr-host-service pair --wait` 顯示配對 QR code。
//!
//! herdr 的快捷鍵只能寫在使用者的 config.toml(plugin 不能自己綁按鍵),所以用與 PATH 相同的標記區塊，
//! 解除安裝時移除(ADR-0010 完整還原)。按鍵已被使用者用掉時不寫入，只給警告。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::user_path::{BLOCK_END, BLOCK_START, read_if_exists, without_block};
use super::{HostEnvironment, InstallError, InstallPlatform};

/// 顯示配對 QR code 的快捷鍵。
pub(super) const KEY: &str = "prefix+alt+p";

/// 安裝對 herdr config 做的變更，解除安裝依它還原。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ShortcutChange {
    /// 加了區塊的 herdr config.toml(絕對路徑)。
    #[serde(default)]
    pub config_file: Option<PathBuf>,
    /// config.toml 是安裝時新建的(移除區塊後是空的就刪掉)。
    #[serde(default)]
    pub config_file_created: bool,
}

/// 加快捷鍵的結果。
pub(super) enum Outcome {
    Added,
    /// 沒加，附上給使用者的說明。
    Skipped(String),
}

/// herdr 讀的 config.toml:`HERDR_CONFIG_PATH`,其次 `XDG_CONFIG_HOME/herdr`(各平台都看),
/// 再來 Windows 的 `APPDATA\herdr`(沒有就 `%USERPROFILE%\AppData\Roaming\herdr`)、macOS／Linux 的 `~/.config/herdr`。
pub(super) fn config_file(environment: &HostEnvironment) -> PathBuf {
    if let Some(file) = &environment.herdr_config_file {
        return file.clone();
    }
    let directory = match (&environment.xdg_config_home, environment.platform) {
        (Some(config_home), _) => config_home.join("herdr"),
        (None, InstallPlatform::Windows) => environment
            .app_data
            .clone()
            .unwrap_or_else(|| environment.home.join("AppData").join("Roaming"))
            .join("herdr"),
        (None, _) => environment.home.join(".config").join("herdr"),
    };
    directory.join("config.toml")
}

/// 在 `file` 加(或換掉)快捷鍵區塊;呼叫前 `file` 的資料夾要已存在。
pub(super) fn add(file: &Path, executable: &Path, platform: InstallPlatform, change: &mut ShortcutChange) -> Result<Outcome, InstallError> {
    if change.config_file.as_deref().is_some_and(|previous| previous != file) {
        remove(change)?;
    }
    let existing = read_if_exists(file)?;
    let mut contents = without_block(existing.as_deref().unwrap_or_default());
    let skipped = |reason: String| Outcome::Skipped(format!("the herdr shortcut for pairing was not added: {reason}"));
    let Ok(config) = contents.parse::<toml::Table>() else {
        remove(change)?;
        return Ok(skipped(format!("{} is not valid TOML", file.display())));
    };
    if binds_key(&config) {
        remove(change)?;
        return Ok(skipped(format!("{KEY} is already bound in {}", file.display())));
    }
    if !contents.is_empty() && !contents.ends_with('\n') {
        contents.push('\n');
    }
    contents.push_str(&shortcut_block(&pair_command(executable, platform)));
    if contents.parse::<toml::Table>().is_err() {
        remove(change)?;
        return Ok(skipped(format!(
            "adding it would make {} invalid (keys.command is defined differently there)",
            file.display()
        )));
    }
    if change.config_file.is_none() {
        change.config_file_created = existing.is_none();
    }
    std::fs::write(file, contents).map_err(InstallError::io)?;
    change.config_file = Some(file.to_path_buf());
    Ok(Outcome::Added)
}

/// `[keys]` 裡是否已有綁到 `KEY` 的按鍵:內建動作的綁定值(字串或字串陣列)與 `[[keys.command]]` 的 `key`。
/// herdr 解析按鍵不分大小寫;說明文字與註解不算。
fn binds_key(config: &toml::Table) -> bool {
    let Some(keys) = config.get("keys").and_then(toml::Value::as_table) else {
        return false;
    };
    let is_key = |value: &toml::Value| value.as_str().is_some_and(|text| text.trim().eq_ignore_ascii_case(KEY));
    keys.iter().any(|(name, value)| match (name.as_str(), value) {
        ("command", toml::Value::Array(commands)) => commands.iter().any(|command| command.get("key").is_some_and(is_key)),
        (_, toml::Value::Array(bindings)) => bindings.iter().any(is_key),
        (_, binding) => is_key(binding),
    })
}

/// 移除快捷鍵區塊;config.toml 是安裝時新建、移除後是空的就刪掉。
pub(super) fn remove(change: &mut ShortcutChange) -> Result<(), InstallError> {
    let Some(file) = change.config_file.take() else {
        return Ok(());
    };
    if let Some(existing) = read_if_exists(&file)? {
        let contents = without_block(&existing);
        if contents.is_empty() && change.config_file_created {
            std::fs::remove_file(&file).map_err(InstallError::io)?;
        } else if contents != existing {
            std::fs::write(&file, contents).map_err(InstallError::io)?;
        }
    }
    change.config_file_created = false;
    Ok(())
}

/// herdr 以 shell 執行自訂指令：macOS／Linux 是 `$SHELL -c`(單引號包住路徑),Windows 是 `cmd.exe /d /c`(雙引號)。
fn pair_command(executable: &Path, platform: InstallPlatform) -> String {
    let path = executable.display().to_string();
    let quoted = match platform {
        InstallPlatform::Windows => format!("\"{path}\""),
        _ => format!("'{}'", path.replace('\'', r"'\''")),
    };
    format!("{quoted} pair --wait")
}

fn shortcut_block(command: &str) -> String {
    let command = command.replace('\\', r"\\").replace('"', r#"\""#);
    format!(
        "{BLOCK_START}\n[[keys.command]]\nkey = \"{KEY}\"\ntype = \"popup\"\ncommand = \"{command}\"\ndescription = \"Purrdr: pair a device\"\nwidth = \"80%\"\nheight = \"80%\"\n{BLOCK_END}\n"
    )
}
