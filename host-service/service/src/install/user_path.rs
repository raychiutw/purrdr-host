//! 安裝時把 `bin/` 加進使用者層級 PATH(raychiutw/purrdr.host#9),不需系統管理員:
//! Windows 寫 `HKCU\Environment` 的 `Path`,macOS／Linux 在 shell 設定檔加以標記註解包住的區塊。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::system::HostSystem;
use super::{InstallError, InstallPlatform, ServiceLayout};

const BLOCK_START: &str = "# >>> purrdr-host-service >>>";
const BLOCK_END: &str = "# <<< purrdr-host-service <<<";

/// 安裝對 PATH 做的變更，解除安裝依它還原。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct PathChange {
    /// Windows:`bin/` 是安裝加進使用者 PATH 的(原本就有的不算)。
    #[serde(default)]
    pub user_path_entry: bool,
    /// macOS／Linux:加了區塊的 shell 設定檔(相對家目錄)。
    #[serde(default)]
    pub shell_profile: Option<String>,
    /// shell 設定檔是安裝時新建的(移除區塊後是空的就刪掉)。
    #[serde(default)]
    pub shell_profile_created: bool,
}

/// macOS／Linux 要加 PATH 的 shell 設定檔(依使用者的 shell);Windows 回 None。
pub(super) fn shell_profile(layout: &ServiceLayout, shell: Option<&str>) -> Option<PathBuf> {
    let shell_name = shell.and_then(|shell| Path::new(shell).file_name()).and_then(|name| name.to_str());
    match (layout.platform, shell_name) {
        (InstallPlatform::Windows, _) => None,
        (_, Some("zsh")) => Some(layout.home.join(".zshrc")),
        (InstallPlatform::MacOs, Some("bash")) => Some(layout.home.join(".bash_profile")),
        (_, Some("bash")) => Some(layout.home.join(".bashrc")),
        (_, Some("fish")) => {
            let config_home = layout.xdg_config_home.clone().unwrap_or_else(|| layout.home.join(".config"));
            Some(config_home.join("fish").join("config.fish"))
        }
        _ => Some(layout.home.join(".profile")),
    }
}

/// 把 `bin/` 加進使用者 PATH:Windows 已經有了就不動;shell 設定檔的區塊重跑安裝時換掉、不重複加。
pub(super) fn add(system: &mut impl HostSystem, layout: &ServiceLayout, shell: Option<&str>, change: &mut PathChange) -> Result<(), InstallError> {
    let Some(profile) = shell_profile(layout, shell) else {
        return add_user_path_entry(system, layout, change);
    };
    let relative = relative_to_home(layout, &profile);
    if change.shell_profile.as_ref().is_some_and(|previous| *previous != relative) {
        remove_shell_block(layout, change)?;
    }
    let existing = read_if_exists(&profile)?;
    if change.shell_profile.is_none() {
        change.shell_profile_created = existing.is_none();
    }
    let mut contents = without_block(&existing.unwrap_or_default());
    if !contents.is_empty() && !contents.ends_with('\n') {
        contents.push('\n');
    }
    let fish = profile.extension().is_some_and(|extension| extension == "fish");
    contents.push_str(&path_block(&layout.bin_directory().display().to_string(), fish));
    std::fs::write(&profile, contents).map_err(InstallError::io)?;
    change.shell_profile = Some(relative);
    Ok(())
}

fn add_user_path_entry(system: &mut impl HostSystem, layout: &ServiceLayout, change: &mut PathChange) -> Result<(), InstallError> {
    let entry = layout.bin_directory().display().to_string();
    let current = system.user_path().map_err(|error| path_error(&error))?.unwrap_or_default();
    if let Some(updated) = with_entry(&current, &entry) {
        system.set_user_path(Some(&updated)).map_err(|error| path_error(&error))?;
        change.user_path_entry = true;
    }
    Ok(())
}

