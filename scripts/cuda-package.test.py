import importlib.util
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest
import os
import subprocess
import sys

sys.dont_write_bytecode = True

spec = importlib.util.spec_from_file_location("cuda_package", Path(__file__).with_name("cuda-package.py"))
package = importlib.util.module_from_spec(spec)
spec.loader.exec_module(package)


class PackageTests(unittest.TestCase):
    def setUp(self):
        test_root = Path(__file__).absolute().parent.parent / "build/checks/cuda-package-tests"
        test_root.mkdir(parents=True, exist_ok=True)
        self.temporary = tempfile.TemporaryDirectory(prefix="fixture-", dir=test_root)
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.repo, self.workers, self.output = (self.root / name for name in ("repo", "workers", "output"))
        self.repo.mkdir()
        self.workers.mkdir()
        self.write("package.json", '{"version":"0.7.22"}')
        self.write("src-tauri/crates/fono-stt-protocol/src/lib.rs", "pub const PROTOCOL_VERSION: u16 = 4;")
        self.write("LICENSE", "MIT Fono fixture; never distributed")
        self.write("src-tauri/vendor/whisper.cpp/LICENSE", "MIT ggml fixture")
        self.write("licenses/dependency-notices.txt", "Complete source inventory fixture")
        entries = []
        for name in ("NVIDIA-CUDA-13.3.1-EULA.txt", "NVIDIA-CUDA-13.3.1-third-party.txt"):
            path = "licenses/upstream/" + name
            self.write(path, f"Original license fixture {name}")
            entries.append({"file": path, "sha256": package.digest((self.repo / path).read_bytes())})
        self.write("licenses/upstream-sources.json", json.dumps({"sources": entries}))
        for name in (*package.CUDA_FILES, "fono-stt-vulkan-worker.exe"):
            (self.workers / name).write_bytes(f"inert worker/DLL fixture {name}".encode())
        self.write("licenses/native-inventory.json", json.dumps({"cudaToolkitVersion": "13.3.1", "files": [{"file": f"src-tauri/resources/stt-workers/{name}", "bytes": (self.workers / name).stat().st_size, "sha256": package.digest((self.workers / name).read_bytes())} for name in package.CUDA_FILES]}))

    def write(self, name, text):
        path = self.repo / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)

    def prepare(self):
        return package.prepare(self.repo, self.workers, self.output, 1)

    def test_manifest_and_archive_are_deterministic_and_complete(self):
        manifest = self.prepare()
        path = self.output / "fono-cuda-windows-x86_64.json"
        first = path.read_bytes()
        archive_first = (self.output / manifest["archive"]["name"]).read_bytes()
        self.prepare()
        self.assertEqual(first, path.read_bytes())
        self.assertEqual(archive_first, (self.output / manifest["archive"]["name"]).read_bytes())
        self.assertEqual(manifest["purpose"], "fono-cuda")
        self.assertEqual(manifest["schema_version"], 1)
        self.assertEqual(manifest["app_version"], "0.7.22")
        self.assertEqual(manifest["protocol_version"], 4)
        self.assertEqual(manifest["revision"], 1)
        self.assertEqual(set(manifest["notices"]), set(package.NOTICES))
        self.assertEqual(manifest, package.verify_archive(path))
        self.assertFalse(path.with_suffix(".json.sig").exists())

    def test_archive_has_exact_flat_files_and_no_vulkan(self):
        manifest = self.prepare()
        expected = set(package.CUDA_FILES) | set(package.NOTICES)
        with tarfile.open(self.output / manifest["archive"]["name"], "r:bz2") as archive:
            members = archive.getmembers()
            self.assertEqual({entry.name for entry in members}, expected)
            for entry in members:
                self.assertTrue(entry.isfile())
                self.assertEqual(entry.mtime, 0)
                record = next(item for item in manifest["files"] if item["path"] == entry.name)
                payload = archive.extractfile(entry).read()
                self.assertEqual(record["size"], len(payload))
                self.assertEqual(record["sha256"], package.digest(payload))

    def test_missing_dll_and_extra_raw_file_fail_closed(self):
        path = self.workers / package.CUDA_FILES[0]
        original = path.read_bytes()
        path.unlink()
        with self.assertRaises(ValueError):
            self.prepare()
        path.write_bytes(original)
        (self.workers / "unexpected.exe").write_text("not allowed")
        with self.assertRaisesRegex(ValueError, "Unexpected"):
            self.prepare()
        self.assertFalse(self.output.exists())

    def test_notice_hash_must_match_recorded_upstream(self):
        self.write("licenses/upstream/NVIDIA-CUDA-13.3.1-EULA.txt", "changed notice")
        with self.assertRaisesRegex(ValueError, "provenance"):
            self.prepare()

    def test_cuda_runtime_must_match_reviewed_toolkit_inventory(self):
        (self.workers / "cublas64_13.dll").write_bytes(b"different toolkit DLL")
        with self.assertRaisesRegex(ValueError, "reviewed toolkit"):
            self.prepare()

    def test_signed_output_cannot_be_overwritten_by_unsigned_prepare(self):
        self.output.mkdir()
        signature = self.output / "fono-cuda-windows-x86_64.json.sig"
        signature.write_text("public signature fixture")
        with self.assertRaisesRegex(ValueError, "signed output"):
            self.prepare()
        self.assertEqual(signature.read_text(), "public signature fixture")

    def test_invalid_revision_and_app_version_rejected(self):
        for revision in (0, -1, 2**53):
            with self.assertRaises(ValueError):
                package.prepare(self.repo, self.workers, self.output, revision)
        self.write("package.json", '{"version":"0.7.22-rc.1"}')
        with self.assertRaises(ValueError):
            self.prepare()

    def test_base_staging_excludes_all_stale_cuda_source_bytes(self):
        output = self.root / "base"
        package.stage_base(self.workers, output)
        self.assertEqual([item.name for item in output.iterdir()], ["fono-stt-vulkan-worker.exe"])
        for name in package.CUDA_FILES:
            self.assertTrue((self.workers / name).exists())

    def test_base_refuses_unexpected_files_without_removing_them(self):
        output = self.root / "base"
        output.mkdir()
        old = output / "cublas64_13.dll"
        old.write_text("unexpected stale cache")
        with self.assertRaisesRegex(ValueError, "Unexpected base"):
            package.stage_base(self.workers, output)
        self.assertEqual(old.read_text(), "unexpected stale cache")
        self.assertFalse((output / "fono-stt-vulkan-worker.exe").exists())

    def test_symlink_sources_and_outputs_rejected(self):
        for target in (self.workers / package.CUDA_FILES[0], self.root / "base"):
            if target.exists():
                target.unlink()
            try:
                target.symlink_to(self.repo / "LICENSE")
            except OSError as error:
                self.skipTest(f"Symlink permission unavailable: {error}")
            with self.assertRaises(ValueError):
                if target.name == "base":
                    package.stage_base(self.workers, target)
                else:
                    self.prepare()
            target.unlink()

    @unittest.skipUnless(os.name == "nt", "Windows junction behavior")
    def test_windows_junction_output_rejected_without_outside_write(self):
        outside = self.root / "outside"
        outside.mkdir()
        link = self.root / "base-link"
        result = subprocess.run(["cmd", "/d", "/c", "mklink", "/J", str(link), str(outside)], capture_output=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        try:
            with self.assertRaisesRegex(ValueError, "Reparse"):
                package.stage_base(self.workers, link)
            self.assertEqual(list(outside.iterdir()), [])
        finally:
            link.rmdir()

    def test_archive_tamper_rejected(self):
        manifest = self.prepare()
        archive = self.output / manifest["archive"]["name"]
        archive.write_bytes(archive.read_bytes() + b"tamper")
        with self.assertRaisesRegex(ValueError, "hash/size"):
            package.verify_archive(self.output / "fono-cuda-windows-x86_64.json")

    def test_unsafe_member_rejected_even_with_matching_outer_hash(self):
        manifest = self.prepare()
        path = self.output / "fono-cuda-windows-x86_64.json"
        archive_path = self.output / manifest["archive"]["name"]
        with tarfile.open(archive_path, "w:bz2") as archive:
            item = tarfile.TarInfo("../outside.exe")
            item.size = 1
            archive.addfile(item, io.BytesIO(b"x"))
        contents = archive_path.read_bytes()
        manifest["archive"].update(size=len(contents), sha256=package.digest(contents))
        path.write_text(json.dumps(manifest))
        with self.assertRaisesRegex(ValueError, "Unsafe/unexpected"):
            package.verify_archive(path)


if __name__ == "__main__":
    unittest.main()
