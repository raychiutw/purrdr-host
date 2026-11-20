//! 送出前比對(raychiutw/purrdr.host#29):確認對象還是 app 看到的那個 Agent 與畫面，避免送進錯的對話。
//!
//! `expected_session` 比對 herdr `session.snapshot` 回報的這個 pane 的 `agent_session` id;
//! `expected_screen` 比對目前可見區的畫面指紋(`pane_screen::fingerprint`)。都沒帶時不讀 herdr。
//! WebSocket 的送出訊息與 Send When Done 佇列共用這一套。

use serde_json::{Value, json};

use crate::herdr_client::{HerdrClient, HerdrFailure};
use crate::pane_screen;

/// 比對不通過的原因。
#[derive(Debug)]
pub(crate) enum GuardFailure {
    /// 對象不是預期的 session 或畫面:不送。
    Changed(String),
    /// 讀不到 herdr:不送。`method` 是失敗的 herdr 方法。
    Herdr { method: &'static str, failure: HerdrFailure },
}

/// 這個 pane 目前的 Agent session id;pane 上沒有 Agent 或沒有回報 session 時 None。
pub(crate) fn session_of(snapshot: &Value, pane_id: &str) -> Option<String> {
    snapshot["snapshot"]["agents"].as_array()?.iter().find(|agent| agent["pane_id"] == pane_id)?["agent_session"]["value"]
        .as_str()
        .map(str::to_owned)
}

/// 依序比對 session 與畫面;全部符合(或沒帶)時 Ok。
pub(crate) async fn check(herdr: &HerdrClient, pane_id: &str, expected_session: Option<&str>, expected_screen: Option<&str>) -> Result<(), GuardFailure> {
    if let Some(expected) = expected_session {
        let snapshot = herdr.request("session.snapshot", json!({})).await.map_err(|failure| GuardFailure::Herdr {
            method: "session.snapshot",
            failure,
        })?;
        if session_of(&snapshot, pane_id).as_deref() != Some(expected) {
            return Err(GuardFailure::Changed(format!("pane {pane_id} is no longer session {expected}")));
        }
    }
    if let Some(expected) = expected_screen {
        let current = pane_screen::current_fingerprint(herdr, pane_id)
            .await
            .map_err(|failure| GuardFailure::Herdr { method: "pane.read", failure })?;
        if current != expected {
            return Err(GuardFailure::Changed(format!("the screen of pane {pane_id} has changed")));
        }
    }
    Ok(())
}
