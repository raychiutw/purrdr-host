//! 主機外掛腳本(`host-plugin/`,raychiutw/purrdr.ios ADR-0004):herdr 在 `pane.agent_status_changed` 時以新程序執行它。
//!
//! 在暫存家目錄真的執行 `host-plugin/notify.sh`,herdr 給的環境變數照 herdr 文件
//! (`HERDR_PLUGIN_EVENT`、`HERDR_PLUGIN_EVENT_JSON`、`HERDR_PLUGIN_CONTEXT_JSON`、`HERDR_BIN_PATH`)手動帶入;
//! `curl` 與 `herdr` 換成家目錄 `bin/` 裡的假程式，記錄收到的參數與 stdin,不連網路、不碰真的 herdr。
//!
//! 移植自 app repo 的 `tests/PurrdrCoreTests/HostPluginScriptTests.swift`,每個測試的說明是原本的測試名稱。
//! `notify.sh` 只在 macOS／Linux 執行(manifest 限定平台),所以執行腳本的測試只在 Unix 跑;
//! Windows 的 `notify.ps1` 只測後台回應處理(`notify_ps1`,本機假 HTTP 後台)。manifest 的檢查三個平台都跑。

use std::path::{Path, PathBuf};

const PLUGIN_ID: &str = "purrdr.push";

/// 交叉編譯到別台執行時以 `PURRDR_HOST_PLUGIN_DIR` 指定。
fn plugin_directory() -> PathBuf {
    std::env::var_os("PURRDR_HOST_PLUGIN_DIR").map_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host-plugin"), PathBuf::from)
}

/// manifest:只掛 pane.agent_status_changed,macOS／Linux 用 sh、Windows 用 PowerShell 腳本(檔案都在)。
/// 解析 TOML 逐一比對每個事件：同一個事件名出現兩次，用字串 `contains` 會放過只改壞其中一個的情況。
#[test]
fn manifest_declares_hooks() {
    let text = std::fs::read_to_string(plugin_directory().join("herdr-plugin.toml")).unwrap();
    let manifest: toml::Table = text.parse().unwrap();
    assert_eq!(manifest["id"].as_str(), Some(PLUGIN_ID));

    let events = manifest["events"].as_array().expect("沒有 [[events]]");
    let hooks: Vec<(&str, Vec<&str>, Vec<&str>)> = events
        .iter()
        .map(|event| {
            let strings = |key: &str| -> Vec<&str> {
                event[key]
                    .as_array()
                    .unwrap_or_else(|| panic!("事件缺 {key}"))
                    .iter()
                    .map(|value| value.as_str().unwrap())
                    .collect()
            };
            (event["on"].as_str().expect("事件缺 on"), strings("command"), strings("platforms"))
        })
        .collect();
    assert_eq!(
        hooks,
        vec![
            ("pane.agent_status_changed", vec!["sh", "notify.sh"], vec!["linux", "macos"]),
            (
                "pane.agent_status_changed",
                vec![
                    "powershell",
                    "-NoProfile",
                    "-NonInteractive",
                    "-ExecutionPolicy",
                    "Bypass",
                    "-File",
                    "notify.ps1"
                ],
                vec!["windows"],
            ),
        ]
    );
    for script in ["notify.sh", "notify.ps1"] {
        assert!(plugin_directory().join(script).is_file(), "缺 {script}");
    }
}

/// notify.ps1 帶 UTF-8 BOM:Windows PowerShell 5.1 讀沒有 BOM 的檔案用系統字碼頁(如 cp950),中文註解會讓腳本解析失敗。
#[test]
fn powershell_script_has_utf8_bom() {
    let bytes = std::fs::read(plugin_directory().join("notify.ps1")).unwrap();
    assert!(bytes.starts_with(&[0xEF, 0xBB, 0xBF]), "notify.ps1 缺 UTF-8 BOM");
}

#[cfg(unix)]
mod notify_sh {
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::process::Command;

    use serde_json::{Map, Value};
    use tempfile::TempDir;

    use super::plugin_directory;

