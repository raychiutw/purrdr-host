//! 協定、狀態檔與稽核紀錄共用的文字格式。

use std::time::SystemTime;

/// 小寫十六進位。
pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// RFC 3339 UTC,精確到秒。
pub(crate) fn rfc3339(time: SystemTime) -> String {
    humantime::format_rfc3339_seconds(time).to_string()
}
