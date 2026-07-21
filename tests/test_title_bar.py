"""Tests for the custom Windows title-bar composition."""
from __future__ import annotations

from unittest.mock import MagicMock
import inspect

import flet as ft

from ui.title_bar import TITLE_BAR_HEIGHT, build_title_bar


def test_title_bar_is_draggable_and_has_three_window_controls():
    page = MagicMock()
    title_bar = build_title_bar(page)
    assert isinstance(title_bar, ft.Container)
    assert title_bar.height == TITLE_BAR_HEIGHT
    controls = title_bar.content.controls
    assert isinstance(controls[0], ft.GestureDetector)
    # Drag surface, theme-slot, then minimize/maximize/close.
    assert len(controls) == 5


def test_drag_and_close_handlers_are_async_window_operations():
    page = MagicMock()
    title_bar = build_title_bar(page)
    drag_handler = title_bar.content.controls[0].on_pan_start
    close_handler = title_bar.content.controls[-1].on_click
    assert inspect.iscoroutinefunction(drag_handler)
    assert inspect.iscoroutinefunction(close_handler)


def test_main_configures_hidden_os_chrome_and_custom_buttons():
    source = open("main.py", encoding="utf-8").read()
    assert "page.window.title_bar_hidden = True" in source
    assert "page.window.title_bar_buttons_hidden = True" in source
