"""Explicit Lucene scoring modes shared by timing, correctness and memory tools."""
import math
import json
import os
import re
import selectors
import struct
import subprocess
import time
from dataclasses import dataclass
from pathlib import Path

from tantivy_statistics import text_token_total


def add_arguments(parser, matched_default=False):
    modes = parser.add_mutually_exclusive_group()
    modes.add_argument('--matched-bm25', dest='bm25_mode', action='store_const', const='matched',
                       help='BM25 1.2/.75 with Tantivy collection statistics and score scale 2.2')
    modes.add_argument('--native-bm25', dest='bm25_mode', action='store_const', const='native',
                       help='BM25 1.2/.75 with unmodified Lucene collection statistics and scores')
    parser.set_defaults(bm25_mode='matched' if matched_default else 'adapter')
    parser.add_argument('--bm25-profile', choices=['k09-b04', 'k25-b1'],
                        help='Paired native binary32 parameters; requires native mode and scale 1')
    parser.add_argument('--lucene-score-scale', type=float,
                        help='Explicit native-mode diagnostic TOP score scale (default 1)')


def configuration(args, parser):
    mode = args.bm25_mode
    if args.lucene_score_scale is not None and mode != 'native':
        parser.error('--lucene-score-scale requires --native-bm25')
    scale = args.lucene_score_scale if args.lucene_score_scale is not None else (2.2 if mode == 'matched' else 1.0)
    if not math.isfinite(scale) or scale <= 0:
        parser.error('--lucene-score-scale must be finite and positive')
    profile = None
    if args.bm25_profile is not None:
        if mode != 'native' or scale != 1.0:
            parser.error('--bm25-profile requires --native-bm25 with score scale 1')
        profile = PROFILES[args.bm25_profile]
    lucene = Path(args.lucene_dir).resolve()
    cp = f'{lucene / "build/classes/java/main"}:{lucene / "build/dependencies"}/*'
    if mode != 'adapter':
        cp = f'{Path(args.lucene_classes).resolve()}:{cp}'
    token_total = text_token_total(args.tantivy_index) if mode == 'matched' else None
    extra = [str(token_total)] if mode == 'matched' else ([str(scale)] if mode == 'native' else [])
    if profile is not None:
        extra += profile.arguments()
    return {
        'profile': profile,
        'tantivy_extra_args': profile.arguments() if profile else [],
        'classpath': cp,
        'query_class': {'native': 'DoQueryNative', 'matched': 'DoQueryMatched', 'adapter': 'DoQuery'}[mode],
        'dump_class': 'DumpNativeLuceneResults' if mode == 'native' else 'DumpLuceneResults',
        'extra_args': extra,
        'mode': mode, 'native_bm25': mode == 'native', 'matched_bm25': mode == 'matched',
        'matched_collection_statistics': mode == 'matched', 'token_total': token_total,
        'lucene_score_scale': scale,
        'lucene_bm25': ({'k1': profile.k1, 'b': profile.b} if profile else
                         {'k1': .9 if mode == 'adapter' else 1.2,
                          'b': .4 if mode == 'adapter' else .75}),
    }


def report(config):
    result = {key: config[key] for key in ('mode', 'native_bm25', 'matched_bm25',
        'matched_collection_statistics', 'token_total', 'lucene_score_scale', 'lucene_bm25')}

    if config['profile'] is not None:
        result['bm25_profile'] = config['profile'].requested()
    return result


@dataclass(frozen=True)
class NativeProfile:
    name: str
    supplied_k1: str
    supplied_b: str
    k1_bits: str
    b_bits: str

    def __post_init__(self):
        for bits in (self.k1_bits, self.b_bits):
            if re.fullmatch(r'[0-9a-f]{8}', bits) is None:
                raise ValueError('Expected eight lowercase hex digits')
        if not math.isfinite(self.k1) or self.k1 < 0:
            raise ValueError('k1 must be finite and nonnegative')
        if not math.isfinite(self.b) or not 0 <= self.b <= 1:
            raise ValueError('b must be finite and in [0,1]')

    @property
    def k1(self):
        return struct.unpack('!f', bytes.fromhex(self.k1_bits))[0]

    @property
    def b(self):
        return struct.unpack('!f', bytes.fromhex(self.b_bits))[0]

    def arguments(self):
        return [self.k1_bits, self.b_bits]

    def requested(self):
        return {'name': self.name,
                'supplied': {'k1': self.supplied_k1, 'b': self.supplied_b},
                'converted_expected': {'k1': self.k1, 'b': self.b,
                                       'k1_bits': self.k1_bits, 'b_bits': self.b_bits}}


