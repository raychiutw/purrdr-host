//! 由 protocol/host-service.schema.json 產生，不要手動修改。重新產生:cd host-service && cargo run -p protocol-codegen

use serde::{Deserialize, Serialize};

/// 協定版本(schema 的 `x-protocol-version`)。
pub const PROTOCOL_VERSION: i64 = 1;

/// 主機取檔的排除清單(schema 的 `x-excluded-file-locations`)。
pub const EXCLUDED_FILE_LOCATIONS: &[&str] = &[".ssh", ".gnupg", ".aws", ".azure", ".config/gcloud", ".kube", ".docker", ".netrc", ".npmrc", ".pypirc", ".git-credentials", ".config/gh", ".cargo/credentials", ".cargo/credentials.toml", ".password-store", ".local/share/keyrings", "Library/Keychains", ".codex/auth.json", ".claude/.credentials.json", ".claude.json", ".purrdr/host-service", ".purrdr/push-*", ".env", ".env.*", ".vault-token", ".terraform.d", ".m2/settings.xml", ".git/config", ".bash_history", ".zsh_history", "ConsoleHost_history.txt", "Library/Application Support/Google/Chrome", "Library/Application Support/Microsoft Edge", "Library/Application Support/BraveSoftware", "Library/Application Support/Firefox", "Library/Cookies", "Library/Safari", ".mozilla", ".config/google-chrome", ".config/chromium", ".config/microsoft-edge", ".config/BraveSoftware", "AppData/Local/Google/Chrome/User Data", "AppData/Local/Microsoft/Edge/User Data", "AppData/Local/BraveSoftware", "AppData/Roaming/Mozilla/Firefox", "AppData/Roaming/Microsoft/Credentials", "AppData/Roaming/Microsoft/Protect"];

/// 主機取檔在工作目錄以外可放行的系統暫存目錄(schema 的 `x-temporary-file-locations`)。
pub const TEMPORARY_FILE_LOCATIONS: &[&str] = &["/tmp", "/private/tmp", "/var/folders", "/private/var/folders"];

/// 主機取檔的大小上限(schema 的 `x-maximum-file-size`)。
pub const MAXIMUM_FILE_SIZE: u64 = 26214400;

/// 主機取檔一次讀取最多回傳的 bytes(schema 的 `x-maximum-file-chunk`)。
pub const MAXIMUM_FILE_CHUNK: u64 = 1048576;

/// app 送給主機服務的訊息，以 type 區分。標 x-write 的是寫入類操作，唯讀裝置送出時被拒絕(read_only_device)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ClientMessage {
    #[serde(rename = "hello")]
    Hello(Hello),
    #[serde(rename = "agents.list")]
    AgentsList(AgentsList),
    #[serde(rename = "pane.send_keys")]
    PaneSendKeys(PaneSendKeys),
    #[serde(rename = "transcript.locate")]
    TranscriptLocate(TranscriptLocate),
    #[serde(rename = "transcript.window")]
    TranscriptWindow(TranscriptWindow),
    #[serde(rename = "transcript.follow")]
    TranscriptFollow(TranscriptFollow),
    #[serde(rename = "transcript.unfollow")]
    TranscriptUnfollow(TranscriptUnfollow),
    #[serde(rename = "transcript.previews")]
    TranscriptPreviews(TranscriptPreviews),
    #[serde(rename = "transcript.subagent")]
    TranscriptSubagent(TranscriptSubagent),
    #[serde(rename = "transcript.workflow")]
    TranscriptWorkflow(TranscriptWorkflow),
    #[serde(rename = "transcript.delegation")]
    TranscriptDelegation(TranscriptDelegation),
    #[serde(rename = "pane.send_text")]
    PaneSendText(PaneSendText),
    #[serde(rename = "agent.prompt")]
    AgentPrompt(AgentPrompt),
    #[serde(rename = "events.subscribe")]
    EventsSubscribe(EventsSubscribe),
    #[serde(rename = "events.unsubscribe")]
    EventsUnsubscribe(EventsUnsubscribe),
    #[serde(rename = "pane.terminal.open")]
    PaneTerminalOpen(PaneTerminalOpen),
    #[serde(rename = "pane.terminal.input")]
    PaneTerminalInput(PaneTerminalInput),
    #[serde(rename = "pane.terminal.resize")]
    PaneTerminalResize(PaneTerminalResize),
    #[serde(rename = "pane.terminal.scroll")]
    PaneTerminalScroll(PaneTerminalScroll),
    #[serde(rename = "pane.terminal.close")]
    PaneTerminalClose(PaneTerminalClose),
    #[serde(rename = "file.stat")]
    FileStat(FileStat),
    #[serde(rename = "file.read")]
    FileRead(FileRead),
    #[serde(rename = "push.status")]
    PushStatus(PushStatus),
    #[serde(rename = "push.install")]
    PushInstall(PushInstall),
    #[serde(rename = "push.remove")]
    PushRemove(PushRemove),
    #[serde(rename = "push.update_muted")]
    PushUpdateMuted(PushUpdateMuted),
    #[serde(rename = "devices.list")]
    DevicesList(DevicesList),
    #[serde(rename = "devices.revoke")]
    DevicesRevoke(DevicesRevoke),
}

