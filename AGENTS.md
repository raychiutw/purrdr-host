# AGENTS.md

## 專案性質

Purrdr 的主機端元件：裝在電腦(主機)上、給 Purrdr app(iPhone／iPad)連的部分。從 app repo [`raychiutw/purrdr.ios`](https://github.com/raychiutw/purrdr.ios) 抽出(保留歷史),公開發行。

- **主機服務**(`host-service/`):HTTPS 連線方式在主機上執行的常駐程式(Rust)。只綁 127.0.0.1,對外由 `tailscale serve` 提供 HTTPS。
- **主機外掛**(`host-plugin/`):herdr plugin,Agent 進入「需要你」時通知 Purrdr 後台推播。
- **協定**(`protocol/`):app 與主機服務之間的協定，`host-service.schema.json` 是唯一定義。

這個 repo 沒有自己的 ADR、行為規格與詞彙表，一律引用 app repo:
- 詞彙 [`GLOSSARY.md`](https://github.com/raychiutw/purrdr.ios/blob/master/GLOSSARY.md)
- 決策 [`docs/adr/`](https://github.com/raychiutw/purrdr.ios/tree/master/docs/adr)(主機服務是 [ADR-0010](https://github.com/raychiutw/purrdr.ios/blob/master/docs/adr/0010-https-via-host-service-and-tailscale-serve.md),推播是 [ADR-0004](https://github.com/raychiutw/purrdr.ios/blob/master/docs/adr/0004-push-via-host-plugin-and-relay.md),Pane 終端機是 [ADR-0005](https://github.com/raychiutw/purrdr.ios/blob/master/docs/adr/0005-pane-terminal-via-session-control.md),會寫入主機的功能是 [ADR-0002](https://github.com/raychiutw/purrdr.ios/blob/master/docs/adr/0002-zero-install-host-over-ssh.md))
- **clean-room**([ADR-0001](https://github.com/raychiutw/purrdr.ios/blob/master/docs/adr/0001-clean-room-from-herdrchat.md)):不閱讀、不複製 herdrchat 原始碼;只依據 app repo 的 `docs/research/herdrchat-behavior-spec.md`、herdr 官方文件與真實主機實測。
- issue 與 spec 都在 app repo 的 GitHub Issues。本 repo 的文件、註解與 commit message 提到 issue 一律寫成 `raychiutw/purrdr.ios#<數字>`(抽出前的舊 commit 寫的 `#<數字>` 也是指 app repo)。

## 目錄

```text
host-service/                  # Rust workspace(根目錄的 cargo 指令都在這裡跑)
  service/                     # 主機服務本體(crate purrdr-host-service)
  codegen/                     # 協定型別產生器(crate protocol-codegen)
  plugin-tests/                # 主機外掛 notify.sh 的測試(crate host-plugin-tests,只有 tests/)
  install/                     # 一行安裝腳本 install.sh／install.ps1(隨 Release 發佈)
host-plugin/                   # herdr plugin:herdr-plugin.toml、notify.sh、notify.ps1
protocol/                      # host-service.schema.json(唯一定義)與 examples/(兩端共用的範例)
generated/swift/               # codegen 產生的 Swift 協定型別(app repo 同步用，不要手改)
fixtures/herdr-0.9.5/          # 從真實主機錄的 herdr 回應(測試回放用)
scripts/host-service-version.sh  # 印出某個 commit 的主機服務版本號(Release 比對用)
.github/workflows/             # ci.yml(PR)、host-service-release.yml(發行)
```

## 常用指令

```bash
cd host-service && cargo test --workspace --locked       # 全部測試(主機服務、codegen 同步、主機外掛腳本)
cd host-service && cargo fmt --all --check
cd host-service && cargo clippy --workspace --all-targets --locked -- -D warnings
cd host-service && cargo run -p protocol-codegen          # 改了 protocol/host-service.schema.json 後重新產生 Rust 與 Swift 型別
cd host-service && cargo run -p protocol-codegen -- --check  # 只檢查產生的檔案是否最新
cd host-service && cargo clean                            # 磁碟有限時，跑完清掉 target/
actionlint                                                # 改了 workflow 後檢查
```

零 warning:CI 以 `RUSTFLAGS=-D warnings` 建置，Linux 另跑 fmt 與 clippy `-D warnings`。

CI(`.github/workflows/ci.yml`,`pull_request` 與 `workflow_call`):macOS(`macos-26`)、Linux(`ubuntu-24.04`)、Windows(`windows-2025`)三平台 `cargo test --workspace --locked`;Linux 另跑 `cargo fmt --all --check` 與 clippy。actions 一律 pin 到 commit SHA。

## 協定與 codegen(app repo 的來源)

- 協定只改 `protocol/host-service.schema.json`,再 `cargo run -p protocol-codegen` 產生 `host-service/service/src/protocol.rs` 與 `generated/swift/HostServiceProtocol.swift`(`HostServiceProtocol` 命名空間)。同步與否由 `codegen/tests/generated_files.rs` 把關。
- schema 標 `x-write: true` 的訊息由 codegen 產生 `is_write()`／`isWrite`,主機服務統一以此拒絕唯讀裝置並寫稽核紀錄，新增寫入訊息只要標 `x-write`。
- **app repo 以同步腳本從這裡拉取** `protocol/`(schema、`examples/` 與兩端共用的案例 `host-file-access.json`、`host-path-mentions.json`)和 `generated/swift/HostServiceProtocol.swift`(放到它的 `src/PurrdrCore/HostServiceProtocol.swift`)。`protocol/examples/` 由 Rust(`service/tests/protocol_examples.rs`)與 Swift(app repo 的 `HostServiceProtocolExampleTests`)兩端各自編解碼。**改了協定，合併後要在 app repo 同步並讓它的測試通過**;app 端要支援新的 herdr 操作時，先在這裡的 schema 加訊息。

## 主機服務(`host-service/service/`,ADR-0010、raychiutw/purrdr.ios#55)

- Rust,只綁 127.0.0.1(預設埠 47820,`--port` 可改;不做 TLS,對外由 `tailscale serve` 提供 HTTPS)。
- WebSocket 路徑 `/v1/ws`(`connection.rs`:讀取、依序處理請求、送出分成三段，處理請求時仍回 pong;每 15 秒送 ping,45 秒沒收到 app 任何 frame 就斷線，`Heartbeat`)。每個 herdr 請求開一條連線直接連 herdr 本機 socket(Unix socket／`\\.\pipe\<socket 路徑>`,解析順序 `--session` → `HERDR_SOCKET_PATH` → `HERDR_SESSION` → 預設)。
- 事件訂閱(raychiutw/purrdr.ios#58)是保持開著的連線(`HerdrClient::subscribe`),以 `events.event` 轉送、herdr 結束時送 `events.ended`,`events.unsubscribe` 或 app 斷線即關掉，不輪詢 snapshot。
- 服務本身不執行任意子行程(`no_command_execution` 測試擋，例外只有安裝指令用的 `install/system.rs`、Pane 終端機的 `pane_terminal.rs` 與推播外掛代裝的 `push_plugin.rs`)。
- **Pane 終端機**(raychiutw/purrdr.ios#61,ADR-0005):`pane.terminal.open` 以子行程執行 `herdr [--session <名稱>] terminal session control <target> --cols --rows [--takeover]`(唯讀裝置用 `observe`,忽略 takeover),stdout 的 frame 原樣轉成 `pane.terminal.frame`(經送出段的有界佇列，送不出去時 herdr 那端等),`terminal.closed` 轉成 `pane.terminal.closed`(`ending`:closed／disconnected(沒有 closed 就 EOF、exit 0)／failed(exit 非 0,帶 stderr));input／resize／scroll 標 `x-write`、不回應(scroll 的 `source` 沒帶時是 wheel,page_key 原樣轉給 herdr),稽核只在 control 接上時記一筆。target 只收 pane／terminal id 的字元、不能以 `-` 開頭，格數上限 500×200,單則輸入 64 KiB。`pane.terminal.close` 或 WSS 結束(含心跳逾時):control 關 stdin(等同 release)等 2 秒，observe 直接結束子行程(Windows 的 observer 關 stdin 不會結束)並補送 closed;子行程也設了 `kill_on_drop`,Windows 以 `CREATE_NO_WINDOW` 執行。herdr 執行檔每次執行時才找(`HerdrCli`:PATH、Homebrew、`~/.local/bin`、`~/bin`、`%LOCALAPPDATA%\Programs\Herdr\bin\herdr.exe`)。
- **對話紀錄**(raychiutw/purrdr.ios#59,`transcript.rs` 讀檔規則、`transcript_session.rs` 一條連線的狀態):`transcript.locate` 只接受 herdr snapshot 目前回報的 agent_session(格式與 cwd 取自 snapshot;id 須是安全字元，path 只限 agy／Pi／OMP 且是絕對、`.jsonl`、無 `..`),視窗與追蹤只接受這條連線定位過的路徑(`transcript_not_allowed`),所以主機服務不提供任意檔案讀取(主機取檔另見下一項)。位置：Claude(`CLAUDE_CONFIG_DIR`,含 worktree 後備)、Codex(`CODEX_HOME`;多個 rollout 回有歧義)、Antigravity、回報的路徑;Grok 與 Pi 以 id 定位回 `transcript_unsupported`。家目錄是 Unix `HOME`／Windows `USERPROFILE`(`TranscriptEnvironment`,測試用暫存家目錄)。追蹤每 250ms 檢查檔案大小，有新的完整行推 `transcript.update`(raychiutw/purrdr.ios#173:每則最多 limit bytes 的完整行，落後超過 limit 時從游標分段接著送，第一行就超過 limit 時只送那一行、剝除圖片後截短到 limit bytes、length 照原始長度，游標照樣前進——規則同 app 的 SSH `catchUp` 腳本;不再以 `too_far_behind` 結束，該值留在 schema 給舊版主機服務),檔案變小(`shrank`)、游標前 64 byte 的內容變了(`replaced`,被換成同長或更長的新檔)、檔案不見或讀不到時推結束原因後停止;`transcript.unfollow` 或連線結束即停止。推送經 `connection.rs` 的送出段與回應、事件共用同一個 WebSocket。行的格式(原始長度 + 剝除圖片)與 app 的 SSH 腳本相同。
- **主機取檔**(raychiutw/purrdr.ios#227,`host_files.rs`,ADR-0010,0.4.0 起):`file.stat`／`file.read`(唯讀，不標 `x-write`)只允許 herdr `session.snapshot` 當下回報的 Agent cwd 之下，或路徑出現在這條連線定位過的對話紀錄裡——某個 JSON 字串值恰好是這個路徑，或字串值的文字中以訊息內路徑規則認得它(raychiutw/purrdr.ios#229,`host_path_mentions.rs`,與 app 的 `HostPathMention` 共用 `protocol/host-path-mentions.json` 案例)(app 送的 `root` 不採用);拒絕相對路徑與 `.`／`..`,請求路徑與 `canonicalize` 後的實際路徑都比對排除清單，cwd 放行的再確認實際路徑在解析後的 cwd 之下(Windows 不分大小寫);上限 25 MiB、每塊最多 1 MiB;版本是內容 SHA-256,從頭讀(或沒有這個版本的快照)時重新檢查並建這條連線的記憶體快照，之後的分塊從快照讀，版本不符回 `file_changed`;`audit.log` 的 `file_access` 記 stat 與每次建快照的路徑、大小、結果(不記內容)。排除清單、暫存目錄與大小上限是 schema 頂層的共用常數(`x-excluded-file-locations`、`x-temporary-file-locations`、`x-maximum-file-size`、`x-maximum-file-chunk`,codegen 兩端都產生);存取規則的共用案例 `protocol/host-file-access.json`(`tests/host_file_access.rs`;app 與 SSH 取檔腳本跑同一份)。
- **推播外掛代裝**(raychiutw/purrdr.host#10、raychiutw/purrdr.ios#261,0.6.0 起,`push_plugin.rs`):`push.status`(唯讀)、`push.install`／`push.remove`(`x-write`)讓 HTTPS 主機也能開通知。以本機使用者身分執行 herdr CLI:`plugin list --plugin purrdr.push --json`(帶 `--session`)、`plugin install raychiutw/purrdr.host/host-plugin --yes` 與 `plugin uninstall purrdr.push`(不帶 `--session`;來源固定，app 不能指定)。檔案與 app 的 SSH 路徑(`PushHost`)相同，兩條路徑可互相移除：配對 `~/.purrdr/push-<裝置 id>/pairing`(`PushPairingFile` 格式)加空的安裝紀錄 `journal.json`、「外掛是 Purrdr 裝的」紀錄 `~/.purrdr/push-plugin/`(`journal.json`、`source`);只有本人可讀(Unix 0700／0600)。裝置 id 規則同 app(1–64 個小寫英數與 `-`,不能是 `plugin`),endpoint 須 https、key 64 位小寫 hex,不合格回 `invalid_message`。多裝置：install 外掛已裝就只寫配對;安裝失敗移除剛寫的配對與紀錄、回 `push_plugin_failed`(message 是 herdr 的輸出);remove 只刪自己的配對，沒有其他 `push-*/pairing` 且有 `push-plugin` 紀錄才 uninstall,使用者自己裝的外掛不動。回應都帶 `plugin_installed`、`paired`、`other_devices`(不提供其他裝置的識別資訊)。請求以一把鎖依序處理;稽核只記操作種類，不記密鑰。測試 `tests/push_plugin.rs`(`harness = false`,假 herdr CLI 是測試執行檔本身，暫存家目錄)。
- **連線配對與裝置權杖**(raychiutw/purrdr.ios#56):HTTP 入口用 axum。`POST /v1/pair`(本文 `PairRequest`)以配對碼換權杖,`POST /v1/revoke` 撤銷自己;WSS 與撤銷都帶 `Authorization: Bearer <權杖>`,無效／撤銷時送 `error` 後以 close code 4401 關閉，且每個訊息都重新驗證。檔案在 `~/.purrdr/host-service/`(`--state-dir` 可改):`devices.json`(只存 SHA-256,跨行程以 `devices.lock` 檔案鎖)、`audit.log`(JSON Lines,不記按鍵、prompt、權杖、配對碼)、`config.json` 的 `https_address`(安裝流程寫入,`pair` 的預設位址)。管理指令 `pair [--read-only] [--address]`、`devices`、`revoke <裝置 ID>`。`welcome` 帶 `read_only`,app 以此停用輸入。
- **安裝**(raychiutw/purrdr.ios#62,`install [--port] [--session] [--https-port] [--yes]`／`uninstall`／`status`,`src/install/`):檢查 Tailscale、MagicDNS、HTTPS Certificates(失敗指向 `login.tailscale.com/admin/dns`),說明 Certificate Transparency 並徵求同意。**與其他 `tailscale serve` 服務並存**(raychiutw/purrdr.ios#218):HTTPS 埠預設 443,安裝紀錄的 `https_port` 保存選擇(0.2.0 的紀錄沒有這欄，視為 443),再次 install 沿用;不覆蓋別人的 serve,沒指定 `--https-port` 而埠被占用時問要不要改用候選埠(`CANDIDATE_HTTPS_PORTS`:443、8443、10000,Funnel 也允許的埠;`--yes` 直接改用),指定的埠被占用就失敗並提示可用埠;換埠時關掉舊埠，uninstall 只關自己的埠(`tailscale serve --https=<埠> off`,且只在仍指向主機服務時)。HTTPS 位址非 443 時帶 `:埠`(`config.json`、`pair` 的 QR code)。失敗訊息(`InstallError::explain`)是「發生什麼、為什麼」加最後的 `Next:` 完整重跑指令，一行安裝腳本以環境變數 `PURRDR_HOST_SERVICE_INSTALL_COMMAND` 傳入重跑指令(主機服務補上原本的選項與 `--https-port`),腳本自己的失敗(下載、SHA256)也有 `Next:`,主機服務失敗時腳本保留其訊息只補一行指回去(`install_messages`、`install_script` 測試);檔案在 `~/.purrdr/host-service/`(`bin/`、`config.json` 的 `https_address`、安裝紀錄 `install.json`;`~/.purrdr/` 與 app 的其他功能共用，只在安裝時新建且空了才刪)。自動啟動:macOS LaunchAgent `com.raychiu.purrdr.host-service`、Linux systemd user unit `purrdr-host-service.service`、Windows `HKCU\…\Run` 的 `PurrdrHostService` 指向不開視窗的 `purrdr-host-service-background.exe`(`windows_subsystem`)。再次 install 就地更新、保留已配對裝置;失敗時依紀錄還原。**PATH**(raychiutw/purrdr.host#9,`install/user_path.rs`):安裝把 `bin/` 加進使用者層級 PATH——Windows 經 `HostSystem::user_path`／`set_user_path` 讀寫 `HKCU\Environment` 的 `Path`(不展開的原始值，以 REG_EXPAND_SZ 寫回，再廣播 WM_SETTINGCHANGE;真實作是 `system.rs` 的 `windows_environment`,以 PowerShell 執行，測試用暫存機碼),已經有就不重複;macOS／Linux 依 `SHELL` 在 `.zshrc`、`.bash_profile`(macOS)／`.bashrc`、fish 的 `config.fish` 或 `.profile` 加 `# >>> purrdr-host-service >>>` 標記區塊，重跑時換掉。安裝紀錄的 `add_to_path`(`--no-path` 關閉、再次安裝沿用，舊紀錄視為開)與 `path`(做了哪些變更)讓解除安裝只移除自己加的(新建的設定檔移除後是空的就刪);自動更新不改 PATH。
- **更新**(raychiutw/purrdr.host#6,`update [--check] [--yes]`／`version`,`src/update.rs`):更新流程模組 `Updater` 只對外「檢查」與「套用」(另有背景用的 `automatic`),往外接兩個介面——Release 來源 `ReleaseSource`(`HttpReleaseSource::github()` 打 `api.github.com/repos/raychiutw/purrdr.host/releases/latest`,只有這個 API 呼叫帶 `Accept: application/json`,下載不帶;tag `host-service-v<版本>`;環境變數 `PURRDR_HOST_SERVICE_RELEASE_URL` 換成 `<base>/releases/latest` 格式的來源，**只在 debug 建置或 `--features release-url-override` 生效**,正式版忽略)與安裝位置 `InstallLocation`(`install/location.rs` 的 `InstalledService`)。只往更大的語意化版本更新;套用時先拿狀態資料夾的 `update.lock`(檔案鎖，手動與自動互斥，拿不到回 `InProgress`),下載 `SHA256SUMS` 與壓縮檔，不符就不替換;解到每次不同名稱的 `update-<pid>-…/`,只接受一般檔案與資料夾(tar 的 symlink／hardlink、zip 的 symlink 整個拒絕);macOS 以 `codesign --verify --strict` 要求 Developer ID 且 Team ID 是常數 `8Z6WVFJ574`(不看已安裝的那份)。替換：Unix 先 hard link 舊檔成 `<名稱>.previous` 再 rename 新檔原子覆蓋;Windows 把執行中的檔案改名成 `.previous` 再放新檔，失敗即換回;成功後刪掉 `.previous`(Windows 上更新程序自己就是 `.previous`,刪不掉時留到下次)。**重啟就地進行、不取消登記**:macOS `launchctl kickstart -k`、Linux `systemctl --user restart`、Windows 只結束背景程序再啟動(plist／unit／Run 值都不動);執行中的服務開始聽埠後寫 `running.json`(版本、埠、pid、啟動時間，`running_service.rs`),重啟後限時內要有「這次重啟之後啟動、回報新版本」的 `running.json` 且埠可連，否則換回 `.previous` 再重啟舊版(`RolledBack`),並把那個版本記在 `update.json` 的 `rolled_back`,背景自動更新不再重試它(`Skipped`),手動 `update` 仍會再試，更新成功後清掉。錯誤沿用「Next:」格式;套用與自動檢查的結果寫進 `update.json`(最後一次，`version` 顯示)與 `logs/update.log`。**自動更新**(raychiutw/purrdr.host#7,`src/auto_update.rs`):只有從安裝位置執行的背景服務(`runs_installed_copy`)才排程，啟動後 1–5 分鐘先檢查，之後每 24 小時加最多 1 小時抖動，網路錯誤 1 小時內重試(排程以注入的亂數與 tokio 暫停時鐘測);`Updater::automatic` 讀安裝紀錄的 `auto_update`(舊紀錄沒有這欄視為開;`auto-update on|off`、`install --no-auto-update` 改它，`status`／`version` 顯示),關閉時不連網路，有新版時不在服務裡替換，而是以獨立程序執行安裝好的 `update --yes`(`system.rs` 的 `spawn_update_helper`:Linux 只用 `systemd-run --user` 放進另一個 unit,失敗就回報、下次排程再試(不改用會被 cgroup 一起結束的方式),macOS 另開 process group,Windows 是 console 執行檔所以不會被結束背景執行檔波及),由它走手動更新同一條流程。測試：`tests/update.rs`(假 Release 伺服器 `install_support/release.rs` + 暫存家目錄 + 假系統，假系統啟動「假主機服務」時真的聽埠並寫 `running.json`)、`tests/update_cli.rs`(執行檔的輸出)、`tests/auto_update.rs`(排程)。
- **一行安裝腳本**(`install/install.sh`／`install.ps1`,raychiutw/purrdr.ios#198):從本 repo 的 Release(`https://github.com/raychiutw/purrdr.host/releases/latest/download/`)下載這台主機的壓縮檔，以 `SHA256SUMS` 驗證後執行 `purrdr-host-service install`。環境變數 `PURRDR_HOST_SERVICE_VERSION`(指定版本)、`PURRDR_HOST_SERVICE_DOWNLOAD_URL`(測試用來源)。安裝說明網頁在 app repo 的 `docs/site/purrdr/host-service/`(https://raychiutw.github.io/purrdr/host-service/)。

## 主機外掛(`host-plugin/`,ADR-0004、raychiutw/purrdr.ios#29)

- herdr plugin manifest(`herdr-plugin.toml`,id `purrdr.push`)+ `notify.sh`(macOS／Linux,sh + curl)／`notify.ps1`(Windows,best-effort;herdr 的 plugin 在 Windows 仍是 preview)。herdr 在 `pane.agent_status_changed` 時以新程序執行，沒有常駐程序、不寫狀態;後台回 410(裝置被遠端關閉、配對撤銷)時刪掉那支裝置的 `push-*` 資料夾(raychiutw/purrdr.host#11)。`notify.ps1` 帶 UTF-8 BOM(Windows PowerShell 5.1 讀沒 BOM 的檔案用系統字碼頁，中文註解會解析失敗)。
- app 經 SSH(或 HTTPS 主機經主機服務的 `push.install`)以 `herdr plugin install raychiutw/purrdr.host/host-plugin --yes` 直接從本 repo 的子資料夾安裝(herdr 支援 `OWNER/REPO/SUBDIR`;app 的來源常數 `PushSettings.pluginSource`),不另外發佈 repo。
- 每支裝置的配對檔 `~/.purrdr/push-<裝置 id>/pairing`(app 經 `ReversibleHostWrite` 寫入，格式見 app 的 `PushPairingFile`:`purrdr-push 1`、`endpoint=`、`key=`、`muted=<session>`)。Agent 進入 blocked 時對每支裝置 POST 後台 `/v1/push`,只帶 workspace 名稱、狀態與 pane id,**不帶終端機標題或任何對話內容**;配對密鑰經 stdin 交給 curl,不出現在參數。某支裝置靜音了這個聊天(Agent 的 session 值)就跳過它，有靜音時才問 herdr 一次(`herdr pane get`)。
- 測試 `host-service/plugin-tests/tests/notify_script.rs`(移植自 app repo 的 `HostPluginScriptTests`):在暫存家目錄以真的 `sh` 執行 `notify.sh`,`curl`／`herdr` 換成家目錄 `bin/` 的假程式，記錄參數與 stdin,不連網路。執行腳本的測試只在 Unix 跑(`#[cfg(unix)]`);`notify.ps1` 只在 Windows 測後台回應處理(`notify_ps1`,真的以 `powershell` 執行，後台是本機假 HTTP 伺服器)。manifest 的檢查三平台都跑。

## 主機服務 Release(`.github/workflows/host-service-release.yml`,raychiutw/purrdr.ios#198)

- 合併進 `master` 且 `host-service/service/Cargo.toml` 的 version 有變才發行(`scripts/host-service-version.sh` 比對合併前後),也可手動觸發(`gh workflow run host-service-release.yml --ref master`);同一個版本的 Release 已存在時失敗(不覆蓋)。發版 = 開一個只遞增 version 的 PR 並合併。PR 的 CI 已跑過三平台測試，這裡不重測。
- 建置 macOS arm64／x86_64、Linux musl x86_64／aarch64、Windows x86_64,發到本 repo 的 Release `host-service-v<版本>`(壓縮檔、`SHA256SUMS`、一行安裝腳本，標為 latest)。
- macOS 一律以 Developer ID 簽章(hardened runtime);**公證是可選的**:GitHub environment `release`(部署分支限定 `master`)有變數 `APPLE_TEAM_ID`、secret `DEVELOPER_ID_P12`(base64 的 .p12)、`DEVELOPER_ID_P12_PASSWORD`;`ASC_KEY_ID`／`ASC_ISSUER_ID`／`ASC_PRIVATE_KEY` 三個都有才公證，缺任何一個就印 warning、只簽章。Windows 執行檔不簽章(安裝說明交代 SmartScreen)。
- **發行新的主機服務時，app repo 的 `HostServiceUpdates.latestKnownVersion` 要一起遞增**(app 以它與 `welcome` 的 `host_service_version` 比較，舊了就提示更新)。

## 測試慣例

- 測試以真正的 WebSocket client 連主機服務，背後是回放 `fixtures/herdr-0.9.5/` 錄製 NDJSON 的假 herdr socket(`service/tests/support/`)。錄製檔一律從真實主機錄(app repo 的 `scripts/record-herdr-fixture.sh`),個人資料換成假值，不照程式碼手寫。
- `tests/pane_terminal.rs` 與 `tests/install_script.rs` 是 `harness = false`:測試執行檔本身帶特定參數或環境變數時，當假的 herdr CLI(回放 `session-control/` 錄製檔)或壓縮檔裡假的主機服務。
- 安裝流程以假系統(`tests/install_support/`)加暫存家目錄，三平台流程在每個 CI 平台都跑，斷言「解除安裝後家目錄快照與系統狀態都與安裝前相同」;**絕不在真機安裝、絕不對真正的家目錄測試**。
- TDD:production code 變更先寫失敗測試，並真的跑出紅燈、留下輸出;移植或新增的守門測試以 mutation(把實作改壞看它轉紅)確認有效。紀錄放 repo 外，摘要寫在 PR。

## 開發流程

- 不直接 commit 到 `master`:開 feature branch,push 後開 PR,CI 三平台全綠後以 **merge commit** 合併(不 squash)。
- 文件、註解、commit message 用繁體中文(台灣用語),技術名詞保留英文。
