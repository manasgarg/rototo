from __future__ import annotations

import unittest

import rototo


class PackagingTest(unittest.TestCase):
    def test_public_api_exports_expected_names(self) -> None:
        self.assertRegex(rototo.__version__, r"^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$")
        self.assertIsNotNone(rototo.RototoError)
        self.assertIsNotNone(rototo.Package)
        self.assertIsNotNone(rototo.RefreshingPackage)
        self.assertIsNotNone(rototo.VariableResolution)
        self.assertIsNotNone(rototo.RefreshStatus)
