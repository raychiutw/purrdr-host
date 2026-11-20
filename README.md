# purrdr.host

[繁體中文](#繁體中文) · [English](#english)

## 繁體中文

[Purrdr](https://github.com/raychiutw/purrdr.ios) 的主機端元件：讓 iPhone／iPad 上的 Purrdr app 經 HTTPS(Tailscale)連到電腦上由 [herdr](https://herdr.dev) 管理的 coding agent。

- **主機服務**(`host-service/`):在電腦上執行的常駐程式，只聽 127.0.0.1,由 `tailscale serve` 對 tailnet 提供 HTTPS。支援 macOS、Linux、Windows。
- **主機外掛**(`host-plugin/`):herdr plugin,Agent 需要你時推播到配對的裝置(由 app 經 SSH 安裝)。
- **協定**(`protocol/`):app 與主機服務之間的協定定義。

### 安裝主機服務

macOS／Linux:

```sh
curl -fsSL https://github.com/raychiutw/purrdr.host/releases/latest/download/install.sh | sh
```

Windows(PowerShell):

```powershell
irm https://github.com/raychiutw/purrdr.host/releases/latest/download/install.ps1 | iex
```

腳本會下載這台電腦的執行檔、以 `SHA256SUMS` 驗證，再執行 `purrdr-host-service install`(檢查 Tailscale、設定自動啟動，不需要 sudo／系統管理員)。安裝時會把主機服務加進使用者的 PATH(Windows 是使用者環境變數，macOS／Linux 是你的 shell 設定檔裡標記起來的一段),**重開終端機後就能直接打 `purrdr-host-service`**;不想改 PATH 就在安裝時加 `--no-path`,解除安裝會移除。完整說明(前置條件、配對、更新與解除安裝):https://raychiutw.github.io/purrdr/host-service/

### 更新

主機服務預設會自動更新：背景服務啟動後幾分鐘先檢查一次，之後每天檢查 `raychiutw/purrdr.host` 的最新 Release，有新版就下載、以 `SHA256SUMS` 驗證(macOS 另驗 Developer ID 簽章)、替換並重新啟動;新版起不來會自動退回原版本。設定、HTTPS 埠與已配對的裝置都會保留。

```sh
purrdr-host-service version          # 已安裝與最新版本、自動更新開關、上次檢查的時間與結果
purrdr-host-service update --check   # 只檢查有沒有新版
purrdr-host-service update           # 立刻更新(--yes 不詢問)
purrdr-host-service auto-update off  # 關閉自動更新(on 再打開);安裝時也可以加 --no-auto-update
```

0.4.x 以前沒有更新功能：重跑一次上面的安裝指令(會保留已配對的裝置),之後就會自動更新。

### 開發

需要 Rust stable。workspace 在 `host-service/`:

```sh
cd host-service
cargo test --workspace --locked
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo run -p protocol-codegen        # 改了 protocol/host-service.schema.json 後重新產生型別
```

開發規範與架構說明見 [`AGENTS.md`](AGENTS.md)。

## English

Host-side components of [Purrdr](https://github.com/raychiutw/purrdr.ios): they let the Purrdr app on iPhone and iPad reach the [herdr](https://herdr.dev)-managed coding agents on your computer over HTTPS (Tailscale).

- **Host service** (`host-service/`): a background service that listens on 127.0.0.1 only and is exposed to your tailnet over HTTPS by `tailscale serve`. Runs on macOS, Linux and Windows.
- **Host plugin** (`host-plugin/`): a herdr plugin that sends a push notification to paired devices when an agent needs you (installed by the app over SSH).
- **Protocol** (`protocol/`): the protocol between the app and the host service.

### Install the host service

macOS / Linux:

```sh
curl -fsSL https://github.com/raychiutw/purrdr.host/releases/latest/download/install.sh | sh
```

Windows (PowerShell):

```powershell
irm https://github.com/raychiutw/purrdr.host/releases/latest/download/install.ps1 | iex
```

The script downloads the build for your computer, verifies it against `SHA256SUMS`, then runs `purrdr-host-service install` (checks Tailscale and sets up autostart; no sudo or administrator needed). It also adds the host service to your user PATH (the user environment variable on Windows, a marked block in your shell's profile on macOS / Linux), so **after opening a new terminal you can run `purrdr-host-service` directly**; install with `--no-path` to leave PATH alone. Uninstalling removes it. Full guide (requirements, pairing, updating and uninstalling): https://raychiutw.github.io/purrdr/host-service/

### Updating

The host service updates itself by default: a few minutes after it starts, and then every day, it checks the latest release of `raychiutw/purrdr.host`; when there is a newer one it downloads it, verifies it against `SHA256SUMS` (and the Developer ID signature on macOS), replaces itself and restarts. If the new version does not start, it goes back to the previous one. Settings, the HTTPS port and paired devices are kept.

```sh
purrdr-host-service version          # installed and latest version, auto update on/off, last check and its result
purrdr-host-service update --check   # only check for a newer release
purrdr-host-service update           # update now (--yes skips the question)
purrdr-host-service auto-update off  # turn auto update off (on turns it back on); or install with --no-auto-update
```

Versions 0.4.x and earlier cannot update themselves: run the install command above once more (paired devices are kept), and it updates itself from then on.

### Development

Requires stable Rust. The workspace lives in `host-service/`:

```sh
cd host-service
cargo test --workspace --locked
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo run -p protocol-codegen        # regenerate types after editing protocol/host-service.schema.json
```

See [`AGENTS.md`](AGENTS.md) (Traditional Chinese) for conventions and architecture.
