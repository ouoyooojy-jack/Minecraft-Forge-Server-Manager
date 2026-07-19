"""Theme wiring. Palettes live in config.py — this module is just glue."""
from __future__ import annotations

import flet as ft

from config import (
    BUTTON_RADIUS,
    DARK_PALETTE,
    DEFAULT_THEME,
    LIGHT_PALETTE,
)

# Re-export aliases for backwards compatibility with `from ui.theme import ...`
# used in older code paths.
LIGHT = LIGHT_PALETTE
DARK = DARK_PALETTE


def get_palette(dark: bool) -> dict[str, str]:
    return DARK_PALETTE if dark else LIGHT_PALETTE


def is_dark(palette: dict[str, str]) -> bool:
    return palette is DARK_PALETTE


def initial_dark() -> bool:
    return DEFAULT_THEME == "dark"


def button_radius() -> int:
    return BUTTON_RADIUS


def apply_theme(page: ft.Page, dark: bool) -> None:
    """Set page-level theme + bg colors.

    page.bgcolor drives the app body; page.window_bgcolor drives the OS-level
    window chrome (the area outside Flet's view). Setting both keeps title
    bar + body in sync.
    """
    palette = get_palette(dark=dark)
    page.bgcolor = palette["bg"]
    page.window_bgcolor = palette["bg"]
    page.theme_mode = ft.ThemeMode.DARK if dark else ft.ThemeMode.LIGHT
    page.theme = ft.Theme(
        color_scheme_seed=palette["accent"],
        color_scheme=ft.ColorScheme(
            primary=palette["accent"],
            on_primary=palette["accent_fg"],
            surface=palette["surface"],
            on_surface=palette["fg"],
            outline=palette["border"],
            error=palette["error"],
            on_error=palette["error_fg"],
        ),
    )
