"""Centralized constants. See docs/configuration.md for rationale."""
from pathlib import Path

FORGE_METADATA_URL = (
    "https://maven.minecraftforge.net/net/minecraftforge/forge/"
    "maven-metadata.xml"
)
FORGE_INSTALLER_URL_TEMPLATE = (
    "https://maven.minecraftforge.net/net/minecraftforge/forge/"
    "{version}/forge-{version}-installer.jar"
)

DEFAULT_DOWNLOAD_DIR = Path("downloads")

HTTP_TIMEOUT_SEC = 60
DOWNLOAD_CHUNK_SIZE = 64 * 1024
PROGRESS_UPDATE_HZ = 30
