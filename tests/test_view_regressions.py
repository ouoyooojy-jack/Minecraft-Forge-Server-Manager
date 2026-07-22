"""Regression checks for view-construction errors reported by the desktop app."""
from __future__ import annotations

from unittest.mock import MagicMock

from ui.views.settings import build_settings_view


def test_settings_view_builds_with_folder_picker_button():
    page = MagicMock()
    palette = {
        "bg": "#121212", "fg": "#fff", "muted": "#aaa", "border": "#333",
        "accent": "#eee", "accent_fg": "#111", "surface": "#1e1e1e",
        "surface_variant": "#262626",
    }
    assert build_settings_view(page, palette) is not None
