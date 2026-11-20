//! Agent 觀察者(raychiutw/purrdr.host#30):主機服務常駐訂閱 herdr 的 Agent 狀態事件，不論 app 是否連線。
//!
//! 第一個用途是 Send When Done:每個 pane 一條佇列，Agent 從 working 變成 idle／done 時送出第一則
//! (經 `pane_guard` 比對 expected_session);`blocked` 之後的 idle 不算做完，session 不符時暫停並標記。
//! 佇列存在狀態資料夾，重啟後還原。觀察者不記活動紀錄與對話內容(佇列本身就是使用者要送的文字)。

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

use crate::herdr_client::HerdrClient;
use crate::pane_guard::{self, GuardFailure};
use crate::protocol::{PaneQueue, QueueChanged, QueuePause, QueuedPrompt, ServerMessage};
use crate::text_format::hex;

/// 每個 pane 最多排幾則。
pub const MAXIMUM_QUEUED_PROMPTS: usize = 20;
/// 每則最多幾 bytes(與 Pane 終端機單則輸入相同)。
pub const MAXIMUM_PROMPT_BYTES: usize = 64 * 1024;
/// 佇列檔名(狀態資料夾裡)。
const QUEUE_FILE: &str = "send-queue.json";

/// 觀察者的時間設定;測試以較短的值注入。
#[derive(Debug, Clone, Copy)]
pub struct ObserverOptions {
    /// herdr 訂閱斷掉後第一次重連前等多久，之後每次加倍。
    pub reconnect_initial: Duration,
    /// 重連等待的上限。
    pub reconnect_maximum: Duration,
}

impl Default for ObserverOptions {
    fn default() -> Self {
        ObserverOptions {
            reconnect_initial: Duration::from_secs(1),
            reconnect_maximum: Duration::from_secs(60),
        }
    }
}

