# Configuration 設計文件

> 本檔講解 `config.py` 的設計哲學,以及為什麼選 `.py` 而不是 `.json`/`.yaml`/`.toml`。

---

## 1. 為什麼是 `.py` 不是 `.json`/`.yaml`/`.ini`?

| 設定格式 | 優點 | 致命缺點(對這個專案) |
|----------|------|----------------------|
| **`.py`(我們選的)** | 打包零摩擦、IDE 友善、型別檢查 | 要重新編譯才能改設定 |
| `.json` | 通用 | 沒 type 提示;Path/byte-size 序列化麻煩 |
| `.yaml` | 人類最可讀 | 需要額外裝 `pyyaml`(exe 大小↑) |
| `.toml` | Python 3.11+ 標準庫支援 | 設定檔要隨 exe 分發(`flet pack` 需手動處理) |
| `.ini` | 經典 | 表達力弱、無巢狀 |

**關鍵:這個專案要打包成 exe**(`flet pack` / `PyInstaller`)。

打包工具自動吃 `.py` 檔,`config.py` 直接編進 exe,使用者拿到的就是單一檔案。
`.json`/`.toml` 則需要隨 exe 分發,或寫進資源,徒增複雜度。

---

## 2. 各常數的選擇理由

### 2.1 `FORGE_METADATA_URL` / `FORGE_INSTALLER_URL_TEMPLATE`

URL 樣板(用 Python 的 `str.format()`)比 f-string 好處是:
- 集中在 config.py,改 URL 只改一處
- 之後 Forge 改網址結構,只改 `FORGE_INSTALLER_URL_TEMPLATE`
- 樣板可以在測試裡 regex 驗證

### 2.2 `DEFAULT_DOWNLOAD_DIR = <project_root>/downloads`

固定在專案根目錄的 `downloads/` 子資料夾,**不開放設定**:

- 打包後 exe 旁邊就會看到 `downloads/`,路徑心智模型簡單。
- 完全不需要處理「相對 / 絕對 / 使用者輸入不存在路徑」三種 corner case。
- 用 `pathlib.Path` 而不是純字串原因:
  - 之後要 `path / filename` 路徑拼接時,字串得自己寫 `os.path.join`
  - type checker 知道這是 Path,IDE 會 autocomplete `.mkdir()`、`.stat()` 等
- 解析規則(跟 `_settings_path()` 共用 `_project_root()` 輔助):
  - 打包後(`sys.frozen`):exe 所在目錄
  - 開發中: `config.py` 所在目錄

如果之後真的需要換位置(例如 `%APPDATA%/McServerManager/`),
改 `_project_root()` 的 fallback 邏輯就好。

### 2.3 `HTTP_TIMEOUT_SEC = 60`

網路 timeout,防止程式永遠卡住。

60 秒是「網路不好但還活的程度」:
- 網頁載入正常: < 1 秒
- Forge maven 偶爾慢: 5-15 秒
- 跨國連線可能: 30 秒

選 60 給安全邊際。如果常 timeout,可以降到 30 看是不是自己網路問題。

### 2.4 `DOWNLOAD_CHUNK_SIZE = 64 * 1024`

64KB 是 HTTP 流量平衡點:
- **更大**(1MB):記憶體浪費、進度更新延遲
- **更小**(4KB):系統呼叫次數爆增、CPU bound

`64 * 1024` 而不是 `65536` 是**語意**:讀過程式碼的人不需要算就知道是 64KB。

### 2.5 `PROGRESS_UPDATE_HZ = 30`

ProgressBar 更新頻率,30Hz = 每 33ms 一次:

- **太高**(60Hz):UI thread 被更新拖累,反而會卡頓
- **太低**(5Hz):進度看起來「跳」,不順暢

30Hz 是「人眼感覺連續」的下限。1.78MB/s 下載速度的話,每 33ms 大概收到 60KB,
所以這頻率剛好跟「每讀一個 chunk」對齊;**`DOWNLOAD_CHUNK_SIZE` × `PROGRESS_UPDATE_HZ`
會影響實際更新粒度**,改任一個要記得同步另一個。

---

## 3. 未來擴充

- **環境變數覆寫** — 12-factor app 風格,`MCSM_DOWNLOAD_DIR=/path`
  可覆寫。常數前面都加前綴比較好識別。
- **設定驗證** — 用 dataclass + `__post_init__`,確保 `DOWNLOAD_CHUNK_SIZE > 0`
  之類的不變條件。
