"""Configuration is sourced from and persisted to TOML."""
from __future__ import annotations

import config


def test_settings_file_is_the_source_of_runtime_configuration():
    assert config.SETTINGS_PATH.name == "settings.toml"
    assert config.SETTINGS_PATH.exists()
    assert config.FORGE_METADATA_URL.startswith("https://")
    assert config.DEFAULT_THEME in {"dark", "light"}


def test_settings_values_expose_editable_options():
    values = config.settings_values()
    assert {"http_timeout_sec", "progress_update_hz", "default_theme"} <= values.keys()
