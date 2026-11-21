# AGENTS.md

## 開發流程

- 不直接 commit 到 `master`:開 feature branch,push 後開 PR,CI 三平台全綠後以 **merge commit** 合併(不 squash)。
- 文件、註解、commit message 用繁體中文(台灣用語),技術名詞保留英文。
- 功能開發走 Matt Pocock 流程:`/grill-with-docs` 釐清需求(術語與決策寫回 app repo 的 `GLOSSARY.md`／`docs/adr/`)→ `/to-spec` 開 spec issue → `/to-tickets` 拆成標 `ready-for-agent` 的 ticket(以 blocked-by 標依賴)→ `/implement`(單張)或 `/implement-spec`(整份 spec,一條 integration branch)以 `/tdd` 實作 → `/code-review` → 依 `/pr` 寫 PR。Bug 先 `/diagnosing-bugs`;設計疑問先 `/prototype`;進來的 issue 用 `/triage` 分類。

## 建置與測試

需要 Rust stable,workspace 在 `host-service/`:

```sh
cd host-service
cargo test --workspace --locked
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo run -p protocol-codegen        # 改了 protocol/host-service.schema.json 後重新產生型別
```

## 發行

- 升 `host-service/service/Cargo.toml` 的 version(`Cargo.lock` 用 `cargo update -p purrdr-host-service --offline` 同步),並在 `host-service/CHANGELOG.md` 最上面加 `## <版本>` 一段給一般使用者看的更新說明(CI 會檢查)。
- 合併進 `master` 後 Release workflow 自動建置、簽章、公證並發行，Release 頁面的「本版更新」取自那一段。
- README 只寫用途、主要功能與安裝;其餘細節放安裝說明網頁或這份文件。
