"""TOML-backed application configuration.

The settings file lives beside the executable when packaged, and in the
project root while developing.  Existing modules can keep importing the
constants below; their values now come from ``settings.toml``.
"""
from __future__ import annotations

from pathlib import Path
import sys
import tomllib


def _project_root() -> Path:
    """Project root: next to the executable when packaged, else next to this file."""
    return Path(sys.executable).parent if getattr(sys, "frozen", False) else Path(__file__).parent


def _settings_path() -> Path:
    return _project_root() / "settings.toml"


SETTINGS_PATH = _settings_path()


# Download directory is fixed at the project's `downloads/` folder; users don't
# configure it. This sidesteps the whole "path is relative/absolute/exists?"
# problem and keeps installer jars alongside the app.
DEFAULT_DOWNLOAD_DIR = _project_root() / "downloads"


def _read() -> dict:
    with SETTINGS_PATH.open("rb") as file:
        return tomllib.load(file)


def _load() -> dict:
    try:
        return _read()
    except (OSError, tomllib.TOMLDecodeError) as exc:
        raise RuntimeError(f"Unable to load settings from {SETTINGS_PATH}: {exc}") from exc


_RAW = _load()
_FORGE = _RAW["forge"]
_NETWORK = _RAW["network"]
_UI = _RAW["ui"]

FORGE_METADATA_URL = _FORGE["metadata_url"]
FORGE_INSTALLER_URL_TEMPLATE = _FORGE["installer_url_template"]
HTTP_TIMEOUT_SEC = int(_NETWORK["http_timeout_sec"])
DOWNLOAD_CHUNK_SIZE = int(_NETWORK["download_chunk_size"])
PROGRESS_UPDATE_HZ = int(_NETWORK["progress_update_hz"])
DEFAULT_THEME = _UI["default_theme"]
BUTTON_RADIUS = int(_UI["button_radius"])
LIGHT_PALETTE: dict[str, str] = dict(_RAW["palette"]["light"])
DARK_PALETTE: dict[str, str] = dict(_RAW["palette"]["dark"])


def settings_values() -> dict[str, int | str]:
    """Values that users may edit from the Settings page."""
    return {
        "http_timeout_sec": HTTP_TIMEOUT_SEC,
        "progress_update_hz": PROGRESS_UPDATE_HZ,
        "default_theme": DEFAULT_THEME,
    }


def save_settings(
    *, http_timeout_sec: int, progress_update_hz: int,
    default_theme: str,
) -> None:
    """Persist editable settings and update this process's configuration."""
    if http_timeout_sec < 1 or progress_update_hz < 1:
        raise ValueError("Timeout and progress frequency must be positive")
    if default_theme not in {"dark", "light"}:
        raise ValueError("Theme must be 'dark' or 'light'")

    global HTTP_TIMEOUT_SEC, PROGRESS_UPDATE_HZ, DEFAULT_THEME
    HTTP_TIMEOUT_SEC = http_timeout_sec
    PROGRESS_UPDATE_HZ = progress_update_hz
    DEFAULT_THEME = default_theme

    content = SETTINGS_PATH.read_text(encoding="utf-8")
    replacements = {
        'http_timeout_sec = ': f'http_timeout_sec = {http_timeout_sec}',
        'progress_update_hz = ': f'progress_update_hz = {progress_update_hz}',
        'default_theme = ': f'default_theme = "{default_theme}"',
    }
    lines = []
    for line in content.splitlines():
        key = line.strip().split("=", 1)[0].strip() if "=" in line else ""
        prefix = f"{key} = "
        lines.append(replacements[prefix] if prefix in replacements else line)
    SETTINGS_PATH.write_text("\n".join(lines) + "\n", encoding="utf-8")