impl ClientMessage {
    /// 寫入類訊息(schema 的 `x-write`);唯讀裝置送出時被拒絕。
    pub fn is_write(&self) -> bool {
        matches!(self, ClientMessage::PaneSendKeys(_) | ClientMessage::PaneSendText(_) | ClientMessage::AgentPrompt(_) | ClientMessage::PaneTerminalInput(_) | ClientMessage::PaneTerminalResize(_) | ClientMessage::PaneTerminalScroll(_) | ClientMessage::PushInstall(_) | ClientMessage::PushRemove(_) | ClientMessage::PushUpdateMuted(_) | ClientMessage::DevicesRevoke(_))
    }
}

/// 主機服務送給 app 的訊息，以 type 區分。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ServerMessage {
    #[serde(rename = "welcome")]
    Welcome(Welcome),
    #[serde(rename = "agents.list.result")]
    AgentsListResult(AgentsListResult),
    #[serde(rename = "pane.send_keys.result")]
    PaneSendKeysResult(PaneSendKeysResult),
    #[serde(rename = "transcript.locate.result")]
    TranscriptLocateResult(TranscriptLocateResult),
    #[serde(rename = "transcript.window.result")]
    TranscriptWindowResult(TranscriptWindowResult),
    #[serde(rename = "transcript.follow.result")]
    TranscriptFollowResult(TranscriptFollowResult),
    #[serde(rename = "transcript.update")]
    TranscriptUpdate(TranscriptUpdate),
    #[serde(rename = "transcript.unfollow.result")]
    TranscriptUnfollowResult(TranscriptUnfollowResult),
    #[serde(rename = "transcript.previews.result")]
    TranscriptPreviewsResult(TranscriptPreviewsResult),
    #[serde(rename = "transcript.subagent.result")]
    TranscriptSubagentResult(TranscriptSubagentResult),
    #[serde(rename = "transcript.workflow.result")]
    TranscriptWorkflowResult(TranscriptWorkflowResult),
    #[serde(rename = "transcript.delegation.result")]
    TranscriptDelegationResult(TranscriptDelegationResult),
    #[serde(rename = "pane.send_text.result")]
    PaneSendTextResult(PaneSendTextResult),
    #[serde(rename = "agent.prompt.result")]
    AgentPromptResult(AgentPromptResult),
    #[serde(rename = "events.subscribe.result")]
    EventsSubscribeResult(EventsSubscribeResult),
    #[serde(rename = "events.event")]
    SubscribedEvent(SubscribedEvent),
    #[serde(rename = "events.ended")]
    EventsEnded(EventsEnded),
    #[serde(rename = "events.unsubscribe.result")]
    EventsUnsubscribeResult(EventsUnsubscribeResult),
    #[serde(rename = "pane.terminal.open.result")]
    PaneTerminalOpenResult(PaneTerminalOpenResult),
    #[serde(rename = "pane.terminal.frame")]
    PaneTerminalFrame(PaneTerminalFrame),
    #[serde(rename = "pane.terminal.closed")]
    PaneTerminalClosed(PaneTerminalClosed),
    #[serde(rename = "file.stat.result")]
    FileStatResult(FileStatResult),
    #[serde(rename = "file.read.result")]
    FileReadResult(FileReadResult),
    #[serde(rename = "push.status.result")]
    PushStatusResult(PushStatusResult),
    #[serde(rename = "push.install.result")]
    PushInstallResult(PushInstallResult),
    #[serde(rename = "push.remove.result")]
    PushRemoveResult(PushRemoveResult),
    #[serde(rename = "push.update_muted.result")]
    PushUpdateMutedResult(PushUpdateMutedResult),
    #[serde(rename = "devices.list.result")]
    DevicesListResult(DevicesListResult),
    #[serde(rename = "devices.revoke.result")]
    DevicesRevokeResult(DevicesRevokeResult),
    #[serde(rename = "error")]
    ErrorMessage(ErrorMessage),
}

/// 握手：連線後第一個請求，帶 app 使用的協定版本。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    /// 請求識別碼，回應帶回同一個值。
    pub request_id: String,
    pub protocol_version: i64,
    /// app 版本，只用於診斷。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_version: Option<String>,
}

