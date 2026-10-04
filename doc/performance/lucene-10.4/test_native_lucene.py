"""Native-mode and on-disk-header gates; never launch either benchmark engine."""
import argparse
import contextlib
import io
import json
import struct
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from lucene_mode import add_arguments, configuration, report
from prepare_native_lucene import native_source
from provenance import capture
from tantivy_statistics import text_token_total, token_header

HERE = Path(__file__).resolve().parent


def vint(value):
    result = bytearray()
    while value >= 128:
        result.append(value & 127)
        value >>= 7
    result.append(value | 128)
    return bytes(result)


def composite(entries, version):
    # Fixtures independently encode the Rust CompositeWrite/footer wire layout.
    body = b''.join(payload for _, payload in entries)
    directory = vint(len(entries))
    previous = offset = 0
    for (field, idx), payload in entries:
        directory += vint(offset - previous) + struct.pack('<I', field) + vint(idx)
        previous = offset
        offset += len(payload)
    body += directory + struct.pack('<I', len(directory))
    footer = json.dumps({'version': {'index_format_version': version}, 'crc': 0}).encode()
    return body + footer + struct.pack('<II', len(footer), 1337)


class CompositeHeaderTests(unittest.TestCase):
    def test_versions_and_nonleading_text_header(self):
        with tempfile.TemporaryDirectory() as temp:
            index = Path(temp)
            (index/'meta.json').write_text(json.dumps({
                'schema': [{'name': 'id'}, {'name': 'text'}],
                'segments': [{'segment_id': 'abcd-ef01'}]}))
            for version in (8, 9, 10):
                for layout in (['absent'] if version < 10 else ['absent', 'before', 'after']):
                    entries = [((0, 0), struct.pack('<Q', 777) + b'other postings')]
                    text = ((1, 0), struct.pack('<Q', 294826965) + b'text postings')
                    count = ((1, 1), struct.pack('<I', 917578))
                    entries.extend([count, text] if layout == 'before' else
                                   [text, count] if layout == 'after' else [text])
                    (index/'abcdef01.idx').write_bytes(composite(entries, version))
                    with self.subTest(version=version, layout=layout):
                        self.assertEqual(text_token_total(index), 294826965)

    def test_wrong_footer_truncated_header_and_missing_text_fail(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp)/'test.idx'
            fixtures = [b'short', composite([((1, 0), b'ab')], 10),
                        composite([((1, 1), struct.pack('<I', 2))], 10)]
            good = composite([((1, 0), struct.pack('<Q', 4))], 9)
            fixtures.append(good[:-4] + struct.pack('<I', 999))
            for contents in fixtures:
                path.write_bytes(contents)
                with self.subTest(contents=contents), self.assertRaises(ValueError):
                    token_header(path, 1)

    def test_multisegment_matcher_is_rejected(self):
        with tempfile.TemporaryDirectory() as temp:
            index = Path(temp)
            (index/'meta.json').write_text(json.dumps({'schema': [{'name': 'text'}],
                                                      'segments': [{}, {}]}))
            with self.assertRaises(ValueError):
                text_token_total(index)


