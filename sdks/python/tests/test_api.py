from __future__ import annotations

import unittest
from pathlib import Path

import rototo


ROOT = Path(__file__).resolve().parents[3]
EXAMPLES_BASIC = str(ROOT / "examples" / "basic")


class ApiTest(unittest.IsolatedAsyncioTestCase):
    async def test_package_exposes_python_runtime_resolution_api(self) -> None:
        package = await rototo.Package.load(EXAMPLES_BASIC)

        variable = await package.resolve_variable(
            "premium-message",
            {"user": {"tier": "premium"}},
        )
        qualifier = await package.resolve_qualifier(
            "premium-users",
            {"user": {"tier": "premium"}},
        )

        self.assertEqual(variable.id, "premium-message")
        self.assertEqual(variable.source, {"kind": "literal"})
        self.assertEqual(variable.value, "Welcome back, premium member.")
        self.assertTrue(qualifier)

    async def test_inspected_package_can_lint_but_not_resolve(self) -> None:
        package = await rototo.Package.inspect(EXAMPLES_BASIC)
        lint = await package.lint()

        self.assertEqual(lint["diagnostics"], [])
        with self.assertRaises(rototo.RototoError) as raised:
            await package.resolve_variable("premium-message", {})

        self.assertIn("package was loaded without a runtime model", str(raised.exception))

    async def test_context_must_be_json_object(self) -> None:
        package = await rototo.Package.load(EXAMPLES_BASIC)

        with self.assertRaises(rototo.RototoError) as raised:
            await package.resolve_variable("premium-message", ["not", "an", "object"])

        self.assertIn("resolve context must be a JSON object", str(raised.exception))

    async def test_context_validation_can_be_skipped(self) -> None:
        package = await rototo.Package.load(EXAMPLES_BASIC)

        result = await package.resolve_variable(
            "premium-message",
            {"user": {"tier": {"bad": "shape"}}},
            validate_context=False,
        )

        self.assertEqual(result.source, {"kind": "literal"})

    async def test_load_rejects_invalid_lint_mode(self) -> None:
        with self.assertRaises(ValueError) as raised:
            await rototo.Package.load(EXAMPLES_BASIC, lint="warn")

        self.assertIn("lint must be 'deny' or 'skip'", str(raised.exception))
