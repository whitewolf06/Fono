"""Offline contract tests for the Linux CI native dependency bootstrap."""

import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


@unittest.skipUnless(sys.platform == "linux", "Linux bootstrap tests require Linux.")
class BootstrapLinuxTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        missing = [tool for tool in ("bash", "sha256sum", "python3") if not shutil.which(tool)]
        if missing:
            raise unittest.SkipTest("Missing required tools: " + ", ".join(missing))

    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="fono-bootstrap-linux-test-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.scripts = self.root / "checkout" / "scripts"
        self.scripts.mkdir(parents=True)
        self.script = self.scripts / "bootstrap-ci-linux.sh"
        shutil.copyfile(Path(__file__).with_name("bootstrap-ci-linux.sh"), self.script)
        self.runner = self.root / "runner"
        self.runner.mkdir()
        self.environment_file = self.root / "github-env"
        self.curl_log = self.root / "curl-arguments.json"
        self.payload = self.root / "payload"
        self.payload.write_bytes(b"verified fixture archive; never extracted or executed")
        self.archive_name = "sherpa-onnx-v1.13.4-linux-x64-shared-lib.tar.bz2"
        self.spec = {
            "version": "1.13.4",
            "url": "https://github.com/k2-fsa/sherpa-onnx/releases/download/v1.13.4/" + self.archive_name,
            "sha256": hashlib.sha256(self.payload.read_bytes()).hexdigest(),
            "downloadTimeoutSeconds": 120,
            "directory": self.archive_name.removesuffix(".tar.bz2"),
        }
        fake_bin = self.root / "bin"
        fake_bin.mkdir()
        fake_curl = fake_bin / "curl"
        fake_curl.write_text(
            "#!/usr/bin/env python3\n"
            "import json, os, pathlib, shutil, sys\n"
            "arguments = sys.argv[1:]\n"
            "pathlib.Path(os.environ['FONO_TEST_CURL_LOG']).write_text(json.dumps(arguments))\n"
            "destination = arguments[arguments.index('--output') + 1]\n"
            "shutil.copyfile(os.environ['FONO_TEST_PAYLOAD'], destination)\n",
            encoding="utf-8",
        )
        fake_curl.chmod(0o755)
        self.environment = {
            **os.environ,
            "PATH": str(fake_bin) + os.pathsep + os.environ.get("PATH", ""),
            "RUNNER_TEMP": str(self.runner),
            "GITHUB_ENV": str(self.environment_file),
            "FONO_TEST_CURL_LOG": str(self.curl_log),
            "FONO_TEST_PAYLOAD": str(self.payload),
        }

    def run_bootstrap(self):
        (self.scripts / "ci-native-dependencies.json").write_text(
            json.dumps({"sherpaLinux": self.spec}), encoding="utf-8"
        )
        return subprocess.run(
            ["bash", str(self.script)],
            cwd=self.root,
            env=self.environment,
            capture_output=True,
            text=True,
            timeout=10,
            check=False,
        )

    def test_success_exports_only_the_verified_canonical_archive(self):
        result = self.run_bootstrap()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        key, value = self.environment_file.read_text(encoding="utf-8").strip().split("=", 1)
        self.assertEqual(key, "SHERPA_ONNX_ARCHIVE_DIR")
        archive_dir = Path(value)
        self.assertEqual(archive_dir.parent, self.runner)
        self.assertEqual((archive_dir / self.archive_name).read_bytes(), self.payload.read_bytes())
        self.assertEqual(list(archive_dir.glob("*.partial")), [])
        arguments = json.loads(self.curl_log.read_text(encoding="utf-8"))
        for option, expected in (
            ("--proto", "=https"),
            ("--proto-redir", "=https"),
            ("--connect-timeout", "20"),
            ("--max-time", "120"),
            ("--retry", "2"),
            ("--retry-max-time", "120"),
        ):
            self.assertEqual(arguments[arguments.index(option) + 1], expected)
        self.assertIn("--fail", arguments)
        self.assertIn("--location", arguments)

    def test_bad_digest_never_exports_or_publishes_the_archive(self):
        self.spec["sha256"] = "0" * 64
        result = self.run_bootstrap()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("SHA-256 mismatch", result.stderr)
        self.assertFalse(self.environment_file.exists())
        self.assertEqual(list(self.runner.rglob("*.tar.bz2")), [])
        self.assertEqual(list(self.runner.rglob("*.partial")), [])

    def test_invalid_url_is_rejected_before_download(self):
        self.spec["url"] = "https://example.invalid/" + self.archive_name
        result = self.run_bootstrap()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("exact official Linux x64 Sherpa release URL", result.stderr)
        self.assertFalse(self.curl_log.exists())
        self.assertFalse(self.environment_file.exists())
        self.assertEqual(list(self.runner.iterdir()), [])


if __name__ == "__main__":
    unittest.main(verbosity=2)