/// 握手成功：主機服務的協定版本、版本，herdr 伺服器 ping 回報的版本與能力，以及這支裝置是否唯讀。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Welcome {
    pub request_id: String,
    pub protocol_version: i64,
    pub host_service_version: String,
    pub herdr: HerdrServer,
    /// 這支裝置配對成唯讀：寫入類操作會被拒絕(read_only_device),app 停用輸入。
    pub read_only: bool,
}

/// herdr 伺服器(不是 CLI)的版本與能力。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HerdrServer {
    pub version: String,
    pub protocol: i64,
    /// ping 的 capabilities 裡值為 true 的名稱。
    pub capabilities: Vec<String>,
}

/// 取得 Agent 列表(herdr session.snapshot)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentsList {
    pub request_id: String,
}

/// Agent 列表與它們所在的 workspace。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentsListResult {
    pub request_id: String,
    pub workspaces: Vec<Workspace>,
    pub agents: Vec<Agent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    pub workspace_id: String,
    pub label: String,
}

/// herdr 在某個 Pane 偵測到的 Agent。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Agent {
    pub pane_id: String,
    pub workspace_id: String,
    pub tab_id: String,
    /// herdr 的 agent 名稱，例如 claude、codex。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    pub status: AgentStatus,
    /// 去掉狀態符號的終端機標題。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<AgentSession>,
}

/// herdr 回報的 Agent 狀態。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AgentStatus {
    #[serde(rename = "idle")]
    Idle,
    #[serde(rename = "working")]
    Working,
    #[serde(rename = "blocked")]
    Blocked,
    #[serde(rename = "done")]
    Done,
    #[serde(rename = "unknown")]
    Unknown,
}

/// herdr 的 agent_session:定位對話紀錄用。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSession {
    pub source: String,
    pub agent: String,
    pub kind: String,
    pub value: String,
}

/// 對 Pane 送出按鍵(herdr pane.send_keys)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneSendKeys {
    pub request_id: String,
    pub pane_id: String,
    /// herdr 鍵名，例如 enter、esc、ctrl+c。
    pub keys: Vec<String>,
}

/// 按鍵已送到 Pane。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneSendKeysResult {
    pub request_id: String,
}

/// 開 Pane 終端機(ADR-0005):主機服務以子行程執行 herdr terminal session control;唯讀裝置改用 observe(只能看)。串流識別碼就是這個請求的 request_id。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneTerminalOpen {
    pub request_id: String,
    /// pane id 或 terminal id。
    pub target: String,
    pub columns: i64,
    pub rows: i64,
    /// 接管別人正在操作的 Pane(唯讀裝置忽略)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub takeover: Option<bool>,
}

/// Pane 終端機已開啟;畫面更新接著以 pane.terminal.frame 送出，結束時送 pane.terminal.closed。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneTerminalOpenResult {
    pub request_id: String,
    /// 等於開啟請求的 request_id。
    pub stream_id: String,
    /// 唯讀裝置以 observe 接上：只能看，輸入會被拒絕。
    pub read_only: bool,
}

/// 一次畫面更新:herdr terminal.frame 原樣轉接。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneTerminalFrame {
    pub stream_id: String,
    /// 每條連線從 1 起算。
    pub seq: i64,
    pub width: i64,
    pub height: i64,
    /// true 是完整重繪。
    pub full: bool,
    /// ANSI 畫面 bytes 的標準 base64。
    pub bytes: String,
}

/// Pane 終端機結束，之後這個串流不再有訊息。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneTerminalClosed {
    pub stream_id: String,
    pub ending: PaneTerminalEnding,
    /// ending 是 closed 時,herdr terminal.closed 的原文。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// ending 是 failed 時,herdr 的 exit code。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i64>,
    /// ending 是 failed 時,herdr 的 stderr(給開發者看)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// closed:herdr 送了 terminal.closed;disconnected:沒有 terminal.closed 就斷了(可重新連線);failed:herdr 沒跑起來。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PaneTerminalEnding {
    #[serde(rename = "closed")]
    Closed,
    #[serde(rename = "disconnected")]
    Disconnected,
    #[serde(rename = "failed")]
    Failed,
}

/// Pane 終端機的鍵盤輸入(herdr terminal.input)。不回應;稽核只在開啟時記一筆，不記內容。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneTerminalInput {
    pub stream_id: String,
    /// 輸入 bytes 的標準 base64,每則不超過 64 KiB。
    pub bytes: String,
}

/// 改變 viewport 格數(herdr terminal.resize)。不回應。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneTerminalResize {
    pub stream_id: String,
    pub columns: i64,
    pub rows: i64,
}

