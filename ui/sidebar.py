"""Low-profile, theme-aware application sidebar."""
from __future__ import annotations

from dataclasses import dataclass

import flet as ft


SIDEBAR_WIDTH_COLLAPSED = 48
SIDEBAR_WIDTH_EXPANDED = 180
SIDEBAR_BG = "#1A1A1A"  # compatibility default; instances use the active palette.


@dataclass(frozen=True)
class _Dest:
    label: str
    icon: str
    selected_icon: str


NAV_DESTS = [
    _Dest("主頁", ft.Icons.HOME_OUTLINED, ft.Icons.HOME),
    _Dest("下載", ft.Icons.FILE_DOWNLOAD_OUTLINED, ft.Icons.FILE_DOWNLOAD),
    _Dest("設定", ft.Icons.SETTINGS_OUTLINED, ft.Icons.SETTINGS),
]
FOOTER_DESTS: list[_Dest] = []
ALL_DESTS = NAV_DESTS
NAV_COUNT = len(NAV_DESTS)


class Sidebar:
    """A subtle 48px rail that opens to a compact 180px menu."""

    def __init__(self, page: ft.Page, on_change=None, palette: dict[str, str] | None = None) -> None:
        self._page = page
        self._on_change = on_change
        self._palette = palette or {"rail_bg": SIDEBAR_BG, "fg": "#F2F2F2", "muted": "#A0A0A0", "surface_variant": "#262626"}
        self._expanded = False
        self._selected = 0
        self._dests = list(ALL_DESTS)
        self._outer = self._render()

    @property
    def expanded(self) -> bool:
        return self._expanded

    @property
    def selected(self) -> int:
        return self._selected

    def build(self) -> ft.Container:
        return self._outer

    def toggle(self) -> None:
        self._expanded = not self._expanded
        self._outer.width = SIDEBAR_WIDTH_EXPANDED if self._expanded else SIDEBAR_WIDTH_COLLAPSED
        self._outer.content = self._content()
        self._page.update()

    def select(self, index: int) -> None:
        if not 0 <= index < len(self._dests):
            raise IndexError(f"unknown sidebar destination: {index}")
        self._selected = index
        self._outer.content = self._content()
        self._page.update()
        if self._on_change:
            self._on_change(index)

    def _button(self, icon: str, tooltip: str, on_click) -> ft.IconButton:
        return ft.IconButton(icon=icon, icon_size=19, icon_color=self._palette["muted"], tooltip=tooltip, on_click=on_click)

    def _row(self, index: int, dest: _Dest) -> ft.Container:
        selected = index == self._selected
        if self._expanded:
            content: ft.Control = ft.Row(
                [ft.Icon(dest.selected_icon if selected else dest.icon, size=18, color=self._palette["fg"] if selected else self._palette["muted"]), ft.Text(dest.label, size=13, color=self._palette["fg"] if selected else self._palette["muted"])],
                spacing=10,
            )
        else:
            content = ft.Icon(dest.selected_icon if selected else dest.icon, size=18, color=self._palette["fg"] if selected else self._palette["muted"])
        return ft.Container(
            content=content,
            height=36,
            border_radius=8,
            bgcolor=self._palette["surface_variant"] if selected else None,
            alignment=ft.alignment.Alignment(-1 if self._expanded else 0, 0),
            padding=ft.Padding(left=10 if self._expanded else 0, top=0, right=8, bottom=0),
            on_click=lambda _e, i=index: self.select(i),
            ink=True,
        )

    def _content(self) -> ft.Control:
        toggle = self._button(ft.Icons.MENU_OPEN if self._expanded else ft.Icons.MENU, "收合側邊欄" if self._expanded else "展開側邊欄", lambda _e: self.toggle())
        rows = [self._row(i, dest) for i, dest in enumerate(self._dests)]
        if self._expanded:
            return ft.Column(
                [
                    ft.Container(content=toggle, height=34, alignment=ft.alignment.Alignment(-1, 0), padding=ft.Padding(left=3, top=0, right=0, bottom=0)),
                    ft.Container(content=ft.Column(rows, spacing=2), padding=ft.Padding(left=4, top=4, right=4, bottom=0)),
                ],
                spacing=0,
                expand=True,
            )
        return ft.Column(
            [ft.Container(content=toggle, height=34, alignment=ft.alignment.Alignment(0, 0)), ft.Column(rows, spacing=2, horizontal_alignment=ft.CrossAxisAlignment.CENTER)],
            spacing=4,
            expand=True,
        )

    def _render(self) -> ft.Container:
        return ft.Container(
            content=self._content(),
            width=SIDEBAR_WIDTH_COLLAPSED,
            bgcolor=self._palette["rail_bg"],
            padding=0,
            margin=0,
        )
