# Domain Docs

engineering skills 探索程式碼時，如何使用領域文件。

## 探索前先讀

本 repo 沒有自己的 `GLOSSARY.md` 與 `docs/adr/`,一律引用 app repo(`raychiutw/purrdr.ios`):

- **[`GLOSSARY.md`](https://github.com/raychiutw/purrdr.ios/blob/master/GLOSSARY.md)**(single-context,沒有 `GLOSSARY-MAP.md`)。
- **[`docs/adr/`](https://github.com/raychiutw/purrdr.ios/tree/master/docs/adr)**:讀跟你要動的範圍有關的 ADR。主機端常用的是 0001(clean-room)、0002(寫入主機)、0004(推播)、0005(Pane 終端機)、0010(主機服務)。

讀法：`gh api repos/raychiutw/purrdr.ios/contents/GLOSSARY.md --jq .content | base64 -d`(ADR 換成 `docs/adr/<檔名>`)。

讀不到時，**直接繼續，不要提**。新術語或決策由 `/domain-modeling`(經 `/grill-with-docs` 與 `/improve-codebase-architecture`)寫回 app repo,不在本 repo 另建。

## 用術語表的詞

輸出裡提到領域概念時(issue 標題、重構提案、假設、測試名稱),一律用 `GLOSSARY.md` 定義的詞，不要換成術語表明列「避免」的同義詞。

需要的概念還不在術語表裡，代表兩種可能：你在發明專案沒在用的說法(請重新考慮),或術語表真的有缺口(記下來交給 `/domain-modeling`)。

## 標出跟 ADR 衝突的地方

輸出跟現有 ADR 矛盾時，要明確指出，不要悄悄推翻:

> _跟 ADR-0010(主機服務只綁 127.0.0.1)矛盾，但值得重新討論，因為……_
