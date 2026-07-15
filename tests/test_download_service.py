"""
Tests for services/download_service.

Approach (TDD):
  1. Write failing test
  2. Run it (expect FAIL)
  3. Implement
  4. Run again (expect PASS)

See docs/download_service.md for design rationale.
"""
from __future__ import annotations

import threading
import time
from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest
import requests as req_lib

from exceptions import (
    DownloadAbortedError,
    NetworkError,
    VersionNotFoundError,
)
from services import download_service
from services.download_service import DownloadService, ProgressInfo


# ════════════════════════════════════════════════════════════════
# helpers
# ════════════════════════════════════════════════════════════════
def _fake_stream_response(
    chunks: list[bytes],
    status: int = 200,
    content_length: int | None = None,
) -> MagicMock:
    """Mock a requests.Response with stream behavior."""
    total = content_length if content_length is not None else sum(len(c) for c in chunks)
    fake = MagicMock()
    fake.status_code = status
    fake.headers = {"Content-Length": str(total)}
    fake.__enter__ = MagicMock(return_value=fake)
    fake.__exit__ = MagicMock(return_value=False)
    fake.iter_content.return_value = chunks
    return fake


# ════════════════════════════════════════════════════════════════
# ProgressInfo
# ════════════════════════════════════════════════════════════════
def test_progress_info_percent_total_known():
    """Percent is straightforward when we know the total size."""
    p = ProgressInfo(downloaded_bytes=50, total_bytes=100, elapsed_seconds=1.0)
    assert p.percent == pytest.approx(50.0)


def test_progress_info_percent_total_unknown():
    """When total=0 (Content-Length missing), percent is 0 (not 100)."""
    p = ProgressInfo(downloaded_bytes=50, total_bytes=0, elapsed_seconds=1.0)
    assert p.percent == 0.0


def test_progress_info_speed_mbps():
    """Speed = bytes / seconds, normalised to MB/s."""
    # 1 MB downloaded in 1 second → 1 MB/s
    p = ProgressInfo(
        downloaded_bytes=1024 * 1024,
        total_bytes=2 * 1024 * 1024,
        elapsed_seconds=1.0,
    )
    assert p.speed_mbps == pytest.approx(1.0, abs=0.01)


def test_progress_info_speed_when_zero_seconds():
    """Avoid division-by-zero; return 0 if elapsed is 0."""
    p = ProgressInfo(downloaded_bytes=1000, total_bytes=2000, elapsed_seconds=0.0)
    assert p.speed_mbps == 0.0


# ════════════════════════════════════════════════════════════════
# DownloadService.download — happy path
# ════════════════════════════════════════════════════════════════
def test_download_writes_file(tmp_path: Path):
    """On success, file content equals the streamed bytes."""
    out = tmp_path / "out.jar"
    content = b"hello forge " * 100
    fake_resp = _fake_stream_response([content], content_length=len(content))

    with patch("services.download_service.requests") as mock_requests:
        mock_requests.get.return_value = fake_resp
        service = DownloadService()
        saved = service.download("https://example/x.jar", out)

    assert saved == out
    assert out.read_bytes() == content


def test_download_uses_stream_true(tmp_path: Path):
    """download() must pass stream=True so we don't load everything at once."""
    fake_resp = _fake_stream_response([b"abc"], content_length=3)
    with patch("services.download_service.requests") as mock_requests:
        mock_requests.get.return_value = fake_resp
        DownloadService().download("https://example/x.jar", tmp_path / "x")

    args, kwargs = mock_requests.get.call_args
    assert kwargs.get("stream") is True
    assert "timeout" in kwargs


def test_download_returns_path_not_none_on_success(tmp_path: Path):
    """Return type is Path (not Optional) on success — UI can rely on it."""
    fake_resp = _fake_stream_response([b"x" * 10], content_length=10)
    with patch("services.download_service.requests") as mock_requests:
        mock_requests.get.return_value = fake_resp
        result = DownloadService().download("u", tmp_path / "out")

    assert isinstance(result, Path)


# ════════════════════════════════════════════════════════════════
# DownloadService.download — error paths
# ════════════════════════════════════════════════════════════════
def test_download_raises_version_not_found_on_404(tmp_path: Path):
    """HTTP 404 → VersionNotFoundError."""
    fake_resp = _fake_stream_response([], status=404)
    with patch("services.download_service.requests") as mock_requests:
        mock_requests.get.return_value = fake_resp
        with pytest.raises(VersionNotFoundError):
            DownloadService().download("u", tmp_path / "out")


def test_download_raises_network_error_on_other_status(tmp_path: Path):
    """HTTP 5xx → NetworkError (not return None)."""
    fake_resp = _fake_stream_response([], status=503)
    with patch("services.download_service.requests") as mock_requests:
        mock_requests.get.return_value = fake_resp
        with pytest.raises(NetworkError):
            DownloadService().download("u", tmp_path / "out")


