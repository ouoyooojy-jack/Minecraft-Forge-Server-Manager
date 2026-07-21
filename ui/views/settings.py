"""Settings view: theme toggle, download folder display, future Java args.

Pure presentation for now.
"""
from __future__ import annotations

import flet as ft

from config import DEFAULT_DOWNLOAD_DIR
from ui.components import make_status_text, make_title
from ui.theme import get_palette


def build_settings_view(palette: dict[str, str] | None = None) -> ft.Container:
    """Return the settings page content."""
    palette = palette or get_palette(dark=False)

    return ft.Container(
        content=ft.Column(
            [
                make_title("Settings", palette, size=22),
                make_status_text(
                    palette, text=f"Downloads folder: {DEFAULT_DOWNLOAD_DIR}"
                ),
            ],
            alignment=ft.MainAxisAlignment.CENTER,
            horizontal_alignment=ft.CrossAxisAlignment.CENTER,
            spacing=12,
            tight=True,
        ),
        padding=24,
        alignment=ft.alignment.Alignment(0, 0),
        bgcolor=palette["bg"],
    )
