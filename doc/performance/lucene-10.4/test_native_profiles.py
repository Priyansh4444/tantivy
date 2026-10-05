"""Configured boundaries and real pipe/lifecycle failures; no benchmark timing."""
import argparse
import contextlib
import io
import json
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path
from unittest.mock import patch

import compare_process_memory as memory
import compare_wiki_correctness as correctness
import lucene_mode as mode
import prepare_native_lucene as prepare
import suite_lucene_interleaved as latency

HERE = Path(__file__).resolve().parent


def parser(matched=False):
    result = argparse.ArgumentParser()
    result.set_defaults(lucene_dir=Path('/lucene'), lucene_classes=Path('/classes'),
                        tantivy_index=Path('/index'))
    mode.add_arguments(result, matched)
    return result


def receipt(engine='tantivy', **changes):
    result = dict(protocol='bm25-profile-v1', engine=engine, field='text',
                  k1_bits='3f666666', b_bits='3ecccccd', scale_bits='3f800000',
                  collection_statistics='physical', query_cache='disabled',
                  doc_count=2, sum_total_term_freq=5)
    result.update(changes)
    return json.dumps(result)


def child(code):
    return subprocess.Popen([sys.executable, '-u', '-c', code], stdin=subprocess.PIPE,
                            stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)


class ProfileBoundaryTests(unittest.TestCase):
    def test_presets_exact_bits_and_shared_commands(self):
        p = parser()
        for name, pair in [('k09-b04', ['3f666666', '3ecccccd']),
                           ('k25-b1', ['40200000', '3f800000'])]:
            args = p.parse_args(['--native-bm25', '--bm25-profile', name])
            with patch('lucene_mode.text_token_total', side_effect=AssertionError('header IO')):
                config = mode.configuration(args, p)
            commands = mode.engine_commands(config, '/bin', '/index', '/lucene', pin=['taskset', '-c', '4'])
            self.assertEqual(commands['tantivy'], ['taskset', '-c', '4', '/bin', '/index', *pair])
            self.assertEqual(commands['lucene'][-4:], ['/lucene/idx', '1.0', *pair])
            self.assertEqual(mode.report(config)['bm25_profile']['converted_expected']['k1_bits'], pair[0])
            dumped = mode.engine_commands(config, '/validator', '/index', '/lucene', task='dump')
            self.assertIn('DumpNativeLuceneResults', dumped['lucene'])
            self.assertNotIn('-XX:+UseParallelGC', dumped['lucene'])
            self.assertEqual(dumped['tantivy'], ['/validator', '/index', *pair])

    def test_whole_legacy_commands_and_reports(self):
        p = parser()
        cases = [([], 'DoQuery', []), (['--matched-bm25'], 'DoQueryMatched', ['123']),
                 (['--native-bm25'], 'DoQueryNative', ['1.0']),
                 (['--native-bm25', '--lucene-score-scale', '2.2'], 'DoQueryNative', ['2.2'])]
        for flags, klass, extra in cases:
            with patch('lucene_mode.text_token_total', return_value=123):
                config = mode.configuration(p.parse_args(flags), p)
            expected_java = ['java', '-XX:+UseParallelGC', '--add-modules', 'jdk.incubator.vector',
                             '--enable-native-access=ALL-UNNAMED', '-cp', config['classpath'],
                             klass, '/lucene/idx', *extra]
            self.assertEqual(mode.engine_commands(config, '/bin', '/index', '/lucene'),
                             {'tantivy': ['/bin', '/index'], 'lucene': expected_java})
            self.assertNotIn('bm25_profile', mode.report(config))
            self.assertIsNone(config['profile'])

    def test_invalid_profiles_reject_before_header_or_path_resolution(self):
        p = parser()
        flags = [['--bm25-profile', 'k09-b04'],
                 ['--matched-bm25', '--bm25-profile', 'k25-b1'],
                 ['--native-bm25', '--bm25-profile', 'k09-b04', '--lucene-score-scale', '2.2']]
        for argv in flags:
            args = p.parse_args(argv)
            with (patch('lucene_mode.text_token_total', side_effect=AssertionError('header')),
                 patch('lucene_mode.Path.resolve', side_effect=AssertionError('path IO')),
                 contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit)):
                mode.configuration(args, p)
        for argv in [['--native-bm25', '--bm25-profile', 'default'],
                     ['--native-bm25', '--bm25-profile', 'custom'],
                     ['--native-bm25', '--bm25-k1', '0.9']]:
            with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
                p.parse_args(argv)

    def test_all_callers_validate_before_query_or_artifact_io(self):
        common = ['--tantivy-index', '/absent', '--lucene-dir', '/absent',
                  '--lucene-classes', '/absent', '--output', '/absent',
                  '--matched-bm25', '--bm25-profile', 'k09-b04']
        cases = [(latency, ['--tantivy-binary', '/absent', '--command', 'COUNT']),
                 (correctness, ['--tantivy-validator', '/absent', '--commit', 'fixture']),
                 (memory, ['--tantivy-binary', '/absent'])]
        for module, required in cases:
            with (patch.object(sys, 'argv', ['tool', *common, *required]),
                 patch.object(Path, 'read_text', side_effect=AssertionError('query IO')),
                 patch.object(subprocess, 'Popen', side_effect=AssertionError('spawn')),
                 patch('lucene_mode.text_token_total', side_effect=AssertionError('header')),
                 contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit)):
                module.main()

    def test_binary32_domain_without_decimal_conversion_claim(self):
        for pair in [('80000000', '80000000'), ('00000001', '3f800000'), ('7f7fffff', '00000000')]:
            value = mode.NativeProfile('fixture', '-0', '-0', *pair)
            self.assertEqual(value.arguments(), list(pair))
        for pair in [('7f800000', '00000000'), ('7fc00000', '00000000'),
                     ('bf800000', '00000000'), ('3f800000', '3f800001'),
                     ('3F800000', '00000000'), ('1', '00000000')]:
            with self.assertRaises(ValueError):
                mode.NativeProfile('fixture', 'fixture', 'fixture', *pair)


