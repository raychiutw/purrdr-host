//! 更新事件推播(raychiutw/purrdr.host#22、ADR-0004):對 `~/.purrdr/push-<裝置>/pairing` 的每支裝置送主機服務事件。
//! 只帶密鑰、事件與版本;密鑰只放在 HTTP 本文。

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::push_plugin::PairingCredentials;
use crate::update::UpdateEvent;

/// 送一則推播;回 HTTP 狀態碼，連不上時回錯誤。
pub trait PushTransport: Send {
    fn post_json(&mut self, endpoint: &str, body: &str) -> Result<u16, String>;
}

/// 以 HTTP 送(正式)。
pub struct HttpPushTransport {
    agent: ureq::Agent,
}

impl Default for HttpPushTransport {
    fn default() -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(10)))
            .http_status_as_error(false)
            .user_agent(concat!("purrdr-host-service/", env!("CARGO_PKG_VERSION")))
            .build()
            .into();
        HttpPushTransport { agent }
    }
}

impl PushTransport for HttpPushTransport {
    fn post_json(&mut self, endpoint: &str, body: &str) -> Result<u16, String> {
        self.agent
            .post(endpoint)
            .header("Content-Type", "application/json")
            .send(body)
            .map(|response| response.status().as_u16())
            .map_err(|error| error.to_string())
    }
}

/// 送過的事件紀錄(狀態資料夾裡)。
const SENT_RECORD: &str = "update-push.json";
/// 紀錄只留最近幾個事件。
const MAXIMUM_SENT_EVENTS: usize = 20;
/// 後台：這支裝置的配對已撤銷。
const REVOKED: u16 = 410;

/// 某個家目錄的推播配對。
pub struct UpdateEventPush<'transport> {
    purrdr_directory: PathBuf,
    sent_record: PathBuf,
    transport: &'transport mut dyn PushTransport,
}

impl<'transport> UpdateEventPush<'transport> {
    pub fn new(home: &Path, transport: &'transport mut dyn PushTransport) -> Self {
        UpdateEventPush {
            purrdr_directory: home.join(".purrdr"),
            sent_record: crate::state_directory::default_state_directory(home).join(SENT_RECORD),
            transport,
        }
    }

    /// 同一事件與版本每支裝置只送一次;連不上的下次觸發再送，其他失敗不重試。都不影響呼叫端。
    pub fn send(&mut self, event: &UpdateEvent) {
        let event_id = format!("{}@{}", event.name(), event.version());
        let mut sent = self.read_sent();
        let already_sent = sent
            .iter()
            .find(|entry| entry.event == event_id)
            .map(|entry| entry.devices.clone())
            .unwrap_or_default();
        let mut delivered = Vec::new();
        for pairing in self.pairings() {
            let device = pairing.device_name();
            if already_sent.contains(&device) {
                continue;
            }
            let body = serde_json::json!({
                "key": pairing.key,
                "event": event.name(),
                "version": event.version().to_string(),
            });
            match self.transport.post_json(&pairing.endpoint, &body.to_string()) {
                Ok(REVOKED) => pairing.remove_if_unchanged(),
                Ok(_) => delivered.push(device),
                Err(_) => {}
            }
        }
        if delivered.is_empty() {
            return;
        }
        sent.retain(|entry| entry.event != event_id);
        sent.push(SentEvent {
            event: event_id,
            devices: already_sent.into_iter().chain(delivered).collect(),
        });
        let overflow = sent.len().saturating_sub(MAXIMUM_SENT_EVENTS);
        sent.drain(..overflow);
        let _ = std::fs::write(&self.sent_record, serde_json::to_string_pretty(&SentRecord { sent }).expect("可序列化"));
    }

    fn read_sent(&self) -> Vec<SentEvent> {
        std::fs::read_to_string(&self.sent_record)
            .ok()
            .and_then(|text| serde_json::from_str::<SentRecord>(&text).ok())
            .map(|record| record.sent)
            .unwrap_or_default()
    }

    fn pairings(&self) -> Vec<Pairing> {
        let Ok(entries) = std::fs::read_dir(&self.purrdr_directory) else {
            return Vec::new();
        };
        let mut pairings: Vec<Pairing> = entries
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("push-"))
            .filter_map(|entry| Pairing::read(&entry.path()))
            .collect();
        pairings.sort_by(|left, right| left.directory.cmp(&right.directory));
        pairings
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct SentRecord {
    sent: Vec<SentEvent>,
}

/// 一個事件(`<event>@<版本>`)已送到的裝置資料夾名稱。
#[derive(serde::Serialize, serde::Deserialize)]
struct SentEvent {
    event: String,
    devices: Vec<String>,
}

/// 一支裝置的配對檔(格式同 app 的 `PushPairingFile`)。
struct Pairing {
    directory: PathBuf,
    endpoint: String,
    key: String,
}

impl Pairing {
    fn read(directory: &Path) -> Option<Pairing> {
        let credentials = PairingCredentials::parse(&std::fs::read_to_string(directory.join("pairing")).ok()?)?;
        if credentials.endpoint.is_empty() || credentials.key.is_empty() {
            return None;
        }
        Some(Pairing {
            directory: directory.to_owned(),
            endpoint: credentials.endpoint,
            key: credentials.key,
        })
    }

    /// 送出期間重新配對了(密鑰換了)就不是被撤銷的那一組，不刪(同主機外掛)。
    fn remove_if_unchanged(&self) {
        if Pairing::read(&self.directory).is_some_and(|current| current.key == self.key) {
            let _ = std::fs::remove_dir_all(&self.directory);
        }
    }

    fn device_name(&self) -> String {
        self.directory.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
    }
}