    const ENDPOINT: &str = "https://backend.test/v1/push";

    fn key_a() -> String {
        "ab".repeat(32)
    }

    fn key_b() -> String {
        "cd".repeat(32)
    }

    /// herdr 序列化的事件 envelope(`EventEnvelope`:`event` 是 snake_case、`data` 以 `type` 標記)。
    /// `title` 是終端機標題，可能帶對話內容——外掛不能送出它。
    fn event(status: &str, pane: &str) -> String {
        format!(
            r#"{{"event":"pane_agent_status_changed","data":{{"type":"pane_agent_status_changed","pane_id":"{pane}","workspace_id":"w2","agent_status":"{status}","agent":"claude","title":"Delete \"prod\" database? secret-plan","display_agent":"Claude"}}}}"#
        )
    }

    fn blocked() -> String {
        event("blocked", "w2:p1")
    }

    fn context(label: &str) -> String {
        format!(
            r#"{{"workspace_id":"w2","workspace_label":"{label}","focused_pane_id":"w2:p1","focused_pane_agent":"claude","focused_pane_status":"blocked","correlation_id":"evt-1"}}"#
        )
    }

    fn default_context() -> String {
        context("weather-app")
    }

    const FAKE_CURL: &str = r#"#!/bin/sh
n=$(ls "$HOME/curl-calls" 2>/dev/null | wc -l | tr -d ' ')
mkdir -p "$HOME/curl-calls/$n"
printf '%s\n' "$@" > "$HOME/curl-calls/$n/args"
cat > "$HOME/curl-calls/$n/stdin"
code=200
if [ -f "$HOME/backend-responses" ]; then
  while read -r key status; do
    grep -q "$key" "$HOME/curl-calls/$n/stdin" && code=$status
  done < "$HOME/backend-responses"
fi
# 送出期間這支裝置重新配對了(app 換了新密鑰)。
if [ -f "$HOME/repair-during-call/device" ]; then
  cp "$HOME/repair-during-call/pairing" "$HOME/.purrdr/push-$(cat "$HOME/repair-during-call/device")/pairing"
fi
printf '%s' "$code"
"#;

    /// `herdr pane get <pane>`:回傳該 pane 的 agent session(錄自 herdr 0.9.5 的 `pane list` 欄位形狀)。
    const FAKE_HERDR: &str = r#"#!/bin/sh
printf '%s\n' "$*" >> "$HOME/herdr-calls"
if [ "$1 $2" = "pane get" ]; then
  printf '%s\n' '{"id":"cli:pane:get","result":{"pane":{"agent":"claude","agent_session":{"agent":"claude","kind":"id","source":"herdr:claude","value":"session-weather"},"agent_status":"blocked","pane_id":"w2:p1","workspace_id":"w2"},"type":"pane_info"}}'
fi
"#;

    struct CurlCall {
        arguments: Vec<String>,
        body: Map<String, Value>,
        raw_body: String,
    }

    /// 暫存家目錄，`bin/` 裡有假的 curl 與 herdr。
    struct Host {
        home: TempDir,
    }

    impl Host {
        fn new() -> Self {
            let host = Host {
                home: tempfile::tempdir().unwrap(),
            };
            host.install_fake("curl", FAKE_CURL);
            host.install_fake("herdr", FAKE_HERDR);
            host
        }

        fn path(&self, relative: &str) -> PathBuf {
            self.home.path().join(relative)
        }

        fn write(&self, relative: &str, contents: &str) -> PathBuf {
            let path = self.path(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, contents).unwrap();
            path
        }