def test_download_raises_network_error_on_connection_failure(tmp_path: Path):
    """DNS / timeout etc. → NetworkError."""
    fake_resp = MagicMock()
    with patch.object(
        download_service.requests, "get",
        side_effect=req_lib.ConnectionError("DNS down"),
    ):
        with pytest.raises(NetworkError):
            DownloadService().download("u", tmp_path / "out")


# ════════════════════════════════════════════════════════════════
# DownloadService.download — progress callback
# ════════════════════════════════════════════════════════════════
def test_download_reports_progress(tmp_path: Path):
    """on_progress callback is invoked with ProgressInfo."""
    progress_calls: list[ProgressInfo] = []

    def on_progress(p: ProgressInfo):
        progress_calls.append(p)

    chunks = [b"a" * 100, b"b" * 100, b"c" * 100]
    fake_resp = _fake_stream_response(chunks, content_length=300)

    with patch("services.download_service.requests") as mock_requests:
        mock_requests.get.return_value = fake_resp
        DownloadService().download(
            "u", tmp_path / "out",
            on_progress=on_progress,
        )

    # exactly one final call (with percent=100) is guaranteed
    final = progress_calls[-1]
    assert final.downloaded_bytes == 300
    assert final.percent == pytest.approx(100.0)


def test_download_progress_is_throttled(tmp_path: Path):
    """Progress callback should not be called for every tiny chunk.

    Use the throttle window to verify only one final call is made when
    download is faster than the throttle interval.
    """
    # patch time.monotonic to give a steady clock
    t = [0.0]

    def fake_monotonic():
        return t[0]

    progress_calls: list[ProgressInfo] = []

    # 50 chunks of 100 bytes = 5000 bytes total
    chunks = [b"x" * 100] * 50
    fake_resp = _fake_stream_response(chunks, content_length=5000)

    with patch("services.download_service.requests") as mock_requests:
        mock_requests.get.return_value = fake_resp
        with patch("services.download_service.time.monotonic", fake_monotonic):
            DownloadService().download(
                "u", tmp_path / "out",
                on_progress=lambda p: progress_calls.append(p),
            )
        # Advance fake clock during download to force emit
        t[0] = 100.0

    # We expect: at minimum the final 100% call, possibly some intermediates
    # but well under 50 (one per chunk).
    assert 1 <= len(progress_calls) <= 5, (
        f"expected throttled emission, got {len(progress_calls)} calls"
    )


def test_download_no_progress_callback_works(tmp_path: Path):
    """on_progress is optional."""
    fake_resp = _fake_stream_response([b"x"], content_length=1)
    with patch("services.download_service.requests") as mock_requests:
        mock_requests.get.return_value = fake_resp
        DownloadService().download("u", tmp_path / "out")  # no callback


# ════════════════════════════════════════════════════════════════
# DownloadService.download — cancellation
# ════════════════════════════════════════════════════════════════
def test_download_supports_cancellation(tmp_path: Path):
    """Pre-set cancel_event triggers abort and cleans up partial file."""
    cancel = threading.Event()
    cancel.set()  # already-cancelled

    chunks = [b"chunk1", b"chunk2", b"chunk3"]
    fake_resp = _fake_stream_response(chunks, content_length=len(b"chunk1") * 3)
    target = tmp_path / "out"

    with patch("services.download_service.requests") as mock_requests:
        mock_requests.get.return_value = fake_resp
        with pytest.raises(DownloadAbortedError):
            DownloadService().download(
                "u", target,
                cancel_event=cancel,
            )

    # partial file (if any) must be cleaned up
    assert not target.exists(), "partial file should be removed"


def test_download_cancel_during_stream_aborts(tmp_path: Path):
    """Set cancel mid-stream → abort on next chunk."""
    cancel = threading.Event()

    call_count = [0]

    def iterator_with_cancel():
        for c in [b"alpha", b"bravo", b"charlie", b"delta"]:
            call_count[0] += 1
            if call_count[0] == 2:
                cancel.set()
            yield c

    fake_resp = MagicMock()
    fake_resp.status_code = 200
    fake_resp.headers = {"Content-Length": "26"}
    fake_resp.__enter__ = MagicMock(return_value=fake_resp)
    fake_resp.__exit__ = MagicMock(return_value=False)
    fake_resp.iter_content.return_value = iterator_with_cancel()

    target = tmp_path / "out"

    with patch("services.download_service.requests") as mock_requests:
        mock_requests.get.return_value = fake_resp
        with pytest.raises(DownloadAbortedError):
            DownloadService().download(
                "u", target,
                cancel_event=cancel,
            )

    assert not target.exists()


# ════════════════════════════════════════════════════════════════
# Atomic write — partial files use .part extension
# ════════════════════════════════════════════════════════════════
def test_download_writes_to_part_then_renames(tmp_path: Path):
    """Atomic write pattern: write to .part, then rename.

    On success, only the final file exists (no .part).
    """
    fake_resp = _fake_stream_response([b"content"], content_length=7)
    target = tmp_path / "out"
    part = tmp_path / "out.part"

    with patch("services.download_service.requests") as mock_requests:
        mock_requests.get.return_value = fake_resp
        DownloadService().download("u", target)

    assert target.exists()
    assert not part.exists(), ".part should have been renamed away"
