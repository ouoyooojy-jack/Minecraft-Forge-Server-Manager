"""Tests for ui/theme: palettes sourced from config, Flet wiring."""
from __future__ import annotations

from unittest.mock import MagicMock

import flet as ft

from config import DARK_PALETTE, LIGHT_PALETTE
from ui.theme import (
    apply_theme,
    button_radius,
    get_palette,
    is_dark,
    LIGHT,
    DARK,
)


REQUIRED_PALETTE_KEYS = {
    "bg", "fg", "muted", "border", "accent", "accent_fg",
    "error", "error_fg", "success", "success_fg",
    "neutral", "neutral_fg", "rail_bg", "surface", "surface_variant",
}


def test_palettes_have_required_keys():
    for palette in (LIGHT, DARK):
        missing = REQUIRED_PALETTE_KEYS - set(palette)
        assert not missing, f"palette missing keys: {missing}"


def test_light_palette_is_white_bg():
    assert LIGHT["bg"] == "#FFFFFF"
    assert LIGHT["fg"] == "#0A0A0A"


def test_dark_palette_is_near_black_bg():
    """Dark theme is near-black (with grey) — not pure."""
    assert DARK["bg"] == "#121212"
    assert DARK["fg"] == "#F2F2F2"


def test_palettes_are_distinct_objects():
    """LIGHT and DARK come from config and are not the same dict."""
    assert LIGHT is not DARK
    assert LIGHT is LIGHT_PALETTE
    assert DARK is DARK_PALETTE


def test_palettes_re_exported():
    """Old imports (ui.theme.LIGHT, ui.theme.DARK) still work."""
    from ui import theme
    assert theme.LIGHT is LIGHT_PALETTE
    assert theme.DARK is DARK_PALETTE


def test_get_palette_dark():
    assert is_dark(get_palette(dark=True)) is True


def test_get_palette_light():
    assert is_dark(get_palette(dark=False)) is False


def test_button_radius_from_config():
    """Button radius comes from config.BUTTON_RADIUS."""
    assert button_radius() > 0
    assert isinstance(button_radius(), int)


def test_apply_theme_dark_sets_page_bgcolor_and_window():
    fake_page = MagicMock()
    apply_theme(fake_page, dark=True)
    assert fake_page.bgcolor == "#121212"
    # window_bgcolor must match so the OS title bar area blends.
    assert fake_page.window_bgcolor == "#121212"
    assert fake_page.theme_mode == ft.ThemeMode.DARK


def test_apply_theme_light_sets_white_bg():
    fake_page = MagicMock()
    apply_theme(fake_page, dark=False)
    assert fake_page.bgcolor == "#FFFFFF"
    assert fake_page.window_bgcolor == "#FFFFFF"
    assert fake_page.theme_mode == ft.ThemeMode.LIGHT


def test_palettes_have_acceptable_contrast():
    """Sanity: bg and fg differ."""
    for palette in (LIGHT, DARK):
        assert palette["bg"] != palette["fg"], (
            f"bg and fg identical in palette: {palette['bg']}"
        )


def test_rail_bg_distinct_from_bg():
    """Sidebar must look slightly different from main bg."""
    for palette in (LIGHT, DARK):
        assert palette["rail_bg"] != palette["bg"], (
            "sidebar should be visually distinguishable from main bg"
        )
