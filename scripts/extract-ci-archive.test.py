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

    def prepare_cache(self):
        digest = self.write_archive([
            self.regular(f"{ROOT}/lib/model.dll", b"trusted DLL"),
            self.regular(f"{ROOT}/lib/model.lib", b"trusted LIB"),
        ])
        extractor.extract_archive(self.archive, self.destination, ROOT, digest, reuse_existing=True)
        return digest

    def test_repeated_reuse_verifies_real_files_without_reextracting(self):
        digest = self.prepare_cache()
        files = [self.destination / ROOT / "lib" / name for name in ("model.dll", "model.lib")]
        before = [(entry.stat().st_mtime_ns, entry.stat().st_ino) for entry in files]
        extractor.extract_archive(self.archive, self.destination, ROOT, digest, reuse_existing=True)
        self.assertEqual(before, [(entry.stat().st_mtime_ns, entry.stat().st_ino) for entry in files])
        self.assertEqual(sorted(entry.name for entry in self.directory.iterdir()), ["extracted", "fixture.tar.bz2"])

    def test_same_size_tamper_is_rejected_without_mutating_cache(self):
        digest = self.prepare_cache()
        cached = self.destination / ROOT / "lib/model.dll"
        cached.write_bytes(b"changed DLL")
        with self.assertRaisesRegex(ValueError, "cache SHA-256"):
            extractor.extract_archive(self.archive, self.destination, ROOT, digest, reuse_existing=True)
        self.assertEqual(cached.read_bytes(), b"changed DLL")

    def test_missing_cache_file_is_rejected_without_repairing(self):
        digest = self.prepare_cache()
        cached = self.destination / ROOT / "lib/model.lib"
        cached.unlink()
        with self.assertRaisesRegex(ValueError, "incomplete"):
            extractor.extract_archive(self.archive, self.destination, ROOT, digest, reuse_existing=True)
        self.assertFalse(cached.exists())

    def test_extra_dll_and_forged_marker_are_rejected(self):
        digest = self.prepare_cache()
        injected = self.destination / ROOT / "lib/injected.dll"
        injected.write_bytes(b"untrusted")
        (self.destination / "verified.sha256").write_text(digest)
        with self.assertRaisesRegex(ValueError, "unexpected entry"):
            extractor.extract_archive(self.archive, self.destination, ROOT, digest, reuse_existing=True)
        self.assertTrue(injected.exists())

    def test_archive_is_rehashed_even_for_existing_cache(self):
        digest = self.prepare_cache()
        self.archive.write_bytes(b"tampered archive")
        with self.assertRaisesRegex(ValueError, "Archive SHA-256 mismatch"):
            extractor.extract_archive(self.archive, self.destination, ROOT, digest, reuse_existing=True)

    def test_reparse_cache_root_is_rejected(self):
        digest = self.prepare_cache()
        with mock.patch.object(extractor, "is_reparse", return_value=True):
            with self.assertRaisesRegex(ValueError, "ordinary directory"):
                extractor.extract_archive(self.archive, self.destination, ROOT, digest, reuse_existing=True)

    def test_archive_path_replacement_after_hash_cannot_change_verified_snapshot(self):
        digest = self.write_archive([self.regular(f"{ROOT}/lib/model.dll", b"trusted snapshot")])
        open_tar = tarfile.open

        def replace_path_before_tar_read(*args, **kwargs):
            self.archive.write_bytes(b"untrusted replacement after SHA-256")
            return open_tar(*args, **kwargs)

        with mock.patch.object(tarfile, "open", replace_path_before_tar_read):
            extractor.extract_archive(self.archive, self.destination, ROOT, digest, reuse_existing=True)
        self.assertEqual((self.destination / ROOT / "lib/model.dll").read_bytes(), b"trusted snapshot")
        self.assertEqual(self.archive.read_bytes(), b"untrusted replacement after SHA-256")

    def test_partial_extraction_failure_leaves_no_final_cache_and_next_run_succeeds(self):
        digest = self.write_archive([
            self.regular(f"{ROOT}/lib/model.dll", b"trusted DLL"),
            self.regular(f"{ROOT}/lib/model.lib", b"trusted LIB"),
        ])
        extractall = tarfile.TarFile.extractall

        def fail_after_first_file(package, staging, *, members, filter):
            extractall(package, staging, members=members[:1], filter=filter)
            self.assertTrue((staging / ROOT / "lib/model.dll").is_file())
            self.assertFalse(self.destination.exists())
            raise OSError("fixture interrupted after first file")

        with mock.patch.object(tarfile.TarFile, "extractall", fail_after_first_file):
            with self.assertRaisesRegex(OSError, "interrupted after first file"):
                extractor.extract_archive(self.archive, self.destination, ROOT, digest, reuse_existing=True)
        self.assertFalse(self.destination.exists())
        self.assertEqual(list(self.directory.iterdir()), [self.archive])
        extractor.extract_archive(self.archive, self.destination, ROOT, digest, reuse_existing=True)
        self.assertEqual((self.destination / ROOT / "lib/model.dll").read_bytes(), b"trusted DLL")
        self.assertEqual((self.destination / ROOT / "lib/model.lib").read_bytes(), b"trusted LIB")

    def test_abandoned_or_active_sibling_staging_does_not_block_or_get_cleaned(self):
        abandoned = self.directory / ".sherpa-staging-another-writer"
        abandoned.mkdir()
        content = abandoned / "partial.dll"
        content.write_bytes(b"still owned by another writer")
        before = content.stat().st_mtime_ns
        self.prepare_cache()
        self.assertEqual(content.read_bytes(), b"still owned by another writer")
        self.assertEqual(content.stat().st_mtime_ns, before)
        self.assertTrue((self.destination / ROOT / "lib/model.lib").is_file())

    def test_atomic_publication_never_replaces_a_cache_created_by_another_writer(self):
        digest = self.write_archive([self.regular(f"{ROOT}/lib/model.dll")])
        verify = extractor.verify_cached_files

        def create_concurrent_cache(package, members, staging):
            verify(package, members, staging)
            self.destination.mkdir()
            (self.destination / "other-writer.dll").write_bytes(b"must remain")

        with mock.patch.object(extractor, "verify_cached_files", create_concurrent_cache):
            with self.assertRaisesRegex(FileExistsError, "refusing to replace"):
                extractor.extract_archive(self.archive, self.destination, ROOT, digest, reuse_existing=True)
        self.assertEqual((self.destination / "other-writer.dll").read_bytes(), b"must remain")
        self.assertEqual(sorted(entry.name for entry in self.directory.iterdir()), ["extracted", "fixture.tar.bz2"])

    def test_transient_windows_sharing_errors_retry_only_atomic_publication(self):
        staging = mock.Mock(spec=pathlib.Path)
        destination = mock.Mock(spec=pathlib.Path)
        destination.exists.return_value = False
        destination.is_symlink.return_value = False
        error = PermissionError("fixture scanner still reading")
        error.winerror = 32
        staging.rename.side_effect = [error, error, None]
        with mock.patch.object(extractor.sys, "platform", "win32"), \
                mock.patch.object(extractor.time, "sleep") as sleep:
            extractor.publish_cache(staging, destination)
        self.assertEqual(staging.rename.call_args_list, [mock.call(destination)] * 3)
        self.assertEqual(sleep.call_count, 2)

    def test_persistent_publication_failure_is_bounded_and_does_not_leave_final_cache(self):
        digest = self.write_archive([self.regular(f"{ROOT}/lib/model.dll")])
        error = PermissionError("fixture permanent denial")
        error.winerror = 5
        with mock.patch.object(pathlib.Path, "rename", side_effect=error), \
                mock.patch.object(extractor.sys, "platform", "win32"), \
                mock.patch.object(extractor.time, "monotonic", side_effect=[0, 6]):
            with self.assertRaisesRegex(PermissionError, "permanent denial"):
                extractor.extract_archive(self.archive, self.destination, ROOT, digest, reuse_existing=True)
        self.assertFalse(self.destination.exists())
        self.assertEqual(list(self.directory.iterdir()), [self.archive])


if __name__ == "__main__":
    extractor.require_supported_python()
    unittest.main(verbosity=2)
