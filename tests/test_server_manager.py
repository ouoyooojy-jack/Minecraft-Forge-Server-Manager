"""File-level tests for local server manager operations."""
from __future__ import annotations

from services.server_manager import eula_accepted, read_properties, set_eula, write_properties


def test_eula_can_be_recorded_and_revoked(tmp_path):
    assert eula_accepted(tmp_path) is False
    set_eula(tmp_path, True)
    assert eula_accepted(tmp_path) is True
    set_eula(tmp_path, False)
    assert eula_accepted(tmp_path) is False


def test_server_properties_preserve_existing_values(tmp_path):
    (tmp_path / "server.properties").write_text("motd=Old\ndifficulty=easy\n", encoding="utf-8")
    write_properties(tmp_path, {"motd": "New", "max-players": "12"})
    assert read_properties(tmp_path) == {
        "motd": "New", "difficulty": "easy", "max-players": "12",
    }


def test_forge_install_uses_server_install_mode():
    import inspect
    from services.server_manager import ServerProcess

    assert '"--installServer"' in inspect.getsource(ServerProcess.install)
    assert "set_eula(server_dir, True)" in inspect.getsource(ServerProcess.install)
