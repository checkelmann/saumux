import hashlib
import importlib.util
from pathlib import Path
import tarfile
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("release", Path(__file__).resolve().parents[1] / "scripts/release.py")
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class ReleaseTest(unittest.TestCase):
    def test_tag_must_match_package(self):
        self.assertEqual(release.validate("v" + release.version()), "v" + release.version())
        for tag in ["main", "v9999.0.0", "v0.1.0/unsafe"]:
            with self.assertRaises(ValueError):
                release.validate(tag)

    def test_archive_contains_executable_docs_and_matching_checksum(self):
        with tempfile.TemporaryDirectory() as directory:
            directory = Path(directory)
            binary = directory / "saumux"
            binary.write_bytes(b"test executable")
            archive = release.package("aarch64-apple-darwin", binary, directory / "dist")
            with tarfile.open(archive) as tar:
                members = tar.getmembers()
                self.assertEqual(len(members), 6)
                executable = next(m for m in members if m.name.endswith("/saumux"))
                self.assertEqual(executable.mode, 0o755)
                self.assertEqual(tar.extractfile(executable).read(), binary.read_bytes())
                self.assertTrue(any(m.name.endswith("/terminfo/saumux.terminfo") for m in members))
                self.assertTrue(any(m.name.endswith("/THIRD_PARTY_NOTICES.md") for m in members))
            checksum = archive.with_name(archive.name + ".sha256").read_text().split()[0]
            self.assertEqual(checksum, hashlib.sha256(archive.read_bytes()).hexdigest())


if __name__ == "__main__":
    unittest.main()
