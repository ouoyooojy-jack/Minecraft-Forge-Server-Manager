import flet as ft
import downloader
from services import forge_api


def main(page: ft.Page):
    page.title = "Mc server manager"
    page.window_width = 500
    page.window_height = 400
    page.vertical_alignment = ft.MainAxisAlignment.CENTER
    page.horizontal_alignment = ft.CrossAxisAlignment.CENTER

    raw_versions = forge_api.get_versions()
    version_map = forge_api.group_by_mc_major(raw_versions)

    # 大版本依數字由新到舊排序（26.2 > 1.21 > 1.20 > ...）
    def major_sort_key(major: str):
        return tuple(int(x) for x in major.split("."))

    sorted_majors = sorted(
        version_map.keys(),
        key=major_sort_key,
        reverse=True,
    )
    
    major_dropdown = ft.Dropdown(
        label="Select a major version",
        width=300,
        menu_height=200,
        options=[
            ft.dropdown.Option(text=major, key=major)
            for major in sorted_majors
        ],
    )
    
    
    sub_dropdown = ft.Dropdown(
        label="select a sub version",
        width=300,
        menu_height=200,
        disabled=True,
        options=[],
    )

    def on_major_select(e):
        # Flet 0.85+ 用 on_select，不是 on_change
        major = major_dropdown.value  # 例如 "1.20"

        sub_dropdown.value = None

        if major and major in version_map:
            # 整份重設 options，比 clear/append 更穩
            sub_dropdown.options = [
                ft.dropdown.Option(text=full_ver, key=full_ver)
                for full_ver in version_map[major]
            ]
            sub_dropdown.disabled = False
        else:
            sub_dropdown.options = []
            sub_dropdown.disabled = True

        page.update()

    major_dropdown.on_select = on_major_select

    page.add(major_dropdown, sub_dropdown)
    
ft.run(main)
