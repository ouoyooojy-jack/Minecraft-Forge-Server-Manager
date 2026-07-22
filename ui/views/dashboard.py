from __future__ import annotations

from pathlib import Path

import flet as ft

from config import _project_root
from ui.components import (
    make_status_text,
    make_title,
)
from ui.theme import get_palette
from ui.views.installer import build_installer_view

# --- 佈局參數 ---
CARD_WIDTH = 230                    # 卡片寬度 230
CARD_HEIGHT = 127                   # 卡片高度 127
CARD_CORNER = 8                     # 圓角 8
ROW_GAP = 172                       # 垂直間距 (127 + 45)
ACTION_ICON_SIZE = 20               # 右下角圖示大小
DOT_SIZE = 6                        # 狀態燈號縮小至 6px
TITLE_FONT_SIZE = 26

# 卡片 X/Y 絕對座標 (將 Y 軸起始位置由 103 修正為 24，消除多餘間距)
ROW1_X = (24, 293, 581, 840)
ROW1_Y = 24                         
ROW2_Y = ROW1_Y + ROW_GAP            
ADD_CARD_X = 24
ADD_CARD_Y = ROW2_Y

# 狀態說明 Legend 資料
LEGEND_LABELS = ("online", "offline", "starting", "error")
LEGEND_COLORS = ("#04ff00ff", "#9e9e9eff", "#ff6600ff", "#ff0000ff")

# Lucide -> Material 圖示對照
LUCIDE_TO_MATERIAL: dict[str, str] = {
    "terminal":     "TERMINAL",
    "pencil-line":  "EDIT",
    "settings-2":   "TUNE",
    "plus":         "ADD",
}

CARD_ACTION_ICONS: tuple[str, ...] = ("terminal", "pencil-line", "settings-2")


def _parse_color(color_str: str) -> str:
    """轉換 Web #RRGGBBAA 色碼為 Flet/Flutter 的 #AARRGGBB 格式"""
    if color_str.startswith("#") and len(color_str) == 9:
        r, g, b, a = color_str[1:3], color_str[3:5], color_str[5:7], color_str[7:9]
        return f"#{a}{r}{g}{b}"
    return color_str


def _servers_root() -> Path:
    root = _project_root() / "servers"
    root.mkdir(parents=True, exist_ok=True)
    return root


def _discover_servers() -> list[Path]:
    root = _servers_root()
    return sorted([p for p in root.iterdir() if p.is_dir()], key=lambda p: p.name.lower())


def _icon(
    name: str,
    color: str | None = None,  # 設為預設 None，避免沒傳顏色時報錯
    *,
    size: int = ACTION_ICON_SIZE,
    tooltip: str | None = None,
) -> ft.Icon:
    material = LUCIDE_TO_MATERIAL.get(name, name.upper())
    icon_attr = getattr(ft.Icons, material, None)
    
    kwargs = {"size": size}
    if color:
        kwargs["color"] = color
    if tooltip:
        kwargs["tooltip"] = tooltip
        
    return ft.Icon(icon_attr, **kwargs)



def _server_card(server_name: str, palette: dict[str, str], status: str = "#9e9e9eff") -> ft.Stack:
    text_color = palette["fg"]

    title = ft.Text(
        server_name,
        size=TITLE_FONT_SIZE,
        color=text_color,
        text_align=ft.TextAlign.CENTER,
        max_lines=1,
        overflow=ft.TextOverflow.ELLIPSIS,
    )

    actions = ft.Row(
        [_icon(name, tooltip=name) for name in CARD_ACTION_ICONS],
        spacing=12,
        alignment=ft.MainAxisAlignment.END,
    )

    card_body = ft.Stack(
        [
            # 中央標題
            ft.Container(
                content=title,
                alignment=ft.alignment.Alignment(0, -0.1),
                expand=True,
            ),
            # 右下角操作按鈕
            ft.Container(
                content=actions,
                right=12,
                bottom=10,
            ),
        ],
        width=CARD_WIDTH,
        height=CARD_HEIGHT,
    )

    card_container = ft.Container(
        content=card_body,
        width=CARD_WIDTH,
        height=CARD_HEIGHT,
        border_radius=CARD_CORNER,
        border=ft.Border.all(1, "$color.border"),
        bgcolor="$color.surface",
    )

    # 左上角狀態燈號 (縮小並稍微調整左上邊距)
    dot = ft.Container(
        width=DOT_SIZE,
        height=DOT_SIZE,
        border_radius=DOT_SIZE // 2,
        bgcolor=_parse_color(status),
    )

    return ft.Stack(
        controls=[
            card_container,
            ft.Container(content=dot, left=10, top=10),
        ],
        width=CARD_WIDTH,
        height=CARD_HEIGHT,
    )


def _add_card(on_click, palette: dict[str, str]) -> ft.Container:
    """新增伺服器按鈕卡片 (230x127)"""
    text_color = palette["fg"]  # 動態取得主題前景色 (Dark為淺灰/白，Light為深灰/黑)

    body = ft.Column(
        [
            _icon("plus", color=text_color, size=24),
            ft.Text("add a new server", size=14, color=text_color),
        ],
        spacing=8,
        horizontal_alignment=ft.CrossAxisAlignment.CENTER,
        alignment=ft.MainAxisAlignment.CENTER,
    )
    return ft.Container(
        content=body,
        width=CARD_WIDTH,
        height=CARD_HEIGHT,
        border_radius=CARD_CORNER,
        border=ft.Border.all(1, palette["border"]),
        bgcolor=palette["surface"],
        alignment=ft.alignment.Alignment(0, 0),
        on_click=on_click,
        ink=True,
    )