/// 捲動(herdr terminal.scroll)。source 沒帶時是 wheel。不回應。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneTerminalScroll {
    pub stream_id: String,
    pub direction: TerminalScrollDirection,
    pub lines: i64,
    pub column: i64,
    pub row: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<TerminalScrollSource>,
}

/// 捲動方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TerminalScrollDirection {
    #[serde(rename = "up")]
    Up,
    #[serde(rename = "down")]
    Down,
}

/// 捲動來源:wheel 是滾輪或單指滑動;page_key 是實體鍵盤 Page Up／Down,herdr 依 Pane 狀態捲主機 scrollback 或送 Page 鍵給程式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TerminalScrollSource {
    #[serde(rename = "wheel")]
    Wheel,
    #[serde(rename = "page_key")]
    PageKey,
}

/// 關閉 Pane 終端機:control 關閉 stdin(等同 release),observe 結束子行程。不回應。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneTerminalClose {
    pub stream_id: String,
}

/// 定位對話紀錄(#59)。session 必須是 herdr snapshot 目前某個 Agent 的 agent_session;格式與 cwd 由主機服務從 snapshot 取得，app 不能指定路徑。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptLocate {
    pub request_id: String,
    pub session: SessionReference,
    /// 是否一併回傳檔案前兩行(Codex、Pi／OMP 的 header 檢查)。
    pub header: bool,
}

/// herdr agent_session 的 kind 與 value。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionReference {
    /// id 或 path。
    pub kind: String,
    pub value: String,
}

/// 對話紀錄的路徑與大小(byte)。之後這條連線才能以這個路徑讀視窗與追蹤。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptLocateResult {
    pub request_id: String,
    pub path: String,
    pub size: i64,
    /// 請求 header 時，檔案前兩行(原樣)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header: Option<Vec<String>>,
}

/// 讀以 end 為終點的最後 lines 行完整的行(開啟與往上翻頁)。path 必須是這條連線定位過的。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptWindow {
    pub request_id: String,
    pub path: String,
    pub end: i64,
    pub lines: i64,
}

/// 視窗的 byte 範圍與範圍內的行。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptWindowResult {
    pub request_id: String,
    pub start: i64,
    pub end: i64,
    pub records: Vec<TranscriptRecord>,
}

/// 對話紀錄的一行：原始 byte 長度(不含換行)與剝除圖片後的內容(行為規格 R-B04、R-B06)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptRecord {
    pub length: i64,
    pub text: String,
}

/// 開始追蹤對話紀錄尾端：從 from 起，主機服務把新增的完整行(每則最多 limit bytes,落後很多時分段接著送)以 transcript.update 推送，直到 transcript.unfollow、追蹤結束或連線關閉。stream_id 由 app 指定，同一條連線內不可重複。path 必須是這條連線定位過的。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptFollow {
    pub request_id: String,
    pub stream_id: String,
    pub path: String,
    pub from: i64,
    /// 每則回應或推送的上限(byte,#173):落後超過時從游標分段送出，每次最多 limit bytes 的完整行，後面還有就下一次接著送;第一行就超過 limit 時只送那一行，剝除圖片後的內容截短到 limit bytes(length 照原始長度，游標照樣前進)。#173 之前的主機服務落後超過 limit 就以 too_far_behind 結束。
    pub limit: i64,
}

/// 追蹤開始，帶目前的大小與 from 之後已有的完整行;ended 有值時主機服務沒有開始追蹤。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptFollowResult {
    pub request_id: String,
    pub stream_id: String,
    pub size: i64,
    /// records 第一行的位置(byte)。
    pub start: i64,
    pub records: Vec<TranscriptRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended: Option<TranscriptStreamEnd>,
}

/// 追蹤中的對話紀錄有了新的完整行，或追蹤結束(ended)。主機服務主動推送，沒有 request_id。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptUpdate {
    pub stream_id: String,
    pub size: i64,
    /// records 第一行的位置(byte)。
    pub start: i64,
    pub records: Vec<TranscriptRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended: Option<TranscriptStreamEnd>,
}

/// 主機服務停止追蹤的原因:shrank 檔案變小(換新)、replaced 檔案被換成同長或更長的新檔(游標前的內容變了)、absent 檔案不見了、unreadable 讀不到。too_far_behind 只有 #173 之前的主機服務會送(落後超過 limit;現在改為分段送出),app 仍要能接受。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TranscriptStreamEnd {
    #[serde(rename = "shrank")]
    Shrank,
    #[serde(rename = "replaced")]
    Replaced,
    #[serde(rename = "too_far_behind")]
    TooFarBehind,
    #[serde(rename = "absent")]
    Absent,
    #[serde(rename = "unreadable")]
    Unreadable,
}

