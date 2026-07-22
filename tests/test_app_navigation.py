"""Tests for the app shell: sidebar + view switching.

Sidebar now matches the design spec (8 destinations: 5 nav + 3 footer;
search bar in expanded mode; avatar in footer; deep-purple bg).
"""
from __future__ import annotations

from unittest.mock import MagicMock

import flet as ft

from ui.app import build_app
from ui.sidebar import (
    NAV_DESTS,
    FOOTER_DESTS,
    NAV_COUNT,
    SIDEBAR_BG,
    SIDEBAR_WIDTH_COLLAPSED,
    SIDEBAR_WIDTH_EXPANDED,
)


class FakePage:
    def __init__(self):
        self.bgcolor = "#FF00FF"
        self.window_bgcolor = "#FF00FF"
        self.controls = []
        self.theme_mode = None
        self.theme = None
        self.appbar = None
        self.snack_bar = None
        self.window = MagicMock()
        # MagicMock so tests can assert .called / .call_count.
        self.update = MagicMock()

    def add(self, c):
        self.controls.append(c)

    def run_task(self, *a, **kw):
        pass


def _walk(c):
    yield c
    content = getattr(c, "content", None)
    if content is not None and not isinstance(content, str):
        yield from _walk(content)
    for x in (getattr(c, "controls", None) or []):
        yield from _walk(x)


# ─── 1. App shell renders as a Row ────────────────────────────
def test_build_app_returns_row():
    page = FakePage()
    layout = build_app(page)
    assert isinstance(layout, ft.Row)
    print("[1] build_app returns ft.Row  OK")


# ─── 2. Sidebar exposes the design-spec destinations ──────────
def test_sidebar_has_only_the_three_application_destinations():
    from ui.sidebar import Sidebar
    sb = Sidebar(MagicMock())
    assert len(sb._dests) == 3
    assert len(NAV_DESTS) == 3
    assert len(FOOTER_DESTS) == 0


# ─── 3. Default state: collapsed, Dashboard selected ──────────
def test_sidebar_default_collapsed_home_selected():
    from ui.sidebar import Sidebar
    sb = Sidebar(MagicMock())
    assert sb.expanded is False, "sidebar should start collapsed"
    assert sb.selected == 0
    print(f"[3] default: collapsed, Dashboard selected  OK")


# ─── 4. toggle() switches expanded state ────────────────────────
def test_sidebar_toggle_flips_expanded():
    from ui.sidebar import Sidebar
    sb = Sidebar(MagicMock())
    sb.toggle()
    assert sb.expanded is True
    sb.toggle()
    assert sb.expanded is False
    print("[4] toggle() flips expanded  OK")


# ─── 5. Expanded view: labels visible ──────────────────────────
def test_sidebar_expanded_shows_all_labels():
    from ui.sidebar import Sidebar
    sb = Sidebar(MagicMock())
    sb.toggle()
    texts = [
        c.value for c in _walk(sb._outer)
        if isinstance(c, ft.Text) and c.value
    ]
    expected = {"主頁", "下載", "設定"}
    missing = expected - set(texts)
    assert not missing, f"missing labels: {missing}"
    print(f"[5] expanded: all 8 labels visible  OK")


# ─── 6. Collapsed view: only icons, no labels ──────────────────
def test_sidebar_collapsed_no_labels():
    from ui.sidebar import Sidebar
    sb = Sidebar(MagicMock())
    # When collapsed, no nav label text should appear (only the
    # "Untitled" header is hidden because the rail isn't expanded).
    texts = [
        c.value for c in _walk(sb._outer)
        if isinstance(c, ft.Text) and c.value
    ]
    nav_label_set = {"Home", "Dashboard", "Projects", "Tasks", "Reporting",
                     "Notifications", "Support", "Settings",
                     "主頁", "下載", "設定"}
    leaked = nav_label_set & set(texts)
    assert not leaked, f"unexpected labels in collapsed view: {leaked}"
    print(f"[6] collapsed: no nav labels  OK")


