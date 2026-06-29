from __future__ import annotations

import tempfile
import textwrap
import unittest
from pathlib import Path

import rototo


class RefreshingPackageTest(unittest.IsolatedAsyncioTestCase):
    async def test_refreshing_package_refreshes_local_source(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            write_package(root, "hello")

            package = await rototo.RefreshingPackage.load(str(root))
            try:
                initial = package.resolve_variable("message", {})
                self.assertEqual(initial.value, "hello")

                write_package(root, "updated")
                outcome = await package.refresh_now()
                self.assertIn(outcome, {"refreshed", "unchanged"})

                refreshed = package.resolve_variable("message", {})
                self.assertEqual(refreshed.value, "updated")

                status = await package.status()
                self.assertIsNotNone(status.last_success)
                self.assertEqual(status.consecutive_failures, 0)
            finally:
                await package.shutdown()

            with self.assertRaises(rototo.RototoError):
                package.resolve_variable("message", {})

    async def test_identity_and_snapshot(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            write_package(root, "hello")

            package = await rototo.RefreshingPackage.load(str(root))
            try:
                identity = await package.identity()
                self.assertTrue(identity.source.startswith(str(root)))
                # A local directory has no fingerprint, so no derived release id.
                self.assertIsNone(identity.release_id)

                snapshot = await package.snapshot()
                self.assertIsNotNone(snapshot.last_success)
                self.assertIsNotNone(snapshot.last_event)
                self.assertEqual(snapshot.last_event.event_type, "loaded")
                self.assertEqual(snapshot.identity.source, identity.source)
            finally:
                await package.shutdown()

    async def test_refresh_events_stream(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            write_package(root, "hello")

            package = await rototo.RefreshingPackage.load(str(root))
            received: list[rototo.RefreshEvent] = []

            async def collect() -> None:
                async for event in package.refresh_events():
                    received.append(event)

            import asyncio

            task = asyncio.create_task(collect())
            # Let the generator run until it has subscribed and is awaiting the
            # first event before we mutate the source.
            await asyncio.sleep(0.02)

            write_package(root, "updated")
            outcome = await package.refresh_now()
            self.assertEqual(outcome, "refreshed")

            # Give the stream a moment, then shut down to close it.
            await asyncio.sleep(0.05)
            await package.shutdown()
            await task

            event_types = [event.event_type for event in received]
            self.assertIn("refreshed", event_types)
            refreshed = next(e for e in received if e.event_type == "refreshed")
            self.assertEqual(refreshed.schema_version, 1)
            self.assertEqual(refreshed.outcome, "refreshed")
            self.assertEqual(refreshed.sdk.language, "rust")
            self.assertIsNotNone(refreshed.current)


def write_package(root: Path, message: str) -> None:
    (root / "variables").mkdir(exist_ok=True)
    (root / "rototo-package.toml").write_text("schema_version = 1\n")
    (root / "variables" / "message.toml").write_text(
        textwrap.dedent(
            f"""
            schema_version = 1

            description = "Message"
            type = "string"

            [resolve]
            default = "{message}"
            """
        ).lstrip()
    )
