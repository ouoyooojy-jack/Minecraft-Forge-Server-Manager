"""Downloads view: version picker + download button + dynamic installer list.

Layout mirrors docs/design/download.pen:
  - top row: Minecraft / Forge dropdowns + download button (horizontal)
  - progress bar + cancel button (between dropdowns and divider)
  - divider
  - scrollable list of installer rows, each row: [filename chip] [trash icon]

The list is dynamic — it follows the actual contents of DEFAULT_DOWNLOAD_DIR.
"""
from __future__ import annotations

import asyncio
import logging
import threading
from pathlib import Path

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
    make_secondary_button,
    make_snack,
)
from ui.theme import get_palette
from ui.throttle import ProgressThrottle

logger = logging.getLogger(__name__)

# Cached across rebuilds to avoid re-fetching on theme toggle.
_cached_versions: list[str] | None = None


def _make_jar_row(
    page: ft.Page,
    palette: dict[str, str],
    jar_path: Path,
    *,
    on_delete: "callable",
    on_rename: "callable",
) -> ft.Container:
    """One installer row, per .pen `rename-row` / `rename-input`:
    an editable filename TextField on the left (rename on submit / blur),
    a trash icon on the right. Width is `fill_container` so it stretches
    with the content column.
    """
    name_field = ft.TextField(
        value=jar_path.name,
        width=580,                          # wide chip, full filename visible (per screenshot)
        height=26,                          # tall chip to match the row
        border_radius=6,                    # matches outer row corner radius
        border_color=palette["border"],
        focused_border_color=palette["fg"],
        bgcolor=palette["surface"],
        color=palette["muted"],             # muted text like a chip
        text_size=14,                       # larger so the full filename is readable
        cursor_color=palette["fg"],
        content_padding=ft.Padding(left=16, right=16, top=8, bottom=8),
        on_submit=lambda e, p=jar_path: on_rename(p, e.control.value),
    )
    # also commit rename on focus loss so users can blur out to save
    name_field.on_blur = lambda e, p=jar_path: on_rename(p, e.control.value)

    trash = ft.IconButton(
        icon=ft.Icons.DELETE_OUTLINE,
        icon_color=palette["muted"],        # grey (per user feedback)
        icon_size=24,
        tooltip="刪除此檔案",
        on_click=lambda _e, p=jar_path: on_delete(p),
    )

    return ft.Container(
        content=ft.Row(
            [name_field, trash],
            alignment=ft.MainAxisAlignment.SPACE_BETWEEN,
            vertical_alignment=ft.CrossAxisAlignment.CENTER,
            spacing=12,
        ),
        width=665,
        height=40,                           # taller row to match screenshot
        border_radius=6,
        border=ft.Border.all(1, palette["border"]),
        bgcolor=palette["surface"],
        padding=ft.Padding(left=12, right=12, top=0, bottom=0),
        alignment=ft.alignment.Alignment(0, 0),
    )


