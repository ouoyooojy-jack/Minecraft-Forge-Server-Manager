"""
測試例外階層是否設計正確。

TDD 精神:測試是「使用說明書」也是「驗收清單」。
讀這檔案就能知道 exceptions.py 該怎麼用。
"""
from __future__ import annotations

import pytest

from exceptions import (
    DownloadAbortedError,
    MinecraftServerError,
    NetworkError,
    VersionNotFoundError,
)


def test_hierarchy():
    """所有自訂例外都應該繼承 MinecraftServerError。

    為什麼要測這個? 因為 UI 層會寫:
        except MinecraftServerError as e:
    如果漏掉一個例外沒繼承,bug 不會立刻冒出來,
    只有碰到那個例外的時候 UI 才會當掉。
    """
    assert issubclass(NetworkError, MinecraftServerError)
    assert issubclass(VersionNotFoundError, MinecraftServerError)
    assert issubclass(DownloadAbortedError, MinecraftServerError)


def test_can_raise_and_catch_base():
    """測試「用基底類別 catch 子類別」會通。

    這是 Python 內建的行為,但我們需要它在我們的階層中對。
    """
    with pytest.raises(MinecraftServerError) as exc_info:
        raise VersionNotFoundError("1.20.1-999.0.0")

    # 確認訊息被保留(對 debug 很重要)
    assert "1.20.1-999.0.0" in str(exc_info.value)