        fn install_fake(&self, name: &str, script: &str) {
            let path = self.write(&format!("bin/{name}"), script);
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        /// 與 app 的 `PushPairingFile.contents` 相同的格式。
        fn pair(&self, device: &str, key: &str, muted: &[&str]) {
            let mut muted: Vec<&str> = muted.to_vec();
            muted.sort();
            let mut lines = vec!["purrdr-push 1".to_owned(), format!("endpoint={ENDPOINT}"), format!("key={key}")];
            lines.extend(muted.iter().map(|session| format!("muted={session}")));
            self.write(&format!(".purrdr/push-{device}/pairing"), &(lines.join("\n") + "\n"));
        }

        /// 假後台對這把密鑰回應的 HTTP 狀態碼(沒設定的回 200)。
        fn respond(&self, key: &str, status: u16) {
            let path = self.path("backend-responses");
            let mut lines = std::fs::read_to_string(&path).unwrap_or_default();
            lines.push_str(&format!("{key} {status}\n"));
            std::fs::write(path, lines).unwrap();
        }

        fn is_paired(&self, device: &str) -> bool {
            self.path(&format!(".purrdr/push-{device}")).exists()
        }

        /// 以 herdr 的方式執行外掛：新程序、帶事件環境變數。
        fn run_hook(&self, event: &str, context: &str) -> i32 {
            let bin = self.path("bin");
            let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default());
            let status = Command::new("sh")
                .arg(plugin_directory().join("notify.sh"))
                .current_dir(plugin_directory())
                .env("HOME", self.home.path())
                .env("PATH", path)
                .env("HERDR_ENV", "1")
                .env("HERDR_PLUGIN_ID", "purrdr.push")
                .env("HERDR_PLUGIN_EVENT", "pane.agent_status_changed")
                .env("HERDR_PLUGIN_EVENT_JSON", event)
                .env("HERDR_PLUGIN_CONTEXT_JSON", context)
                .env("HERDR_BIN_PATH", bin.join("herdr"))
                .status()
                .unwrap();
            status.code().unwrap_or(-1)
        }

        fn curl_calls(&self) -> Vec<CurlCall> {
            let directory = self.path("curl-calls");
            let Ok(entries) = std::fs::read_dir(&directory) else {
                return Vec::new();
            };
            let mut names: Vec<String> = entries.map(|entry| entry.unwrap().file_name().into_string().unwrap()).collect();
            names.sort();
            names
                .iter()
                .map(|name| {
                    let raw_body = std::fs::read_to_string(directory.join(name).join("stdin")).unwrap();
                    let arguments = std::fs::read_to_string(directory.join(name).join("args"))
                        .unwrap()
                        .lines()
                        .filter(|line| !line.is_empty())
                        .map(str::to_owned)
                        .collect();
                    let body = match serde_json::from_str::<Value>(&raw_body) {
                        Ok(Value::Object(body)) => body,
                        _ => Map::new(),
                    };
                    CurlCall { arguments, body, raw_body }
                })
                .collect()
        }

        fn keys(&self) -> Vec<String> {
            self.curl_calls()
                .iter()
                .filter_map(|call| call.body.get("key")?.as_str().map(str::to_owned))
                .collect()
        }
    }

    /// Agent 進入「需要你」:帶配對密鑰呼叫後台，內容只有 workspace 名稱、狀態與 pane
    #[test]
    fn blocked_notifies_backend() {
        let host = Host::new();
        host.pair("a1", &key_a(), &[]);

        assert_eq!(host.run_hook(&blocked(), &default_context()), 0);

        let calls = host.curl_calls();
        assert_eq!(calls.len(), 1);
        let call = &calls[0];
        assert_eq!(call.arguments.last().map(String::as_str), Some(ENDPOINT));
        let mut keys: Vec<&str> = call.body.keys().map(String::as_str).collect();
        keys.sort();
        assert_eq!(keys, ["key", "pane", "status", "workspace"]);
        assert_eq!(call.body["key"], key_a().as_str());
        assert_eq!(call.body["workspace"], "weather-app");
        assert_eq!(call.body["status"], "blocked");
        assert_eq!(call.body["pane"], "w2:p1");
    }

    /// 推播請求不含終端機標題等對話內容
    #[test]
    fn request_carries_no_conversation_content() {
        let host = Host::new();
        host.pair("a1", &key_a(), &[]);

        host.run_hook(&blocked(), &default_context());

        let calls = host.curl_calls();
        let call = calls.first().expect("有送出推播");
        assert!(!call.raw_body.contains("secret-plan"));
        assert!(!call.raw_body.contains("prod"));
        assert!(!call.raw_body.contains("Claude"));
    }

