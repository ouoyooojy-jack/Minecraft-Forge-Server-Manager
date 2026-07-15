# `services/forge_api` 教學文件

> 本檔記載 `services/forge_api.py` 的「為什麼這樣寫」——設計決定、權衡、歷史脈絡。
> 函式簽名與行為請直接看程式碼本身的 docstring。

---

## 1. 模組存在的原因

Forge Minecraft 在 [Maven](https://maven.minecraftforge.net) 上發佈了所有歷史版本,
介面是 `maven-metadata.xml`(結構化清單) + 一個檔案路徑樣板(`forge-{version}-installer.jar`)。

`services/forge_api.py` 把這兩件事的「邏輯」集中起來:

1. **抓清單** — `get_versions()` 解析 metadata
2. **組下載 URL** — `installer_url()` 把版本字串套到樣板
3. **分類** — `group_by_mc_major()` 把 `1.20.1` / `1.20.4` 歸到 `1.20`

UI 層(或 CLI、測試、網頁前端)只需要呼叫,不需要懂 Forge 的網址結構。

---

## 2. 設計目標(按優先順序)

| 優先 | 目標 | 實踐 |
|------|------|------|
| 1 | 可測試 | `Session` 從外面注入,測試不必 mock global |
| 2 | 可重用 | 不 import 任何 UI 元件(`flet`、`tkinter`...) |
| 3 | 可觀察 | 例外帶 `__cause__`,UI 可以拿到原始 requests 例外 |
| 4 | 可教學 | 函式簽名直接告訴呼叫端怎麼用 |

---

## 3. 關鍵設計決定(為什麼這樣寫、不那樣寫)

### 3.1 為什麼用 `requests.Session` 而不是裸 `requests.get`?

每次 `requests.get(url)` 都會重新 TCP 握手 + TLS 握手(昂貴,典型 100-300ms)。
複用一個 `Session` 讓所有 GET 共用同一條連線。

對於 Forge metadata(5KB)省下的時間不多,但**架構正確比省下那 200ms 重要**,
因為之後 `DownloadService`(Task 3)會抓 100MB+ 的 installer jar,Session 是必備的。
早點定型,避免風格不一致。

### 3.2 為什麼分開 `build_session()` / `get_session()` / `reset_session()`?

| 函式 | 何時用 | 為什麼分開 |
|------|--------|------------|
| `build_session()` | 工廠,給「全新實例」 | 建立是 stateless,測試可以獨立呼叫 |
| `get_session()` | 共用 instance,惰性建立 | 整個 process 一個 session 就夠,並且 lazy 才不會無謂 import 就建連線 |
| `reset_session()` | 測試或 hot reload | 給測試乾淨起點,正式程式不該呼叫 |

如果只有 `get_session()`:測試要 patch module-level 變數,容易漏。
如果只有 `build_session()`:每次呼叫都建新 TCP 連線,浪費。

### 3.3 為什麼 `get_versions()` 把 404 當 NetworkError,而不是 VersionNotFoundError?

`VersionNotFoundError` 我們用在「查某個具體版本是否可下載」(見 `install_exists_with_reason`)。

而 `get_versions()` 是「抓清單」— metadata 404 通常代表 Forge 後端整個掛了,
或是 URL 整個改掉了,**這都是嚴重的問題**,應該報 `NetworkError` 讓 UI 顯示「請檢查網路或回報 bug」。

如果回 `VersionNotFoundError` 會誤導使用者以為「我選了錯的版本」。

### 3.4 為什麼 `is_version_available()` 網路錯誤回 `False`,不 raise?

這個函式語意是 **yes / no**(UI 在 dropdown 預檢查時用):
- 「不存在的版本」→ False
- 「網路壞掉了」→ False

回 False 兩種情況合一是合理的(它本來就是個「沒那麼嚴重」的預檢)。
呼叫端真的要區分,用 `install_exists_with_reason()`。

### 3.5 為什麼 `Iterable[str]` 而不是 `list[str]`,作為 `group_by_mc_major` 簽名?

`Iterable[str]` 接受 list / tuple / generator,呼叫端可以 stream:
```python
for v in (f"{ver}" for ver in big_list):
    ...
```

對純函式來說,簽名越寬越好 — 反正輸出總是 `dict` 不會 mutate 輸入。

### 3.6 為什麼 `versions.reverse()` 在 `get_versions()` 裡?

XML 原始順序是「舊到新」(`1.16` 在前,`1.21` 在後)。
但 UI 想要「新到舊」(dropdown 預設顯示最新)。

兩個選擇:
1. 在原始資料就 reverse(我們選這個 — 一勞永逸)
2. UI 自己再 sort(每個用到的人都得寫一次)

選 1 是因為使用者體驗永遠是「最近在上面」。

---

## 4. 模組依賴方向

```
config.py          ← 常數
exceptions.py      ← 例外階層
   ↑
services/forge_api.py  ← 本檔,商業邏輯
   ↑
ui/*               ← UI 層,呼叫 services
```

**單向依賴,不可反過來**。`ui/main_view.py` 知道 `forge_api` 的存在,
`services/forge_api.py` 絕對不能 `import flet`。

---

## 5. 未來擴充點

TODO 留給以後做:

- **版本快取** — `get_versions()` 每次都抓 HTTP,可以加 `functools.lru_cache` 或寫到 disk,加 `cache_ttl_sec` 參數。
- **離線模式** — 失敗時退到快取的版本清單。
- **Forge 鏡像** — 台灣或中國使用者可能想用自家鏡像,加 `forge_base_url` 參數。
- **整合其他 loader** — Fabric、Quilt、NeoForge 也都有類似結構,可以抽象成 `services/loader_api.py`。
