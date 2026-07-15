"""Forge version API. See docs/forge_api.md for design rationale."""
from __future__ import annotations

import logging
from xml.etree import ElementTree as ET
from typing import Iterable

import requests

from config import (
    FORGE_INSTALLER_URL_TEMPLATE,
    FORGE_METADATA_URL,
    HTTP_TIMEOUT_SEC,
)
from exceptions import NetworkError

logger = logging.getLogger(__name__)

VersionList = list[str]
VersionMap = dict[str, VersionList]


def build_session() -> requests.Session:
    s = requests.Session()
    s.headers.update({
        "User-Agent": "McServerManager/1.0 (+https://example.local)",
        "Accept": "application/xml, text/xml;q=0.9, */*;q=0.5",
    })
    return s


_session: requests.Session | None = None


def get_session() -> requests.Session:
    global _session
    if _session is None:
        _session = build_session()
    return _session


def reset_session() -> None:
    """For tests only."""
    global _session
    if _session is not None:
        _session.close()
    _session = None


def get_versions(session: requests.Session | None = None) -> VersionList:
    """Return Forge versions (oldest first). Raises NetworkError on failure."""
    s = session if session is not None else get_session()

    try:
        response = s.get(FORGE_METADATA_URL, timeout=HTTP_TIMEOUT_SEC)
        response.raise_for_status()
    except requests.RequestException as exc:
        raise NetworkError(
            f"Could not fetch Forge version list from {FORGE_METADATA_URL}"
        ) from exc

    try:
        root = ET.fromstring(response.text)
    except ET.ParseError as exc:
        raise NetworkError(f"Invalid XML from Forge: {exc}") from exc

    versions = [v.text for v in root.findall(".//version") if v.text]
    versions.reverse()
    logger.info("Found %d Forge versions", len(versions))
    return versions


def installer_url(version: str) -> str:
    """Build the installer download URL for a Forge version."""
    return FORGE_INSTALLER_URL_TEMPLATE.format(version=version)


def group_by_mc_major(versions: Iterable[str]) -> VersionMap:
    """Group versions by MC major.minor (e.g. "1.20.1" -> "1.20")."""
    groups: VersionMap = {}
    for ver in versions:
        if "-" not in ver:
            logger.warning("Skipping malformed version (no dash): %r", ver)
            continue
        mc = ver.split("-", 1)[0]
        parts = mc.split(".")
        if len(parts) < 2:
            logger.warning("Skipping version without minor: %r", ver)
            continue
        major = f"{parts[0]}.{parts[1]}"
        groups.setdefault(major, []).append(ver)
    return groups


def is_version_available(
    version: str,
    session: requests.Session | None = None,
) -> bool:
    """Check if a specific Forge version exists on Maven (HEAD request)."""
    s = session if session is not None else get_session()
    url = installer_url(version)
    try:
        resp = s.head(url, timeout=HTTP_TIMEOUT_SEC, allow_redirects=True)
        return resp.status_code == 200
    except requests.RequestException:
        return False


def install_exists_with_reason(
    version: str,
    session: requests.Session | None = None,
) -> tuple[bool, str]:
    """Like is_version_available but returns (bool, reason) for the UI."""
    s = session if session is not None else get_session()
    url = installer_url(version)
    try:
        resp = s.head(url, timeout=HTTP_TIMEOUT_SEC, allow_redirects=True)
    except requests.RequestException as exc:
        return False, f"NetworkError: {exc}"
    if resp.status_code == 200:
        return True, ""
    if resp.status_code == 404:
        return False, "VersionNotFound"
    return False, f"HTTP {resp.status_code}"
