"""Main view: assembly + theme toggle.

Threading model (read before changing):
  - Flet GUI thread owns all ft.* controls.
  - Long I/O goes through asyncio.to_thread() so the GUI thread never blocks.
  - Background-thread progress callbacks must use page.run_task() to update UI.
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
    make_theme_toggle,
    make_title,
)
from ui.theme import apply_theme, get_palette, is_dark
from ui.throttle import ProgressThrottle

logger = logging.getLogger(__name__)

# Mutable: theme state is held in a list-of-one to mutate from closures.
_state: dict[str, bool] = {"dark": False}


# Shared, module-level cache so theme-toggle rebuilds don't re-fetch.
_cached_versions: list[str] | None = None


def build_main_view(page: ft.Page) -> ft.Container:
    """Construct the main view.

    The theme is mutated by clicking the small icon in the top-right.
    The whole view rebuilds itself when the palette flips.
    """
    download_service = DownloadService()
    cancel_event = threading.Event()
    throttle = ProgressThrottle(hz=30)

    dark = _state["dark"]
    palette = get_palette(dark=dark)
    apply_theme(page, dark=dark)

    # ── Initial version fetch (cached across rebuilds) ────────
    status_text = make_status_text(palette)
    global _cached_versions
    if _cached_versions is None:
        try:
            _cached_versions = get_versions()
        except NetworkError as exc:
            logger.exception("Initial fetch failed")
            status_text.value = f"Could not fetch versions: {exc}"
            _cached_versions = []
    versions = _cached_versions

    grouped = group_by_mc_major(versions)

    def major_key(m: str):
        return tuple(int(x) for x in m.split("."))

    sorted_majors = sorted(grouped.keys(), key=major_key, reverse=True)

    # ── UI components ─────────────────────────────────────────
    major_dropdown = make_dropdown(
        "Major version",
        palette,
        options=[ft.dropdown.Option(text=m) for m in sorted_majors],
    )
    sub_dropdown = make_dropdown(
        "Forge version",
        palette,
        disabled=True,
    )

    progress_bar, progress_status = make_progress_section()

    download_button = make_primary_button(
        "Download installer",
        palette,
        icon=ft.Icons.FILE_DOWNLOAD_OUTLINED,
    )
    cancel_button = make_secondary_button(
        "Cancel",
        palette,
        visible=False,
    )

    # ── Event handlers ────────────────────────────────────────
    def refresh_button_state():
        download_button.disabled = (
            major_dropdown.value is None
            or sub_dropdown.value is None
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
                    url,
                    save_path,
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

    # ── theme toggle: rebuild view with new palette ──────────
    # Reused later from the sidebar's Settings page.
    def on_theme_toggle(e: ft.ControlEvent):
        _state["dark"] = not _state["dark"]
        new_container = build_main_view(page)
        page.controls.clear()
        page.add(new_container)
        page.update()

    theme_toggle = make_theme_toggle(palette, is_dark(dark), on_theme_toggle)

    # ── wire events ────────────────────────────────────────────
    major_dropdown.on_select = on_major_select
    sub_dropdown.on_select = on_sub_select
    download_button.on_click = on_download_click
    cancel_button.on_click = on_cancel_click

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