/// 佇列操作失敗的原因。
#[derive(Debug)]
pub(crate) enum QueueFailure {
    Full,
    UnknownItem,
    /// 存不了佇列檔。
    Storage(String),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct StoredQueues {
    #[serde(default)]
    panes: BTreeMap<String, StoredQueue>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct StoredQueue {
    #[serde(default)]
    items: Vec<StoredPrompt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    paused: Option<QueuePause>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredPrompt {
    item_id: String,
    text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    expected_session: Option<String>,
}

/// Send When Done 佇列與變更通知;連線與觀察者 task 共用。
#[derive(Clone)]
pub(crate) struct AgentObserver {
    queues: Arc<Mutex<StoredQueues>>,
    file: PathBuf,
    changes: broadcast::Sender<ServerMessage>,
}

impl AgentObserver {
    /// 從狀態資料夾讀回佇列(檔案不在或壞了就從空的開始)。
    pub(crate) fn load(state_directory: &Path) -> Self {
        let file = state_directory.join(QUEUE_FILE);
        let queues = std::fs::read(&file)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let (changes, _) = broadcast::channel(64);
        AgentObserver {
            queues: Arc::new(Mutex::new(queues)),
            file,
            changes,
        }
    }

    /// 佇列變更的推送(queue.changed)。
    pub(crate) fn subscribe(&self) -> broadcast::Receiver<ServerMessage> {
        self.changes.subscribe()
    }

    pub(crate) fn list(&self, pane_id: Option<&str>) -> Vec<PaneQueue> {
        let queues = self.queues.lock().unwrap();
        queues
            .panes
            .iter()
            .filter(|(pane, queue)| pane_id.is_none_or(|wanted| wanted == pane.as_str()) && !queue.items.is_empty())
            .map(|(pane, queue)| pane_queue(pane, queue))
            .collect()
    }

    pub(crate) fn add(&self, pane_id: &str, text: String, expected_session: Option<String>) -> Result<String, QueueFailure> {
        let item_id = new_item_id().map_err(|error| QueueFailure::Storage(error.to_string()))?;
        self.change(pane_id, None, |queue| {
            if queue.items.len() >= MAXIMUM_QUEUED_PROMPTS {
                return Err(QueueFailure::Full);
            }
            queue.items.push(StoredPrompt {
                item_id: item_id.clone(),
                text,
                expected_session,
            });
            queue.paused = None;
            Ok(())
        })?;
        Ok(item_id)
    }

    pub(crate) fn remove(&self, pane_id: &str, item_id: &str) -> Result<(), QueueFailure> {
        self.change(pane_id, None, |queue| {
            let position = queue.items.iter().position(|item| item.item_id == item_id).ok_or(QueueFailure::UnknownItem)?;
            queue.items.remove(position);
            queue.paused = None;
            Ok(())
        })
    }

    /// `item_ids` 必須恰好是目前佇列的所有項目。
    pub(crate) fn reorder(&self, pane_id: &str, item_ids: &[String]) -> Result<(), QueueFailure> {
        self.change(pane_id, None, |queue| {
            let mut current: Vec<&str> = queue.items.iter().map(|item| item.item_id.as_str()).collect();
            let mut wanted: Vec<&str> = item_ids.iter().map(String::as_str).collect();
            current.sort_unstable();
            wanted.sort_unstable();
            if current != wanted {
                return Err(QueueFailure::UnknownItem);
            }
            let mut items = std::mem::take(&mut queue.items);
            for item_id in item_ids {
                let position = items.iter().position(|item| &item.item_id == item_id).expect("已確認是同一組 id");
                queue.items.push(items.remove(position));
            }
            queue.paused = None;
            Ok(())
        })
    }

    /// 改一個 pane 的佇列：成功時存檔並推 queue.changed;失敗時不動。
    fn change(&self, pane_id: &str, sent_item_id: Option<String>, edit: impl FnOnce(&mut StoredQueue) -> Result<(), QueueFailure>) -> Result<(), QueueFailure> {
        let mut queues = self.queues.lock().unwrap();
        let mut queue = queues.panes.get(pane_id).cloned().unwrap_or_default();
        edit(&mut queue)?;
        let mut updated = queues.clone();
        if queue.items.is_empty() {
            updated.panes.remove(pane_id);
        } else {
            updated.panes.insert(pane_id.to_owned(), queue.clone());
        }
        save(&self.file, &updated).map_err(|error| QueueFailure::Storage(error.to_string()))?;
        *queues = updated;
        let _ = self.changes.send(ServerMessage::QueueChanged(QueueChanged {
            queue: pane_queue(pane_id, &queue),
            sent_item_id,
        }));
        Ok(())
    }

    /// 這個 pane 下一則要送的(暫停時沒有)。
    fn next(&self, pane_id: &str) -> Option<StoredPrompt> {
        let queues = self.queues.lock().unwrap();
        let queue = queues.panes.get(pane_id)?;
        if queue.paused.is_some() {
            return None;
        }
        queue.items.first().cloned()
    }
}

fn pane_queue(pane_id: &str, queue: &StoredQueue) -> PaneQueue {
    PaneQueue {
        pane_id: pane_id.to_owned(),
        items: queue
            .items
            .iter()
            .map(|item| QueuedPrompt {
                item_id: item.item_id.clone(),
                text: item.text.clone(),
                expected_session: item.expected_session.clone(),
            })
            .collect(),
        paused: queue.paused,
    }
}

/// 先寫暫存檔再改名，中途失敗不會留下半個檔案。
fn save(file: &Path, queues: &StoredQueues) -> std::io::Result<()> {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temporary = file.with_extension("json.tmp");
    std::fs::write(&temporary, serde_json::to_vec_pretty(queues).map_err(std::io::Error::other)?)?;
    std::fs::rename(&temporary, file)
}

fn new_item_id() -> Result<String, getrandom::Error> {
    let mut bytes = [0_u8; 8];
    getrandom::fill(&mut bytes)?;
    Ok(hex(&bytes))
}

/// 啟動常駐 task:訂閱 herdr 事件(斷線退避重連)並依狀態轉換送出佇列。
pub(crate) fn spawn(herdr: HerdrClient, observer: AgentObserver, options: ObserverOptions) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut statuses: HashMap<String, String> = HashMap::new();
        let mut delay = options.reconnect_initial;
        loop {
            if let Ok(mut subscription) = herdr.subscribe(json!({"subscriptions": [{"type": "pane.agent_status_changed"}]})).await {
                delay = options.reconnect_initial;
                // 訂閱之後才讀 snapshot:中間的轉換會在訂閱裡，不會漏掉。
                if let Ok(snapshot) = herdr.request("session.snapshot", json!({})).await {
                    statuses = snapshot_statuses(&snapshot);
                }
                while let Ok(Some(event)) = subscription.next_event().await {
                    if let Some((pane_id, status)) = status_change(&event) {
                        let previous = statuses.insert(pane_id.clone(), status.clone());
                        if previous.as_deref() == Some("working") && matches!(status.as_str(), "idle" | "done") {
                            send_next(&herdr, &observer, &pane_id).await;
                        }
                    }
                }
            }
            tokio::time::sleep(delay).await;
            delay = (delay * 2).min(options.reconnect_maximum);
        }
    })
}

fn snapshot_statuses(snapshot: &Value) -> HashMap<String, String> {
    snapshot["snapshot"]["agents"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|agent| Some((agent["pane_id"].as_str()?.to_owned(), agent["agent_status"].as_str()?.to_owned())))
        .collect()
}

fn status_change(event: &Value) -> Option<(String, String)> {
    if event["event"] != "pane.agent_status_changed" {
        return None;
    }
    let data = &event["data"];
    Some((data["pane_id"].as_str()?.to_owned(), data["agent_status"].as_str()?.to_owned()))
}

/// 送出佇列第一則：比對通過才送，送出後從佇列移除;session 不符時暫停並標記;herdr 失敗時留著等下次。
async fn send_next(herdr: &HerdrClient, observer: &AgentObserver, pane_id: &str) {
    let Some(item) = observer.next(pane_id) else {
        return;
    };
    match pane_guard::check(herdr, pane_id, item.expected_session.as_deref(), None).await {
        Ok(()) => {}
        Err(GuardFailure::Changed(_)) => {
            let _ = observer.change(pane_id, None, |queue| {
                queue.paused = Some(QueuePause::SessionChanged);
                Ok(())
            });
            return;
        }
        Err(GuardFailure::Herdr { .. }) => return,
    }
    if herdr.request("agent.prompt", json!({"target": pane_id, "text": item.text})).await.is_ok() {
        let sent = item.item_id.clone();
        let _ = observer.change(pane_id, Some(sent), |queue| {
            queue.items.retain(|queued| queued.item_id != item.item_id);
            Ok(())
        });
    }
}
