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


# View-index map: keep app shell aware of how Sidebar nav translates to views.
# Sidebar indices 0..4 = nav (Home/Dashboard/Projects/Tasks/Reporting).
# Sidebar indices 5..7 = footer items (Notifications/Support/Settings).
# We map index 1 (Dashboard) and 2 (Projects-ish placeholder) and 7
# (Settings) to real views; everything else shows a placeholder.
_DOWNLOADS_INDEX = 1  # we still treat "Dashboard" as the main Dashboard view


def _make_view(index: int, page: ft.Page) -> ft.Container:
    if index == 0:                          # Home
        return build_dashboard_view()
    if index == 1:                          # Dashboard / Downloads area
        return build_downloads_view(page)
    if index in (5, 6):                     # Notifications / Support
        return _placeholder("Notifications/Support coming soon")
    if index == 7:                          # Settings
        return build_settings_view()
    return _placeholder(f"Index {index} not implemented yet")


def _placeholder(message: str) -> ft.Container:
    from ui.theme import get_palette as _gp
    palette = _gp(dark=False)
    return ft.Container(
        content=ft.Column(
            [
                ft.Text(
                    message, color=palette["muted"], size=14,
                ),
            ],
            alignment=ft.MainAxisAlignment.CENTER,
            horizontal_alignment=ft.CrossAxisAlignment.CENTER,
            tight=True,
        ),
        padding=24,
        expand=True,
        bgcolor=palette["bg"],
    )


def build_app(page: ft.Page) -> ft.Row:
    """Construct the app shell."""
    palette = get_palette(dark=True)        # whole app defaults to dark
    apply_theme(page, dark=True)

    content_area = ft.Container(
        content=_make_view(1, page),       # start at Dashboard/Downloads
        expand=True,
        bgcolor=palette["bg"],
    )

    def on_sidebar_select(idx: int) -> None:
        content_area.content = _make_view(idx, page)
        page.update()

    sidebar = Sidebar(page, on_change=on_sidebar_select)
    sidebar_widget = sidebar.build()

    return ft.Row(
        [sidebar_widget, content_area],
        expand=True,
        spacing=8,                         # small gap between panel & body
        tight=False,
    )
