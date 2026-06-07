from __future__ import annotations

import tempfile
import textwrap
import unittest
from pathlib import Path

import rototo


class RefreshingWorkspaceTest(unittest.IsolatedAsyncioTestCase):
    async def test_refreshing_workspace_refreshes_local_source(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            write_workspace(root, "hello")

            workspace = await rototo.RefreshingWorkspace.load(str(root))
            try:
                initial = await workspace.resolve_variable("message", {})
                self.assertEqual(initial.value, "hello")

                write_workspace(root, "updated")
                outcome = await workspace.refresh_now()
                self.assertIn(outcome, {"refreshed", "unchanged"})

                refreshed = await workspace.resolve_variable("message", {})
                self.assertEqual(refreshed.value, "updated")

                status = await workspace.status()
                self.assertIsNotNone(status.last_success)
                self.assertEqual(status.consecutive_failures, 0)
            finally:
                await workspace.shutdown()

            with self.assertRaises(rototo.RototoError):
                await workspace.resolve_variable("message", {})


def write_workspace(root: Path, message: str) -> None:
    (root / "variables").mkdir(exist_ok=True)
    (root / "rototo-workspace.toml").write_text("schema_version = 1\n")
    (root / "variables" / "message.toml").write_text(
        textwrap.dedent(
            f"""
            schema_version = 1

            description = "Message"
            type = "string"

            [values]
            default = "{message}"

            [resolve]
            default = "default"
            """
        ).lstrip()
    )
