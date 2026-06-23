"""Python SDK for rototo runtime configuration workspaces."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Mapping

from ._rototo import RototoError, _RefreshingWorkspace, _Workspace, version as _version


JsonObject = Mapping[str, Any]
__version__ = _version()


@dataclass(frozen=True)
class VariableResolution:
    """Selected value for a rototo variable."""

    id: str
    value: Any
    source: Any


@dataclass(frozen=True)
class RefreshStatus:
    """Current refresh state for a long-running workspace handle."""

    current_fingerprint: Any | None
    last_success: float | None
    last_attempt: float | None
    consecutive_failures: int
    last_error: str | None
    refreshing: bool
    immutable: bool


class Workspace:
    """Loaded rototo workspace."""

    def __init__(self, inner: _Workspace) -> None:
        self._inner = inner

    @classmethod
    async def load(
        cls,
        source: str,
        *,
        workspace_token: str | None = None,
        lint: str = "deny",
    ) -> Workspace:
        inner = await _Workspace.load(
            str(source),
            workspace_token=workspace_token,
            lint=lint,
        )
        return cls(inner)

    @classmethod
    async def inspect(
        cls,
        source: str,
        *,
        workspace_token: str | None = None,
    ) -> Workspace:
        inner = await _Workspace.inspect(str(source), workspace_token=workspace_token)
        return cls(inner)

    @property
    def root(self) -> str:
        return self._inner.root()

    async def lint(self) -> dict[str, Any]:
        return await self._inner.lint()

    async def resolve_variable(
        self,
        id: str,
        context: JsonObject,
        *,
        validate_context: bool = True,
    ) -> VariableResolution:
        result = await self._inner.resolve_variable(
            id,
            context,
            validate_context=validate_context,
        )
        return VariableResolution(
            id=result["id"],
            value=result["value"],
            source=result["source"],
        )

    async def resolve_qualifier(
        self,
        id: str,
        context: JsonObject,
        *,
        validate_context: bool = True,
    ) -> bool:
        return await self._inner.resolve_qualifier(
            id,
            context,
            validate_context=validate_context,
        )


class RefreshingWorkspace:
    """Refreshing rototo workspace for long-running services."""

    def __init__(self, inner: _RefreshingWorkspace) -> None:
        self._inner = inner

    @classmethod
    async def load(
        cls,
        source: str,
        *,
        period_seconds: float | None = None,
        workspace_token: str | None = None,
        lint: str = "deny",
    ) -> RefreshingWorkspace:
        inner = await _RefreshingWorkspace.load(
            str(source),
            period_seconds=period_seconds,
            workspace_token=workspace_token,
            lint=lint,
        )
        return cls(inner)

    async def resolve_variable(
        self,
        id: str,
        context: JsonObject,
        *,
        validate_context: bool = True,
    ) -> VariableResolution:
        result = await self._inner.resolve_variable(
            id,
            context,
            validate_context=validate_context,
        )
        return VariableResolution(
            id=result["id"],
            value=result["value"],
            source=result["source"],
        )

    async def resolve_qualifier(
        self,
        id: str,
        context: JsonObject,
        *,
        validate_context: bool = True,
    ) -> bool:
        return await self._inner.resolve_qualifier(
            id,
            context,
            validate_context=validate_context,
        )

    async def refresh_now(self) -> str:
        return await self._inner.refresh_now()

    async def status(self) -> RefreshStatus:
        status = await self._inner.status()
        return RefreshStatus(**status)

    async def shutdown(self) -> None:
        await self._inner.shutdown()


__all__ = [
    "JsonObject",
    "RefreshingWorkspace",
    "RefreshStatus",
    "RototoError",
    "VariableResolution",
    "Workspace",
    "__version__",
]
