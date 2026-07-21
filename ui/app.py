"""App shell: Hermes-style sidebar + content area.

The Sidebar is flush against the window's left edge (no padding).
The shell is a Row of two Containers (sidebar panel, content area).
"""
from __future__ import annotations

import flet as ft

from ui.sidebar import Sidebar
from ui.theme import apply_theme, get_palette
from ui.views.dashboard import build_dashboard_view
from ui.views.downloads import build_downloads_view
from ui.views.settings import build_settings_view


def _make_view(index: int, page: ft.Page, palette: dict[str, str]) -> ft.Container:
    if index == 0:
        return build_dashboard_view(palette)
    if index == 1:
        return build_downloads_view(page, palette)
    if index == 2:
        return build_settings_view(palette)
    raise ValueError(f"unknown application view: {index}")


def build_app(page: ft.Page, dark: bool = True) -> ft.Row:
    """Construct the app shell."""
    palette = get_palette(dark=dark)
    apply_theme(page, dark=dark)

    content_area = ft.Container(
        content=_make_view(0, page, palette),
        expand=True,
        bgcolor=palette["bg"],
    )

    def on_sidebar_select(idx: int) -> None:
        content_area.content = _make_view(idx, page, palette)
        page.update()

    sidebar = Sidebar(page, on_change=on_sidebar_select, palette=palette)
    sidebar_widget = sidebar.build()

    # The zero spacing keeps the black sidebar flush against the body, like
    # the ChatGPT/Hermes application shell rather than a floating card.
    return ft.Row(
        [sidebar_widget, content_area],
        expand=True,
        spacing=0,
        tight=False,
    )
