# `services/download_service` 設計文件

> 為什麼這個模組這樣寫、不那樣寫。
> 簽名跟行為請看 `download_service.py` 的 docstring。

---

## 1. 模組在做什麼

Forge installer jar 動輒 50-200 MB。如果用 `requests.get(url).content` 一次下載:

- **記憶體爆** — 一次撐 100MB 在 RAM 等著寫檔
- **UI thread 卡** — 同步 `requests` 在 Flet 的 asyncio event loop 裡直接 block
- **沒進度** — 拿到 content 才知道總大小,使用者只看到轉圈圈

`DownloadService` 解決這三件事:`stream=True` 切 chunk、`threading.Event` 提供取消、節流的 `ProgressInfo` callback 給 UI。

---

## 2. 三層防護(為什麼 UI 不卡)

```
┌─────────────────────┐
│ Layer 1: stream=True│   一次不超過 64KB 在記憶體
└──────────┬──────────┘
           ▼
┌─────────────────────┐
│ Layer 2: asyncio    │   同步下載丟到 thread pool
│ .to_thread(...)     │   GUI thread 透過 await 不被 block
└──────────┬──────────┘
           ▼
┌─────────────────────┐
│ Layer 3: 30 Hz      │   ProgressBar 更新節流
│ ProgressThrottle    │   不會被 100MB × 1600 chunks 灌爆
└─────────────────────┘
```

> 實作細節:`asyncio.to_thread` 在 `ui/main_view.py` 那一層處理(`Task 5`)。
> 本模組只負責「同步、安全、可取消」的下載邏輯,執行緒模型不該管。

---

## 3. 關鍵設計決定

### 3.1 為什麼用 `stream=True` + chunk 寫入,不要整個 `read()`?

雖然 `requests.get(url).content` 是更短的寫法,但對 100MB jar:

| 寫法 | 記憶體峰值 | IO 模式 |
|------|-----------|---------|
| `read()` 一次 | ~ 100 MB | 1 次系統呼叫 |
| `iter_content(chunk=64KB)` | ~ 64 KB | 1600 次系統呼叫 |

理論上 1600 次系統呼叫比 1 次慢,但實際:

- **磁碟有 page cache**,第二次寫同一個 block 記憶體命中
- **進度是真實的**:每 chunk 寫完 → 觸發 UI 更新 → 使用者安心
- **可以取消**:任何 chunk 邊界檢查 `cancel_event.is_set()`

結論:看起來「效能差」,實際上是「使用者體驗好 + 可中斷」,遠勝過節省的 IO 開銷。

### 3.2 為什麼 `ProgressInfo` 是 `@dataclass(frozen=True)` 而不是 dict?

```python
# dataclass — IDE autocomplete、type checker 給力
info.percent           # .percent 是 property
info.speed_mbps

# dict — 寫一次就忘記 key 名
info['percent']
info['speed_mb/s']     # 注意是 mb/s 不是 mbps,typo 不會被發現
```

`frozen=True` 讓 callback 不可能「不小心」改到進度狀態。

### 3.3 為什麼用 `.part` + atomic rename?

下載途中如果程式崩潰或被取消:

| 寫法 | 崩潰後狀態 |
|------|------------|
| 直接寫 `out.jar` | 半成品留在硬碟,使用者看到「假 jar」執行會壞掉 |
| `.part` 暫存後 rename | 只有「完整的 jar」或「什麼都沒有」 |

`.part` 雖然多一個檔案風險,但失敗時的清理有定義明確:

```python
except DownloadAbortedError:
    if tmp_path.exists():
        tmp_path.unlink()
    raise
```

### 3.4 為什麼 throttle 30Hz 寫在 closure 裡,不獨立 class?

節流邏輯很簡單,只有:

```python
last_emit = 0.0
def emit():
    if (now - last_emit) < throttle:
        return
    on_progress(...)
    last_emit = now
```

做成獨立 class 反而要學 pytest 端怎麼 mock。Closure 寫法直觀、本機變數清晰、測試好寫。

> 之後 `ui/components.py` 的 `ProgressThrottle` 會是**獨立的 class**,
> 因為那層要節流「UI 控制項更新」,不是「callback emit」,關注點不同。

### 3.5 為什麼 `cancel_event` 是 `threading.Event` 而非 polling flag?

```python
# threading.Event — 標準庫、跨平台
event = threading.Event()
event.set()  # 取消(任意 thread)
event.is_set()  # 檢查(下載 thread)

# polling flag — 需要自己處理 race condition
self.cancelled = True  # 兩個 thread 同時改變會出事
```

`threading.Event` 已經處理了 memory barrier,跨 thread 安全。
另外 `Event.wait()` 可以當 timeout 用,如果以後想做「卡住太久自動取消」也很自然。

### 3.6 為什麼 404 是 `VersionNotFoundError`,其他網路問題是 `NetworkError`?

對 UI 來說,這兩種失敗的「建議動作」不同:

| 例外 | UI 顯示 | UI 動作 |
|------|---------|---------|
| `VersionNotFoundError` | 「這個版本不存在」 | 「重新選版本」 |
| `NetworkError` | 「網路連線失敗」 | 「重試」按鈕 |

把這個語意差別丟進例外階層,UI 不用 `if err.status_code == 404` 這種醜 code。

### 3.7 為什麼 `download()` 是同步介面?

雖然本檔最終要被 `await asyncio.to_thread(self.download, ...)` 呼叫,**但本身是同步的**,因為:

- CLI 可以直接用:不需要 asyncio
- 測試好寫:不用學 `pytest-asyncio`
- 介面明確:signature 一眼看得出呼叫端要做什麼

把「同步」vs「async 介面」分層:

```
services/download_service.py     純同步,可測
    ↑ asyncio.to_thread 包裝
ui/main_view.py (Task 5)         給 GUI 用的協程
```

---

## 4. 模組依賴方向

```
config.py      ← 常數
exceptions.py  ← 例外階層
   ↑
services/download_service.py  (本檔) ← I/O 邏輯
```

可以向上呼叫 `services.forge_api.installer_url()`,但**目前沒這個需求**。需要時再加,別先寫空的耦合。

---

## 5. 之後想加但還沒做的

- **斷點續傳**:`Range: bytes=N-` header,從中斷處繼續(會增加複雜度,先不做)
- **多任務佇列**:`DownloadService` 現在一個任務一次呼叫,之後想做「批次下載清單」再加 manager class
- **下載校驗**:拿到 jar 後 `sha256` 比對 `maven-metadata.xml` 的 `<sha1>` 或 `<md5>`(Forge 提供的話)