/// 停止追蹤(對話關閉)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptUnfollow {
    pub request_id: String,
    pub stream_id: String,
}

/// 已停止追蹤;之後不再推送這個 stream_id。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptUnfollowResult {
    pub request_id: String,
}

/// 列表預覽(raychiutw/purrdr.host#17):一次讀多個對話紀錄的檔尾。path 必須是這條連線定位過的，任一個不是就整個請求回 transcript_not_allowed。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptPreviews {
    pub request_id: String,
    pub entries: Vec<TranscriptPreviewEntry>,
}

/// 一個要讀預覽的對話紀錄。known_version 是上次這個主機服務回的 version(app 原樣帶回，不解讀)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptPreviewEntry {
    pub key: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub known_version: Option<String>,
}

/// 每個讀得到的對話紀錄一項(讀不到的不在結果裡，同 SSH 的預覽腳本)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptPreviewsResult {
    pub request_id: String,
    pub previews: Vec<TranscriptPreview>,
}

/// 一個對話紀錄的預覽。lines 是剝除圖片後的行(同 SSH 預覽腳本，不帶原始長度):whole 是最後 48000 bytes(第一行可能被切在中間)、appended 是 known_version 之後附加的部分、unchanged 沒有內容。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptPreview {
    pub key: String,
    pub change: TranscriptPreviewChange,
    pub version: String,
    pub lines: Vec<String>,
}

/// 這次讀到的範圍:unchanged 與 known_version 相同、appended 只有附加的部分、whole 整段檔尾。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TranscriptPreviewChange {
    #[serde(rename = "unchanged")]
    Unchanged,
    #[serde(rename = "appended")]
    Appended,
    #[serde(rename = "whole")]
    Whole,
}

/// 找呼叫 call_id 啟動的 subagent 對話紀錄(raychiutw/purrdr.host#17):在 path 的 session 資料夾的 subagents/ 下找 toolUseId 完全等於 call_id 的 agent-*.meta.json。path 必須是這條連線可讀的對話紀錄。沒有 meta 回 transcript_absent,不只一個回 transcript_ambiguous。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptSubagent {
    pub request_id: String,
    pub path: String,
    pub call_id: String,
}

/// subagent 對話紀錄的路徑。session 資料夾 subagents/ 底下的 .jsonl 都可以讀視窗與追蹤。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptSubagentResult {
    pub request_id: String,
    pub path: String,
}

/// 讀 workflow 進度(raychiutw/purrdr.host#17):path 的 session 資料夾的 workflows/<run_id>.json(結束才寫)優先，否則 subagents/workflows/<run_id>/journal.jsonl。兩個都沒有回 transcript_absent。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptWorkflow {
    pub request_id: String,
    pub path: String,
    pub run_id: String,
    /// 上次回的 checksum;相同時不回內容。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub known_checksum: Option<String>,
}

/// kind 是讀到哪個檔(run 或 journal);content 是檔案原樣內容，checksum 等於 known_checksum 時省略。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptWorkflowResult {
    pub request_id: String,
    pub kind: TranscriptWorkflowKind,
    pub checksum: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

/// run:workflows/<run_id>.json;journal:subagents/workflows/<run_id>/journal.jsonl。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TranscriptWorkflowKind {
    #[serde(rename = "run")]
    Run,
    #[serde(rename = "journal")]
    Journal,
}

/// 父對話紀錄中含 call_id 或 agent_id 的行(委派是否結束，raychiutw/purrdr.host#17)。path 必須是這條連線可讀的對話紀錄。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptDelegation {
    pub request_id: String,
    pub path: String,
    pub call_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    /// 上次回的 checksum;相同時不回內容。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub known_checksum: Option<String>,
}

/// 這些行(剝除圖片後)的 checksum 與行;checksum 等於 known_checksum 時省略 records。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptDelegationResult {
    pub request_id: String,
    pub checksum: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub records: Option<Vec<TranscriptRecord>>,
}

/// 對 Pane 送出文字(herdr pane.send_text),不補 Enter。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneSendText {
    pub request_id: String,
    pub pane_id: String,
    pub text: String,
}

/// 文字已送到 Pane。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneSendTextResult {
    pub request_id: String,
}

/// 對 Agent 送出 prompt(herdr agent.prompt,target 是 pane_id)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentPrompt {
    pub request_id: String,
    pub pane_id: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait: Option<PromptWait>,
}

/// herdr agent.prompt 的 wait:等 Agent 進入其中一個狀態;主機服務等 herdr 的時間隨 timeout_ms 延長。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptWait {
    pub until: Vec<AgentStatus>,
    pub timeout_ms: i64,
}

