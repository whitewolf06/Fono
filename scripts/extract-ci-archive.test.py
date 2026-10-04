"""Security fixtures for the CI archive extraction boundary; no third-party dependencies."""

import hashlib
import importlib.util
import io
import pathlib
import tarfile
import tempfile
import unittest
from unittest import mock

SCRIPT_ROOT = pathlib.Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("extract_ci_archive", SCRIPT_ROOT / "extract-ci-archive.py")
extractor = importlib.util.module_from_spec(spec)
spec.loader.exec_module(extractor)
ROOT = "sherpa-fixture"


class ArchiveSecurityTests(unittest.TestCase):
    def setUp(self):
        fixture_root = SCRIPT_ROOT.parent / "build" / "ci-archive-tests"
        fixture_root.mkdir(parents=True, exist_ok=True)
        self.temporary = tempfile.TemporaryDirectory(dir=fixture_root)
        self.addCleanup(self.temporary.cleanup)
        self.directory = pathlib.Path(self.temporary.name)
        self.archive = self.directory / "fixture.tar.bz2"
        self.destination = self.directory / "extracted"

    def write_archive(self, members):
        with tarfile.open(self.archive, "w:bz2") as package:
            for member, content in members:
                package.addfile(member, None if content is None else io.BytesIO(content))
        return hashlib.sha256(self.archive.read_bytes()).hexdigest()

    def regular(self, name, content=b"safe"):
        member = tarfile.TarInfo(name)
        member.size = len(content)
        return member, content

    def reject(self, members):
        digest = self.write_archive(members)
        with self.assertRaises(ValueError):
            extractor.extract_archive(self.archive, self.destination, ROOT, digest)
        self.assertFalse(self.destination.exists())

    def test_valid_archive_extracts_exact_content(self):
        digest = self.write_archive([self.regular(f"{ROOT}/lib/model.dll")])
        extractor.extract_archive(self.archive, self.destination, ROOT, digest)
        self.assertEqual((self.destination / ROOT / "lib/model.dll").read_bytes(), b"safe")

    def test_unsupported_python_is_rejected(self):
        with mock.patch.object(extractor.sys, "version_info", (3, 11)):
            with self.assertRaisesRegex(RuntimeError, "3.12 or newer"):
                extractor.require_supported_python()

    def test_empty_archive_is_rejected(self):
        self.reject([])

    def test_compressed_size_limit_is_enforced(self):
        digest = self.write_archive([self.regular(f"{ROOT}/valid")])
        with mock.patch.object(extractor, "MAX_ARCHIVE_BYTES", 1):
            with self.assertRaisesRegex(ValueError, "Compressed archive"):
                extractor.extract_archive(self.archive, self.destination, ROOT, digest)
        self.assertFalse(self.destination.exists())

    def test_missing_archive_is_rejected(self):
        with self.assertRaises(FileNotFoundError):
            extractor.extract_archive(self.archive, self.destination, ROOT, "0" * 64)
        self.assertFalse(self.destination.exists())

    def test_wrong_checksum_is_rejected(self):
        self.write_archive([self.regular(f"{ROOT}/valid")])
        with self.assertRaisesRegex(ValueError, "SHA-256 mismatch"):
            extractor.extract_archive(self.archive, self.destination, ROOT, "0" * 64)
        self.assertFalse(self.destination.exists())

    def test_unsafe_paths_are_rejected(self):
        for name in (
            "/absolute",
            "../outside",
            f"{ROOT}/../outside",
            "other/file",
            f"{ROOT}/C:/file",
            f"{ROOT}\\file",
            f"{ROOT}/CON.txt",
            f"{ROOT}/LPT1",
            f"{ROOT}/file.",
            f"{ROOT}/file ",
            f"{ROOT}/./file",
            f"{ROOT}//file",
            f"{ROOT}/bad\x01name",
            f"{ROOT}/file?.dll",
        ):
            with self.subTest(name=name):
                self.reject([self.regular(name)])

    def test_links_and_special_members_are_rejected(self):
        for kind in (
            tarfile.SYMTYPE,
            tarfile.LNKTYPE,
            tarfile.CHRTYPE,
            tarfile.BLKTYPE,
            tarfile.FIFOTYPE,
            tarfile.GNUTYPE_SPARSE,
        ):
            with self.subTest(kind=kind):
                member = tarfile.TarInfo(f"{ROOT}/entry")
                member.type = kind
                member.linkname = "../../outside"
                self.reject([(member, None)])

    def test_pax_sparse_regular_member_is_rejected(self):
        member, content = self.regular(f"{ROOT}/sparse")
        member.pax_headers = {"GNU.sparse.map": "300000000,4", "GNU.sparse.size": "4"}
        digest = self.write_archive([(member, content)])
        with tarfile.open(self.archive, "r:bz2") as package:
            parsed = next(iter(package))
            self.assertEqual(parsed.type, tarfile.REGTYPE)
            self.assertEqual(parsed.size, 4)
            self.assertIsNotNone(parsed.sparse)
        with self.assertRaisesRegex(ValueError, "sparse entry"):
            extractor.extract_archive(self.archive, self.destination, ROOT, digest)
        self.assertFalse(self.destination.exists())

    def test_superscript_and_console_device_names_are_rejected(self):
        for name in ("COM¹", "COM².txt", "COM³", "LPT¹", "LPT²", "LPT³.txt", "CONIN$", "CONOUT$.txt"):
            with self.subTest(name=name):
                self.reject([self.regular(f"{ROOT}/{name}")])

    def test_case_insensitive_duplicate_paths_are_rejected(self):
        self.reject([self.regular(f"{ROOT}/file"), self.regular(f"{ROOT}/FILE")])

    def test_file_as_parent_directory_is_rejected(self):
        self.reject([self.regular(f"{ROOT}/parent"), self.regular(f"{ROOT}/parent/file")])

    def test_member_count_limit_is_enforced(self):
        self.reject([self.regular(f"{ROOT}/{index}") for index in range(extractor.MAX_MEMBERS + 1)])

    def test_expanded_size_limit_precedes_body_read(self):
        member = tarfile.TarInfo(f"{ROOT}/huge")
        member.size = extractor.MAX_EXPANDED_BYTES + 1
        self.reject([(member, None)])

    def test_existing_destination_is_rejected(self):
        digest = self.write_archive([self.regular(f"{ROOT}/valid")])
        self.destination.mkdir()
        with self.assertRaisesRegex(ValueError, "new directory"):
            extractor.extract_archive(self.archive, self.destination, ROOT, digest)
        self.assertEqual(list(self.destination.iterdir()), [])


if __name__ == "__main__":
    extractor.require_supported_python()
    unittest.main(verbosity=2)