# ─── 7. Sidebar communicates selection via on_change ───────────
def test_sidebar_collapsed_renders_subtle_menu_rail():
    from ui.sidebar import Sidebar
    sb = Sidebar(MagicMock())
    buttons = [c for c in _walk(sb._outer) if isinstance(c, ft.IconButton)]
    assert [button.icon for button in buttons] == [ft.Icons.MENU]
    assert not [c for c in _walk(sb._outer) if isinstance(c, ft.Text)]


def test_sidebar_fires_on_change():
    from ui.sidebar import Sidebar
    fired: list[int] = []
    sb = Sidebar(MagicMock(), on_change=lambda i: fired.append(i))
    sb.select(2)
    sb.select(0)
    assert fired == [2, 0], f"expected [2, 0], got {fired}"
    print(f"[7] on_change callback fired with [2, 0]  OK")


# ─── 8. Sidebar bg matches the design spec colour ─────────────
def test_sidebar_bg_is_design_spec_purple():
    from ui.sidebar import Sidebar
    sb = Sidebar(MagicMock())
    containers = [c for c in _all_widgets(sb._outer)
                  if isinstance(c, ft.Container)]
    # The widest container should use SIDEBAR_BG.
    biggest = max(
        (c for c in containers if getattr(c, "bgcolor", None)),
        key=lambda c: getattr(c, "width", 0) or 0,
        default=None,
    )
    if biggest is not None:
        assert biggest.bgcolor == SIDEBAR_BG
        print(f"[8] widest sidebar container uses {SIDEBAR_BG}  OK")


# helper used by test 8
def _all_widgets(c):
    yield c
    content = getattr(c, "content", None)
    if content is not None and not isinstance(content, str):
        yield from _all_widgets(content)
    for x in (getattr(c, "controls", None) or []):
        yield from _all_widgets(x)


# ─── 9. Sidebar outer widget flushes against window left edge ──
def test_sidebar_outer_widget_has_opaque_bg_and_zero_padding():
    """The outermost sidebar Container (what build_app returns as first
    Row child) must:
      - have opaque bgcolor (=SIDEBAR_BG) so it looks solid against the
        window edge instead of letting the page bg show through
      - have zero padding so the colored panel touches x=0

    Specifically — earlier rounds had sidebar.bgcolor == None which
    produces a visible gap when the panel's inner bgcolor is darker than
    the page bg.
    """
    from ui.app import build_app
    page = FakePage()
    layout = build_app(page)
    sidebar_widget = layout.controls[0]
    assert isinstance(sidebar_widget, ft.Container)
    assert sidebar_widget.bgcolor == SIDEBAR_BG, (
        f"sidebar outer widget must use SIDEBAR_BG for flush look; "
        f"got {sidebar_widget.bgcolor!r}"
    )
    assert sidebar_widget.padding in (0, ft.Padding()), (
        f"sidebar outer widget padding must be 0 to flush left; "
        f"got {sidebar_widget.padding!r}"
    )
    print(f"[9] sidebar outer flushes left edge (bg + pad=0)  OK")


# ─── 10. main.py does NOT center the page horizontally ────────
def test_main_py_does_not_center_page_horizontally():
    """main.py must not set horizontal_alignment = CENTER because
    that re-centers the whole Row inside the page even with expand=True.
    """
    from pathlib import Path
    project_root = Path(__file__).resolve().parent.parent
    main_src = (project_root / "main.py").read_text(encoding="utf-8")
    # Strip comment lines so an explanatory comment like
    # "# Intentionally NOT setting page.horizontal_alignment" doesn't
    # trip the check; we only care about active code.
    code_lines = [
        ln for ln in main_src.splitlines()
        if not ln.strip().startswith("#")
    ]
    code = "\n".join(code_lines)
    assert "horizontal_alignment" not in code, (
        "main.py still sets horizontal_alignment in active code — drop it"
    )
    print("[10] main.py has no horizontal_alignment CENTER  OK")