def _status_legend(palette: dict[str, str] | None = None) -> ft.Row:
    """右下角狀態圖例說明"""
    if palette is None:
        from ui.theme import initial_dark, get_palette
        palette = get_palette(dark=initial_dark())

    text_color = palette["muted"]  # 使用次要文字顏色，確保深淺色主題都清晰

    items = []
    for label, color in zip(LEGEND_LABELS, LEGEND_COLORS):
        items.append(
            ft.Row(
                [
                    ft.Container(
                        width=DOT_SIZE,
                        height=DOT_SIZE,
                        border_radius=DOT_SIZE // 2,
                        bgcolor=_parse_color(color),
                    ),
                    ft.Text(label, size=13, color=text_color),
                ],
                spacing=6,
                tight=True,
                vertical_alignment=ft.CrossAxisAlignment.CENTER,
            )
        )

    return ft.Row(
        items,
        spacing=18,
        alignment=ft.MainAxisAlignment.END,
    )

def build_dashboard_view(page: ft.Page, palette: dict[str, str] | None = None) -> ft.Container:
    """Home page: server cards + add button, with an installer sub-page
    that opens when "+ add a new server" is clicked.

    Mode state lives in a list-of-one so closures can mutate it without
    ``nonlocal`` shenanigans. The outer Container is built once; only
    ``root.content`` swaps between the two views, so the sidebar stays
    mounted and the user keeps context (selected sidebar icon, palette).
    """
    palette = palette or get_palette(dark=False)

    # Mode: "list" (default) shows server cards; "installer" shows the
    # sub-page. Mutated via list-of-one to escape closure scoping.
    state: list[str] = ["list"]

    def go_to_installer(_e: ft.ControlEvent) -> None:
        state[0] = "installer"
        root.content = _installer_content()
        page.update()

    def go_back_to_list(_e: ft.ControlEvent) -> None:
        state[0] = "list"
        root.content = _list_content()
        page.update()

    # ── list content (server cards + add card + legend) ─────────
    def _list_content() -> ft.Container:
        servers = _discover_servers()
        STATUS_ROTATION = ("#04ff00ff", "#9e9e9eff", "#ff6600ff", "#ff0000ff")

        def status_for(idx: int) -> str:
            return STATUS_ROTATION[idx % len(STATUS_ROTATION)]

        positioned_controls = []
        for idx, server in enumerate(servers):
            col = idx % 4
            row = idx // 4
            x = ROW1_X[col]
            y = ROW1_Y + row * ROW_GAP
            card_stack = _server_card(
                server.name, palette=palette, status=status_for(idx),
            )
            positioned_controls.append(
                ft.Container(
                    content=card_stack,
                    left=x, top=y,
                    width=CARD_WIDTH, height=CARD_HEIGHT,
                )
            )

        add_card = _add_card(go_to_installer, palette=palette)
        add_row = len(servers) // 4
        add_col = len(servers) % 4
        add_x = ROW1_X[add_col]
        add_y = ROW1_Y + add_row * ROW_GAP
        positioned_controls.append(
            ft.Container(
                content=add_card,
                left=add_x, top=add_y,
                width=CARD_WIDTH, height=CARD_HEIGHT,
            )
        )

        cards_layer = ft.Stack(
            controls=positioned_controls,
            width=1100,
            height=add_y + CARD_HEIGHT + 20,
        )

        return ft.Container(
            content=ft.Column(
                [
                    make_title("Forge Server Manager", palette, size=22),
                    make_status_text(palette, text="請選擇伺服器資料夾並安裝 Forge。"),
                    ft.Divider(color=palette["border"]),
                    cards_layer,
                    ft.Container(expand=True),
                    _status_legend(),
                ],
                alignment=ft.MainAxisAlignment.START,
                horizontal_alignment=ft.CrossAxisAlignment.START,
                spacing=12,
                expand=True,
            ),
            padding=24,
            alignment=ft.alignment.Alignment(-1, -1),
            bgcolor=palette["bg"],
            expand=True,
        )

    # ── installer content (sub-page) ───────────────────────────
    def _installer_content() -> ft.Container:
        installer_view = build_installer_view(page, palette)
        # Inject a back button in the title row so users can return.
        back_btn = ft.IconButton(
            icon=ft.Icons.ARROW_BACK,
            icon_color=palette["muted"],
            tooltip="返回伺服器列表",
            on_click=go_back_to_list,
        )
        # Wrap installer content with a back-button row at the top.
        return ft.Container(
            content=ft.Column(
                [
                    ft.Row(
                        [back_btn, ft.Text("")],
                        alignment=ft.MainAxisAlignment.START,
                    ),
                    installer_view,
                ],
                spacing=0,
                expand=True,
            ),
            padding=24,
            alignment=ft.alignment.Alignment(-1, -1),
            bgcolor=palette["bg"],
            expand=True,
        )

    root = ft.Container(
        content=_list_content(),
        bgcolor=palette["bg"],
        expand=True,
    )
    return root
