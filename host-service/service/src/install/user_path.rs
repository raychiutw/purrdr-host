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
    if let Some(updated) = with_entry(&current, &entry, &layout.home) {
        if updated.chars().count() > MAX_ENVIRONMENT_VALUE_LENGTH {
            return Err(InstallError::Io(format!(
                "cannot add {entry} to the user PATH: it would be longer than {MAX_ENVIRONMENT_VALUE_LENGTH} characters; shorten PATH or install with --no-path"
            )));
        }
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
            let updated = without_entry(&current, &entry, &layout.home);
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

/// 去掉標記區塊：只移除開始與結束標記都在的區塊(結束標記配最近的開始標記);缺結束標記時整段保留。
fn without_block(contents: &str) -> String {
    let lines: Vec<&str> = contents.split_inclusive('\n').collect();
    let mut removed = vec![false; lines.len()];
    let mut open_block: Option<usize> = None;
    for (index, line) in lines.iter().enumerate() {
        match line.trim_end_matches(['\r', '\n']) {
            BLOCK_START => open_block = Some(index),
            BLOCK_END => {
                if let Some(start) = open_block.take() {
                    removed[start..=index].iter_mut().for_each(|flag| *flag = true);
                }
            }
            _ => {}
        }
    }
    lines.iter().zip(removed).filter(|(_, removed)| !removed).map(|(line, _)| *line).collect()
}

fn path_error(error: &std::io::Error) -> InstallError {
    InstallError::Io(format!("cannot update the user PATH: {error}"))
}

/// Windows 環境變數值的上限(字元)。
const MAX_ENVIRONMENT_VALUE_LENGTH: usize = 32_767;

/// 同一個資料夾：展開 `%變數%` 後不分大小寫、忽略結尾的 `\`(Windows 路徑)。
fn same_entry(existing: &str, entry: &str, home: &Path) -> bool {
    let expanded = expand_environment(existing, home);
    expanded.trim_end_matches('\\').eq_ignore_ascii_case(entry.trim_end_matches('\\'))
}

/// 展開 `%名稱%`:`USERPROFILE` 用安裝的家目錄，其他用目前行程的環境變數;不認得的保留原樣。
fn expand_environment(value: &str, home: &Path) -> String {
    let mut expanded = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(start) = rest.find('%') {
        let Some(length) = rest[start + 1..].find('%') else { break };
        let name = &rest[start + 1..start + 1 + length];
        let replacement = if name.eq_ignore_ascii_case("USERPROFILE") {
            Some(home.display().to_string())
        } else {
            std::env::var(name).ok().filter(|_| !name.is_empty())
        };
        expanded.push_str(&rest[..start]);
        match replacement {
            Some(replacement) => expanded.push_str(&replacement),
            None => expanded.push_str(&rest[start..start + length + 2]),
        }
        rest = &rest[start + length + 2..];
    }
    expanded.push_str(rest);
    expanded
}

/// 在 `;` 分隔的 PATH 最後加上 `entry`;已經有了回 None。
fn with_entry(path: &str, entry: &str, home: &Path) -> Option<String> {
    if path.split(';').any(|existing| same_entry(existing, entry, home)) {
        return None;
    }
    Some(match path {
        "" => entry.to_owned(),
        _ if path.ends_with(';') => format!("{path}{entry}"),
        _ => format!("{path};{entry}"),
    })
}

fn without_entry(path: &str, entry: &str, home: &Path) -> String {
    path.split(';')
        .filter(|existing| !same_entry(existing, entry, home))
        .collect::<Vec<_>>()
        .join(";")
}
