"""Downloads view: version picker + download button + progress.

Behavioral note (carry-over from the original main_view):
  - This view owns its own download service + cancel event.
  - Theme state lives in main_view legacy module (temporarily).

Future cleanup: pull download service into a shared owner module.
"""
from __future__ import annotations

import asyncio
import logging
import threading

import flet as ft

from config import DEFAULT_DOWNLOAD_DIR, FORGE_INSTALLER_URL_TEMPLATE
from exceptions import (
    DownloadAbortedError,
    MinecraftServerError,
    NetworkError,
    VersionNotFoundError,
)
from services.download_service import DownloadService, ProgressInfo
from services.forge_api import get_versions, group_by_mc_major
from ui.components import (
    make_dropdown,
    make_primary_button,
    make_progress_section,
    make_secondary_button,
    make_snack,
    make_status_text,
)
from ui.theme import apply_theme, get_palette
from ui.throttle import ProgressThrottle

logger = logging.getLogger(__name__)

# Cached across rebuilds to avoid re-fetching on theme toggle.
_cached_versions: list[str] | None = None


def build_downloads_view(page: ft.Page, palette: dict[str, str] | None = None) -> ft.Container:
    """Construct the downloads page."""
    palette = palette or get_palette(dark=False)

    download_service = DownloadService()
    cancel_event = threading.Event()
    throttle = ProgressThrottle(hz=30)

    def major_key(m: str):
        return tuple(int(x) for x in m.split("."))

    # The view must render before contacting Forge Maven.  ``requests`` is
    # synchronous, so performing it while constructing this view freezes the
    # Flet UI thread for as long as the HTTP timeout.
    grouped: dict[str, list[str]] = {}
    status_text = make_status_text(palette, text="正在載入 Forge 版本…")

    major_dropdown = make_dropdown(
        "Major version",
        palette,
        options=[],
        disabled=True,
    )
    sub_dropdown = make_dropdown("Forge version", palette, disabled=True)

    progress_bar, progress_status = make_progress_section()
    download_button = make_primary_button(
        "Download installer",
        palette,
        icon=ft.Icons.FILE_DOWNLOAD_OUTLINED,
    )
    cancel_button = make_secondary_button("Cancel", palette, visible=False)

    def refresh_button_state():
        download_button.disabled = (
            major_dropdown.value is None or sub_dropdown.value is None
        )

    def set_ui_busy(busy: bool):
        download_button.disabled = busy
        cancel_button.disabled = not busy
        cancel_button.visible = busy
        progress_bar.visible = busy
        major_dropdown.disabled = busy
        sub_dropdown.disabled = busy

    def on_major_select(e: ft.ControlEvent):
        major = major_dropdown.value
        sub_dropdown.value = None
        if major and major in grouped:
            sub_dropdown.options = [
                ft.dropdown.Option(text=v) for v in grouped[major]
            ]
            sub_dropdown.disabled = False
        else:
            sub_dropdown.options = []
            sub_dropdown.disabled = True
        refresh_button_state()
        page.update()

    def load_options(versions: list[str]) -> None:
        grouped.clear()
        grouped.update(group_by_mc_major(versions))
        sorted_majors = sorted(grouped.keys(), key=major_key, reverse=True)
        major_dropdown.options = [ft.dropdown.Option(text=m) for m in sorted_majors]
        major_dropdown.disabled = not bool(sorted_majors)
        status_text.value = (
            "選擇 Minecraft 與 Forge 版本。"
            if sorted_majors else "目前沒有可用的 Forge 版本。"
        )

    async def load_versions() -> None:
        global _cached_versions
        try:
            versions = await asyncio.to_thread(get_versions)
        except NetworkError as exc:
            logger.warning("Initial fetch failed: %s", exc)
            versions = []
            status_text.value = "無法載入 Forge 版本，請稍後再試。"
        else:
            status_text.value = "選擇 Minecraft 與 Forge 版本。"
        _cached_versions = versions
        load_options(versions)
        page.update()

    def on_sub_select(e: ft.ControlEvent):
        refresh_button_state()
        page.update()

    def on_download_click(e: ft.ControlEvent):
        version = sub_dropdown.value
        if not version:
            return
        url = FORGE_INSTALLER_URL_TEMPLATE.format(version=version)
        save_path = DEFAULT_DOWNLOAD_DIR / f"forge-{version}-installer.jar"

        cancel_event.clear()
        throttle._last_emit = 0.0
        set_ui_busy(True)
        progress_bar.value = 0
        progress_status.value = f"Starting {version}..."
        page.update()

        def on_progress(info: ProgressInfo) -> None:
            if not throttle.should_emit():
                return
            pct = info.percent / 100.0

            async def update():
                progress_bar.value = pct
                progress_status.value = (
                    f"{info.percent:.1f}% · {info.speed_mbps:.2f} MB/s"
                )
                page.update()
            page.run_task(update)

        async def download_flow():
            try:
                saved = await asyncio.to_thread(
                    download_service.download,
                    url, save_path,
                    on_progress=on_progress,
                    cancel_event=cancel_event,
                )
                progress_bar.value = 1.0
                progress_status.value = f"Saved → {saved.name}"
                page.snack_bar = make_snack(
                    f"Downloaded: {saved.name}", palette, "success",
                )
                page.snack_bar.open = True
            except VersionNotFoundError:
                progress_status.value = "Version not found"
                page.snack_bar = make_snack(
                    f"Not on Maven: {version}", palette, "error",
                )
                page.snack_bar.open = True
            except NetworkError as exc:
                progress_status.value = f"Network error: {exc}"
                page.snack_bar = make_snack(
                    f"Network: {exc}", palette, "error",
                )
                page.snack_bar.open = True
            except DownloadAbortedError:
                progress_status.value = "Cancelled"
                page.snack_bar = make_snack("Cancelled", palette, "neutral")
                page.snack_bar.open = True
            except MinecraftServerError as exc:
                progress_status.value = f"Error: {exc}"
                page.snack_bar = make_snack(str(exc), palette, "error")
                page.snack_bar.open = True
            finally:
                set_ui_busy(False)
                page.update()

        page.run_task(download_flow)

    def on_cancel_click(e: ft.ControlEvent):
        cancel_event.set()
        cancel_button.disabled = True
        progress_status.value = "Cancelling..."
        page.update()

    major_dropdown.on_select = on_major_select
    sub_dropdown.on_select = on_sub_select
    download_button.on_click = on_download_click
    cancel_button.on_click = on_cancel_click

    global _cached_versions
    if _cached_versions is None:
        page.run_task(load_versions)
    else:
        load_options(_cached_versions)

    return ft.Container(
        content=ft.Column(
            [
                status_text,
                major_dropdown,
                sub_dropdown,
                download_button,
                cancel_button,
                progress_bar,
                progress_status,
            ],
            alignment=ft.MainAxisAlignment.CENTER,
            horizontal_alignment=ft.CrossAxisAlignment.CENTER,
            spacing=14,
            tight=True,
        ),
        padding=24,
        alignment=ft.alignment.Alignment(0, 0),
        bgcolor=palette["bg"],
    )
