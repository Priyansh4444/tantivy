"""Explicit Lucene scoring modes shared by timing, correctness and memory tools."""
import math
from pathlib import Path

from tantivy_statistics import text_token_total


def add_arguments(parser, matched_default=False):
    modes = parser.add_mutually_exclusive_group()
    modes.add_argument('--matched-bm25', dest='bm25_mode', action='store_const', const='matched',
                       help='BM25 1.2/.75 with Tantivy collection statistics and score scale 2.2')
    modes.add_argument('--native-bm25', dest='bm25_mode', action='store_const', const='native',
                       help='BM25 1.2/.75 with unmodified Lucene collection statistics and scores')
    parser.set_defaults(bm25_mode='matched' if matched_default else 'adapter')
    parser.add_argument('--lucene-score-scale', type=float,
                        help='Explicit native-mode diagnostic TOP score scale (default 1)')


def configuration(args, parser):
    mode = args.bm25_mode
    if args.lucene_score_scale is not None and mode != 'native':
        parser.error('--lucene-score-scale requires --native-bm25')
    scale = args.lucene_score_scale if args.lucene_score_scale is not None else (2.2 if mode == 'matched' else 1.0)
    if not math.isfinite(scale) or scale <= 0:
        parser.error('--lucene-score-scale must be finite and positive')
    lucene = Path(args.lucene_dir).resolve()
    cp = f'{lucene / "build/classes/java/main"}:{lucene / "build/dependencies"}/*'
    if mode != 'adapter':
        cp = f'{Path(args.lucene_classes).resolve()}:{cp}'
    token_total = text_token_total(args.tantivy_index) if mode == 'matched' else None
    extra = [str(token_total)] if mode == 'matched' else ([str(scale)] if mode == 'native' else [])
    return {
        'classpath': cp,
        'query_class': {'native': 'DoQueryNative', 'matched': 'DoQueryMatched', 'adapter': 'DoQuery'}[mode],
        'dump_class': 'DumpNativeLuceneResults' if mode == 'native' else 'DumpLuceneResults',
        'extra_args': extra,
        'mode': mode, 'native_bm25': mode == 'native', 'matched_bm25': mode == 'matched',
        'matched_collection_statistics': mode == 'matched', 'token_total': token_total,
        'lucene_score_scale': scale,
        'lucene_bm25': {'k1': .9 if mode == 'adapter' else 1.2,
                        'b': .4 if mode == 'adapter' else .75},
    }


def report(config):
    return {key: config[key] for key in ('mode', 'native_bm25', 'matched_bm25',
        'matched_collection_statistics', 'token_total', 'lucene_score_scale', 'lucene_bm25')}