    /// 配對密鑰經 stdin 交給 curl,不出現在指令參數(其他使用者用 ps 看得到參數)
    #[test]
    fn key_is_not_in_arguments() {
        let host = Host::new();
        host.pair("a1", &key_a(), &[]);

        host.run_hook(&blocked(), &default_context());

        let calls = host.curl_calls();
        let call = calls.first().expect("有送出推播");
        assert!(!call.arguments.join(" ").contains(&key_a()));
        assert!(call.arguments.iter().any(|argument| argument == "--max-time"));
    }

    /// 其他狀態(工作中、完成、閒置)不通知
    #[test]
    fn other_statuses_do_nothing() {
        for status in ["working", "done", "idle", "unknown"] {
            let host = Host::new();
            host.pair("a1", &key_a(), &[]);

            assert_eq!(host.run_hook(&event(status, "w2:p1"), &default_context()), 0, "{status}");

            assert!(host.curl_calls().is_empty(), "{status} 不該通知");
        }
    }

    /// 沒有推播配對時什麼都不做
    #[test]
    fn no_pairing_does_nothing() {
        let host = Host::new();

        assert_eq!(host.run_hook(&blocked(), &default_context()), 0);
        assert!(host.curl_calls().is_empty());
    }

    /// 一台主機配對多支裝置：每支各送一次，用各自的密鑰
    #[test]
    fn every_paired_device_is_notified() {
        let host = Host::new();
        host.pair("a1", &key_a(), &[]);
        host.pair("b2", &key_b(), &[]);

        host.run_hook(&blocked(), &default_context());

        let mut keys = host.keys();
        keys.sort();
        assert_eq!(keys, [key_a(), key_b()]);
    }

    /// 某支裝置靜音了這個聊天(session):只跳過那支裝置
    #[test]
    fn muted_chat_skips_that_device() {
        let host = Host::new();
        host.pair("a1", &key_a(), &["session-weather"]);
        host.pair("b2", &key_b(), &["session-other"]);

        host.run_hook(&blocked(), &default_context());

        assert_eq!(host.keys(), [key_b()]);
        let herdr_calls = std::fs::read_to_string(host.path("herdr-calls")).unwrap();
        assert!(herdr_calls.contains("pane get w2:p1"));
    }

    /// 沒有任何靜音時不必問 herdr
    #[test]
    fn without_mutes_herdr_is_not_asked() {
        let host = Host::new();
        host.pair("a1", &key_a(), &[]);

        host.run_hook(&blocked(), &default_context());

        assert!(!host.path("herdr-calls").exists());
    }

