//! 讀 Pane 畫面(raychiutw/purrdr.host#28)與畫面指紋。
//!
//! 主機服務只轉送 herdr `pane.read` 並回傳文字與指紋，不解析語意(提問卡、建議提示詞等由 app 解析)。
//! 指紋是文字 UTF-8 bytes 的 SHA-256:送出前比對(`expected_screen`)與卡住偵測都用同一個定義。

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::connection::{error, herdr_error};
use crate::herdr_client::HerdrClient;
use crate::pane_terminal;
use crate::protocol::{ErrorCode, PaneRead, PaneReadFormat, PaneReadResult, PaneReadSource, ServerMessage};
use crate::text_format::hex;

/// 畫面文字的指紋(小寫 hex 的 SHA-256)。
pub(crate) fn fingerprint(text: &str) -> String {
    hex(&Sha256::digest(text.as_bytes()))
}

/// `pane.read`:pane id 規則同 Pane 終端機的 target,不合格不送到 herdr。
pub(crate) async fn read(herdr: &HerdrClient, request: PaneRead) -> ServerMessage {
    if !pane_terminal::is_valid_target(&request.pane_id) {
        return error(Some(request.request_id), ErrorCode::InvalidMessage, "invalid pane_id");
    }
    let format = request.format.unwrap_or(PaneReadFormat::Text);
    let mut params = json!({"pane_id": request.pane_id, "source": source_name(request.source), "format": format_name(format)});
    if format == PaneReadFormat::Ansi {
        params["strip_ansi"] = json!(false);
    }
    if let Some(lines) = request.lines {
        params["lines"] = json!(lines);
    }
    match herdr.request("pane.read", params).await {
        Ok(result) => match read_text(&result) {
            Some((text, truncated)) => ServerMessage::PaneReadResult(PaneReadResult {
                request_id: request.request_id,
                screen: fingerprint(&text),
                text,
                truncated,
            }),
            None => error(Some(request.request_id), ErrorCode::HerdrError, "herdr pane.read returned no text"),
        },
        Err(failure) => herdr_error(request.request_id, "pane.read", failure),
    }
}

fn read_text(result: &Value) -> Option<(String, bool)> {
    let read = &result["read"];
    Some((read["text"].as_str()?.to_owned(), read["truncated"].as_bool().unwrap_or(false)))
}

fn source_name(source: PaneReadSource) -> &'static str {
    match source {
        PaneReadSource::Visible => "visible",
        PaneReadSource::Recent => "recent",
        PaneReadSource::RecentUnwrapped => "recent_unwrapped",
        PaneReadSource::Detection => "detection",
    }
}

fn format_name(format: PaneReadFormat) -> &'static str {
    match format {
        PaneReadFormat::Text => "text",
        PaneReadFormat::Ansi => "ansi",
    }
}