class ReceiptTests(unittest.TestCase):
    def test_getter_receipts_and_pair_statistics(self):
        profile = mode.PROFILES['k09-b04']
        rows = {engine: mode.validate_receipt(profile, engine, receipt(engine))
                for engine in ['tantivy', 'lucene']}
        mode.validate_pair(rows)
        rows['lucene'] = mode.validate_receipt(profile, 'lucene', receipt('lucene', doc_count=3))
        with self.assertRaises(ValueError):
            mode.validate_pair(rows)
        stderr = 'JVM warning\n' + mode.RECEIPT_PREFIX + receipt() + '\nother diagnostic\n'
        self.assertEqual(mode.batch_profile(stderr, profile, 'tantivy')['observed']['doc_count'], 2)
        with self.assertRaises(ValueError):
            mode.batch_profile(stderr + stderr, profile, 'tantivy')

    def test_malformed_mismatched_or_untyped_receipts(self):
        profile = mode.PROFILES['k09-b04']
        for line in ['[]', '{}', 'not-json', receipt(engine='lucene'), receipt(field='id'),
                     receipt(protocol='old'), receipt(k1_bits='3f99999a'), receipt(doc_count=True),
                     receipt()[:-1] + ',"doc_count":2}',
                     receipt(sum_total_term_freq=-1), receipt(scale_bits='400ccccd')]:
            with self.assertRaises((ValueError, json.JSONDecodeError)):
                mode.validate_receipt(profile, 'tantivy', line)

    def test_complete_line_deadline_eof_stale_and_byte_limit(self):
        profile = mode.PROFILES['k09-b04']
        cases = ["import sys,time;sys.stdin.readline();sys.stdout.write('{');sys.stdout.flush();time.sleep(30)",
                 "import sys;sys.stdin.readline()", # EOF
                 "import sys,time;sys.stdin.readline();print(1);time.sleep(30)", # stale numeric helper
                 "import sys,time;sys.stdin.readline();print('x'*5000);time.sleep(30)"]
        for code in cases:
            proc = child(code)
            try:
                begin = time.monotonic()
                with self.assertRaises((ValueError, TimeoutError)):
                    mode.request_profile(proc, profile, 'tantivy', timeout=.15)
                self.assertLess(time.monotonic() - begin, 1.0)
            finally:
                mode.stop_process(proc)
            self.assertIsNotNone(proc.poll())

    def test_success_and_duplicate_control_output(self):
        profile = mode.PROFILES['k09-b04']
        for duplicate in (False, True):
            output = receipt() + ('\n' + receipt() if duplicate else '')
            proc = child(f"import sys;sys.stdin.readline();print({output!r},flush=True);sys.stdin.read()")
            try:
                if duplicate:
                    with self.assertRaises(ValueError):
                        mode.request_profile(proc, profile, 'tantivy', timeout=1)
                else:
                    actual = mode.request_profile(proc, profile, 'tantivy', timeout=1)
                    self.assertEqual(actual['observed']['k1_bits'], profile.k1_bits)
            finally:
                mode.stop_process(proc)

    def test_second_start_failure_and_receipt_failure_reap_first_child(self):
        common = ['tool', '--tantivy-binary', '/absent', '--tantivy-index', '/absent',
                  '--lucene-dir', '/absent', '--lucene-classes', '/absent',
                  '--output', '/absent', '--command', 'COUNT']
        for configured in (False, True):
            proc = child('import sys;sys.stdin.read()')
            try:
                argv = common + (['--native-bm25', '--bm25-profile', 'k09-b04'] if configured else [])
                with (patch.object(sys, 'argv', argv),
                     patch.object(latency, 'start', side_effect=[proc, OSError('second startup')]),
                     patch.object(latency, 'request_profile', side_effect=ValueError('bad receipt'))):
                    with self.assertRaises((OSError, ValueError)):
                        latency.main()
                self.assertIsNotNone(proc.poll())
            finally:
                if proc.poll() is None:
                    mode.stop_process(proc)

    def test_delayed_duplicate_fails_memory_warmup_and_reaps_child(self):
        # A second receipt arriving only after a query must not pass as a
        # nonempty COUNT reply and produce a misleading memory result.
        proc = child(f"import sys;sys.stdin.readline();print({receipt()!r},flush=True);"
                     f"sys.stdin.readline();print({receipt()!r},flush=True);sys.stdin.read()")
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            binary = root/'binary'; binary.write_bytes(b'fixture')
            queries = root/'queries'; queries.write_text('{"query":"alpha"}\n')
            output = root/'result.json'
            argv = ['tool', '--tantivy-binary', str(binary), '--tantivy-index', '/absent',
                    '--lucene-dir', '/absent', '--lucene-classes', '/absent',
                    '--query-file', str(queries), '--output', str(output), '--cpu-core', 'none',
                    '--native-bm25', '--bm25-profile', 'k09-b04']
            try:
                with (patch.object(sys, 'argv', argv), patch.object(memory, 'capture', return_value={}),
                      patch.object(memory.subprocess, 'Popen', return_value=proc)):
                    with self.assertRaises(ValueError):
                        memory.main()
                self.assertFalse(output.exists())
                self.assertIsNotNone(proc.poll())
            finally:
                mode.stop_process(proc)

    def test_cleanup_is_bounded_and_closes_all_pipes(self):
        proc = subprocess.Popen([sys.executable, '-u', '-c', 'import time;time.sleep(30)'],
                                stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                stderr=subprocess.PIPE, text=True)
        begin = time.monotonic()
        mode.stop_process(proc)
        self.assertLess(time.monotonic() - begin, 5)
        self.assertIsNotNone(proc.poll())
        self.assertTrue(all(pipe.closed for pipe in (proc.stdin, proc.stdout, proc.stderr)))
        mode.stop_process(proc)


