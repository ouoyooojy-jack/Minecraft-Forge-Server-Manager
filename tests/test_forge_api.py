"""
測試 services/forge_api 的完整行為。

每個測試都是「使用範例」,讀這個檔可以學會整個模組怎麼用。
"""
from __future__ import annotations

from unittest.mock import MagicMock, patch

import pytest
import requests as req_lib

from exceptions import NetworkError
from services import forge_api
from services.forge_api import (
    build_session,
    get_session,
    get_versions,
    group_by_mc_major,
    install_exists_with_reason,
    installer_url,
    is_version_available,
    reset_session,
)


# ══════════════════════════════════════════════════════════════
# helpers
# ══════════════════════════════════════════════════════════════
def _fake_response(status_code: int, text: str = "") -> MagicMock:
    """產生一個 mock Response 給 session.get 用。"""
    resp = MagicMock()
    resp.status_code = status_code
    resp.text = text
    resp.raise_for_status.side_effect = (
        req_lib.HTTPError(f"HTTP {status_code}") if status_code >= 400 else None
    )
    return resp


@pytest.fixture(autouse=True)
def _reset_session_cache():
    """每個測試都重置 module-level session,避免互相干擾。"""
    forge_api._session = None
    yield
    forge_api._session = None


# ══════════════════════════════════════════════════════════════
# Session 工廠 / 共用邏輯
# ══════════════════════════════════════════════════════════════
def test_build_session_creates_fresh_instance():
    """build_session() 每次都回新的(無快取)。"""
    s1 = build_session()
    s2 = build_session()
    assert s1 is not s2, "build_session should not cache"


def test_build_session_sets_defaults():
    """預設 header 應包含 User-Agent 和 Accept。"""
    s = build_session()
    assert "McServerManager" in s.headers["User-Agent"]
    assert "application/xml" in s.headers["Accept"]


def test_get_session_returns_same_instance():
    """get_session() 重用同一個(連線池的意義)。"""
    s1 = get_session()
    s2 = get_session()
    assert s1 is s2


def test_get_session_lazy_creates_on_first_call():
    """第一次呼叫才建立 Session(之前 _session 為 None)。"""
    assert forge_api._session is None
    s = get_session()
    assert forge_api._session is s


def test_reset_session_clears_cache():
    """reset_session() 後下次 get_session() 會拿新實例。"""
    s1 = get_session()
    reset_session()
    assert forge_api._session is None
    s2 = get_session()
    assert s1 is not s2, "after reset, new instance expected"


# ══════════════════════════════════════════════════════════════
# get_versions
# ══════════════════════════════════════════════════════════════
def test_get_versions_parses_xml_via_injected_session():
    """注入 session 是可行的(測試不必打 patch)。"""
    xml = """<?xml version="1.0"?>
<metadata>
  <versioning>
    <versions>
      <version>1.20.1-47.4.21</version>
      <version>1.20.1-47.3.0</version>
    </versions>
  </versioning>
</metadata>"""
    fake_session = MagicMock()
    fake_session.get.return_value = _fake_response(200, xml)

    versions = get_versions(session=fake_session)

    assert versions == ["1.20.1-47.3.0", "1.20.1-47.4.21"]  # 舊→新
    fake_session.get.assert_called_once()


def test_get_versions_calls_correct_metadata_url():
    """應該 GET metadata URL,不是別的。"""
    fake_session = MagicMock()
    fake_session.get.return_value = _fake_response(200, "<metadata/>")

    get_versions(session=fake_session)

    args, kwargs = fake_session.get.call_args
    assert "maven-metadata.xml" in args[0]
    assert kwargs.get("timeout") == 60


def test_get_versions_raises_network_error_on_connection_failure():
    """網路錯誤時拋 NetworkError,帶有原例外(__cause__)。"""
    fake_session = MagicMock()
    fake_session.get.side_effect = req_lib.ConnectionError("DNS down")

    with pytest.raises(NetworkError) as exc_info:
        get_versions(session=fake_session)

    # 必須保留 __cause__ 才能除錯
    assert isinstance(exc_info.value.__cause__, req_lib.ConnectionError)


def test_get_versions_raises_network_error_on_invalid_xml():
    """Forge 回垃圾 XML 時也要拋(不能 crash)。"""
    fake_session = MagicMock()
    fake_session.get.return_value = _fake_response(200, "<<<NOT XML>>>")

    with pytest.raises(NetworkError, match="(?i)invalid XML"):
        get_versions(session=fake_session)


