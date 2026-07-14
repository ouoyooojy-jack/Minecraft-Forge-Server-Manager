import requests
from rich.console import Console
from pathlib import Path

console = Console()

# Forge Maven 上 installer 的固定路徑前綴
FORGE_MAVEN_BASE = (
    "https://maven.minecraftforge.net/net/minecraftforge/forge"
)

# 預設下載資料夾（相對「執行程式時的工作目錄」）
DOWNLOAD_DIR = Path("downloads")


def installer_url(version: str) -> str:
    """
    用完整 Forge 版本字串組出 installer 下載網址。

    例:
        version = "1.20.1-47.4.21"
        → https://maven.minecraftforge.net/net/minecraftforge/forge/
          1.20.1-47.4.21/forge-1.20.1-47.4.21-installer.jar
    """
    return (
        f"{FORGE_MAVEN_BASE}/{version}/forge-{version}-installer.jar"
    )


def installer_path(version: str, download_dir: Path = DOWNLOAD_DIR) -> Path:
    """依版本產生本地儲存路徑，避免不同版本互相覆蓋。"""
    return download_dir / f"forge-{version}-installer.jar"


def download(url: str, save_path: Path) -> Path | None:
    """
    從 url 下載檔案到 save_path。

    成功 → 回傳 Path
    失敗 → 印錯誤並回傳 None
    """
    # ① 先確保資料夾存在，再寫檔（順序很重要）
    save_path.parent.mkdir(parents=True, exist_ok=True)

    console.print(f"[bold cyan]Starting download[/bold cyan]\n  {url}")

    try:
        # timeout：避免網路卡住時程式永遠等待
        response = requests.get(url, timeout=60)
    except requests.RequestException as e:
        # 連線逾時、DNS 失敗、斷線等都走這裡
        console.print(f"[bold red]Request failed:[/bold red] {e}")
        return None

    status = response.status_code
    console.print(f"HTTP status: {status}")

    # ② 只有 200 才寫檔；404/403 的內容不是 jar
    if status != 200:
        if status == 404:
            console.print("[red]Not found (404). Check version string / URL.[/red]")
        elif status == 403:
            console.print("[red]Forbidden (403). Server refused the request.[/red]")
        else:
            console.print(f"[red]Download failed with status {status}.[/red]")
        return None

    # ③ 寫入磁碟
    save_path.write_bytes(response.content)
    size_mb = save_path.stat().st_size / (1024 * 1024)
    console.print(
        f"[bold green]Saved[/bold green] {save_path}  ({size_mb:.2f} MB)"
    )
    return save_path


def download_forge_installer(
    version: str,
    download_dir: Path = DOWNLOAD_DIR,
) -> Path | None:
    """
    便利函式：給完整版本字串，自動組 URL + 路徑並下載。

    例:
        download_forge_installer("1.20.1-47.4.21")
    """
    url = installer_url(version)
    path = installer_path(version, download_dir)
    return download(url, path)


# 直接執行本檔時可做快速測試：
#   python downloader.py
if __name__ == "__main__":
    test_version = "1.20.1-47.4.21"
    console.print(f"Test URL: {installer_url(test_version)}")
    result = download_forge_installer(test_version)
    if result:
        console.print("[bold green]Test download OK[/bold green]")
    else:
        console.print("[bold red]Test download failed[/bold red]")