class NativeModeTests(unittest.TestCase):
    def make_parser(self, matched_default=False):
        parser = argparse.ArgumentParser()
        parser.set_defaults(lucene_dir=Path('/lucene'), lucene_classes=Path('/classes'),
                            tantivy_index=Path('/absent-native-index'))
        add_arguments(parser, matched_default)
        return parser

    def test_native_never_reads_matcher_header_or_names_matched_class(self):
        parser = self.make_parser(matched_default=True)
        for extra, scale in [([], 1.0), (['--lucene-score-scale', '2.2'], 2.2)]:
            args = parser.parse_args(['--native-bm25'] + extra)
            with patch('lucene_mode.text_token_total', side_effect=AssertionError('native scanned header')):
                config = configuration(args, parser)
            self.assertEqual(config['query_class'], 'DoQueryNative')
            self.assertEqual(config['dump_class'], 'DumpNativeLuceneResults')
            self.assertEqual(config['extra_args'], [str(scale)])
            self.assertIsNone(config['token_total'])
            self.assertTrue(report(config)['native_bm25'])
            self.assertFalse(report(config)['matched_collection_statistics'])
            self.assertEqual(config['lucene_bm25'], {'k1': 1.2, 'b': .75})

    def test_historical_defaults_and_explicit_matched_are_preserved(self):
        for matched_default in (False, True):
            parser = self.make_parser(matched_default)
            for flags in ([], ['--matched-bm25']):
                args = parser.parse_args(flags)
                with patch('lucene_mode.text_token_total', return_value=123) as header:
                    config = configuration(args, parser)
                matched = matched_default or bool(flags)
                self.assertEqual(config['query_class'], 'DoQueryMatched' if matched else 'DoQuery')
                self.assertEqual(config['extra_args'], ['123'] if matched else [])
                self.assertEqual(header.call_count, int(matched))
                self.assertEqual(config['lucene_score_scale'], 2.2 if matched else 1.0)
                self.assertEqual(config['matched_collection_statistics'], matched)

    def test_invalid_mode_combinations_and_scales_fail(self):
        parser = self.make_parser()
        with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
            parser.parse_args(['--native-bm25', '--matched-bm25'])
        for flags in [['--lucene-score-scale', '2.2'],
                      ['--matched-bm25', '--lucene-score-scale', '2.2'],
                      ['--native-bm25', '--lucene-score-scale', 'nan'],
                      ['--native-bm25', '--lucene-score-scale', 'inf'],
                      ['--native-bm25', '--lucene-score-scale', '0']]:
            with self.subTest(flags=flags), contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
                configuration(parser.parse_args(flags), parser)

    def test_all_cli_entrypoints_reject_both_modes_before_io(self):
        common = ['--tantivy-index', '/absent', '--lucene-dir', '/absent',
                  '--lucene-classes', '/absent', '--output', '/absent',
                  '--native-bm25', '--matched-bm25']
        cases = [('suite_lucene_interleaved.py', ['--tantivy-binary', '/absent', '--command', 'COUNT']),
                 ('compare_wiki_correctness.py', ['--tantivy-validator', '/absent', '--commit', 'fixture']),
                 ('compare_process_memory.py', ['--tantivy-binary', '/absent'])]
        for name, required in cases:
            result = subprocess.run([sys.executable, str(HERE/name)] + common + required,
                                    capture_output=True, text=True)
            with self.subTest(name=name):
                self.assertEqual(result.returncode, 2)
                self.assertIn('not allowed with argument --native-bm25', result.stderr)
                self.assertNotIn('Traceback', result.stderr)

    def test_native_provenance_hashes_only_native_classes(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            classes = root/'classes'
            classes.mkdir()
            for name in ['DoQueryNative.class', 'DumpNativeLuceneResults.class']:
                (classes/name).write_bytes(name.encode())
            binary = root/'binary'
            binary.write_bytes(b'binary')
            java = subprocess.CompletedProcess(['java', '-version'], 0, '', 'fixture java')
            with patch('provenance.subprocess.run', return_value=java):
                info = capture(binary, root, classes, matched=False, native=True, score_scale=1.0)
            self.assertEqual(info['lucene_mode'], 'native')
            self.assertFalse(info['matched_collection_statistics'])
            self.assertEqual({Path(p).name for p in info['java_classes_sha256']},
                             {'DoQueryNative.class', 'DumpNativeLuceneResults.class'})
            with self.assertRaises(ValueError):
                capture(binary, root, classes, matched=True, native=True)

    def test_native_builder_checks_targets_and_leaves_count_unwrapped(self):
        original = """public class DoQuery {
            final Path indexDir = Paths.get(args[0]);
            final IndexSearcher searcher = new IndexSearcher(reader);
            searcher.setQueryCache(null);
            searcher.setSimilarity(new BM25Similarity(0.9f, 0.4f));
            Query query = queryParser.parse(query_str);
            count = searcher.count(query);
        }"""
        generated = native_source(original)
        self.assertIn('new BM25Similarity(1.2f, 0.75f)', generated)
        self.assertIn('command.startsWith("TOP_") && scoreScale != 1f', generated)
        self.assertIn('count = searcher.count(query);', generated)
        self.assertNotIn('MatchedStatisticsSearcher', generated)
        self.assertNotIn('2.2f', generated)
        with self.assertRaises(ValueError):
            native_source(original.replace('new BM25Similarity(0.9f, 0.4f)', 'changed upstream'))
