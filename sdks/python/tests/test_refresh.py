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
