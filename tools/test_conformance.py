"""Fault checks for optional conformance provenance, framing and fail-closed reports."""
import hashlib
import io
import json
from pathlib import Path
import struct
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch

import check_conformance as conformance


class ConformanceTests(unittest.TestCase):
    def archive(self, directory, entries, contents=None):
        path = Path(directory) / 'vectors.tar.gz'
        with tarfile.open(path, 'w:gz') as archive:
            for name, data, kind in entries:
                info = tarfile.TarInfo(name)
                if kind == 'file':
                    info.size = len(data)
                    archive.addfile(info, io.BytesIO(data))
                else:
                    info.type = tarfile.SYMTYPE if kind == 'symlink' else tarfile.LNKTYPE
                    info.linkname = '/outside/untrusted'
                    archive.addfile(info)
        if contents is None:
            contents = {'testvector01.bit': b'known vector'}
        manifest = {'archive_sha256': conformance.digest_file(path),
                    'sha1': {name: hashlib.sha1(data).hexdigest() for name, data in contents.items()}}
        return path, manifest

    def test_validated_regular_files_extract_without_tar_metadata(self):
        with tempfile.TemporaryDirectory() as directory:
            archive, manifest = self.archive(directory, [('opus_newvectors/testvector01.bit', b'known vector', 'file')])
            destination = Path(directory) / 'out'
            self.assertEqual(conformance.extract_verified(archive, destination, manifest), 12)
            self.assertEqual((destination / 'testvector01.bit').read_bytes(), b'known vector')
            self.assertEqual(list(destination.iterdir()), [destination / 'testvector01.bit'])

    def test_names_links_and_duplicate_members_are_rejected_before_writes(self):
        for name, kind in [('opus_newvectors/../testvector01.bit', 'file'),
                           ('/opus_newvectors/testvector01.bit', 'file'),
                           ('opus_newvectors/./testvector01.bit', 'file'),
                           ('opus_newvectors/testvector01.bit', 'symlink'),
                           ('opus_newvectors/testvector01.bit', 'hardlink')]:
            with self.subTest(name=name, kind=kind), tempfile.TemporaryDirectory() as directory:
                archive, manifest = self.archive(directory, [(name, b'known vector', kind)])
                destination = Path(directory) / 'out'
                with self.assertRaises(ValueError):
                    conformance.extract_verified(archive, destination, manifest)
                self.assertFalse(destination.exists())
        with tempfile.TemporaryDirectory() as directory:
            entry = ('opus_newvectors/testvector01.bit', b'known vector', 'file')
            archive, manifest = self.archive(directory, [entry, entry])
            with self.assertRaisesRegex(ValueError, 'Duplicate'):
                conformance.extract_verified(archive, Path(directory) / 'out', manifest)

    def test_archive_hash_member_hash_and_expansion_limits_fail_closed(self):
        with tempfile.TemporaryDirectory() as directory:
            archive, manifest = self.archive(directory, [('opus_newvectors/testvector01.bit', b'corrupt bytes', 'file')])
            destination = Path(directory) / 'out'
            with self.assertRaisesRegex(ValueError, 'SHA1'):
                conformance.extract_verified(archive, destination, manifest)
            self.assertEqual(list(destination.iterdir()), [])
            wrong_archive = dict(manifest, archive_sha256='0' * 64)
            with self.assertRaisesRegex(ValueError, 'SHA256'):
                conformance.extract_verified(archive, destination, wrong_archive)
            with patch.object(conformance, 'MAX_ARCHIVE', archive.stat().st_size - 1):
                with self.assertRaisesRegex(ValueError, 'Archive exceeds'):
                    conformance.extract_verified(archive, destination, manifest)
            with patch.object(conformance, 'MAX_MEMBER', 8):
                with self.assertRaisesRegex(ValueError, 'size exceeds'):
                    conformance.extract_verified(archive, destination, manifest)
            with patch.object(conformance, 'MAX_TOTAL', 8):
                with self.assertRaisesRegex(ValueError, 'Expanded'):
                    conformance.extract_verified(archive, destination, manifest)

    def test_missing_unexpected_and_preexisting_targets_are_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            entry = ('opus_newvectors/testvector01.bit', b'known vector', 'file')
            archive, manifest = self.archive(directory, [entry], {'testvector01.bit': b'known vector', 'testvector02.bit': b'other'})
            with self.assertRaisesRegex(ValueError, 'file set'):
                conformance.extract_verified(archive, Path(directory) / 'out', manifest)
            archive, manifest = self.archive(directory, [('opus_newvectors/unexpected', b'known vector', 'file')])
            with self.assertRaisesRegex(ValueError, 'name/type'):
                conformance.extract_verified(archive, Path(directory) / 'out', manifest)
            archive, manifest = self.archive(directory, [entry])
            destination = Path(directory) / 'out'
            destination.mkdir()
            (destination / 'testvector01.bit').write_bytes(b'preserve me')
            with self.assertRaisesRegex(ValueError, 'already exists'):
                conformance.extract_verified(archive, destination, manifest)
            self.assertEqual((destination / 'testvector01.bit').read_bytes(), b'preserve me')
            self.assertFalse((destination / 'testvector01.bit.partial').exists())

    def test_big_endian_counts_and_truncated_or_oversized_records(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'test.bit'
            path.write_bytes(struct.pack('>II', 3, 0x01020304) + b'abc' + struct.pack('>II', 1, 0) + b'd')
            self.assertEqual(conformance.vector_counts(path), (2, 1))
            for data in [b'', b'\x00', b'\x00' * 7, struct.pack('>II', 65537, 0), struct.pack('>II', 3, 1) + b'ab']:
                path.write_bytes(data)
                with self.assertRaises(ValueError):
                    conformance.vector_counts(path)

    def test_metric_requires_success_marker_exit_and_finite_bounded_quality(self):
        def result(code, text):
            return subprocess.CompletedProcess([], code, stdout='', stderr=text)
        passed = 'Test vector PASSES\nOpus quality metric: 66.6 % (internal weighted error is0.1)\n'
        self.assertEqual(conformance.metric_result(result(0, passed)), {'passed': True, 'quality_percent': 66.6})
        for code, text in [(1, passed), (0, passed.replace('PASSES', 'FAILS')), (0, 'unrecognized output'),
                           (0, passed.replace('66.6', 'nan')), (0, passed.replace('66.6', 'inf')),
                           (0, passed.replace('66.6', '-0.1')), (0, passed.replace('66.6', '100.1'))]:
            with self.subTest(code=code, text=text):
                self.assertFalse(conformance.metric_result(result(code, text))['passed'])

    def test_report_replacement_is_atomic_when_serialization_fails(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'report.json'
            path.write_text('{"preserve":true}\n')
            with self.assertRaises(ValueError):
                conformance.write_report(path, {'quality': float('nan')})
            self.assertEqual(json.loads(path.read_text()), {'preserve': True})
            self.assertEqual(list(Path(directory).iterdir()), [path])
            with patch.object(conformance.os, 'fsync', side_effect=OSError('injected sync failure')):
                with self.assertRaises(OSError):
                    conformance.write_report(path, {'passed': True})
            self.assertEqual(json.loads(path.read_text()), {'preserve': True})
            self.assertEqual(list(Path(directory).iterdir()), [path])
            conformance.write_report(path, {'passed': False, 'complete': False})
            self.assertEqual(json.loads(path.read_text()), {'passed': False, 'complete': False})


if __name__ == '__main__':
    unittest.main()
