"""Regression checks for non-blocking download-page version loading."""
from pathlib import Path


def test_version_fetch_is_dispatched_to_a_worker_thread():
    source = (Path(__file__).resolve().parents[1] / "ui" / "views" / "downloads.py").read_text(encoding="utf-8")
    assert "await asyncio.to_thread(get_versions)" in source
    assert "正在載入 Forge 版本" in source
