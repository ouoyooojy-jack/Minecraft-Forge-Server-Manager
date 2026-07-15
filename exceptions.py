"""Project exception hierarchy. See docs/exceptions.md for rationale."""


class MinecraftServerError(Exception):
    """Base class for all Mc Server Manager errors."""


class NetworkError(MinecraftServerError):
    """Network layer failure: DNS, timeout, 5xx."""


class VersionNotFoundError(MinecraftServerError):
    """Requested Forge version returned 404."""


class DownloadAbortedError(MinecraftServerError):
    """User cancelled the download."""
