# AGENTS.md

## 開發流程

- 不直接 commit 到 `master`:開 feature branch,push 後開 PR,CI 三平台全綠後以 **merge commit** 合併(不 squash)。
- 文件、註解、commit message 用繁體中文(台灣用語),技術名詞保留英文。
- 功能開發走 Matt Pocock 流程:`/grill-with-docs` 釐清需求(術語與決策寫回 app repo 的 `GLOSSARY.md`／`docs/adr/`)→ `/to-spec` 開 spec issue → `/to-tickets` 拆成標 `ready-for-agent` 的 ticket(以 blocked-by 標依賴)→ `/implement`(單張)或 `/implement-spec`(整份 spec,一條 integration branch)以 `/tdd` 實作 → `/code-review` → 依 `/pr` 寫 PR。Bug 先 `/diagnosing-bugs`;設計疑問先 `/prototype`;進來的 issue 用 `/triage` 分類。