    /// workspace 名稱含引號與反斜線時仍送出合法 JSON,名稱不變
    #[test]
    fn workspace_label_escapes_survive() {
        let host = Host::new();
        host.pair("a1", &key_a(), &[]);

        host.run_hook(&blocked(), &context(r#"my \"app\" \\ 天氣"#));

        let calls = host.curl_calls();
        let call = calls.first().expect("有送出推播");
        assert_eq!(call.body["workspace"], r#"my "app" \ 天氣"#);
    }

    /// 沒有 workspace 名稱時用 workspace id
    #[test]
    fn missing_label_falls_back_to_workspace_id() {
        let host = Host::new();
        host.pair("a1", &key_a(), &[]);

        host.run_hook(&blocked(), r#"{"workspace_id":"w2"}"#);

        assert_eq!(host.curl_calls().first().map(|call| call.body["workspace"].clone()), Some(Value::from("w2")));
    }

    /// pane id 形狀不對時不送 pane
    #[test]
    fn malformed_pane_is_omitted() {
        let host = Host::new();
        host.pair("a1", &key_a(), &[]);

        host.run_hook(&event("blocked", "w2:p1;rm"), &default_context());

        let calls = host.curl_calls();
        let call = calls.first().expect("有送出推播");
        assert!(!call.body.contains_key("pane"));
    }

    /// 後台回 410(配對已撤銷):刪掉那支裝置的配對資料夾，其他裝置照送
    #[test]
    fn revoked_pairing_is_removed() {
        let host = Host::new();
        host.pair("a1", &key_a(), &[]);
        host.pair("b2", &key_b(), &[]);
        host.respond(&key_a(), 410);

        assert_eq!(host.run_hook(&blocked(), &default_context()), 0);

        let mut keys = host.keys();
        keys.sort();
        assert_eq!(keys, [key_a(), key_b()]);
        assert!(!host.is_paired("a1"));
        assert!(host.is_paired("b2"));
    }

    /// 送出後才重新配對(新密鑰)時，舊密鑰的 410 不刪新的配對
    #[test]
    fn revoked_old_key_keeps_a_pairing_renewed_during_the_call() {
        let host = Host::new();
        host.pair("a1", &key_a(), &[]);
        host.respond(&key_a(), 410);
        host.write("repair-during-call/device", "a1");
        host.write("repair-during-call/pairing", &format!("purrdr-push 1\nendpoint={ENDPOINT}\nkey={}\n", key_b()));

        host.run_hook(&blocked(), &default_context());

        assert!(host.is_paired("a1"));
    }

    /// 撤銷的配對刪掉後，下次不再對它呼叫後台
    #[test]
    fn removed_pairing_is_not_called_again() {
        let host = Host::new();
        host.pair("a1", &key_a(), &[]);
        host.respond(&key_a(), 410);

        host.run_hook(&blocked(), &default_context());
        host.run_hook(&blocked(), &default_context());

        assert_eq!(host.keys(), [key_a()]);
    }

    /// 後台其他錯誤(暫時性)不刪配對
    #[test]
    fn other_backend_errors_keep_pairing() {
        for status in [400, 401, 404, 429, 500, 503] {
            let host = Host::new();
            host.pair("a1", &key_a(), &[]);
            host.respond(&key_a(), status);

            host.run_hook(&blocked(), &default_context());

            assert!(host.is_paired("a1"), "{status} 不該刪配對");
        }
    }
}

/// `notify.ps1` 的後台回應處理：真的以 Windows PowerShell 執行，後台換成本機的假 HTTP 伺服器。
#[cfg(windows)]
mod notify_ps1 {
    use std::collections::HashMap;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::path::PathBuf;
    use std::process::Command;
    use std::sync::{Arc, Mutex};

    use serde_json::Value;
    use tempfile::TempDir;

    use super::plugin_directory;

    const BLOCKED: &str =
        r#"{"event":"pane_agent_status_changed","data":{"type":"pane_agent_status_changed","pane_id":"w2:p1","workspace_id":"w2","agent_status":"blocked"}}"#;
    const CONTEXT: &str = r#"{"workspace_id":"w2","workspace_label":"weather-app"}"#;

    /// 依請求本文的 `key` 回應指定狀態碼(沒設定的回 200),記下收到的密鑰。
    struct Backend {
        endpoint: String,
        received: Arc<Mutex<Vec<String>>>,
    }

    impl Backend {
        fn start(responses: HashMap<String, u16>) -> Self {
            Backend::start_renewing(responses, None)
        }

        /// 回應前先把 `renewal` 的內容寫進那個檔案(送出期間重新配對)。
        fn start_renewing(responses: HashMap<String, u16>, renewal: Option<(PathBuf, String)>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let endpoint = format!("http://{}/v1/push", listener.local_addr().unwrap());
            let received = Arc::new(Mutex::new(Vec::new()));
            let recorder = Arc::clone(&received);
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    let Ok(mut stream) = stream else { continue };
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut length = 0;
                    loop {
                        let mut line = String::new();
                        if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                            break;
                        }
                        if let Some((name, value)) = line.split_once(':')
                            && name.eq_ignore_ascii_case("content-length")
                        {
                            length = value.trim().parse().unwrap_or(0);
                        }
                    }
                    let mut body = vec![0; length];
                    if reader.read_exact(&mut body).is_err() {
                        continue;
                    }
                    let key = serde_json::from_slice::<Value>(&body)
                        .ok()
                        .and_then(|body| body["key"].as_str().map(str::to_owned))
                        .unwrap_or_default();
                    let status = responses.get(&key).copied().unwrap_or(200);
                    recorder.lock().unwrap().push(key);
                    if let Some((path, contents)) = &renewal {
                        std::fs::write(path, contents).unwrap();
                    }
                    let reply = format!("HTTP/1.1 {status} Status\r\ncontent-type: application/json\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{{}}");
                    let _ = stream.write_all(reply.as_bytes());
                }
            });
            Backend { endpoint, received }
        }

        fn keys(&self) -> Vec<String> {
            let mut keys = self.received.lock().unwrap().clone();
            keys.sort();
            keys
        }
    }

