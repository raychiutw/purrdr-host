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

腳本會下載這台電腦的執行檔、以 `SHA256SUMS` 驗證，再執行 `purrdr-host-service install`(檢查 Tailscale、設定自動啟動，不需要 sudo／系統管理員)。完整說明(前置條件、配對、更新與解除安裝):https://raychiutw.github.io/purrdr/host-service/

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

The script downloads the build for your computer, verifies it against `SHA256SUMS`, then runs `purrdr-host-service install` (checks Tailscale and sets up autostart; no sudo or administrator needed). Full guide (requirements, pairing, updating and uninstalling): https://raychiutw.github.io/purrdr/host-service/

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
