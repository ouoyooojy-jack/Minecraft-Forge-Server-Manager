"""Hermes-style custom sidebar.

Layout (matches the design spec):
  COLLAPSED (default):
    ┌──┐
    │≡ │  ← toggle
    │🏠│  ← icon-only nav column
    │📊│
    │📁│
    │✓ │
    │📈│
    │  │  ← spacer
    │🔔│
    │❓│
    │⚙ │
    │👤│  ← avatar
    └──┘
  EXPANDED:
    ┌──────────┐
    │🟪 Untitled ☰│  ← header (logo + toggle)
    │[ Search     ]│
    │🏠 Home      │
    │📊 Dashboard │ ← active highlight
    │📁 Projects  │
    │✓  Tasks    │
    │📈 Reporting │
    │            │  ← spacer
    │🔔 Notif...   │
    │❓ Support   │
    │⚙  Settings  │
    │🟢 avatar    │
    └──────────┘

Sidebar communicates selection to the host via on_change(index).
Destination model:
  - 5 nav items (Home, Dashboard, Projects, Tasks, Reporting)
  - 3 footer items (Notifications, Support, Settings)
  - 1 avatar (footer user)
"""
from __future__ import annotations

from dataclasses import dataclass

import flet as ft

from config import (
    SIDEBAR_RADIUS,
    SIDEBAR_SEARCH_HEIGHT,
    SIDEBAR_WIDTH_COLLAPSED,
    SIDEBAR_WIDTH_EXPANDED,
)


# ─── palette (sidebar-specific; centralised here) ──────────────
# We hardcode for now because the design spec is fixed; later these can
# move into config.py if you want themed variants.
SIDEBAR_BG = "#1E1B2E"          # very-dark purple/navy
SIDEBAR_BORDER = "#2A2738"
SIDEBAR_LABEL = "#E6E4EE"
SIDEBAR_LABEL_MUTED = "#8A87A0"
SIDEBAR_ACTIVE_BG = "#2A2738"    # selection indicator
SIDEBAR_AVATAR_BG = "#5B57D9"    # purple chip for the user icon


@dataclass(frozen=True)
class _Dest:
    label: str
    icon: str
    selected_icon: str
    is_footer: bool = False  # visual group toggle (nav top vs footer bottom)


NAV_DESTS: list[_Dest] = [
    _Dest("Home",       ft.Icons.HOME_OUTLINED,          ft.Icons.HOME),
    _Dest("Dashboard",  ft.Icons.DASHBOARD_OUTLINED,     ft.Icons.DASHBOARD),
    _Dest("Projects",   ft.Icons.FOLDER_OUTLINED,        ft.Icons.FOLDER),
    _Dest("Tasks",      ft.Icons.CHECK_CIRCLE_OUTLINED,  ft.Icons.CHECK_CIRCLE),
    _Dest("Reporting",  ft.Icons.BAR_CHART_OUTLINED,      ft.Icons.BAR_CHART),
]
FOOTER_DESTS: list[_Dest] = [
    _Dest("Notifications", ft.Icons.NOTIFICATIONS_OUTLINED,
          ft.Icons.NOTIFICATIONS, is_footer=True),
    _Dest("Support",    ft.Icons.HELP_OUTLINED,           ft.Icons.HELP_OUTLINED,
          is_footer=True),
    _Dest("Settings",   ft.Icons.SETTINGS_OUTLINED,       ft.Icons.SETTINGS,
          is_footer=True),
]


# Union of nav + footer: index 0-4 are nav, 5-7 are footer.
ALL_DESTS: list[_Dest] = NAV_DESTS + FOOTER_DESTS
NAV_COUNT = len(NAV_DESTS)


