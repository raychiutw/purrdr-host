//! `bin/` 裡的指令別名 `purrdr`(raychiutw/purrdr.host#53):打 `purrdr pair` 等同 `purrdr-host-service pair`。
//!
//! 執行檔不改名(自動更新、Release 壓縮檔、自動啟動都依賴原名)。macOS／Linux 是指向同資料夾執行檔的
//! 相對 symlink,自動更新換檔後仍指到新版;Windows 是轉呼叫同資料夾 exe 的 `purrdr.cmd`。
//! 別名在服務資料夾裡，解除安裝刪整個資料夾時一起移除。已有不是我們放的 `purrdr` 時不碰。

use std::path::{Path, PathBuf};

use super::{InstallPlatform, ServiceLayout};

/// 別名的指令名稱。
pub const NAME: &str = "purrdr";

/// 放別名的結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandAlias {
    /// 新建(或換掉我們放的舊別名)。
    Created,
    /// 已經是正確的別名，沒有動。
    Present,
    /// 這個位置已有別的檔案，沒有動。
    Taken(PathBuf),
}

/// Windows `purrdr.cmd` 的第一行：有這行的就是我們放的(內容不同時重寫，讓之後的版本能換掉它)。
const WINDOWS_MARKER: &str = "@rem purrdr-host-service command alias";

/// Windows 的別名內容。沒帶參數時印出可用指令並以 2 結束(只打 `purrdr` 不該在前景啟動服務)。
/// 最後一行直接執行 exe、不接 `& exit /b`:不帶結束碼的 `exit /b` 會把錯誤碼蓋成 0。
/// `purrdr uninstall` 刪掉 `bin/` 後 cmd.exe 可能印出 "The batch file cannot be found",
/// 在 Windows PC 實測同一行接 `exit /b`、`call exit` 或 `if errorlevel` 都避不掉，只能接受。
fn windows_script(executable_name: &str) -> String {
    format!(
        "{WINDOWS_MARKER}\r\n\
         @if \"%~1\"==\"\" \"%~dp0{executable_name}\" help & exit /b 2\r\n\
         @\"%~dp0{executable_name}\" %*\r\n"
    )
}

pub(super) fn file(layout: &ServiceLayout) -> PathBuf {
    match layout.platform {
        InstallPlatform::Windows => layout.bin_directory().join(format!("{NAME}.cmd")),
        _ => layout.bin_directory().join(NAME),
    }
}

/// 確保 `bin/` 有指向執行檔的別名;呼叫前 `bin/` 要已存在。
pub(super) fn ensure(layout: &ServiceLayout) -> std::io::Result<CommandAlias> {
    let alias = file(layout);
    let executable_name = layout.executable_name();
    match layout.platform {
        InstallPlatform::Windows => {
            let script = windows_script(executable_name);
            match std::fs::read_to_string(&alias) {
                Ok(existing) if existing == script => Ok(CommandAlias::Present),
                Ok(existing) if existing.starts_with(WINDOWS_MARKER) => {
                    std::fs::write(&alias, script)?;
                    Ok(CommandAlias::Created)
                }
                Ok(_) => Ok(CommandAlias::Taken(alias)),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    std::fs::write(&alias, script)?;
                    Ok(CommandAlias::Created)
                }
                Err(_) => Ok(CommandAlias::Taken(alias)),
            }
        }
        _ => ensure_link(&alias, executable_name),
    }
}

/// 只有指向同資料夾 `executable_name` 的相對 symlink 算我們的;其他東西(含指向別處的同名執行檔)不碰。
/// 相對 symlink 只要執行檔在就不會壞，所以不需要重建。
fn ensure_link(alias: &Path, executable_name: &str) -> std::io::Result<CommandAlias> {
    match std::fs::read_link(alias) {
        Ok(target) if target == Path::new(executable_name) => Ok(CommandAlias::Present),
        _ if alias.symlink_metadata().is_ok() => Ok(CommandAlias::Taken(alias.to_path_buf())),
        _ => {
            link(executable_name, alias)?;
            Ok(CommandAlias::Created)
        }
    }
}

#[cfg(unix)]
fn link(target: &str, alias: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, alias)
}

/// 只有在 Windows 上模擬 macOS／Linux 安裝(測試)才會走到。
#[cfg(not(unix))]
fn link(_target: &str, _alias: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "symlinks need macOS or Linux"))
}
