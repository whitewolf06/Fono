"""Packaging safety and exact inference-source parity, without recordings/devices."""
import ast
import importlib.util
from pathlib import Path
import tempfile
import unittest
import zipfile

spec = importlib.util.spec_from_file_location("prepare", Path(__file__).with_name("prepare-wake-runtime.py"))
package = importlib.util.module_from_spec(spec)
spec.loader.exec_module(package)


class Packaging(unittest.TestCase):
    def test_archive_paths_cannot_escape_or_use_windows_alternate_streams(self):
        for name in ["../recording.wav", "/x", "C:/secret", "x\\y", "x:stream"]:
            with self.subTest(name=name), self.assertRaises(ValueError):
                package.safe_name(name)

    def test_runtime_excludes_installed_packages_and_development_tools(self):
        for name in ["python/Lib/site-packages/private.py", "python/Lib/test/a.py", "python/Lib/ensurepip/x.py", "python/Lib/tkinter/x.py", "python/DLLs/_testclinic.pyd", "python/pythonw.exe"]:
            self.assertFalse(package.python_member(Path(name).parts))
        self.assertTrue(package.python_member(("python", "DLLs", "_ctypes.pyd")))

    def test_wheels_cannot_supply_linked_files(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with zipfile.ZipFile(root / "x.whl", "w") as wheel:
                entry = zipfile.ZipInfo("numpy/linked.py")
                entry.external_attr = 0o120777 << 16
                wheel.writestr(entry, "../../private")
            with self.assertRaises(ValueError):
                package.unpack_wheel(root / "x.whl", root / "out")
            self.assertFalse((root / "out").exists())

    def test_packaged_stream_is_ast_identical_to_accepted_detector(self):
        names = ["MAX_REJECTED_RETRIES", "FOCUSED_SAMPLES", "classify_contexts", "Stream", "exact_read", "reply"]
        original = ast.parse((package.SOURCE / "live_worker.py").read_text(encoding="utf-8"))
        selected = package.selected_source(package.SOURCE / "live_worker.py", names, "import candidate as common")
        compiled = ast.parse(selected)
        originals = {node.name: ast.dump(node) for node in original.body if isinstance(node, (ast.FunctionDef, ast.ClassDef))}
        for node in compiled.body:
            if isinstance(node, (ast.FunctionDef, ast.ClassDef)):
                self.assertEqual(ast.dump(node), originals[node.name])
        self.assertNotIn("dev_check", selected)
        self.assertNotIn("Diagnostics", selected)
        self.assertNotIn("torch", selected)

    def test_missing_source_member_stops_packaging(self):
        with self.assertRaises(ValueError):
            package.selected_source(package.SOURCE / "live_worker.py", ["missing"], "")


if __name__ == "__main__":
    unittest.main()