class PreparationTests(unittest.TestCase):
    def test_configured_anchors_and_main_always_uses_strict_generation(self):
        source = '''public class DoQuery {
final Path indexDir = Paths.get(args[0]);
new IndexSearcher(reader);
searcher.setQueryCache(null);
searcher.setSimilarity(new BM25Similarity(0.9f, 0.4f));
final String[] fields = line.trim().split("\\t");
Query query = queryParser.parse(query_str);
count = searcher.count(query);
}'''
        generated = prepare.native_source(source, configured=True)
        self.assertLess(generated.index('line.equals("BM25_CONFIG\\t")'), generated.index('line.trim().split'))
        self.assertLess(generated.index('NativeBm25Profile.parse(args)'), generated.index('Paths.get(args[0])'))
        self.assertIn('new BM25Similarity(1.2f, 0.75f)', generated)
        for damaged in [source.replace('line.trim()', 'line.strip()'), source + source]:
            with self.assertRaises(ValueError):
                prepare.native_source(damaged, configured=True)
        with tempfile.TemporaryDirectory() as tmp:
            lucene = Path(tmp)/'lucene'; output = Path(tmp)/'classes'
            game = lucene/'src/main/java/DoQuery.java'; game.parent.mkdir(parents=True); game.write_text(source)
            def compile_sources(command, *, check):
                self.assertTrue(check)
                for name in ('DoQueryNative', 'DumpNativeLuceneResults', 'NativeBm25Profile'):
                    (output/f'{name}.class').write_bytes(f'compiled {name}'.encode())
            with (patch.object(sys, 'argv', ['prepare', '--lucene-dir', str(lucene), '--output-dir', str(output), '--fresh-output']),
                 patch.object(prepare, 'native_source', wraps=prepare.native_source) as rewrite,
                 patch.object(prepare.subprocess, 'run', side_effect=compile_sources) as compile_java):
                prepare.main()
                rewrite.assert_called_once_with(source, configured=True)
                self.assertIn(str(output/'NativeBm25Profile.java'), compile_java.call_args.args[0])
                self.assertIn('NativeBm25Profile.class', json.loads((output/'native-build.json').read_text())['classes_sha256'])
                with self.assertRaises(FileExistsError):
                    prepare.main()
