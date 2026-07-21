"""Dashboard view: the main / home page.

Pure presentation: shows app intro + quick navigation hints.
No business logic lives here.
"""
from __future__ import annotations

import flet as ft

from ui.components import make_status_text, make_title
from ui.theme import get_palette


def build_dashboard_view(palette: dict[str, str] | None = None) -> ft.Container:
    """Return the dashboard content wrapped in a Container.

    Stateless — re-creating this view costs nothing.
    """
    palette = palette or get_palette(dark=False)

    return ft.Container(
        content=ft.Column(
            [
                make_title("Dashboard", palette, size=22),
                make_status_text(
                    palette, text=(
                        "Use the sidebar to download a Forge installer, "
                        "manage servers, or change settings."
                    ),
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
