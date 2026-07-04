"""Python SDK for rototo runtime configuration packages."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any, AsyncIterator, Mapping

from ._rototo import RototoError, _RefreshingPackage, _Package, version as _version


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
    """Current refresh state for a long-running package handle."""

    current_fingerprint: Any | None
    last_success: float | None
    last_attempt: float | None
    consecutive_failures: int
    last_error: str | None
    refreshing: bool
    immutable: bool
    serving_fallback: bool


@dataclass(frozen=True)
class PackageLayerIdentity:
    """Identity of one layer in a layered package."""

    source: str
    fingerprint: Any | None
    release_id: str | None
    immutable: bool

    @classmethod
    def _from_dict(cls, data: Mapping[str, Any]) -> PackageLayerIdentity:
        return cls(
            source=data["source"],
            fingerprint=data["fingerprint"],
            release_id=data["release_id"],
            immutable=data["immutable"],
        )


@dataclass(frozen=True)
class PackageIdentity:
    """Stable identity of the package currently active in this process."""

    source: str
    fingerprint: Any | None
    release_id: str | None
    loaded_at: float
    immutable: bool
    layers: list[PackageLayerIdentity]

    @classmethod
    def _from_dict(cls, data: Mapping[str, Any]) -> PackageIdentity:
        return cls(
            source=data["source"],
            fingerprint=data["fingerprint"],
            release_id=data["release_id"],
            loaded_at=data["loaded_at"],
            immutable=data["immutable"],
            layers=[PackageLayerIdentity._from_dict(layer) for layer in data["layers"]],
        )


@dataclass(frozen=True)
class SdkIdentity:
    """Identity of the SDK that emitted a refresh event."""

    name: str
    version: str
    language: str


@dataclass(frozen=True)
class RefreshEventSummary:
    """Compact record of the most recent refresh event."""

    event_id: str
    event_type: str
    release_id: str | None
    completed_at: float

    @classmethod
    def _from_dict(cls, data: Mapping[str, Any]) -> RefreshEventSummary:
        return cls(
            event_id=data["event_id"],
            event_type=data["event_type"],
            release_id=data["release_id"],
            completed_at=data["completed_at"],
        )


@dataclass(frozen=True)
class RefreshSnapshot:
    """Refresh state joined with package identity: what is true now."""

    identity: PackageIdentity
    last_attempt: float | None
    last_success: float | None
    last_event: RefreshEventSummary | None
    consecutive_failures: int
    last_error: str | None
    refreshing: bool
    immutable: bool
    serving_fallback: bool

    @classmethod
    def _from_dict(cls, data: Mapping[str, Any]) -> RefreshSnapshot:
        last_event = data["last_event"]
        return cls(
            identity=PackageIdentity._from_dict(data["identity"]),
            last_attempt=data["last_attempt"],
            last_success=data["last_success"],
            last_event=(
                RefreshEventSummary._from_dict(last_event)
                if last_event is not None
                else None
            ),
            consecutive_failures=data["consecutive_failures"],
            last_error=data["last_error"],
            refreshing=data["refreshing"],
            immutable=data["immutable"],
            serving_fallback=data["serving_fallback"],
        )


@dataclass(frozen=True)
class RefreshEvent:
    """A refresh state-transition event."""

    schema_version: int
    event_id: str
    event_type: str
    source: str
    previous: PackageIdentity | None
    current: PackageIdentity | None
    attempted_at: float
    completed_at: float
    duration_ms: int
    outcome: str | None
    consecutive_failures: int
    error: str | None
    sdk: SdkIdentity

    @classmethod
    def _from_dict(cls, data: Mapping[str, Any]) -> RefreshEvent:
        previous = data["previous"]
        current = data["current"]
        return cls(
            schema_version=data["schema_version"],
            event_id=data["event_id"],
            event_type=data["event_type"],
            source=data["source"],
            previous=PackageIdentity._from_dict(previous) if previous is not None else None,
            current=PackageIdentity._from_dict(current) if current is not None else None,
            attempted_at=data["attempted_at"],
            completed_at=data["completed_at"],
            duration_ms=data["duration_ms"],
            outcome=data["outcome"],
            consecutive_failures=data["consecutive_failures"],
            error=data["error"],
            sdk=SdkIdentity(**data["sdk"]),
        )


class Package:
    """Loaded rototo package."""

    def __init__(self, inner: _Package) -> None:
        self._inner = inner

    @classmethod
    async def load(
        cls,
        source: str,
        *,
        package_token: str | None = None,
        lint: str = "deny",
        fallback_source: str | None = None,
    ) -> Package:
        inner = await _Package.load(
            str(source),
            package_token=package_token,
            lint=lint,
            fallback_source=fallback_source,
        )
        return cls(inner)

    @classmethod
    async def inspect(
        cls,
        source: str,
        *,
        package_token: str | None = None,
    ) -> Package:
        inner = await _Package.inspect(str(source), package_token=package_token)
        return cls(inner)

    @property
    def root(self) -> str:
        return self._inner.root()

    @property
    def served_fallback(self) -> bool:
        """True when this package was loaded from the fallback source because
        the primary source failed."""
        return self._inner.served_fallback()

    def identity(self) -> PackageIdentity:
        return PackageIdentity._from_dict(self._inner.identity())

    async def lint(self) -> dict[str, Any]:
        return await self._inner.lint()

    def resolve_variable(
        self,
        id: str,
        context: JsonObject,
        *,
        validate_context: bool = True,
        trace: bool = False,
    ) -> VariableResolution:
        """Resolve a variable."""
        result = self._inner.resolve_variable(
            id,
            context,
            validate_context=validate_context,
            trace=trace,
        )
        return VariableResolution(
            id=result["id"],
            value=result["value"],
            source=result["source"],
        )

    async def trace_events(self) -> AsyncIterator[dict[str, Any]]:
        """Yield resolution trace stream items as they occur. Each item is a
        dict: a trace (``{"kind": "trace", "trace": {...}}``) or a drop marker
        (``{"kind": "dropped", "count": n}``). Tracing is computed only while
        this iterator is consumed; with no subscriber a ``[[trace]]`` policy
        costs nothing."""
        events = self._inner.subscribe_trace_events()
        while True:
            item = await events.recv()
            if item is None:
                return
            yield item


class RefreshingPackage:
    """Refreshing rototo package for long-running services."""

    def __init__(self, inner: _RefreshingPackage) -> None:
        self._inner = inner

    @classmethod
    async def load(
        cls,
        source: str,
        *,
        period_seconds: float | None = None,
        package_token: str | None = None,
        lint: str = "deny",
        fallback_source: str | None = None,
    ) -> RefreshingPackage:
        inner = await _RefreshingPackage.load(
            str(source),
            period_seconds=period_seconds,
            package_token=package_token,
            lint=lint,
            fallback_source=fallback_source,
        )
        return cls(inner)

    def resolve_variable(
        self,
        id: str,
        context: JsonObject,
        *,
        validate_context: bool = True,
        trace: bool = False,
    ) -> VariableResolution:
        """Resolve a variable."""
        result = self._inner.resolve_variable(
            id,
            context,
            validate_context=validate_context,
            trace=trace,
        )
        return VariableResolution(
            id=result["id"],
            value=result["value"],
            source=result["source"],
        )

    async def refresh_now(self) -> str:
        return await self._inner.refresh_now()

    async def status(self) -> RefreshStatus:
        status = await self._inner.status()
        return RefreshStatus(**status)

    async def identity(self) -> PackageIdentity:
        return PackageIdentity._from_dict(await self._inner.identity())

    async def snapshot(self) -> RefreshSnapshot:
        return RefreshSnapshot._from_dict(await self._inner.snapshot())

    async def refresh_events(self) -> AsyncIterator[RefreshEvent]:
        """Yield refresh events as they occur. The stream ends when the package
        is shut down. A lagging consumer skips dropped events rather than
        erroring; recover ground truth from ``snapshot()``."""
        events = self._inner.subscribe_events()
        while True:
            event = await events.recv()
            if event is None:
                return
            yield RefreshEvent._from_dict(event)

    async def trace_events(self) -> AsyncIterator[dict[str, Any]]:
        """Yield resolution trace stream items as they occur. Each item is a
        dict: a trace (``{"kind": "trace", "trace": {...}}``) or a drop marker
        (``{"kind": "dropped", "count": n}``)."""
        events = self._inner.subscribe_trace_events()
        while True:
            item = await events.recv()
            if item is None:
                return
            yield item

    async def shutdown(self) -> None:
        await self._inner.shutdown()


__all__ = [
    "JsonObject",
    "PackageIdentity",
    "PackageLayerIdentity",
    "RefreshEvent",
    "RefreshEventSummary",
    "RefreshSnapshot",
    "RefreshingPackage",
    "RefreshStatus",
    "RototoError",
    "SdkIdentity",
    "VariableResolution",
    "Package",
    "__version__",
]
