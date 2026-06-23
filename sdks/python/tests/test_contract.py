from __future__ import annotations

import json
import unittest
from pathlib import Path
from typing import Any

import rototo


ROOT = Path(__file__).resolve().parents[3]
CASES = ROOT / "tests" / "sdk-contract" / "cases.jsonl"


class ContractTest(unittest.IsolatedAsyncioTestCase):
    async def test_shared_contract_cases(self) -> None:
        for case in contract_cases():
            with self.subTest(case=case["name"]):
                if case["expect"]["ok"]:
                    actual = await run_case(case)
                    assert_expected_subset(self, actual, case["expect"])
                else:
                    with self.assertRaises(rototo.RototoError) as raised:
                        await run_case(case)
                    self.assertIn(
                        case["expect"]["error"]["contains"],
                        str(raised.exception),
                    )


async def run_case(case: dict[str, Any]) -> dict[str, Any]:
    operation = case["operation"]
    workspace_source = str(ROOT / case["workspace"])

    if operation == "load_workspace":
        await rototo.Workspace.load(workspace_source)
        return {"ok": True}

    if operation == "lint_workspace":
        workspace = await rototo.Workspace.inspect(workspace_source)
        lint = await workspace.lint()
        return {"diagnostics": len(lint["diagnostics"])}

    if operation == "resolve_variable":
        workspace = await rototo.Workspace.load(workspace_source)
        result = await workspace.resolve_variable(case["id"], case.get("context", {}))
        return {
            "id": result.id,
            "value": result.value,
            "source": result.source,
        }

    if operation == "resolve_qualifier":
        workspace = await rototo.Workspace.load(workspace_source)
        return await workspace.resolve_qualifier(case["id"], case.get("context", {}))

    raise AssertionError(f"unsupported contract operation: {operation}")


def assert_expected_subset(
    test: unittest.TestCase,
    actual: dict[str, Any],
    expect: dict[str, Any],
) -> None:
    if "diagnostics" in expect:
        test.assertEqual(actual["diagnostics"], expect["diagnostics"])
    if "result" in expect:
        assert_subset(test, actual, expect["result"])


def assert_subset(test: unittest.TestCase, actual: Any, expected: Any) -> None:
    if isinstance(expected, dict):
        test.assertIsInstance(actual, dict)
        for key, value in expected.items():
            test.assertIn(key, actual)
            assert_subset(test, actual[key], value)
    else:
        test.assertEqual(actual, expected)


def contract_cases() -> list[dict[str, Any]]:
    return [
        json.loads(line)
        for line in CASES.read_text().splitlines()
        if line.strip()
    ]