/// herdr 收下 prompt。等待逾時、Agent 在等回答等情況以 herdr_error 回報(herdr_code 如 timeout、agent_blocked)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentPromptResult {
    pub request_id: String,
    /// herdr 回報的送達方式(例如 written_to_pty);沒有時省略。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery: Option<String>,
}

/// 開一條 Agent 事件串流(herdr events.subscribe)。herdr 接受後回 events.subscribe.result,之後以 events.event 推送，直到 events.ended 或 events.unsubscribe。事件只是「該重讀了」的訊號，對帳用 agents.list。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventsSubscribe {
    pub request_id: String,
    /// 串流識別碼(app 自訂，同一條連線內不可重複);這條串流的訊息都帶它。
    pub stream_id: String,
    pub subscriptions: Vec<EventSubscription>,
}

/// 一筆 herdr 事件訂閱。herdr 不認得的事件或不存在的 Pane 會讓整個訂閱被拒(herdr_error)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventSubscription {
    /// herdr 的訂閱型別，例如 pane.agent_status_changed、workspace.created。
    pub event: String,
    /// pane.agent_status_changed 必填;其他型別不帶。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane_id: Option<String>,
}

/// herdr 接受了訂閱。此後的變化才會推送;之前的要靠 agents.list 對帳。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventsSubscribeResult {
    pub request_id: String,
    pub stream_id: String,
}

/// herdr 推送的一個事件(沒有 request_id)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubscribedEvent {
    pub stream_id: String,
    /// herdr 事件的 wire 名稱，例如 pane.agent_status_changed、workspace_closed。
    pub event: String,
    /// 事件指名 Pane 時才有。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane_id: Option<String>,
}

/// 事件串流在主機端結束(沒有 request_id)。有 code 表示失敗;herdr 的 events_lost 是 code herdr_error 加 herdr_code events_lost,app 重新訂閱並對帳。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventsEnded {
    pub stream_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<ErrorCode>,
    /// 給開發者看的說明。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// code 是 herdr_error 時，herdr 原本的錯誤碼。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub herdr_code: Option<String>,
}

/// 關閉一條事件串流，主機服務關掉對應的 herdr 訂閱。不認得的串流識別碼也回成功。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventsUnsubscribe {
    pub request_id: String,
    pub stream_id: String,
}

/// 事件串流已關閉，之後不再有這條串流的訊息。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventsUnsubscribeResult {
    pub request_id: String,
}

/// 主機取檔(raychiutw/purrdr.ios#227):查檔案狀態。只允許 herdr snapshot 目前回報的 Agent 工作目錄之下，或這條連線定位過的對話紀錄裡出現的路徑;主機服務解析 symlink 後再比對工作目錄與排除清單(x-excluded-file-locations)。唯讀，唯讀裝置也可用。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileStat {
    pub request_id: String,
    /// 主機上的絕對路徑(不可含 . 或 .. 段落)。
    pub path: String,
}

/// 檔案狀態。status 是 file 時帶版本(內容的 SHA-256)與大小;too_large 只帶大小。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileStatResult {
    pub request_id: String,
    pub status: FileStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<i64>,
}

/// file:可讀的一般檔案;missing:不存在;not_regular_file:資料夾、FIFO、裝置等;unreadable:沒有權限或讀取失敗;denied:存取規則不允許;too_large:超過 25 MiB。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FileStatus {
    #[serde(rename = "file")]
    File,
    #[serde(rename = "missing")]
    Missing,
    #[serde(rename = "not_regular_file")]
    NotRegularFile,
    #[serde(rename = "unreadable")]
    Unreadable,
    #[serde(rename = "denied")]
    Denied,
    #[serde(rename = "too_large")]
    TooLarge,
}

/// 依版本讀 offset 起最多 length bytes(每次最多 1 MiB)。從頭讀(offset 0)或還沒有這個檔案的快照時，主機服務先檢查存取規則、把檔案讀成這條連線的快照並比對版本;之後的分塊從快照讀，分塊之間檔案被覆寫也不會拼出兩個版本。版本不符回 file_changed。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRead {
    pub request_id: String,
    pub path: String,
    /// file.stat 回報的版本。
    pub version: String,
    pub offset: i64,
    pub length: i64,
}

/// status 是 file 時帶 offset 與 bytes(標準 base64,超過檔尾時為空);其他狀態同 file.stat,不帶內容。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileReadResult {
    pub request_id: String,
    pub status: FileStatus,
    /// status 是 too_large 時的大小。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes: Option<String>,
}

