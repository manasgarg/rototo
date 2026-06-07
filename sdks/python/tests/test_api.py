from __future__ import annotations

import unittest
from pathlib import Path

import rototo


ROOT = Path(__file__).resolve().parents[3]
EXAMPLES_BASIC = str(ROOT / "examples" / "basic")


class ApiTest(unittest.IsolatedAsyncioTestCase):
    async def test_workspace_exposes_python_resolution_objects(self) -> None:
        workspace = await rototo.Workspace.load(EXAMPLES_BASIC)

        variable = await workspace.resolve_variable(
            "premium-message",
            {"user": {"tier": "premium"}},
        )
        qualifier = await workspace.resolve_qualifier(
            "premium-users",
            {"user": {"tier": "premium"}},
        )

        self.assertEqual(variable.id, "premium-message")
        self.assertEqual(variable.value_key, "premium")
        self.assertEqual(variable.value, "Welcome back, premium member.")
        self.assertEqual(qualifier.id, "premium-users")
        self.assertTrue(qualifier.value)

    async def test_inspected_workspace_can_lint_but_not_resolve(self) -> None:
        workspace = await rototo.Workspace.inspect(EXAMPLES_BASIC)
        lint = await workspace.lint()

        self.assertEqual(lint["diagnostics"], [])
        with self.assertRaises(rototo.RototoError) as raised:
            await workspace.resolve_variable("premium-message", {})

        self.assertIn("workspace was loaded without a runtime model", str(raised.exception))

    async def test_context_must_be_json_object(self) -> None:
        workspace = await rototo.Workspace.load(EXAMPLES_BASIC)

        with self.assertRaises(rototo.RototoError) as raised:
            await workspace.resolve_variable("premium-message", ["not", "an", "object"])

        self.assertIn("resolve context must be a JSON object", str(raised.exception))

    async def test_context_validation_can_be_skipped(self) -> None:
        workspace = await rototo.Workspace.load(EXAMPLES_BASIC)

        result = await workspace.resolve_variable(
            "premium-message",
            {"user": {"tier": {"bad": "shape"}}},
            validate_context=False,
        )

        self.assertEqual(result.value_key, "control")

    async def test_load_rejects_invalid_lint_mode(self) -> None:
        with self.assertRaises(ValueError) as raised:
            await rototo.Workspace.load(EXAMPLES_BASIC, lint="warn")

        self.assertIn("lint must be 'deny' or 'skip'", str(raised.exception))