class Sidebar:
    """Collapsible, dark-themed, rounded sidebar.

    Public:
      - expanded        read-only bool
      - selected        read-only int (index into ALL_DESTS)
      - on_change       callback fired with int when the user picks a dest
      - toggle()        flip expanded
      - select(i)       programmatic switch (also fires on_change)
      - build()         return the outer Container
      - _outer          the outer Container (set by __init__)
    """

    def __init__(
        self,
        page: ft.Page,
        on_change=None,
    ) -> None:
        self._page = page
        self._on_change = on_change
        self._expanded: bool = False
        self._selected: int = 1   # default to "Dashboard" per design
        self._dests: list[_Dest] = list(ALL_DESTS)

        # Toggle icon (top-right when expanded, the only one in collapsed).
        self._toggle_btn = ft.IconButton(
            icon=ft.Icons.MENU,
            icon_color=SIDEBAR_LABEL_MUTED,
            icon_size=20,
            tooltip="Toggle sidebar",
            on_click=lambda _e: self.toggle(),
        )

        # Pre-build all destination row containers (re-rendered on toggle
        # / select).
        self._row_containers: list[ft.Container] = [
            self._make_row(i, d) for i, d in enumerate(self._dests)
        ]

        # Build the outer container once.
        self._outer = self._render()

    # ── public API ──────────────────────────────────────────────
    @property
    def expanded(self) -> bool:
        return self._expanded

    @property
    def selected(self) -> int:
        return self._selected

    def toggle(self) -> None:
        self._expanded = not self._expanded
        self._toggle_btn.icon = (
            ft.Icons.CLOSE if self._expanded else ft.Icons.MENU
        )
        self._refresh()
        self._page.update()

    def select(self, index: int) -> None:
        if index == self._selected:
            return
        self._selected = index
        self._refresh()
        self._page.update()
        if self._on_change is not None:
            self._on_change(index)

    def build(self) -> ft.Container:
        return self._outer

    # ── rendering ───────────────────────────────────────────────
    def _make_row(self, idx: int, d: _Dest) -> ft.Container:
        """Per-destination chip: an icon (always) plus optional label."""
        is_sel = (idx == self._selected)
        icon_color = SIDEBAR_LABEL if is_sel else SIDEBAR_LABEL_MUTED

        icon = ft.Icon(
            d.selected_icon if is_sel else d.icon,
            color=icon_color,
            size=20,
        )

        # In COLLAPSED mode we just want the icon centered in a 40x40 chip.
        # In EXPANDED mode the row holds [icon, label].
        if self._expanded:
            label = ft.Text(
                d.label,
                color=SIDEBAR_LABEL if is_sel else SIDEBAR_LABEL_MUTED,
                size=13,
                weight="w500" if is_sel else "w400",
            )
            children: list[ft.Control] = [icon, label]
        else:
            children = [icon]

        return ft.Container(
            content=ft.Row(
                children,
                spacing=12,
                vertical_alignment=ft.CrossAxisAlignment.CENTER,
                tight=True,
            ),
            padding=ft.Padding(
                top=8, bottom=8, left=12 if self._expanded else 0, right=12,
            ),
            border_radius=10,
            bgcolor=SIDEBAR_ACTIVE_BG if is_sel else None,
            ink=not is_sel,                     # ripple on hover for non-active
            on_click=lambda _e, i=idx: self._on_select(i),
            width=(
                SIDEBAR_WIDTH_EXPANDED - 24
                if self._expanded else SIDEBAR_WIDTH_COLLAPSED
            ),
            alignment=ft.alignment.Alignment(0, 0),
        )

    def _on_select(self, idx: int) -> None:
        self._selected = idx
        self._refresh()
        self._page.update()
        if self._on_change is not None:
            self._on_change(idx)

    def _refresh(self) -> None:
        """Re-render the row chips with new selected/expanded state."""
        self._row_containers = [
            self._make_row(i, d) for i, d in enumerate(self._dests)
        ]
        self._outer.content = self._build_content()

    def _header(self) -> ft.Control:
        """Top bar — shows title + toggle when expanded, toggle only when collapsed."""
        if self._expanded:
            # Logo + title + toggle button on the right.
            return ft.Container(
                content=ft.Row(
                    [
                        # Logo-ish avatar
                        ft.Container(
                            width=28, height=28,
                            border_radius=8,
                            bgcolor=SIDEBAR_AVATAR_BG,
                            alignment=ft.alignment.Alignment(0, 0),
                            content=ft.Text(
                                "M", color="white",
                                size=14, weight="bold",
                            ),
                        ),
                        ft.Text(
                            "Untitled",
                            color=SIDEBAR_LABEL,
                            size=14, weight="w500",
                        ),
                        # Spacer
                        ft.Container(expand=True),
                        # Toggle on the right
                        ft.Container(
                            content=self._toggle_btn,
                            padding=ft.Padding(
                                top=0, bottom=0, left=0, right=0,
                            ),
                        ),
                    ],
                    spacing=10,
                    vertical_alignment=ft.CrossAxisAlignment.CENTER,
                    tight=True,
                ),
                padding=ft.Padding(top=12, bottom=8, left=12, right=4),
            )
        # Collapsed: only the toggle, centered.
        return ft.Container(
            content=ft.Row(
                [ft.Container(content=self._toggle_btn,
                              padding=ft.Padding(
                                  top=12, bottom=8, left=12, right=12))],
                alignment=ft.MainAxisAlignment.CENTER,
                tight=True,
            ),
            padding=0,
        )

    def _search(self) -> ft.Control:
        """Search bar — visible only when expanded."""
        return ft.Container(
            content=ft.Row(
                [
                    ft.Icon(
                        ft.Icons.SEARCH,
                        size=16,
                        color=SIDEBAR_LABEL_MUTED,
                    ),
                    ft.Text(
                        "Search",
                        color=SIDEBAR_LABEL_MUTED,
                        size=13,
                    ),
                ],
                spacing=10,
                vertical_alignment=ft.CrossAxisAlignment.CENTER,
                tight=True,
            ),
            height=SIDEBAR_SEARCH_HEIGHT,
            border_radius=10,
            bgcolor=SIDEBAR_BORDER,
            padding=ft.Padding(top=0, bottom=0, left=12, right=12),
            margin=ft.Margin(left=8, top=0, right=8, bottom=8),
        )

    def _avatar(self) -> ft.Control:
        """Footer avatar — circular purple chip with an icon."""
        return ft.Container(
            width=36, height=36,
            border_radius=18,
            bgcolor=SIDEBAR_AVATAR_BG,
            alignment=ft.alignment.Alignment(0, 0),
            content=ft.Icon(
                ft.Icons.PERSON,
                color="white",
                size=18,
            ),
            margin=ft.Margin(top=4, bottom=4, left=0, right=0),
        )

    def _build_content(self) -> ft.Control:
        """Compose the whole sidebar inner content from current state."""
        nav_rows = self._row_containers[:NAV_COUNT]
        footer_rows = self._row_containers[NAV_COUNT:]

        nav_col = ft.Column(
            nav_rows, spacing=2, tight=True,
        )
        footer_col = ft.Column(
            footer_rows, spacing=2, tight=True,
        )

        # Vertical layout — header (optional search) → nav → spacer → footer → avatar.
        main_col = ft.Column(
            [
                self._header(),
                self._search() if self._expanded else ft.Container(height=0),
                nav_col,
                ft.Container(expand=True),               # spacer
                footer_col,
                self._avatar(),
            ],
            spacing=0,
            tight=True,
            expand=True,
        )

        # The whole panel.
        panel = ft.Container(
            content=main_col,
            width=(
                SIDEBAR_WIDTH_EXPANDED
                if self._expanded else SIDEBAR_WIDTH_COLLAPSED
            ),
            bgcolor=SIDEBAR_BG,
            border_radius=SIDEBAR_RADIUS,
            padding=ft.Padding(top=8, bottom=12, left=0, right=0),
        )
        return panel

    def _render(self) -> ft.Container:
        """Wrap the panel in an outer container that hugs the window edge.

        The outer Container is intentionally:
          - padding = 0 (panel sits flush against x=0)
          - bgcolor = SIDEBAR_BG (the outer shell itself is opaque, so the
            sidebar reads as a solid panel right up to the window edge
            instead of letting the page bg show through as a gap)
          - alignment = top-left
        """
        self._outer = ft.Container(
            content=self._build_content(),
            padding=0,
            bgcolor=SIDEBAR_BG,
            alignment=ft.alignment.Alignment(-1, -1),
        )
        return self._outer