def build_downloads_view(page: ft.Page, palette: dict[str, str] | None = None) -> ft.Container:
    """Construct the downloads page."""
    palette = palette or get_palette(dark=False)

    download_service = DownloadService()
    cancel_event = threading.Event()
    throttle = ProgressThrottle(hz=30)

    def major_key(m: str):
        return tuple(int(x) for x in m.split("."))

    grouped: dict[str, list[str]] = {}
    status_text = ft.Text(
        "正在載入 Forge 版本…",
        size=12,
        color=palette["muted"],
    )

    # ── Top row: 3 widgets side by side, left-aligned ────────────
    major_dropdown = make_dropdown(
        "Minecraft 版本",
        palette,
        width=280,
        options=[],
        disabled=True,
    )
    sub_dropdown = make_dropdown(
        "Forge Server 版本",
        palette,
        width=280,
        disabled=True,
    )
    download_button = make_primary_button(
        "下載 Forge Server Installer",
        palette,
        icon=ft.Icons.FILE_DOWNLOAD_OUTLINED,
    )

    top_row = ft.Row(
        [major_dropdown, sub_dropdown, download_button],
        alignment=ft.MainAxisAlignment.START,
        vertical_alignment=ft.CrossAxisAlignment.CENTER,
        spacing=12,
    )

    # ── Progress (between dropdowns and divider, per user instruction) ──
    progress_bar = ft.ProgressBar(
        value=0,
        width=856,  # matches `rename-input` width
        height=2,
        visible=False,
        color=palette["accent"],
    )
    cancel_button = make_secondary_button("Cancel", palette, width=120, visible=False)

    progress_row = ft.Row(
        [progress_bar, cancel_button],
        alignment=ft.MainAxisAlignment.START,
        vertical_alignment=ft.CrossAxisAlignment.CENTER,
        spacing=12,
    )

    # ── Bottom: scrollable dynamic installer list (per .pen `rename-row`) ─
    list_column = ft.Column(spacing=8, scroll=ft.ScrollMode.AUTO)

    def refresh_jars() -> None:
        DEFAULT_DOWNLOAD_DIR.mkdir(parents=True, exist_ok=True)
        jars = sorted(DEFAULT_DOWNLOAD_DIR.glob("*.jar"), key=lambda p: p.name.lower())
        list_column.controls = [
            _make_jar_row(page, palette, jar,
                          on_delete=delete_jar,
                          on_rename=rename_jar)
            for jar in jars
        ]

    def delete_jar(jar_path: Path) -> None:
        if jar_path.is_file() and jar_path.parent.resolve() == DEFAULT_DOWNLOAD_DIR.resolve():
            jar_path.unlink()
            status_text.value = f"已刪除 {jar_path.name}"
            refresh_jars()
        else:
            status_text.value = "請選擇 downloads/ 中的 JAR。"
        page.update()

    def rename_jar(jar_path: Path, new_name: str) -> None:
        name = (new_name or "").strip()
        if not name:
            return  # empty submit is a no-op (don't clobber the user's name)
        if Path(name).name != name:
            status_text.value = "請輸入有效檔名（不可含路徑分隔符）。"
            page.update()
            return
        if not (jar_path.is_file() and jar_path.parent.resolve() == DEFAULT_DOWNLOAD_DIR.resolve()):
            status_text.value = "請選擇 downloads/ 中的 JAR。"
            page.update()
            return
        target = jar_path.with_name(name if name.endswith(".jar") else f"{name}.jar")
        if target == jar_path:
            return
        if target.exists():
            status_text.value = f"已存在同名檔案：{target.name}"
            page.update()
            return
        jar_path.rename(target)
        status_text.value = f"已重新命名為 {target.name}"
        refresh_jars()
        page.update()

    refresh_jars()

    # ── Event handlers (unchanged from before) ─────────────────────
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
        status_text.value = f"正在下載 Forge Server Installer {version}…"
        page.update()

        def on_progress(info: ProgressInfo) -> None:
            if not throttle.should_emit():
                return
            pct = info.percent / 100.0

            async def update() -> None:
                progress_bar.value = pct
                status_text.value = (
                    f"{info.percent:.1f}% · {info.speed_mbps:.2f} MB/s"
                )
                page.update()
            page.run_task(update)

        async def download_flow() -> None:
            try:
                saved = await asyncio.to_thread(
                    download_service.download,
                    url, save_path,
                    on_progress=on_progress,
                    cancel_event=cancel_event,
                )
                progress_bar.value = 1.0
                refresh_jars()
                status_text.value = f"Saved → {saved.name}"
                page.snack_bar = make_snack(
                    f"Downloaded: {saved.name}", palette, "success",
                )
                page.snack_bar.open = True
            except VersionNotFoundError:
                status_text.value = "Version not found"
                page.snack_bar = make_snack(
                    f"Not on Maven: {version}", palette, "error",
                )
                page.snack_bar.open = True
            except NetworkError as exc:
                status_text.value = f"Network error: {exc}"
                page.snack_bar = make_snack(
                    f"Network: {exc}", palette, "error",
                )
                page.snack_bar.open = True
            except DownloadAbortedError:
                status_text.value = "Cancelled"
                page.snack_bar = make_snack("Cancelled", palette, "neutral")
                page.snack_bar.open = True
            except MinecraftServerError as exc:
                status_text.value = f"Error: {exc}"
                page.snack_bar = make_snack(str(exc), palette, "error")
                page.snack_bar.open = True
            finally:
                set_ui_busy(False)
                page.update()

        page.run_task(download_flow)

    def on_cancel_click(e: ft.ControlEvent):
        cancel_event.set()
        cancel_button.disabled = True
        status_text.value = "Cancelling..."
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
                top_row,
                progress_row,
                ft.Divider(color=palette["border"]),
                list_column,
            ],
            alignment=ft.MainAxisAlignment.START,
            horizontal_alignment=ft.CrossAxisAlignment.START,
            spacing=16,
            scroll=ft.ScrollMode.AUTO,
        ),
        padding=24,
        alignment=ft.alignment.Alignment(-1, -1),
        bgcolor=palette["bg"],
        expand=True,
    )
