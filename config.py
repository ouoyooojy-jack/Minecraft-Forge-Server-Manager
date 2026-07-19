"""Centralized constants. See docs/configuration.md for rationale."""
from pathlib import Path

# ─── Forge Maven ─────────────────────────────────────────────
FORGE_METADATA_URL = (
    "https://maven.minecraftforge.net/net/minecraftforge/forge/"
    "maven-metadata.xml"
)
FORGE_INSTALLER_URL_TEMPLATE = (
    "https://maven.minecraftforge.net/net/minecraftforge/forge/"
    "{version}/forge-{version}-installer.jar"
)

DEFAULT_DOWNLOAD_DIR = Path("downloads")

# ─── HTTP behavior ──────────────────────────────────────────
HTTP_TIMEOUT_SEC = 60
DOWNLOAD_CHUNK_SIZE = 64 * 1024
PROGRESS_UPDATE_HZ = 30

# ─── Theme palettes ──────────────────────────────────────────
# Centralised here so URLs, timeouts, and colors share one home.
# Keys consumed by ui/theme.py: bg, fg, muted, border, accent, accent_fg,
# error, error_fg, success, success_fg, neutral, neutral_fg,
# rail_bg, surface, surface_variant.
LIGHT_PALETTE: dict[str, str] = {
    "bg":               "#FFFFFF",
    "fg":               "#0A0A0A",
    "muted":            "#6B6B6B",
    "border":           "#E5E5E5",
    "accent":           "#2A2A2A",      # softer than pure black
    "accent_fg":        "#FFFFFF",
    "error":            "#B3261E",
    "error_fg":         "#FFFFFF",
    "success":          "#1B5E20",
    "success_fg":       "#FFFFFF",
    "neutral":          "#3A3A3A",
    "neutral_fg":       "#FFFFFF",
    "rail_bg":          "#F5F5F5",      # sidebar slightly off-white
    "surface":          "#FAFAFA",      # card background
    "surface_variant":  "#EFEFEF",
}

DARK_PALETTE: dict[str, str] = {
    "bg":               "#121212",      # soft black (Material dark)
    "fg":               "#F2F2F2",
    "muted":            "#A0A0A0",
    "border":           "#2C2C2C",
    "accent":           "#E5E5E5",      # softer than pure white
    "accent_fg":        "#121212",
    "error":            "#CF6679",
    "error_fg":         "#121212",
    "success":          "#03DAC6",
    "success_fg":       "#121212",
    "neutral":          "#9E9E9E",
    "neutral_fg":       "#121212",
    "rail_bg":          "#1A1A1A",      # sidebar slightly off-black
    "surface":          "#1E1E1E",      # card background
    "surface_variant":  "#262626",
}

DEFAULT_THEME = "dark"
BUTTON_RADIUS = 8          # px — moderate rounding (not square, not pill)

# ─── Sidebar dimensions ───────────────────────────────────────
# Configured here so the sidebar's visual footprint is one constant
# set to change, not scattered across the renderer.
SIDEBAR_WIDTH_EXPANDED = 232    # px — fits full label text comfortably
SIDEBAR_WIDTH_COLLAPSED = 72     # px — icon-only, flush with mobile pattern
SIDEBAR_RADIUS = 14              # outer panel radius
SIDEBAR_SEARCH_HEIGHT = 36      # search-bar touch height
