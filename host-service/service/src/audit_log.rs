//! 稽核紀錄 `audit.log`(JSON Lines):配對、連線、撤銷、寫入類操作與主機取檔。不記 prompt、按鍵、終端機與檔案內容。

use std::path::PathBuf;
use std::time::SystemTime;

use serde::Serialize;

use crate::service_log::RotatingLog;

/// 一筆稽核事件。欄位只放識別資訊，不放操作內容。
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum AuditEvent {
    PairingCodeCreated {
        read_only: bool,
    },
    Paired {
        device_id: String,
        device_name: String,
        read_only: bool,
    },
    PairingFailed {
        reason: &'static str,
    },
    Connected {
        device_id: String,
    },
    ConnectionRejected {
        reason: &'static str,
        device_id: Option<String>,
    },
    Revoked {
        device_id: String,
        by: RevokedBy,
        /// `by` 是 `app` 時，發出撤銷的裝置。
        #[serde(skip_serializing_if = "Option::is_none")]
        by_device_id: Option<String>,
    },
    Write {
        device_id: String,
        operation: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        workspace_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pane_id: Option<String>,
        allowed: bool,
    },
    /// 主機取檔(raychiutw/purrdr.ios#227):路徑、結果(`FileStatus` 或 `changed`)與大小，不記內容。
    FileAccess {
        device_id: String,
        operation: &'static str,
        path: String,
        result: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        size: Option<u64>,
    },
    /// 上傳(raychiutw/purrdr.host#16):存成的路徑、宣告的大小與結果(stored、too_large、invalid、failed),不記內容。
    Upload {
        device_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        path: Option<String>,
        size: u64,
        result: &'static str,
    },
}

/// 誰撤銷的。
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RevokedBy {
    /// 主機上的管理指令。
    Host,
    /// 裝置自己(app 刪除主機時)。
    Device,
    /// 某支裝置的 app 以 devices.revoke 撤銷。
    App,
}

#[derive(Serialize)]
struct AuditLine<'a> {
    time: String,
    #[serde(flatten)]
    event: &'a AuditEvent,
}

/// 稽核紀錄的大小上限;超過就輪替成 `.1`(比服務 log 大:事後查「誰做了什麼」要能回溯得久一點)。
pub const AUDIT_LOG_MAXIMUM_BYTES: u64 = 5 * 1024 * 1024;

/// 主機服務資料夾裡的 `logs/audit.log`(舊版在資料夾根層的 `audit.log`,第一次開啟時搬進來)。
#[derive(Debug, Clone)]
pub struct AuditLog {
    log: RotatingLog,
}

impl AuditLog {
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self::with_maximum_bytes(directory, AUDIT_LOG_MAXIMUM_BYTES)
    }

    /// 指定輪替上限(測試用很小的值)。
    pub fn with_maximum_bytes(directory: impl Into<PathBuf>, maximum_bytes: u64) -> Self {
        let directory = directory.into();
        let log = RotatingLog::new(directory.join("logs").join("audit.log"), maximum_bytes);
        if let Err(error) = log.absorb(&directory.join("audit.log")) {
            eprintln!("cannot move the old audit log into {}: {error}", directory.join("logs").display());
        }
        AuditLog { log }
    }

    /// 附加一筆;寫不進去只印到 stderr,不影響請求本身。
    pub fn record(&self, event: AuditEvent, now: SystemTime) {
        let line = AuditLine {
            time: humantime::format_rfc3339_seconds(now).to_string(),
            event: &event,
        };
        let line = serde_json::to_string(&line).expect("稽核事件可序列化");
        if let Err(error) = self.log.append_line(&line) {
            eprintln!("cannot write audit log: {error}");
        }
    }
}
