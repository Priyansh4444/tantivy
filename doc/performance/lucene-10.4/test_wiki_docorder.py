"""Strict corpus/map boundaries; actual index witnesses live in verify_wiki_docorder."""
import argparse
import contextlib
import hashlib
import io
import json
from pathlib import Path
import tempfile
import unittest
import sys
import sqlite3
from unittest.mock import patch

import wiki_docorder as order


class ReplayBoundaryTests(unittest.TestCase):
    def test_exact_bytes_unsigned_sorts_norms_and_permutation(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            raw = (b'{ "id": "z", "text": "alpha beta", "sort_field": 18446744073709551615 }\r\n'
                   b'{"id":"empty","text":"","sort_field":9223372036854775809}\n')
            corpus = root/'source'; corpus.write_bytes(raw)
            mapping = root/'map'; mapping.write_text('0\tempty\t9223372036854775809\t0\n1\tz\t18446744073709551615\t2\n')
            result = order.replay(corpus, mapping, root/'replay', expected_docs=2,
                                  expected_sha256=hashlib.sha256(raw).hexdigest(), fixture=True)
            self.assertEqual((root/'replay').read_bytes(), b''.join(raw.splitlines(keepends=True)[::-1]))
            self.assertEqual(result['source_id_lines_sha256'], result['replay_id_lines_sha256'])
            self.assertEqual(result['field_doc_count'], 1)
            self.assertTrue(result['fixture_mode'])
            with self.assertRaises(FileExistsError):
                order.replay(corpus, mapping, root/'replay', expected_docs=2,
                             expected_sha256=hashlib.sha256(raw).hexdigest(), fixture=True)

    def test_duplicate_keys_bool_sort_and_domain_fail(self):
        rows = [b'{"id":"a","id":"b","text":"a","sort_field":1}',
                b'{"id":"a","text":"a","sort_field":true}',
                b'{"id":"a","text":"A","sort_field":1}',
                b'{"id":"a","text":"a","sort_field":18446744073709551616}',
                b'{"id":"a","text":"a","sort_field":-1}',
                b'{"id":"a\\t","text":"a","sort_field":1}',
                b'{"id":"a","text":"a","sort_field":1,"other":2}']
        for raw in rows:
            with self.subTest(raw=raw), self.assertRaises(ValueError):
                order.corpus_row(raw)
        with self.assertRaises(ValueError):
            order.corpus_row(json.dumps({'id': 'a', 'text': 'x'*256, 'sort_field': 1}))

    def test_map_bijection_sort_norm_and_changed_raw_bytes_fail_without_receipt(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            raw = b'{"id":"a","text":"alpha","sort_field":9}\n{"id":"b","text":"beta","sort_field":8}\n'
            corpus = root/'source'; corpus.write_bytes(raw)
            cases = ['0\ta\t9\t1\n', '0\ta\t9\t1\n1\ta\t9\t1\n',
                     '0\ta\t9\t1\n1\tforeign\t8\t1\n', '0\ta\t8\t1\n1\tb\t8\t1\n',
                     '0\ta\t9\t2\n1\tb\t8\t1\n', '1\ta\t9\t1\n2\tb\t8\t1\n']
            for number, value in enumerate(cases):
                mapping = root/f'map{number}'; mapping.write_text(value)
                output = root/f'output{number}'
                with self.assertRaises(ValueError):
                    order.replay(corpus, mapping, output, expected_docs=2,
                                 expected_sha256=hashlib.sha256(raw).hexdigest(), fixture=True)
                self.assertFalse(output.with_name(output.name+'.replay.json').exists())
            mapping.write_text('0\ta\t9\t1\n1\tb\t8\t1\n')
            corpus.write_bytes(raw.replace(b'alpha', b'gamma'))
            with self.assertRaises(ValueError):
                order.replay(corpus, mapping, root/'changed', expected_docs=2,
                             expected_sha256=hashlib.sha256(raw).hexdigest(), fixture=True)

    def test_fixture_options_are_paired_and_full_boundary_is_frozen(self):
        parser = argparse.ArgumentParser()
        for count, value in [(1, None), (None, 'a'*64), (0, 'a'*64), (1, 'invalid')]:
            with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
                order.expected_fixture(argparse.Namespace(fixture_docs=count, fixture_corpus_sha256=value), parser)
        with self.assertRaises(ValueError):
            order.replay('/absent', '/absent', '/absent-output', expected_docs=2, expected_sha256='a'*64)

    def test_duplicate_source_ids_fail_before_replay_receipt(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            raw = b'{"id":"a","text":"alpha","sort_field":9}\n'*2
            corpus = root/'source'; corpus.write_bytes(raw)
            mapping = root/'map'; mapping.write_text('0\ta\t9\t1\n1\tb\t8\t1\n')
            with self.assertRaises(sqlite3.IntegrityError):
                order.replay(corpus, mapping, root/'output', expected_docs=2,
                             expected_sha256=hashlib.sha256(raw).hexdigest(), fixture=True)
            self.assertFalse((root/'output.replay.json').exists())
            self.assertFalse((root/'output').exists())

    def test_physical_map_wire_and_order_comparator_detect_permutation(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            first = root/'first'; second = root/'second'
            first.write_text('0\tb\t9\t2\n1\ta\t9\t2\n')
            second.write_bytes(first.read_bytes())
            self.assertEqual(order.compare_physical(first, second)['documents'], 2)
            second.write_text('0\ta\t9\t2\n1\tb\t9\t2\n')
            with self.assertRaises(ValueError):
                order.compare_physical(first, second)
            for row in ['00\ta\t9\t2\n', '0\ta\t+9\t2\n', '0\ta\t9\t256\n',
                        '0\ta\t9\t2\r\n', '0\ta\t9\t2', '0\ta\t18446744073709551616\t2\n']:
                second.write_text(row)
                with self.assertRaises(ValueError):
                    list(order.physical_rows(second))

    def test_native_norm_byte_controls(self):
        self.assertEqual(order.norm_byte(0), 0)
        self.assertEqual(order.norm_byte(2), 2)
        self.assertEqual(order.norm_byte(65000), 135)
        self.assertEqual(order.norm_byte(2**31-1), 255)

    def test_comparison_rejects_input_mutation_without_success_receipt(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            t, l, output = root/'t', root/'l', root/'receipt'
            raw = '0\ta\t9\t2\n1\tb\t9\t2\n'
            t.write_text(raw); l.write_text(raw)
            actual_compare = order.compare_physical
            def compare_then_mutate(*args):
                result = actual_compare(*args)
                t.write_text('0\tb\t9\t2\n1\ta\t9\t2\n')
                return result
            argv = ['tool', 'compare', '--tantivy-physical', str(t), '--lucene-physical', str(l), '--output', str(output)]
            with patch.object(sys, 'argv', argv), patch.object(order, 'compare_physical', side_effect=compare_then_mutate):
                with self.assertRaisesRegex(ValueError, 'changed during verification'):
                    order.main()
            self.assertFalse(output.exists())


class PayloadBoundaryTests(unittest.TestCase):
    def test_actual_payload_counters_headers_and_unique_text_required(self):
        payload = dict(sha256='a'*64, terms=2, postings=2, derived_tokens=3, positions=3, derived_field_docs=1)
        tantivy = {'logical': {'max_doc': 2, 'live_docs': 2, 'fields': [{'name': 'text', 'postings': payload}]},
                   'metadata_headers': [{'name': 'text', 'serialized_token_total': 3}]}
        lucene = dict(protocol='wiki-postings-v1', documents=2, field_doc_count=1, serialized_total_tokens=3, **payload)
        with tempfile.TemporaryDirectory() as tmp:
            t, l = Path(tmp)/'t', Path(tmp)/'l'
            t.write_text(json.dumps(tantivy)); l.write_text(json.dumps(lucene))
            self.assertEqual(order.compare_payload(t, l), dict(payload, documents=2,
                             field_doc_count=1, serialized_total_tokens=3))
            for key, value in [('positions', 2), ('postings', 3), ('terms', True),
                               ('sha256', 'b'*64), ('serialized_total_tokens', 4), ('field_doc_count', 2)]:
                l.write_text(json.dumps(dict(lucene, **{key: value})))
                with self.assertRaises(ValueError):
                    order.compare_payload(t, l)
            tantivy['logical']['fields'].append(tantivy['logical']['fields'][0])
            t.write_text(json.dumps(tantivy)); l.write_text(json.dumps(lucene))
            with self.assertRaises(ValueError):
                order.compare_payload(t, l)
