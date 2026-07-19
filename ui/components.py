"""Flet UI components. All style values sourced from config/theme.

Why factories?
  Every control has the same style settings (border, radius, colors...).
  Without factories we'd repeat ~20 lines per button — both DRY violation
  and a maintenance trap when a style value changes.
"""
from __future__ import annotations

import flet as ft

from ui.theme import button_radius, get_palette


# ─── progress ───────────────────────────────────────────────
def make_progress_section() -> tuple[ft.ProgressBar, ft.Text]:
    """Thin progress bar + status text."""
    return (
        ft.ProgressBar(
            value=0,
            width=320,
            height=2,
            visible=False,
            color=get_palette(dark=False)["accent"],
        ),
        ft.Text("", size=12),
    )


# ─── inputs ─────────────────────────────────────────────────
def make_dropdown(label: str, palette: dict[str, str], **kwargs) -> ft.Dropdown:
    return ft.Dropdown(
        label=label,
        width=320,
        menu_height=200,
        border_color=palette["border"],
        focused_border_color=palette["fg"],
        bgcolor=palette["bg"],
        color=palette["fg"],
        label_style=ft.TextStyle(color=palette["muted"]),
        **kwargs,
    )


# ─── buttons ────────────────────────────────────────────────
def _rounded_shape() -> ft.RoundedRectangleBorder:
    return ft.RoundedRectangleBorder(radius=button_radius())


def make_primary_button(
    text: str,
    palette: dict[str, str],
    icon: str | None = None,
    **kwargs,
) -> ft.ElevatedButton:
    """Filled accent button (rounded)."""
    return ft.ElevatedButton(
        content=ft.Text(text, color=palette["accent_fg"]),
        icon=icon,
        width=320,
        disabled=True,
        style=ft.ButtonStyle(
            shape=_rounded_shape(),
            bgcolor=palette["accent"],
            side=ft.BorderSide(1, palette["border"]),
        ),
        **kwargs,
    )


def make_secondary_button(
    text: str,
    palette: dict[str, str],
    **kwargs,
) -> ft.OutlinedButton:
    return ft.OutlinedButton(
        content=ft.Text(text, color=palette["fg"]),
        width=320,
        style=ft.ButtonStyle(
            shape=_rounded_shape(),
            side=ft.BorderSide(1, palette["border"]),
        ),
        **kwargs,
    )


# ─── text ───────────────────────────────────────────────────
def make_title(text: str, palette: dict[str, str], size: int = 18) -> ft.Text:
    """Section title. Smaller than a hero header."""
    return ft.Text(text, size=size, weight="bold", color=palette["fg"])


def make_status_text(palette: dict[str, str], text: str = "", **kwargs) -> ft.Text:
    """A muted text. Pass `text` (not as kwarg) to set initial content."""
    return ft.Text(text, size=11, color=palette["muted"], **kwargs)


# ─── theme toggle ───────────────────────────────────────────
def make_theme_toggle(
    palette: dict[str, str],
    is_dark_now: bool,
    on_click,
) -> ft.IconButton:
    """Small icon button — sun shows when in dark mode, moon shows when light."""
    return ft.IconButton(
        icon=ft.Icons.LIGHT_MODE if is_dark_now else ft.Icons.DARK_MODE,
        tooltip="Switch theme",
        icon_size=18,
        icon_color=palette["muted"],
        on_click=on_click,
    )


# ─── feedback ───────────────────────────────────────────────
def make_snack(text: str, palette: dict[str, str], kind: str) -> ft.SnackBar:
    """SnackBar with rounded corners. kind ∈ {success, error, neutral}."""
    bg = {
        "success": palette["success"],
        "error":   palette["error"],
        "neutral": palette["neutral"],
    }[kind]
    fg = {
        "success": palette["success_fg"],
        "error":   palette["error_fg"],
        "neutral": palette["neutral_fg"],
    }[kind]
    return ft.SnackBar(
        content=ft.Text(text, color=fg),
        bgcolor=bg,
        shape=_rounded_shape(),
        duration=3000,
    )
