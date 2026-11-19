//! 主機外掛腳本(`host-plugin/`,raychiutw/purrdr.ios ADR-0004):herdr 在 `pane.agent_status_changed` 時以新程序執行它。
//!
//! 在暫存家目錄真的執行 `host-plugin/notify.sh`,herdr 給的環境變數照 herdr 文件
//! (`HERDR_PLUGIN_EVENT`、`HERDR_PLUGIN_EVENT_JSON`、`HERDR_PLUGIN_CONTEXT_JSON`、`HERDR_BIN_PATH`)手動帶入;
//! `curl` 與 `herdr` 換成家目錄 `bin/` 裡的假程式，記錄收到的參數與 stdin,不連網路、不碰真的 herdr。
//!
//! 移植自 app repo 的 `tests/PurrdrCoreTests/HostPluginScriptTests.swift`,每個測試的說明是原本的測試名稱。
//! `notify.sh` 只在 macOS／Linux 執行(manifest 限定平台),所以執行腳本的測試只在 Unix 跑;
//! Windows 的 `notify.ps1` 是 best-effort,沒有自動測試。manifest 的檢查三個平台都跑。

use std::path::{Path, PathBuf};

const PLUGIN_ID: &str = "purrdr.push";

fn plugin_directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host-plugin")
}

/// manifest:只掛 pane.agent_status_changed,macOS／Linux 用 sh、Windows 用 PowerShell 腳本(檔案都在)
#[test]
fn manifest_declares_hooks() {
    let manifest = std::fs::read_to_string(plugin_directory().join("herdr-plugin.toml")).unwrap();
    assert!(manifest.contains(&format!(r#"id = "{PLUGIN_ID}""#)));
    assert!(manifest.contains(r#"on = "pane.agent_status_changed""#));
    assert!(manifest.contains(r#"command = ["sh", "notify.sh"]"#));
    assert!(manifest.contains("notify.ps1"));
    assert!(plugin_directory().join("notify.ps1").is_file());
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
}
