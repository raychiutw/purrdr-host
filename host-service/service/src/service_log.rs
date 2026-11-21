//! 主機服務的 log(raychiutw/purrdr.host#42):服務自己寫進主機服務資料夾的 `logs/host-service.log`,
//! 不依賴 launchd、systemd 或 Windows 的輸出轉接,三個平台的位置相同,解除安裝時隨資料夾一起移除。

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

/// 服務 log 的大小上限;超過就輪替成 `.1`。
pub const SERVICE_LOG_MAXIMUM_BYTES: u64 = 1024 * 1024;

/// 同一個程序裡所有 log 共用一把鎖:寫入與輪替不會交錯。
static WRITE_LOCK: Mutex<()> = Mutex::new(());

/// 逐行寫入、超過上限輪替成 `.1` 的 log 檔(只留一份舊檔)。
#[derive(Debug, Clone)]
pub struct RotatingLog {
    path: PathBuf,
    maximum_bytes: u64,
}

impl RotatingLog {
    pub fn new(path: impl Into<PathBuf>, maximum_bytes: u64) -> Self {
        RotatingLog {
            path: path.into(),
            maximum_bytes,
        }
    }

    /// 寫入一行。這一行放不下(寫了會超過上限)而檔案不是空的，就先把檔案改名為 `.1`(覆蓋舊的)再寫。
    pub fn append_line(&self, line: &str) -> io::Result<()> {
        let _guard = WRITE_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(folder) = self.path.parent() {
            std::fs::create_dir_all(folder)?;
        }
        let size = std::fs::metadata(&self.path).map_or(0, |metadata| metadata.len());
        if size > 0 && size + line.len() as u64 + 1 > self.maximum_bytes {
            let mut rotated = self.path.clone().into_os_string();
            rotated.push(".1");
            std::fs::rename(&self.path, rotated)?;
        }
        let mut file = OpenOptions::new().create(true).append(true).open(&self.path)?;
        writeln!(file, "{line}")
    }
}

/// 主機服務自己的 log:`<主機服務資料夾>/logs/host-service.log`。
#[derive(Debug, Clone)]
pub struct ServiceLog {
    file: RotatingLog,
}

impl ServiceLog {
    pub fn under(state_directory: &Path) -> Self {
        ServiceLog {
            file: RotatingLog::new(state_directory.join("logs").join("host-service.log"), SERVICE_LOG_MAXIMUM_BYTES),
        }
    }

    /// 記一行(開頭是時間)。同時輸出到 stderr:前景執行時看得到;安裝的服務沒有人接 stderr,不會重複。
    /// 寫不進檔案時只剩 stderr,不能因為記 log 讓服務出錯。
    pub fn record(&self, message: &str) {
        let line = format!("{} {message}", humantime::format_rfc3339_seconds(SystemTime::now()));
        eprintln!("{line}");
        if let Err(error) = self.file.append_line(&line) {
            eprintln!("cannot write service log {}: {error}", self.file.path.display());
        }
    }
}
