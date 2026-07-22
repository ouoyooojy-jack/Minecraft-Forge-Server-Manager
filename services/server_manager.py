"""Local Forge server installation, configuration, and process management."""
from __future__ import annotations

from pathlib import Path
import queue
import subprocess
import threading


class ServerManagerError(RuntimeError):
    """Raised when a server operation cannot be completed safely."""


def read_properties(server_dir: Path) -> dict[str, str]:
    path = server_dir / "server.properties"
    if not path.exists():
        return {}
    values: dict[str, str] = {}
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        if "=" in line and not line.lstrip().startswith("#"):
            key, value = line.split("=", 1)
            values[key] = value
    return values


def write_properties(server_dir: Path, updates: dict[str, str]) -> None:
    server_dir.mkdir(parents=True, exist_ok=True)
    path = server_dir / "server.properties"
    existing = path.read_text(encoding="utf-8", errors="replace").splitlines() if path.exists() else []
    written: set[str] = set()
    lines: list[str] = []
    for line in existing:
        if "=" in line and not line.lstrip().startswith("#"):
            key, _ = line.split("=", 1)
            if key in updates:
                lines.append(f"{key}={updates[key]}")
                written.add(key)
                continue
        lines.append(line)
    lines.extend(f"{key}={value}" for key, value in updates.items() if key not in written)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def eula_accepted(server_dir: Path) -> bool:
    path = server_dir / "eula.txt"
    return path.exists() and "eula=true" in path.read_text(encoding="utf-8", errors="replace").lower()


def set_eula(server_dir: Path, accepted: bool) -> None:
    server_dir.mkdir(parents=True, exist_ok=True)
    (server_dir / "eula.txt").write_text(
        "# By changing the setting below to TRUE you are indicating your agreement to the EULA.\n"
        f"eula={'true' if accepted else 'false'}\n",
        encoding="utf-8",
    )


class ServerProcess:
    """Owns a running server process and exposes its console output safely."""

    def __init__(self) -> None:
        self.process: subprocess.Popen[str] | None = None
        self.output: queue.Queue[str] = queue.Queue()

    @property
    def running(self) -> bool:
        return self.process is not None and self.process.poll() is None

    def install(self, installer: Path, server_dir: Path, java_command: str = "java") -> int:
        if not installer.is_file() or installer.suffix.lower() != ".jar":
            raise ServerManagerError("請選擇有效的 Forge installer .jar 檔案。")
        server_dir.mkdir(parents=True, exist_ok=True)
        result = subprocess.run(
            [java_command, "-jar", str(installer), "--installServer"],
            cwd=server_dir, text=True, capture_output=True, check=False,
        )
        self._emit(result.stdout)
        self._emit(result.stderr)
        if result.returncode:
            raise ServerManagerError(f"Forge 安裝失敗（結束碼 {result.returncode}）。請查看終端日誌。")
        # A newly installed local server is immediately made runnable. The UI
        # still displays the accepted EULA state for clear user visibility.
        set_eula(server_dir, True)
        return result.returncode

    def start(self, server_dir: Path, java_command: str = "java", memory_mb: int = 2048) -> None:
        if self.running:
            raise ServerManagerError("伺服器已在執行中。")
        if not eula_accepted(server_dir):
            raise ServerManagerError("請先同意 Minecraft EULA。")
        run_bat = server_dir / "run.bat"
        server_jar = server_dir / "server.jar"
        if run_bat.exists():
            command = ["cmd", "/c", str(run_bat), "nogui"]
        elif server_jar.exists():
            command = [java_command, f"-Xms{memory_mb}M", f"-Xmx{memory_mb}M", "-jar", str(server_jar), "nogui"]
        else:
            raise ServerManagerError("找不到 run.bat 或 server.jar；請先安裝 Forge。")
        self.process = subprocess.Popen(
            command, cwd=server_dir, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT, text=True, encoding="utf-8", errors="replace",
        )
        threading.Thread(target=self._read_output, daemon=True).start()

    def send(self, command: str) -> None:
        if not self.running or self.process is None or self.process.stdin is None:
            raise ServerManagerError("伺服器尚未啟動。")
        self.process.stdin.write(command.rstrip() + "\n")
        self.process.stdin.flush()

    def stop(self) -> None:
        if self.running:
            self.send("stop")

    def drain_output(self) -> list[str]:
        lines: list[str] = []
        while True:
            try:
                lines.append(self.output.get_nowait())
            except queue.Empty:
                return lines

    def _read_output(self) -> None:
        assert self.process is not None and self.process.stdout is not None
        for line in self.process.stdout:
            self._emit(line)

    def _emit(self, text: str) -> None:
        if text:
            self.output.put(text)
