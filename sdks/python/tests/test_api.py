from __future__ import annotations

import unittest
from pathlib import Path

import rototo


ROOT = Path(__file__).resolve().parents[3]
EXAMPLES_BASIC = str(ROOT / "examples" / "basic")


class ApiTest(unittest.IsolatedAsyncioTestCase):
    async def test_package_exposes_python_runtime_resolution_api(self) -> None:
        package = await rototo.Package.load(EXAMPLES_BASIC)

        variable = package.resolve_variable(
            "premium_message",
            {"user": {"tier": "premium"}},
        )
        condition = package.resolve_variable(
            "premium_users",
            {"user": {"tier": "premium"}},
        )

        self.assertEqual(variable.id, "premium_message")
        self.assertEqual(variable.source, {"kind": "literal"})
        self.assertEqual(variable.value, "Welcome back, premium member.")
        self.assertTrue(condition.value)

    async def test_inspected_package_can_lint_but_not_resolve(self) -> None:
        package = await rototo.Package.inspect(EXAMPLES_BASIC)
        lint = await package.lint()

        self.assertEqual(lint["diagnostics"], [])
        with self.assertRaises(rototo.RototoError) as raised:
            package.resolve_variable("premium_message", {})

        self.assertIn("package was loaded without a runtime model", str(raised.exception))

    async def test_context_must_be_json_object(self) -> None:
        package = await rototo.Package.load(EXAMPLES_BASIC)

        with self.assertRaises(rototo.RototoError) as raised:
            package.resolve_variable("premium_message", ["not", "an", "object"])

        self.assertIn("evaluation context must be a JSON object", str(raised.exception))

    async def test_context_validation_can_be_skipped(self) -> None:
        package = await rototo.Package.load(EXAMPLES_BASIC)

        result = package.resolve_variable(
            "premium_message",
            {"user": {"tier": {"bad": "shape"}}},
            validate_context=False,
        )

        self.assertEqual(result.source, {"kind": "literal"})

    async def test_load_rejects_invalid_lint_mode(self) -> None:
        with self.assertRaises(ValueError) as raised:
            await rototo.Package.load(EXAMPLES_BASIC, lint="warn")

        self.assertIn("lint must be 'deny' or 'skip'", str(raised.exception))

    async def test_scoped_package_tokens_load_and_stay_off_local_sources(self) -> None:
        # Scoped tokens map https:// URL prefixes to bearer tokens; they never
        # touch a local load, so parsing and loading both succeed.
        package = await rototo.Package.load(
            EXAMPLES_BASIC,
            package_tokens={"https://config.acme.com/team-a": "token"},
        )
        self.assertFalse(package.served_fallback)

    async def test_bare_and_scoped_package_tokens_are_mutually_exclusive(self) -> None:
        with self.assertRaises(ValueError) as raised:
            await rototo.Package.load(
                EXAMPLES_BASIC,
                package_token="bare",
                package_tokens={"https://config.acme.com": "scoped"},
            )
        self.assertIn("cannot both be set", str(raised.exception))

    async def test_scoped_package_token_prefixes_are_validated(self) -> None:
        with self.assertRaises(rototo.RototoError) as raised:
            await rototo.Package.load(
                EXAMPLES_BASIC,
                package_tokens={"http://config.acme.com": "token"},
            )
        self.assertIn("must start with https://", str(raised.exception))


class ReflectionTest(unittest.IsolatedAsyncioTestCase):
    async def test_reflection_surface(self) -> None:
        package = await rototo.Package.load(str(ROOT / "examples" / "billing"))

        self.assertIn("plan_tiers", package.list_enums())
        plan_tiers = package.read_enum("plan_tiers")
        self.assertEqual(plan_tiers["memberType"], "string")
        self.assertIn("business", plan_tiers["members"])

        entries = package.list_entries("features")
        self.assertIn("sso", entries)
        sso = package.read_entry("features", "sso")
        self.assertEqual(sso["name"], "Single sign-on")

        value = package.resolve_reference("catalog=features:entry=sso#/name")
        self.assertEqual(value, "Single sign-on")
        value = package.resolve_entry_ref("sso#/name", ["features"])
        self.assertEqual(value, "Single sign-on")

        with self.assertRaises(rototo.RototoError) as raised:
            package.resolve_reference("catalog=features:entry=absent")
        self.assertIn("does not resolve", str(raised.exception))
