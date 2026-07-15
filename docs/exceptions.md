# Exceptions 設計文件

> 為什麼要自訂例外階層、為什麼長成這個樣子。

---

## 1. 為什麼不直接用 `requests.HTTPError` / `requests.ConnectionError`?

`requests` 套件自己有一套例外階層(`RequestException` 系列)。
問題是對 GUI 來說,這層太細,UI 程式會很囉嗦:

```python
# 用 requests 例外(沒自訂)
try:
    response = session.get(url)
    response.raise_for_status()
except requests.ConnectionError:
    show("網路斷了")
except requests.Timeout:
    show("連線逾時")
except requests.HTTPError:
    if response.status_code == 404:
        show("找不到")
    else:
        show("其他 HTTP 錯誤")
```

我們自己定義的 `NetworkError` / `VersionNotFoundError` 把這層次扁平化:

```python
# 用自訂例外
try:
    response = session.get(url)
    response.raise_for_status()
except MinecraftServerError as err:
    show(str(err))   # 一個 catch 全部
```

UI 層**永遠**只需要 `except MinecraftServerError`,然後檢查 `str(err)`
(或是 subclass 的具體型別)決定顯示什麼字。

---

## 2. 階層長這樣,為什麼?

```
MinecraftServerError  ← 所有「專案內錯誤」的基底
├── NetworkError         網路層問題(連不上、DNS 壞、逾時)
├── VersionNotFoundError 某個 Forge 版本 404
└── DownloadAbortedError 使用者按了取消
```

### 2.1 為什麼有「基底類別 `MinecraftServerError`」?

讓 UI 寫 `except MinecraftServerError` 就能接所有專案錯誤。
這是 Python 慣用法,可以一次 catch 整族例外。

如果之後加新例外(例如 `EulaRejectedError`),只要繼承基底,UI 不必動。

### 2.2 為什麼分 `NetworkError` 跟 `VersionNotFoundError`?

雖然兩者都是「請求失敗」,但語意不同:

| 情境 | 應該是 | UI 反應 |
|------|--------|---------|
| 整個 Forge 連不上 | NetworkError | 「請檢查網路」+ 重試按鈕 |
| 連得上但某個版本 404 | VersionNotFoundError | 「這個版本不存在」+ 隱藏此選項 |

用同一個例外會讓 UI 不知道該反應哪個。

### 2.3 為什麼有 `DownloadAbortedError`?

雖然可以歸進 `NetworkError`(「我們中止了請求」算網路問題),
但**語意不同**:

| NetworkError | DownloadAbortedError |
|--------------|----------------------|
| 預期可重試 | 預期不重試(使用者反悔) |

UI 對這兩種情況反應不同:
- NetworkError → 「重試」按鈕
- DownloadAbortedError → 「已取消」訊息,不需要重試

---

## 3. 例外資訊:用 `__cause__`,不用客製屬性

Python 例外 chain(`raise X from Y`)讓 `X.__cause__ == Y`,
這是 Python 標準機制,所有 `logging.exception()`、traceback 都自動處理。

```python
try:
    response = session.get(url)
except requests.ConnectionError as exc:
    raise NetworkError("連不上 Forge") from exc
```

之後在 UI 可以查:
```python
except NetworkError as e:
    print(f"NetworkError: {e}")                    # 給使用者看
    print(f"Caused by: {e.__cause__}")              # 給 log / 除錯用
```

**不要**自訂 `exc.status_code`、`exc.url` 之類 —
如果之後需要結構化資訊,加 `tuple[bool, str]` 風格的函式回傳值(如 `install_exists_with_reason`),
別污染例外的職責。

---

## 4. 命名規則

- 全部以 `Error` 結尾(跟 Python 慣例 `Exception` 也行,但 `Error` 比較一致)
- 不加前綴(`FletError`、`DownloaderError`)
  - 例外階層的命名是「語意」不是「來源」
  - 之後同個 NetworkError 可能在 CLI、GUI、網頁都用,不該綁 UI 來源