/// 移除安裝加的 PATH 設定;PATH 因此變成空的就刪掉這個值。
pub(super) fn remove(system: &mut impl HostSystem, layout: &ServiceLayout, change: &mut PathChange) -> Result<(), InstallError> {
    remove_shell_block(layout, change)?;
    if change.user_path_entry {
        let entry = layout.bin_directory().display().to_string();
        if let Some(current) = system.user_path().map_err(|error| path_error(&error))? {
            let updated = without_entry(&current, &entry);
            if updated != current {
                system
                    .set_user_path(Some(&updated).filter(|updated| !updated.is_empty()).map(String::as_str))
                    .map_err(|error| path_error(&error))?;
            }
        }
        change.user_path_entry = false;
    }
    Ok(())
}

/// 移除 shell 設定檔的區塊;設定檔是安裝時新建、移除後是空的就刪掉。
fn remove_shell_block(layout: &ServiceLayout, change: &mut PathChange) -> Result<(), InstallError> {
    let Some(relative) = change.shell_profile.take() else {
        return Ok(());
    };
    let profile = layout.home.join(&relative);
    if let Some(existing) = read_if_exists(&profile)? {
        let contents = without_block(&existing);
        if contents.is_empty() && change.shell_profile_created {
            std::fs::remove_file(&profile).map_err(InstallError::io)?;
        } else if contents != existing {
            std::fs::write(&profile, contents).map_err(InstallError::io)?;
        }
    }
    change.shell_profile_created = false;
    Ok(())
}

fn read_if_exists(file: &Path) -> Result<Option<String>, InstallError> {
    match std::fs::read_to_string(file) {
        Ok(contents) => Ok(Some(contents)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(InstallError::io(error)),
    }
}

fn relative_to_home(layout: &ServiceLayout, file: &Path) -> String {
    file.strip_prefix(&layout.home).unwrap_or(file).to_string_lossy().replace('\\', "/")
}

/// 以標記註解包住的 PATH 設定(fish 與 POSIX shell 語法不同;值放在雙引號內並跳脫)。
fn path_block(bin: &str, fish: bool) -> String {
    let escaped = bin.replace('\\', r"\\").replace('"', r#"\""#).replace('$', r"\$");
    let setting = if fish {
        format!("set -gx PATH \"{escaped}\" $PATH")
    } else {
        format!("export PATH=\"{}:$PATH\"", escaped.replace('`', r"\`"))
    };
    format!("{BLOCK_START}\n{setting}\n{BLOCK_END}\n")
}

/// 去掉標記區塊(從開始那行到結束那行)。
fn without_block(contents: &str) -> String {
    let mut kept = String::with_capacity(contents.len());
    let mut inside = false;
    for line in contents.split_inclusive('\n') {
        let text = line.trim_end_matches(['\r', '\n']);
        if !inside && text == BLOCK_START {
            inside = true;
        } else if inside {
            inside = text != BLOCK_END;
        } else {
            kept.push_str(line);
        }
    }
    kept
}

fn path_error(error: &std::io::Error) -> InstallError {
    InstallError::Io(format!("cannot update the user PATH: {error}"))
}

/// 同一個資料夾：不分大小寫、忽略結尾的 `\`(Windows 路徑)。
fn same_entry(first: &str, second: &str) -> bool {
    first.trim_end_matches('\\').eq_ignore_ascii_case(second.trim_end_matches('\\'))
}

/// 在 `;` 分隔的 PATH 最後加上 `entry`;已經有了回 None。
fn with_entry(path: &str, entry: &str) -> Option<String> {
    if path.split(';').any(|existing| same_entry(existing, entry)) {
        return None;
    }
    Some(match path {
        "" => entry.to_owned(),
        _ if path.ends_with(';') => format!("{path}{entry}"),
        _ => format!("{path};{entry}"),
    })
}

fn without_entry(path: &str, entry: &str) -> String {
    path.split(';').filter(|existing| !same_entry(existing, entry)).collect::<Vec<_>>().join(";")
}
