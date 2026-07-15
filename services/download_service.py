"""Streaming HTTP download with progress and cancellation. See docs/download_service.md."""
from __future__ import annotations

import logging
import threading
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Callable

import requests

from config import DOWNLOAD_CHUNK_SIZE, HTTP_TIMEOUT_SEC
from exceptions import (
    DownloadAbortedError,
    NetworkError,
    VersionNotFoundError,
)

logger = logging.getLogger(__name__)


@dataclass(frozen=True)
class ProgressInfo:
    """Snapshot of download progress at one moment in time."""
    downloaded_bytes: int
    total_bytes: int
    elapsed_seconds: float

    @property
    def percent(self) -> float:
        if self.total_bytes <= 0:
            return 0.0
        return self.downloaded_bytes / self.total_bytes * 100.0

    @property
    def speed_mbps(self) -> float:
        if self.elapsed_seconds <= 0:
            return 0.0
        mb = self.downloaded_bytes / (1024 * 1024)
        return mb / self.elapsed_seconds

    @property
    def eta_seconds(self) -> float | None:
        if self.speed_mbps <= 0 or self.total_bytes <= self.downloaded_bytes:
            return None
        remaining_mb = (self.total_bytes - self.downloaded_bytes) / (1024 * 1024)
        return remaining_mb / self.speed_mbps


ProgressCallback = Callable[[ProgressInfo], None]


class DownloadService:
    """Stream-based, cancellable HTTP downloader."""

    def download(
        self,
        url: str,
        save_path: Path,
        *,
        on_progress: ProgressCallback | None = None,
        cancel_event: threading.Event | None = None,
        chunk_size: int = DOWNLOAD_CHUNK_SIZE,
        timeout: int = HTTP_TIMEOUT_SEC,
    ) -> Path:
        """Download `url` to `save_path`. Returns the saved Path.

        Args:
            url: HTTP URL.
            save_path: Local destination (parent dirs are created).
            on_progress: Optional callback invoked (at most ~30 Hz) with
                ProgressInfo snapshots.
            cancel_event: If set() before/during download, raises
                DownloadAbortedError and removes any partial file.
            chunk_size: Read buffer in bytes.
            timeout: HTTP connect timeout (seconds).

        Returns:
            Path to the saved file.

        Raises:
            VersionNotFoundError: HTTP 404.
            NetworkError: Other network or non-2xx failures.
            DownloadAbortedError: cancel_event was set.
        """
        cancel_event = cancel_event or threading.Event()
        save_path = Path(save_path)
        save_path.parent.mkdir(parents=True, exist_ok=True)

        try:
            response = requests.get(url, timeout=timeout, stream=True)
        except requests.RequestException as exc:
            raise NetworkError(f"Could not start download: {exc}") from exc

        with response:
            if response.status_code == 404:
                raise VersionNotFoundError(f"Not found at {url}")
            if response.status_code != 200:
                raise NetworkError(
                    f"HTTP {response.status_code} from {url}"
                )
            try:
                total_bytes = int(response.headers.get("Content-Length", "0"))
            except ValueError:
                total_bytes = 0
            return self._stream_to_file(
                response, save_path, total_bytes, chunk_size,
                on_progress, cancel_event,
            )

    def _stream_to_file(
        self,
        response: requests.Response,
        save_path: Path,
        total_bytes: int,
        chunk_size: int,
        on_progress: ProgressCallback | None,
        cancel_event: threading.Event,
    ) -> Path:
        start = time.monotonic()
        last_emit = 0.0
        throttle = 1.0 / 30  # ~30 Hz
        downloaded = 0

        # Atomic-write: write to .part, rename on success
        tmp_path = save_path.with_suffix(save_path.suffix + ".part")

        def emit(percent: float | None = None) -> None:
            if on_progress is None:
                return
            nonlocal last_emit
            now = time.monotonic()
            if percent is None and (now - last_emit) < throttle:
                return
            info = ProgressInfo(
                downloaded_bytes=downloaded,
                total_bytes=total_bytes,
                elapsed_seconds=now - start,
            )
            on_progress(info)
            last_emit = now

        try:
            with tmp_path.open("wb") as f:
                for chunk in response.iter_content(chunk_size=chunk_size):
                    if cancel_event.is_set():
                        raise DownloadAbortedError(
                            f"Download cancelled: {save_path}"
                        )
                    if not chunk:
                        continue
                    f.write(chunk)
                    downloaded += len(chunk)
                    emit()
        except DownloadAbortedError:
            if tmp_path.exists():
                tmp_path.unlink()
            raise

        tmp_path.replace(save_path)

        # final 100% emit (force-bypass throttle)
        emit(percent=100.0)

        logger.info(
            "Downloaded %s (%.2f MB)",
            save_path, downloaded / (1024 * 1024),
        )
        return save_path
