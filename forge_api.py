import requests
import xml.etree.ElementTree as ET

def get_versions():
    url = "https://maven.minecraftforge.net/net/minecraftforge/forge/maven-metadata.xml"
    try:
        response = requests.get(url)
        root = ET.fromstring(response.text)
        versions = root.findall(".//version")
        version_list=[]
        for version in versions:
            version_list.append(version.text)
        
        version_list.reverse()
        return version_list

    except Exception as e:
        print(f"connection error: {e}")
        return []

def group_by_mc_major(versions: list[str]) -> dict[str, list[str]]:
    """把完整 Forge 版本列表，依 MC 大版本分類。"""
    version_map: dict[str, list[str]] = {}

    for ver in versions:
        # ① MC 段（- 前面）
        mc = ver.split("-", 1)[0]

        # ② 切成 ["1", "20", "1"]
        parts = mc.split(".")

        # ③ 格式不對就跳過
        if len(parts) < 2:
            continue

        # ④ 大版本 "1.20"
        major = f"{parts[0]}.{parts[1]}"

        # ⑤ 沒有這個 key 就先建空 list，再 append
        version_map.setdefault(major, []).append(ver)

    return version_map