/// 推播外掛代裝(raychiutw/purrdr.ios#261,0.6.0 起):查 herdr 是否已安裝推播外掛(purrdr.push)、這支裝置在這台主機是否已有推播配對(~/.purrdr/push-<device_id>/pairing)、其他已配對裝置數。唯讀，唯讀裝置也可用。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushStatus {
    pub request_id: String,
    /// app 的推播裝置 id(與 SSH 路徑相同):1–64 個小寫英數與 -,不能是 plugin。不合格回 invalid_message。
    pub device_id: String,
}

/// 開啟這支裝置在這台主機的通知：寫入推播配對(與 SSH 路徑相同的 ~/.purrdr/push-<device_id>/pairing,已有就覆寫),外掛還沒安裝就以本機使用者身分執行 herdr plugin install raychiutw/purrdr.host/host-plugin --yes(來源固定，app 不能指定)。安裝失敗時還原成寫入前的樣子(原有的配對保留)並回 push_plugin_failed。重送一律代表「以這把 key 重新配對」(覆寫配對檔;外掛已裝就不重裝)。只改靜音請用 push.update_muted。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushInstall {
    pub request_id: String,
    /// app 的推播裝置 id(與 SSH 路徑相同):1–64 個小寫英數與 -,不能是 plugin。不合格回 invalid_message。
    pub device_id: String,
    pub pairing: PushPairing,
}

/// 關閉這支裝置在這台主機的通知：只移除這支裝置的配對資料夾;沒有其他裝置的配對、且外掛是 Purrdr 裝的(~/.purrdr/push-plugin/ 的紀錄)才執行 herdr plugin uninstall purrdr.push。使用者自己裝的外掛不動。外掛移除失敗回 push_plugin_failed(配對已移除)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushRemove {
    pub request_id: String,
    /// app 的推播裝置 id(與 SSH 路徑相同):1–64 個小寫英數與 -,不能是 plugin。不合格回 invalid_message。
    pub device_id: String,
}

/// 推播配對檔的內容(app 的 PushPairingFile):主機服務寫成 purrdr-push 1、endpoint=、key=、muted= 各一行。endpoint 必須是 https:// 開頭、不含空白與控制字元;key 是 64 位小寫 hex;muted 不能含控制字元。不合格回 invalid_message。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushPairing {
    /// 後台 /v1/push 的網址。
    pub endpoint: String,
    /// 推播配對密鑰(64 位小寫 hex);只寫進主機上的配對檔，不記進稽核紀錄。
    pub key: String,
    /// 這支裝置靜音的聊天(Agent 的 session 值)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub muted: Option<Vec<String>>,
}

/// 這台主機的推播狀態。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushStatusResult {
    pub request_id: String,
    /// herdr 已安裝 purrdr.push(不論是誰裝的)。
    pub plugin_installed: bool,
    /// 這支裝置在這台主機有推播配對。
    pub paired: bool,
    /// 其他已配對裝置數(不含這支;不提供其他裝置的識別資訊)。
    pub other_devices: i64,
}

/// 開啟完成，帶完成後的推播狀態(欄位同 push.status.result)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushInstallResult {
    pub request_id: String,
    pub plugin_installed: bool,
    pub paired: bool,
    pub other_devices: i64,
}

/// 關閉完成，帶完成後的推播狀態(欄位同 push.status.result)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushRemoveResult {
    pub request_id: String,
    pub plugin_installed: bool,
    pub paired: bool,
    pub other_devices: i64,
}

/// 同步這支裝置在這台主機的靜音清單(app 不保存配對密鑰，不能重送 push.install):只以原子方式改寫配對檔的 muted 行，保留 key 與 endpoint,不呼叫 herdr。沒有配對檔回 push_not_paired;muted 含控制字元或空字串回 invalid_message。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushUpdateMuted {
    pub request_id: String,
    /// 同 push.install 的 device_id。
    pub device_id: String,
    /// 靜音的聊天(Agent 的 session 值),整份取代;空陣列表示取消全部靜音。
    pub muted: Vec<String>,
}

/// 靜音已更新。不問 herdr,plugin_installed 一律是 true(要確認外掛請用 push.status)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushUpdateMutedResult {
    pub request_id: String,
    pub plugin_installed: bool,
    pub paired: bool,
    pub other_devices: i64,
}

/// 列出這台主機已配對的裝置(raychiutw/purrdr.host#19;不含已撤銷的)。唯讀，唯讀裝置也可用。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DevicesList {
    pub request_id: String,
}

/// 已配對的裝置，依配對先後。不含權杖或其雜湊。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DevicesListResult {
    pub request_id: String,
    pub devices: Vec<PairedDevice>,
}