def test_get_versions_handles_version_text_none():
    """防呆:即使 <version>text 為 None,也別崩。"""
    # 用 stdlib ET,空 <version/> 會讓 .text 回 None
    xml = """<?xml version="1.0"?>
<metadata>
  <versioning>
    <versions>
      <version>1.20.1-47.4.21</version>
      <version></version>
    </versions>
  </versioning>
</metadata>"""
    fake_session = MagicMock()
    fake_session.get.return_value = _fake_response(200, xml)

    versions = get_versions(session=fake_session)
    # 只留有 text 的
    assert "" not in versions
    assert "1.20.1-47.4.21" in versions


# ══════════════════════════════════════════════════════════════
# installer_url(純字串組合)
# ══════════════════════════════════════════════════════════════
def test_installer_url_substitutes_version():
    """模板裡的 {version} 應被替換。"""
    url = installer_url("1.20.1-47.4.21")
    assert "1.20.1-47.4.21" in url
    assert "{version}" not in url  # 沒有剩餘 placeholder
    assert url.endswith("forge-1.20.1-47.4.21-installer.jar")


def test_installer_url_uses_https():
    """HTTPS 是必要的(forge maven 從 2021 後強制)。"""
    url = installer_url("1.20.1-47.4.21")
    assert url.startswith("https://")


# ══════════════════════════════════════════════════════════════
# group_by_mc_major(純函式)
# ══════════════════════════════════════════════════════════════
def test_group_by_mc_major_basic():
    """基本分組。"""
    out = group_by_mc_major(["1.20.1-a", "1.21.0-b", "1.20.1-c"])
    assert out == {
        "1.20": ["1.20.1-a", "1.20.1-c"],
        "1.21": ["1.21.0-b"],
    }


def test_group_by_mc_major_accepts_iterable():
    """接受任何 iterable,不只 list(用 generator 測試)。"""
    gen = (v for v in ["1.20.1-a", "1.21.0-b"])
    out = group_by_mc_major(gen)
    assert "1.20" in out
    assert "1.21" in out


def test_group_by_mc_major_skips_malformed():
    """格式不對的跳過(不 crash)。"""
    out = group_by_mc_major(["no-dash", "1.20.1-good", "1"])
    assert "no-dash" not in out
    assert "1" not in out
    assert "1.20" in out


def test_group_by_mc_major_preserves_input_order():
    """同 group 內的順序應等於輸入順序。"""
    versions = ["1.20.1-c", "1.20.1-a", "1.20.1-b"]  # 故意亂序
    out = group_by_mc_major(versions)
    assert out["1.20"] == ["1.20.1-c", "1.20.1-a", "1.20.1-b"]


def test_group_by_mc_major_three_part_minor():
    """三段版(1.20.4)視為 1.20(只取前兩段)。"""
    out = group_by_mc_major(["1.20.4-100.0.0"])
    assert "1.20" in out


# ══════════════════════════════════════════════════════════════
# is_version_available / install_exists_with_reason
# ══════════════════════════════════════════════════════════════
def test_is_version_available_true_on_200():
    """HEAD 200 → 存在。"""
    fake_session = MagicMock()
    fake_session.head.return_value = _fake_response(200)

    assert is_version_available("1.20.1-47.4.21", session=fake_session) is True
    # 必須用 HEAD 不是 GET
    fake_session.head.assert_called_once()
    fake_session.get.assert_not_called()


def test_is_version_available_false_on_404():
    """HEAD 404 → 不存在。"""
    fake_session = MagicMock()
    fake_session.head.return_value = _fake_response(404)

    assert is_version_available("1.20.1-999.0.0", session=fake_session) is False


def test_is_version_available_false_on_network_error():
    """網路問題回 False(不要 raise,語意是 yes/no)。"""
    fake_session = MagicMock()
    fake_session.head.side_effect = req_lib.ConnectionError("timeout")

    assert is_version_available("1.20.1-47.4.21", session=fake_session) is False


def test_install_exists_with_reason_returns_structured():
    """回 (bool, str) 給 UI 顯示。"""
    fake_session = MagicMock()
    fake_session.head.return_value = _fake_response(404)

    ok, reason = install_exists_with_reason("1.20.1-999.0.0", session=fake_session)
    assert ok is False
    assert "VersionNotFound" in reason


def test_install_exists_with_reason_network_error():
    """網路錯誤時 reason 帶 NetworkError prefix。"""
    fake_session = MagicMock()
    fake_session.head.side_effect = req_lib.ConnectionError("DNS down")

    ok, reason = install_exists_with_reason("1.20.1-47.4.21", session=fake_session)
    assert ok is False
    assert "NetworkError" in reason
    assert "DNS down" in reason
