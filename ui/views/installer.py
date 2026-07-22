from __future__ import annotations

import asyncio
import threading
from pathlib import Path

import flet as ft

from config import DEFAULT_DOWNLOAD_DIR, FORGE_INSTALLER_URL_TEMPLATE
from exceptions import (
    DownloadAbortedError,
    MinecraftServerError,
    NetworkError,
    VersionNotFoundError,
)
from services.download_service import DownloadService, ProgressInfo
from ui.components import make_status_text
from ui.theme import get_palette
from ui.throttle import ProgressThrottle

# Layout Constants - 嚴格對應 sub page.pen 設計稿尺寸
CARD_WIDTH = 756
CARD_HEIGHT = 472
DROPDOWN_WIDTH = 720
DROPDOWN_HEIGHT = 70
INSTALL_BUTTON_WIDTH = 320
INSTALL_BUTTON_HEIGHT = 50
CHECKBOX_SIZE = 20

def build_installer_view(page: ft.Page, palette: dict[str, str] | None = None) -> ft.Container:
    palette = palette or get_palette(dark=True)
    download_service = DownloadService()
    cancel_event = threading.Event()
    throttle = ProgressThrottle(hz=30)
    
    status_text = make_status_text(palette, text="")
    DEFAULT_DOWNLOAD_DIR.mkdir(parents=True, exist_ok=True)
    jars = sorted(DEFAULT_DOWNLOAD_DIR.glob("forge-*-installer.jar"), key=lambda p: p.name.lower())

    # 1. 下拉選單：套用設計稿的 360x70 尺寸與樣式
    installer_dd = ft.Dropdown(
        options=[ft.dropdown.Option(text=jar.name) for jar in jars],
        width=360,
        height=70,
        border_color=palette["border"],
        bgcolor=palette["surface"],
        color=palette["fg"],
        text_style=ft.TextStyle(size=16, color=palette["fg"], font_family="Inter"),  # 透過 text_style 強制覆寫大小
        hint_text="choose a installer",
        hint_style=ft.TextStyle(color=palette["fg"], size=16, font_family="Inter"),
        border_radius=6,
        content_padding=ft.Padding(left=12, right=12, top=0, bottom=0),
    )


    # 2. 客製化 EULA 核取方塊：還原 20x20 方塊，取代走鐘的原生 Checkbox
    eula_state = {"checked": False}
    eula_icon = ft.Icon(ft.Icons.CHECK, size=16, color=palette["bg"], visible=False)
    eula_box = ft.Container(
        content=eula_icon,
        width=CHECKBOX_SIZE,
        height=CHECKBOX_SIZE,
        border_radius=3,
        border=ft.Border.all(1, palette["border"]),
        bgcolor=palette["bg"],
        alignment=ft.alignment.Alignment(0, 0),
        ink=True
    )

    def toggle_eula(e):
        eula_state["checked"] = not eula_state["checked"]
        eula_icon.visible = eula_state["checked"]
        eula_box.bgcolor = palette["fg"] if eula_state["checked"] else palette["bg"]
        update_btn_state(None)
        page.update()

    eula_box_clickable = ft.GestureDetector(on_tap=toggle_eula, content=eula_box)
    eula_text_clickable = ft.GestureDetector(
        on_tap=toggle_eula, 
        content=ft.Text("I agree Minecraft EULA", size=14, color=palette["fg"], font_family="Inter")
    )

    # 3. 安裝按鈕：套用設計稿的 320x60 尺寸
    # 3. 安裝按鈕：透過 overlay_color 強制指定點擊時的水波紋/遮罩顏色
    install_btn = ft.ElevatedButton(
        content=ft.Text("install server", size=20, color=palette["accent_fg"], font_family="Inter"),
        width=INSTALL_BUTTON_WIDTH,
        height=INSTALL_BUTTON_HEIGHT,
        disabled=True,
        bgcolor=palette["accent"],
        style=ft.ButtonStyle(
            shape=ft.RoundedRectangleBorder(radius=8),
            side=ft.BorderSide(1, palette["border"]),
            overlay_color="#33000000",  # 加上這行，點擊按鈕時就會浮現清晰的水波紋遮罩
        ),
    )


    def update_btn_state(_e):
        install_btn.disabled = not (eula_state["checked"] and installer_dd.value)
        page.update()

    installer_dd.on_change = update_btn_state

    # 4. 玻璃透視卡片：使用 Stack 與絕對座標 100% 綁定 sub page.pen 的排版
    card_content = ft.Stack(
        [
            # 下拉選單 (x: 198, y: 127) 
            ft.Container(
                content=installer_dd,
                left=198,
                top=150,
                width=DROPDOWN_WIDTH,
                height=DROPDOWN_HEIGHT,
            ),
            # 安裝按鈕 (x: 218, y: 219) 
            ft.Container(
                content=install_btn,
                left=218,
                top=219,
                width=INSTALL_BUTTON_WIDTH,
                height=INSTALL_BUTTON_HEIGHT,
            ),
            # EULA 核取方塊 (x: 260, y: 300) 
            ft.Container(
                content=eula_box_clickable,
                left=260,
                top=290,
            ),
            # EULA 文字標籤 (x: 308, y: 300)
            ft.Container(
                content=eula_text_clickable,
                left=308,
                top=290,
                height=20,
                alignment=ft.alignment.Alignment(-1, 0),
            ),
        ],
        width=CARD_WIDTH,
        height=CARD_HEIGHT,
    )

    # 封裝卡片外觀：使用 15% 透明度白色 (#26ffffff)
    card = ft.Container(
        content=card_content,
        width=CARD_WIDTH,
        height=CARD_HEIGHT,
        bgcolor="#26ffffff", 
        border_radius=12,
        alignment=ft.alignment.Alignment(0, 0),
    )

    # 安裝執行邏輯 
    def on_install_click(_e):
        if not installer_dd.value or not eula_state["checked"]:
            return
            
        try:
            stem = installer_dd.value.removesuffix(".jar")
            version = stem.split("forge-", 1)[1].removesuffix("-installer")
        except (IndexError, ValueError):
            return

        url = FORGE_INSTALLER_URL_TEMPLATE.format(version=version)
        save_path = DEFAULT_DOWNLOAD_DIR / installer_dd.value

        cancel_event.clear()
        throttle._last_emit = 0.0
        install_btn.disabled = True
        status_text.value = f"Installing {version}..."
        page.update()

        def on_progress(info: ProgressInfo):
            if throttle.should_emit():
                page.run_task(lambda: setattr(status_text, 'value', f"{info.percent:.1f}% · {info.speed_mbps:.2f} MB/s") or page.update())

        async def install_flow():
            try:
                saved = await asyncio.to_thread(
                    download_service.download, url, save_path,
                    on_progress=on_progress, cancel_event=cancel_event
                )
                status_text.value = f"Installed → {saved.name}"
            except Exception as exc:
                status_text.value = f"Error: {exc}"
            finally:
                update_btn_state(None)

        page.run_task(install_flow)

    install_btn.on_click = on_install_click

    # 5. 主視圖組裝
    return ft.Container(
        content=ft.Stack(
            [
                # 頂部標題與分隔線
                ft.Column([
                    ft.Text("Forge Server Installer", size=75, weight=ft.FontWeight.W_700, font_family="Inter", color=palette["fg"]),
                    ft.Divider(color=palette["border"], height=1),
                ], spacing=16),
                
                # 置中安裝卡片
                ft.Container(content=card, alignment=ft.alignment.Alignment(0, 0)),
                
                # 底部狀態列
                ft.Container(content=status_text, bottom=0, left=0),
            ],
            expand=True,
        ),
        bgcolor=palette["bg"],
        padding=24,
        expand=True,
    )
