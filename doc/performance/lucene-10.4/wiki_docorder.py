#!/usr/bin/env python3
"""Exact-byte corpus replay and observed physical/payload comparison, offline only."""
import argparse
from dataclasses import dataclass
import hashlib
import json
import os
from pathlib import Path
import re
import sqlite3
import struct

WIKI_SHA256 = '2b630549676f1c58a579017b6cd949e25115fe63989f9e75cadadc5c1a1a8238'
WIKI_DOCS = 1_000_000
WIKI_TOKENS = 294_827_020
WIKI_FIELD_DOCS = 917_578


def digest(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def strict_json(raw):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError(f'duplicate JSON key: {key}')
            result[key] = value
        return result
    return json.loads(raw, object_pairs_hook=unique)


def ascii_id(value):
    if not isinstance(value, str) or not value.isascii() or any(c in value for c in '\t\r\n'):
        raise ValueError('ID outside ASCII TSV domain')
    return value


def unsigned(raw, maximum):
    if re.fullmatch(r'0|[1-9][0-9]*', raw) is None or int(raw) > maximum:
        raise ValueError('invalid canonical unsigned integer')
    return int(raw)


@dataclass(frozen=True)
class PhysicalRow:
    doc_id: int
    external_id: str
    sort: int
    norm: int

    def wire(self):
        return f'{self.doc_id}\t{self.external_id}\t{self.sort}\t{self.norm}\n'.encode('ascii')


def physical_rows(path):
    seen = set()
    with Path(path).open('rb') as stream:
        for ordinal, raw in enumerate(stream):
            if not raw.endswith(b'\n') or raw.endswith(b'\r\n'):
                raise ValueError('physical map requires LF-terminated canonical rows')
            fields = raw[:-1].decode('ascii').split('\t')
            if len(fields) != 4:
                raise ValueError('physical map requires docID/ID/u64 sort/u8 norm')
            row = PhysicalRow(unsigned(fields[0], 2**32-1), ascii_id(fields[1]),
                              unsigned(fields[2], 2**64-1), unsigned(fields[3], 255))
            if row.doc_id != ordinal or row.external_id in seen:
                raise ValueError('physical docID gap/order or duplicate ID')
            seen.add(row.external_id)
            yield row


def norm_byte(length):
    # Lucene SmallFloat.intToByte4 for this overlap-free ASCII field.
    def int4(value):
        bits = value.bit_length()
        if bits < 4:
            return value
        shift = bits - 4
        return ((value >> shift) & 7) | ((shift + 1) << 3)
    free = 255 - int4(2**31-1)
    if not 0 <= length <= 2**31-1:
        raise ValueError('token count outside Lucene field-length domain')
    return length if length < free else free + int4(length-free)


def corpus_row(raw):
    row = strict_json(raw)
    if not isinstance(row, dict) or set(row) != {'id', 'text', 'sort_field'}:
        raise ValueError('row requires exactly id/text/sort_field')
    external_id = ascii_id(row['id'])
    text = row['text']
    if not isinstance(text, str) or re.fullmatch(r'[a-z ]*', text) is None:
        raise ValueError('text outside [a-z ]')
    words = text.split()
    if any(len(word) > 255 for word in words):
        raise ValueError('word longer than 255 bytes')
    sort = row['sort_field']
    if type(sort) is not int or not 0 <= sort < 2**64:
        raise ValueError('sort_field must be a u64 integer, never bool')
    return external_id, sort, len(words)


def expected_fixture(args, parser):
    fixture = args.fixture_docs is not None
    if fixture != (args.fixture_corpus_sha256 is not None):
        parser.error('--fixture-docs and --fixture-corpus-sha256 must be paired')
    if fixture:
        if args.fixture_docs < 1 or re.fullmatch('[0-9a-f]{64}', args.fixture_corpus_sha256) is None:
            parser.error('invalid fixture count/hash')
        return args.fixture_docs, args.fixture_corpus_sha256, True
    return WIKI_DOCS, WIKI_SHA256, False


def replay(corpus, physical_map, output, *, expected_docs=WIKI_DOCS,
           expected_sha256=WIKI_SHA256, fixture=False):
    corpus, physical_map, output = map(Path, (corpus, physical_map, output))
    receipt = output.with_name(output.name + '.replay.json')
    locator = output.with_name(output.name + '.locator.sqlite')
    if any(path.exists() for path in (output, receipt, locator)):
        raise FileExistsError('replay output/receipt/locator must all be absent')
    if expected_docs < 1 or re.fullmatch('[0-9a-f]{64}', expected_sha256) is None:
        raise ValueError('invalid expected corpus boundary')
    if not fixture and (expected_docs, expected_sha256) != (WIKI_DOCS, WIKI_SHA256):
        raise ValueError('full replay requires the frozen corpus count/hash')
    map_hash = digest(physical_map)
    # Exclusive creation also prevents a reused database from providing stale offsets.
    with locator.open('xb'):
        pass
    with sqlite3.connect(locator) as db:
        db.execute('CREATE TABLE source (id TEXT PRIMARY KEY, offset INTEGER, size INTEGER, '
                   'line_hash BLOB, replay_hash BLOB, sort TEXT, norm INTEGER, tokens INTEGER, '
                   'doc_id INTEGER UNIQUE)')
        source_hash = hashlib.sha256()
        documents = tokens = field_docs = byte_count = 0
        with corpus.open('rb') as stream:
            while raw := stream.readline():
                offset = byte_count
                byte_count += len(raw)
                if not raw.endswith(b'\n'):
                    raise ValueError('corpus row must end with newline')
                external_id, sort, count = corpus_row(raw)
                source_hash.update(raw)
                documents += 1
                tokens += count
                field_docs += count > 0
                if documents > expected_docs:
                    raise ValueError('corpus exceeds expected row count')
                # SQLite INTEGER is signed. Store the original u64 as decimal TEXT.
                db.execute('INSERT INTO source(id,offset,size,line_hash,sort,norm,tokens) VALUES(?,?,?,?,?,?,?)',
                           (external_id, offset, len(raw), hashlib.sha256(raw).digest(),
                            str(sort), norm_byte(count), count))
        if documents != expected_docs or source_hash.hexdigest() != expected_sha256:
            raise ValueError('corpus count/raw SHA-256 differs from frozen boundary')
        if not fixture and (tokens, field_docs) != (WIKI_TOKENS, WIKI_FIELD_DOCS):
            raise ValueError('full corpus physical N/TTF differs')
        mapped = 0
        for row in physical_rows(physical_map):
            found = db.execute('SELECT sort,norm,doc_id FROM source WHERE id=?', (row.external_id,)).fetchone()
            if found is None or found[2] is not None:
                raise ValueError('foreign or duplicate physical ID')
            if found[:2] != (str(row.sort), row.norm):
                raise ValueError('physical sort/norm differs from original row')
            db.execute('UPDATE source SET doc_id=? WHERE id=?', (row.doc_id, row.external_id))
            mapped += 1
        if mapped != documents or db.execute('SELECT count(*) FROM source WHERE doc_id IS NULL').fetchone()[0]:
            raise ValueError('physical map is not an exact corpus ID bijection')
        replay_hash = hashlib.sha256()
        replay_bytes = 0
        with corpus.open('rb') as source, output.open('xb') as destination:
            for external_id, offset, size, expected_line in db.execute(
                    'SELECT id,offset,size,line_hash FROM source ORDER BY doc_id'):
                source.seek(offset)
                raw = source.read(size)
                observed = hashlib.sha256(raw).digest()
                if observed != expected_line or corpus_row(raw)[0] != external_id:
                    raise ValueError('source bytes changed during exact-row replay')
                destination.write(raw)
                replay_hash.update(raw)
                replay_bytes += len(raw)
                db.execute('UPDATE source SET replay_hash=? WHERE id=?', (observed, external_id))
            destination.flush()
            os.fsync(destination.fileno())
        original_manifest = hashlib.sha256()
        replay_manifest = hashlib.sha256()
        for external_id, original, observed in db.execute('SELECT id,line_hash,replay_hash FROM source ORDER BY id'):
            framing = struct.pack('<Q', len(external_id)) + external_id.encode('ascii')
            original_manifest.update(framing + original)
            replay_manifest.update(framing + observed)
        if original_manifest.digest() != replay_manifest.digest() or replay_bytes != byte_count:
            raise ValueError('per-ID raw bytes or byte counts differ')
        if digest(corpus) != expected_sha256 or digest(physical_map) != map_hash:
            raise ValueError('source corpus/map changed during replay')
        if digest(output) != replay_hash.hexdigest():
            raise ValueError('observed replay output hash differs')
        result = {'protocol': 'wiki-physical-replay-v1', 'fixture_mode': fixture,
                  'corpus': str(corpus.resolve()), 'physical_map': str(physical_map.resolve()),
                  'replay': str(output.resolve()), 'documents': documents, 'tokens': tokens,
                  'field_doc_count': field_docs, 'source_bytes': byte_count, 'replay_bytes': replay_bytes,
                  'corpus_raw_sha256': expected_sha256, 'replay_raw_sha256': replay_hash.hexdigest(),
                  'physical_map_sha256': map_hash, 'source_id_lines_sha256': original_manifest.hexdigest(),
                  'replay_id_lines_sha256': replay_manifest.hexdigest(), 'exact_id_bijection': True,
                  'tool_sha256': digest(__file__)}
        with receipt.open('x') as stream:
            stream.write(json.dumps(result, indent=2) + '\n')
    return result


def compare_physical(tantivy, lucene):
    trows, lrows = physical_rows(tantivy), physical_rows(lucene)
    observed = hashlib.sha256()
    count = 0
    field_docs = 0
    while True:
        t, l = next(trows, None), next(lrows, None)
        if t is None or l is None:
            if t != l or count == 0:
                raise ValueError('physical map count differs or is empty')
            break
        if t != l:
            raise ValueError(f'actual physical tuple differs at docID {count}')
        observed.update(t.wire())
        count += 1
        field_docs += t.norm != 0
    return {'documents': count, 'field_doc_count': field_docs, 'physical_sha256': observed.hexdigest()}


def compare_payload(tantivy_path, lucene_path):
    tantivy = strict_json(Path(tantivy_path).read_bytes())
    lucene = strict_json(Path(lucene_path).read_bytes())
    logical = tantivy['logical']
    if (type(logical.get('max_doc')) is not int or not 0 < logical['max_doc'] < 2**32
            or type(logical.get('live_docs')) is not int or logical['max_doc'] != logical['live_docs']):
        raise ValueError('deleted Tantivy documents are unsupported')
    fields = [field for field in logical['fields'] if field['name'] == 'text']
    headers = [field for field in tantivy['metadata_headers'] if field['name'] == 'text']
    if len(fields) != 1 or len(headers) != 1 or lucene.get('protocol') != 'wiki-postings-v1':
        raise ValueError('missing or ambiguous actual text payload/header')
    payload = fields[0]['postings']
    keys = ('sha256', 'terms', 'postings', 'derived_tokens', 'positions', 'derived_field_docs')
    if re.fullmatch('[0-9a-f]{64}', payload['sha256']) is None:
        raise ValueError('invalid canonical postings digest')
    for key in keys:
        if key != 'sha256' and (type(payload[key]) is not int or payload[key] < 0):
            raise ValueError('untyped text postings counter')
        if type(lucene.get(key)) is not type(payload[key]) or lucene[key] != payload[key]:
            raise ValueError(f'actual text postings differ: {key}')
    if payload['positions'] != payload['derived_tokens'] or not 0 < payload['derived_field_docs'] <= logical['max_doc']:
        raise ValueError('positions/token/field population counters differ')
    if (type(headers[0].get('serialized_token_total')) is not int
            or type(lucene.get('documents')) is not int or lucene['documents'] != logical['max_doc']
            or type(lucene.get('field_doc_count')) is not int or lucene['field_doc_count'] != payload['derived_field_docs']
            or type(lucene.get('serialized_total_tokens')) is not int
            or lucene['serialized_total_tokens'] != headers[0]['serialized_token_total']
            or lucene['serialized_total_tokens'] != payload['derived_tokens']):
        raise ValueError('actual text population/token headers differ')
    return {'documents': logical['max_doc'], 'field_doc_count': payload['derived_field_docs'],
            'serialized_total_tokens': headers[0]['serialized_token_total'], **{key: payload[key] for key in keys}}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='command', required=True)
    replay_parser = commands.add_parser('replay')
    for name in ('corpus', 'physical-map', 'output'):
        replay_parser.add_argument('--' + name, type=Path, required=True)
    replay_parser.add_argument('--fixture-docs', type=int)
    replay_parser.add_argument('--fixture-corpus-sha256')
    comparison = commands.add_parser('compare')
    for name in ('tantivy-physical', 'lucene-physical', 'output'):
        comparison.add_argument('--' + name, type=Path, required=True)
    comparison.add_argument('--tantivy-payload', type=Path)
    comparison.add_argument('--lucene-postings', type=Path)
    args = parser.parse_args()
    if args.command == 'replay':
        count, expected_hash, fixture = expected_fixture(args, parser)
        result = replay(args.corpus, args.physical_map, args.output,
                        expected_docs=count, expected_sha256=expected_hash, fixture=fixture)
    else:
        if bool(args.tantivy_payload) != bool(args.lucene_postings):
            parser.error('--tantivy-payload and --lucene-postings must be paired')
        if args.output.exists():
            raise FileExistsError('comparison output must be absent')
        paths = [args.tantivy_physical, args.lucene_physical, args.tantivy_payload, args.lucene_postings]
        before = {str(path.resolve()): digest(path) for path in paths if path is not None}
        result = {'protocol': 'wiki-physical-comparison-v1', **compare_physical(args.tantivy_physical, args.lucene_physical)}
        result['postings_verified'] = args.tantivy_payload is not None
        if args.tantivy_payload:
            result['postings'] = compare_payload(args.tantivy_payload, args.lucene_postings)
            if any(result[key] != result['postings'][key] for key in ('documents', 'field_doc_count')):
                raise ValueError('actual physical and payload populations differ')
        after = {str(path.resolve()): digest(path) for path in paths if path is not None}
        if before != after:
            raise ValueError('comparison input changed during verification')
        result['inputs_sha256'] = before
        result['tool_sha256'] = digest(__file__)
        with args.output.open('x') as stream:
            stream.write(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()