/// 一支已配對的裝置。時間是 RFC 3339 UTC(到秒)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairedDevice {
    pub device_id: String,
    /// 配對時 app 給的裝置名稱。
    pub name: String,
    pub read_only: bool,
    pub paired_at: String,
    /// 最後一次 WSS 連線的時間;還沒連過就沒有這欄。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_connected_at: Option<String>,
    /// 是發出這個請求的裝置。
    pub current: bool,
}

/// 撤銷一支已配對的裝置：之後它的每個 WSS 訊息都以 device_token_revoked 與 close code 4401 拒絕。已撤銷的再撤銷一次也成功;不認得的 device_id 回 device_unknown。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DevicesRevoke {
    pub request_id: String,
    /// devices.list.result 裡的 device_id。
    pub device_id: String,
}

/// 已撤銷。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DevicesRevokeResult {
    pub request_id: String,
    pub device_id: String,
}

/// POST /v1/pair 的本文：以配對碼換裝置權杖。QR code 的內容是 purrdr://pair?address=<HTTPS 位址>&code=<配對碼>。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairRequest {
    /// 大小寫與連字號不拘，例如 K7QM-2XPA。
    pub pairing_code: String,
    /// 顯示在主機裝置列表的名稱。
    pub device_name: String,
}

/// 配對成功。device_token 只在這裡出現一次，主機只存雜湊。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairResponse {
    pub device_id: String,
    pub device_token: String,
    /// 唯讀裝置的寫入類操作會被拒絕(read_only_device)。
    pub read_only: bool,
}

/// 請求失敗(WSS 訊息，也是配對與撤銷 HTTPS 請求失敗時的本文)。訊息本身無法解析時沒有 request_id。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorMessage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    pub code: ErrorCode,
    /// 給開發者看的說明，不顯示給使用者。
    pub message: String,
    /// code 是 herdr_error 時，herdr 原本的錯誤碼。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub herdr_code: Option<String>,
    /// code 是 protocol_version_unsupported 時，主機服務支援的協定版本。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supported_protocol_versions: Option<Vec<i64>>,
}

/// 錯誤種類。WSS 收到 device_token_invalid／device_token_revoked 後，主機服務以 close code 4401 關閉連線;HTTPS 請求回 401。transcript_not_allowed:session 不是 herdr 目前回報的，或路徑不是這條連線定位過的(或其 session 資料夾 subagents/ 底下的 .jsonl);transcript_unsupported:這種 agent 的對話紀錄主機服務還讀不了;file_changed:file.read 時檔案已不是要求的版本;push_plugin_failed:herdr plugin list／install／uninstall 失敗(message 是 herdr 的輸出);push_not_paired:push.update_muted 時這支裝置在這台主機沒有推播配對;device_unknown:devices.revoke 的 device_id 不是這台主機配對過的裝置;其他 transcript_* 對應行為規格 §4.7 的缺席、不可讀、有歧義、已壓縮。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ErrorCode {
    #[serde(rename = "invalid_message")]
    InvalidMessage,
    #[serde(rename = "handshake_required")]
    HandshakeRequired,
    #[serde(rename = "protocol_version_unsupported")]
    ProtocolVersionUnsupported,
    #[serde(rename = "herdr_unavailable")]
    HerdrUnavailable,
    #[serde(rename = "herdr_capability_missing")]
    HerdrCapabilityMissing,
    #[serde(rename = "herdr_error")]
    HerdrError,
    #[serde(rename = "device_token_invalid")]
    DeviceTokenInvalid,
    #[serde(rename = "device_token_revoked")]
    DeviceTokenRevoked,
    #[serde(rename = "read_only_device")]
    ReadOnlyDevice,
    #[serde(rename = "pairing_code_invalid")]
    PairingCodeInvalid,
    #[serde(rename = "pairing_code_expired")]
    PairingCodeExpired,
    #[serde(rename = "transcript_not_allowed")]
    TranscriptNotAllowed,
    #[serde(rename = "transcript_unsupported")]
    TranscriptUnsupported,
    #[serde(rename = "transcript_absent")]
    TranscriptAbsent,
    #[serde(rename = "transcript_unreadable")]
    TranscriptUnreadable,
    #[serde(rename = "transcript_ambiguous")]
    TranscriptAmbiguous,
    #[serde(rename = "transcript_compressed")]
    TranscriptCompressed,
    #[serde(rename = "file_changed")]
    FileChanged,
    #[serde(rename = "push_plugin_failed")]
    PushPluginFailed,
    #[serde(rename = "push_not_paired")]
    PushNotPaired,
    #[serde(rename = "device_unknown")]
    DeviceUnknown,
    #[serde(rename = "internal_error")]
    InternalError,
}