# These fixed literals have independently known binary32 representations. No
# arbitrary decimal conversion contract is exposed by the CLI.
PROFILES = {
    'k09-b04': NativeProfile('k09-b04', '0.9', '0.4', '3f666666', '3ecccccd'),
    'k25-b1': NativeProfile('k25-b1', '2.5', '1', '40200000', '3f800000'),
}
RECEIPT_PREFIX = 'BM25_PROFILE\t'


def engine_commands(config, binary, index, lucene_dir, *, task='protocol', pin=()):
    if task not in ('protocol', 'dump'):
        raise ValueError('Unknown engine task')
    java = ['java'] + (['-XX:+UseParallelGC'] if task == 'protocol' else [])
    java += ['--add-modules', 'jdk.incubator.vector', '--enable-native-access=ALL-UNNAMED',
             '-cp', config['classpath'], config['query_class' if task == 'protocol' else 'dump_class'],
             str(Path(lucene_dir).resolve()/'idx')] + config['extra_args']
    return {'tantivy': list(pin) + [str(binary), str(index)] + config['tantivy_extra_args'],
            'lucene': list(pin) + java}


def validate_receipt(profile, engine, line):
    def unique_object(pairs):
        row = {}
        for key, value in pairs:
            if key in row:
                raise ValueError('Duplicate key in profile receipt')
            row[key] = value
        return row
    row = json.loads(line, object_pairs_hook=unique_object)
    if not isinstance(row, dict):
        raise ValueError('Profile receipt must be an object')
    required = {'protocol': 'bm25-profile-v1', 'engine': engine, 'field': 'text',
                'k1_bits': profile.k1_bits, 'b_bits': profile.b_bits,
                'scale_bits': '3f800000', 'collection_statistics': 'physical',
                'query_cache': 'disabled'}
    if any(row.get(key) != value for key, value in required.items()):
        raise ValueError(f'{engine}: profile receipt differs from configured native profile')
    for key in ('doc_count', 'sum_total_term_freq'):
        if type(row.get(key)) is not int or row[key] < 0:
            raise ValueError(f'{engine}: invalid physical {key}')
    return {'raw': line, 'observed': row,
            'policy_evidence': 'native/cache declarations are helper claims; bits and N/TTF are getters'}


def validate_pair(receipts):
    if set(receipts) != {'tantivy', 'lucene'}:
        raise ValueError('Both engine receipts are required')
    t, l = (receipts[name]['observed'] for name in ('tantivy', 'lucene'))
    if any(t[key] != l[key] for key in ('doc_count', 'sum_total_term_freq')):
        raise ValueError('Configured engines have different physical N/TTF')


def request_profile(process, profile, engine, *, timeout=15.0, max_bytes=4096):
    process.stdin.write('BM25_CONFIG\t\n')
    process.stdin.flush()
    deadline = time.monotonic() + timeout
    data = bytearray()
    with selectors.DefaultSelector() as selector:
        selector.register(process.stdout, selectors.EVENT_READ)
        while b'\n' not in data:
            remaining = deadline - time.monotonic()
            if remaining <= 0 or not selector.select(remaining):
                raise TimeoutError(f'{engine}: profile receipt deadline exceeded')
            chunk = os.read(process.stdout.fileno(), max_bytes + 1 - len(data))
            if not chunk:
                raise ValueError(f'{engine}: EOF before profile receipt')
            data.extend(chunk)
            if len(data) > max_bytes:
                raise ValueError(f'{engine}: profile receipt exceeds byte limit')
        line, extra = bytes(data).split(b'\n', 1)
        if extra or selector.select(0):
            raise ValueError(f'{engine}: unexpected extra profile output')
    return validate_receipt(profile, engine, line.decode('utf-8'))


def batch_profile(stderr, profile, engine):
    lines = [line[len(RECEIPT_PREFIX):] for line in stderr.splitlines()
             if line.startswith(RECEIPT_PREFIX)]
    if len(lines) != 1:
        raise ValueError(f'{engine}: expected one profile diagnostic')
    return validate_receipt(profile, engine, lines[0])


def stop_process(process):
    # Cleanup is bounded even when a child ignores EOF or fails during startup.
    try:
        process.stdin.close()
    except (BrokenPipeError, OSError, ValueError):
        pass
    try:
        process.wait(timeout=2)
    except subprocess.TimeoutExpired:
        process.terminate()
        try:
            process.wait(timeout=2)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=2)
    finally:
        process.stdout.close()
        if process.stderr is not None:
            process.stderr.close()
