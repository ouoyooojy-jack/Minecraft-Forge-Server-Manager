"""Settings view: theme toggle + HTTP/progress tunables.

Download directory is intentionally not editable here — installer jars
live in a fixed ``downloads/`` folder next to the app.
"""
from __future__ import annotations

import flet as ft

from config import save_settings, settings_values
from ui.components import (
    make_primary_button,
    make_status_text,
    make_title,
)
from ui.theme import get_palette


def _themed_text_field(
    label: str, value: str, palette: dict[str, str], *,
    width: int = 340, read_only: bool = False,
) -> ft.TextField:
    """TextField styled to follow the active palette."""
    return ft.TextField(
        label=label,
        value=value,
        width=width,
        read_only=read_only,
        border_color=palette["border"],
        focused_border_color=palette["fg"],
        bgcolor=palette["bg"],
        color=palette["fg"],
        label_style=ft.TextStyle(color=palette["muted"]),
        cursor_color=palette["fg"],
    )


def build_settings_view(page: ft.Page, palette: dict[str, str] | None = None) -> ft.Container:
    """Return the settings page content."""
    palette = palette or get_palette(dark=False)

    values = settings_values()
    timeout = _themed_text_field("HTTP 逾時（秒）", str(values["http_timeout_sec"]), palette)
    progress_hz = _themed_text_field("進度更新頻率（Hz）", str(values["progress_update_hz"]), palette)
    theme = ft.Dropdown(
        label="預設主題（下次啟動）",
        value=str(values["default_theme"]),
        width=340,
        border_color=palette["border"],
        focused_border_color=palette["fg"],
        bgcolor=palette["bg"],
        color=palette["fg"],
        label_style=ft.TextStyle(color=palette["muted"]),
        options=[ft.dropdown.Option(text="dark"), ft.dropdown.Option(text="light")],
    )
    status = make_status_text(palette)
    save_button = make_primary_button("儲存設定", palette, disabled=False)

    def save(_event: ft.ControlEvent) -> None:
        try:
            save_settings(
                http_timeout_sec=int(timeout.value or "0"),
                progress_update_hz=int(progress_hz.value or "0"),
                default_theme=theme.value or "dark",
            )
        except ValueError as exc:
            status.value = f"設定無法儲存：{exc}"
        else:
            status.value = "已儲存至 settings.toml；部分服務設定將於下次啟動套用。"

    save_button.on_click = save

    return ft.Container(
        content=ft.Column(
            [
                make_title("App Settings", palette, size=22),
                make_status_text(palette, text="應用程式外觀與網路行為。伺服器設定請在主頁管理。"),
                timeout,
                progress_hz,
                theme,
                save_button,
                status,
            ],
            alignment=ft.MainAxisAlignment.CENTER,
            horizontal_alignment=ft.CrossAxisAlignment.CENTER,
            spacing=12,
            tight=True,
            scroll=ft.ScrollMode.AUTO,
        ),
        padding=24,
        alignment=ft.alignment.Alignment(0, 0),
        bgcolor=palette["bg"],
    )
