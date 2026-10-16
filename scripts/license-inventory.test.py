import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

sys.dont_write_bytecode = True
SCRIPT = Path(__file__).with_name("license-inventory.py")
spec = importlib.util.spec_from_file_location("license_inventory", SCRIPT)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class InventoryTests(unittest.TestCase):
    def setUp(self):
        workspace_temp = SCRIPT.parent.parent / "build/checks/license-inventory-tests"
        workspace_temp.mkdir(parents=True, exist_ok=True)
        self.fixture = tempfile.TemporaryDirectory(prefix="fono-license-test-", dir=workspace_temp)
        self.addCleanup(self.fixture.cleanup)
        self.root = Path(self.fixture.name)
        self.cache = self.root / "cargo-home"
        self.write("package-lock.json", json.dumps({"packages": {"": {"name": "fono"}}}))
        for path in module.LOCKFILES[1:]:
            self.write(path, "version = 3\npackage = []\n")

    def write(self, path, text):
        file = self.root / path
        file.parent.mkdir(parents=True, exist_ok=True)
        file.write_text(text, encoding="utf-8", newline="\n")
        return file

    def test_preserves_dual_licenses_and_shared_dependencies_in_both_workspaces(self):
        package = '\n[[package]]\nname = "example"\nversion = "1.0.0"\nsource = "registry+https://github.com/rust-lang/crates.io-index"\nchecksum = "abc"\n'
        for path in module.LOCKFILES[1:]:
            self.write(path, "version = 3\n" + package)
        crate = "cargo-home/registry/src/index.crates.io-fixture/example-1.0.0/"
        self.write(crate + "Cargo.toml", '[package]\nname="example"\nversion="1.0.0"\nlicense="MIT OR Apache-2.0"\n')
        original = "Copyright Example Authors\nFull upstream license notice.\n"
        self.write(crate + "LICENSE-MIT", original)
        result, notices = module.inventory(self.root, self.cache)
        self.assertEqual(len(result["cargo"]), 1)
        self.assertEqual(result["cargo"][0]["declaredLicense"], "MIT OR Apache-2.0")
        self.assertEqual(result["cargo"][0]["lockfiles"], list(module.LOCKFILES[1:]))
        self.assertIn(original, notices)
        self.assertFalse(result["issues"])
        self.assertNotIn(str(self.root), json.dumps(result))

    def test_license_declaration_without_notice_remains_an_evidence_gap(self):
        self.write("package-lock.json", json.dumps({"packages": {"node_modules/private-ui": {"version": "0.6.0", "license": "MIT"}}}))
        self.write("node_modules/private-ui/package.json", '{"version":"0.6.0","license":"MIT"}')
        result, _ = module.inventory(self.root, self.cache)
        self.assertEqual(result["npm"][0]["declaredLicense"], "MIT")
        self.assertEqual(result["npm"][0]["notices"], [])
        self.assertEqual(result["issues"][0]["issue"], "no-collected-license-text")

    def test_never_borrows_a_notice_from_a_different_installed_version(self):
        self.write("package-lock.json", json.dumps({"packages": {"node_modules/example": {"version": "1.0.0", "license": "BSD-3-Clause"}}}))
        self.write("node_modules/example/package.json", '{"version":"2.0.0","license":"MIT"}')
        self.write("node_modules/example/LICENSE", "Wrong-version notice")
        result, notices = module.inventory(self.root, self.cache)
        self.assertEqual(result["npm"][0]["sourceStatus"], "installed-version-mismatch")
        self.assertEqual(result["npm"][0]["declaredLicense"], "BSD-3-Clause")
        self.assertNotIn("Wrong-version notice", notices)

    def test_lock_package_paths_cannot_read_outside_node_modules(self):
        self.write("package-lock.json", json.dumps({"packages": {"../private": {"version": "1"}}}))
        with self.assertRaisesRegex(ValueError, "inside node_modules"):
            module.inventory(self.root, self.cache)

    def test_optional_uninstalled_package_is_not_declared_a_shipped_binary(self):
        self.write("package-lock.json", json.dumps({"packages": {"node_modules/other-platform": {"version": "1", "license": "MIT", "optional": True, "dev": True}}}))
        result, _ = module.inventory(self.root, self.cache)
        self.assertTrue(result["npm"][0]["optional"])
        self.assertTrue(result["npm"][0]["dev"])
        self.assertEqual(result["npm"][0]["sourceStatus"], "not-installed")
        self.assertIn("not a linked-binary SBOM", result["scope"])

    def test_cli_check_rejects_a_changed_lockfile(self):
        command = [sys.executable, str(SCRIPT), "--root", str(self.root), "--cargo-home", str(self.cache)]
        subprocess.run(command + ["--write"], check=True, capture_output=True)
        self.assertEqual(subprocess.run(command + ["--check"], capture_output=True).returncode, 0)
        self.write("tools/fono-setup/Cargo.lock", "version = 3\npackage = []\n# changed\n")
        failure = subprocess.run(command + ["--check"], capture_output=True, text=True)
        self.assertNotEqual(failure.returncode, 0)
        self.assertIn("tools/fono-setup/Cargo.lock", failure.stderr)

    def test_cli_check_cannot_pass_with_omitted_lockfile_coverage(self):
        command = [sys.executable, str(SCRIPT), "--root", str(self.root), "--cargo-home", str(self.cache)]
        subprocess.run(command + ["--write"], check=True, capture_output=True)
        saved = json.loads((self.root / "licenses/dependency-inventory.json").read_text())
        saved["inputs"] = saved["inputs"][:1]
        self.write("licenses/dependency-inventory.json", json.dumps(saved))
        failure = subprocess.run(command + ["--check"], capture_output=True, text=True)
        self.assertNotEqual(failure.returncode, 0)
        self.assertIn("all three lockfiles", failure.stderr)


if __name__ == "__main__":
    unittest.main()