    struct Host {
        home: TempDir,
    }

    impl Host {
        fn new() -> Self {
            Host {
                home: tempfile::tempdir().unwrap(),
            }
        }

        fn pairing_directory(&self, device: &str) -> PathBuf {
            self.home.path().join(".purrdr").join(format!("push-{device}"))
        }

        fn pair(&self, device: &str, key: &str, endpoint: &str) {
            let directory = self.pairing_directory(device);
            std::fs::create_dir_all(&directory).unwrap();
            std::fs::write(directory.join("pairing"), format!("purrdr-push 1\nendpoint={endpoint}\nkey={key}\n")).unwrap();
        }

        fn run_hook(&self) -> i32 {
            Command::new("powershell")
                .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"])
                .arg(plugin_directory().join("notify.ps1"))
                .env("USERPROFILE", self.home.path())
                .env("HERDR_PLUGIN_EVENT", "pane.agent_status_changed")
                .env("HERDR_PLUGIN_EVENT_JSON", BLOCKED)
                .env("HERDR_PLUGIN_CONTEXT_JSON", CONTEXT)
                .status()
                .unwrap()
                .code()
                .unwrap_or(-1)
        }
    }

    fn key_a() -> String {
        "ab".repeat(32)
    }

    fn key_b() -> String {
        "cd".repeat(32)
    }

    /// 後台回 410(配對已撤銷):刪掉那支裝置的配對資料夾，其他裝置照送
    #[test]
    fn revoked_pairing_is_removed() {
        let backend = Backend::start(HashMap::from([(key_a(), 410)]));
        let host = Host::new();
        host.pair("a1", &key_a(), &backend.endpoint);
        host.pair("b2", &key_b(), &backend.endpoint);

        assert_eq!(host.run_hook(), 0);

        assert_eq!(backend.keys(), [key_a(), key_b()]);
        assert!(!host.pairing_directory("a1").exists());
        assert!(host.pairing_directory("b2").exists());
    }

    /// 送出後才重新配對(新密鑰)時，舊密鑰的 410 不刪新的配對
    #[test]
    fn revoked_old_key_keeps_a_pairing_renewed_during_the_call() {
        let host = Host::new();
        let renewed = host.pairing_directory("a1").join("pairing");
        let backend = Backend::start_renewing(
            HashMap::from([(key_a(), 410)]),
            Some((renewed, format!("purrdr-push 1\nendpoint=http://127.0.0.1:9/v1/push\nkey={}\n", key_b()))),
        );
        host.pair("a1", &key_a(), &backend.endpoint);

        host.run_hook();

        assert!(host.pairing_directory("a1").exists());
    }

    /// 後台其他錯誤(暫時性)不刪配對
    #[test]
    fn other_backend_errors_keep_pairing() {
        let backend = Backend::start(HashMap::from([(key_a(), 500), (key_b(), 404)]));
        let host = Host::new();
        host.pair("a1", &key_a(), &backend.endpoint);
        host.pair("b2", &key_b(), &backend.endpoint);

        host.run_hook();

        assert_eq!(backend.keys(), [key_a(), key_b()]);
        assert!(host.pairing_directory("a1").exists());
        assert!(host.pairing_directory("b2").exists());
    }
}
