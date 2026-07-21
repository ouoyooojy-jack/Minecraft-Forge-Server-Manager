"""Minimal Fluent-style client title bar with functional window controls."""
from __future__ import annotations

import flet as ft


TITLE_BAR_HEIGHT = 28
TITLE_BAR_BG = "#1A1A1A"


def build_title_bar(page: ft.Page, palette: dict[str, str] | None = None, on_theme_toggle=None) -> ft.Control:
    """Build a thin draggable bar and handle minimize, maximize, and close."""
    palette = palette or {"rail_bg": TITLE_BAR_BG, "muted": "#A0A0A0"}

    async def drag(_event: ft.DragStartEvent) -> None:
        await page.window.start_dragging()

    def minimize(_event: ft.ControlEvent) -> None:
        page.window.minimized = True

    def maximize(_event: ft.ControlEvent) -> None:
        page.window.maximized = not page.window.maximized

    async def close(_event: ft.ControlEvent) -> None:
        await page.window.close()

    def control(icon: str, tooltip: str, action, close_button: bool = False) -> ft.IconButton:
        return ft.IconButton(icon=icon, icon_size=15, icon_color="#FFFFFF" if close_button else palette["muted"], tooltip=tooltip, on_click=action, style=ft.ButtonStyle(shape=ft.RoundedRectangleBorder(radius=0)))

    return ft.Container(
        content=ft.Row(
            [
                ft.GestureDetector(
                    expand=True,
                    on_pan_start=drag,
                    content=ft.Container(content=ft.Text("Mc Server Manager", size=12, color=palette["muted"]), expand=True, padding=ft.Padding(left=10, top=0, right=0, bottom=0), alignment=ft.alignment.Alignment(-1, 0)),
                ),
                ft.IconButton(
                    icon=ft.Icons.LIGHT_MODE if palette["bg"] == "#121212" else ft.Icons.DARK_MODE,
                    icon_size=15,
                    icon_color=palette["muted"],
                    tooltip="切換主題",
                    on_click=on_theme_toggle,
                ) if on_theme_toggle else ft.Container(),
                control(ft.Icons.MINIMIZE, "最小化", minimize),
                control(ft.Icons.CROP_SQUARE, "最大化或還原", maximize),
                control(ft.Icons.CLOSE, "關閉", close, close_button=True),
            ],
            spacing=0,
            vertical_alignment=ft.CrossAxisAlignment.CENTER,
        ),
        height=TITLE_BAR_HEIGHT,
        bgcolor=palette["rail_bg"],
    )
