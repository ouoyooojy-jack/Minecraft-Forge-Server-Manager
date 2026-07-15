# Mc Server Manager — 設計文件

> 「為什麼這樣寫」全部放在這。程式碼本身只描述「做什麼 + 簽名」。

## 入口

- [forge_api.md](forge_api.md) — `services/forge_api.py` 的設計決定
- [configuration.md](configuration.md) — `config.py` 的選擇理由 + 常數說明
- [exceptions.md](exceptions.md) — 例外階層的設計
- [download_service.md](download_service.md) — `services/download_service.py` 的設計(三層防護、節流、原生取消)

## 整體架構(30 秒看懂)

```
main.py              ← 進入點
└── ui/              ← Flet 控制項,只負責組裝
    ├── main_view.py ← dropdowns、buttons、events
    └── components.py ← 節流 ProgressBar 等重用元件
└── services/        ← 純商業邏輯(無 UI 相依)
    └── forge_api.py ← 版本清單 + URL 組合 + 分組
└── config.py        ← 全域常數
└── exceptions.py    ← 統一例外階層
└── tests/           ← pytest 單元測試
```

## 重構原則(不寫在程式碼裡的規則)

1. **解耦合**:UI 不直接碰商業邏輯,透過 callback / event 與 service 通訊。
2. **可測試**:任何測試都不發網路;`Session`、`path`、`time` 全可注入。
3. **漸進**:慢慢來,一次只解決一個問題;commit 對應 issue 範圍。
4. **極簡**:程式碼只描述「做什麼」,「為什麼」放這裡(`.md`)。

## 計劃中的下一步

- Task 3:`services/download_service.py`(stream 下載 + 進度 + 取消)
- Task 5:`ui/main_view.py` + `main.py` 改寫(`page.run_task` + asyncio)
- Task 6:清理、`.gitignore`、logging
- Task 7:打包 (`flet pack` 或 PyInstaller)
