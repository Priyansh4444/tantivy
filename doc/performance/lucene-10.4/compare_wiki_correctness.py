#!/usr/bin/env python3
"""Compare live-doc exhaustive Tantivy results and external IDs with Lucene."""
import argparse
import json
import math
import subprocess
from pathlib import Path
from provenance import capture
from lucene_mode import (add_arguments, configuration, report, engine_commands,
                         batch_profile, validate_pair)

BENCH = Path(__file__).resolve().parent


SCORE_TOLERANCE = 2e-6

def score_close(a, b):
    return abs(a - b) <= SCORE_TOLERANCE * max(abs(a), abs(b), 1e-20)

def validate_result(row, engine):
    """Reject malformed/truncated dumps before dictionaries can hide duplicates."""
    count = row['count']
    if type(count) is not int or count < 0:
        raise ValueError(f'{engine}: invalid count {count!r}')
    docs = row['top100']
    if len(docs) != min(count, 100):
        raise ValueError(f'{engine}: expected {min(count, 100)} results, got {len(docs)}')
    ids = [doc['id'] for doc in docs]
    if any(not isinstance(id, str) or not id for id in ids) or len(set(ids)) != len(ids):
        raise ValueError(f'{engine}: empty/non-string/duplicate external IDs')
    scores = [doc['score'] for doc in docs]
    if any(type(score) not in (int, float) or not math.isfinite(score) for score in scores):
        raise ValueError(f'{engine}: non-finite/non-numeric score')
    if any(a < b for a, b in zip(scores, scores[1:])):
        raise ValueError(f'{engine}: results are not sorted by descending score')

def validate_tie_equivalent_top10(t, l):
    """Permit reordered/replaced IDs only when both scoring models resolve a tie.

    For a cutoff replacement, every removed and added document must have a
    known score in both dumps and be tied with both tenth-place thresholds.
    For an order inversion, both models must tie the inverted pair. A genuinely
    better document can therefore never be excused by checking only one model.
    The frozen suite additionally requires identical top-ten sets below.
    """
    ids = [[doc['id'] for doc in row['top100'][:10]] for row in (t, l)]
    scores = [{doc['id']: doc['score'] for doc in row['top100']} for row in (t, l)]
    if len(ids[0]) != len(ids[1]):
        raise ValueError('different top-ten lengths')
    if not ids[0]:
        return
    selected = set(ids[0]) | set(ids[1])
    if any(not selected <= model.keys() for model in scores):
        raise ValueError('missing cross-engine score evidence for a cutoff document')
    replaced = set(ids[0]) ^ set(ids[1])
    for model, ranking in zip(scores, ids):
        cutoff = model[ranking[-1]]
        if any(not score_close(model[id], cutoff) for id in replaced):
            raise ValueError('different top-ten membership outside a cutoff tie')
    common = set(ids[0]) & set(ids[1])
    other_positions = {id: i for i, id in enumerate(ids[1])}
    for i, a in enumerate(ids[0]):
        if a not in common:
            continue
        for b in ids[0][i + 1:]:
            if b in common and other_positions[a] > other_positions[b]:
                if any(not score_close(model[a], model[b]) for model in scores):
                    raise ValueError(f'score-resolved order inversion: {a!r}, {b!r}')

def compare_results(query, t, l):
    validate_result(t, 'Tantivy')
    validate_result(l, 'Lucene')
    if t['count_matches_exhaustive'] is not True or t['ranking_matches_exhaustive'] is not True:
        raise ValueError(f'{query}: Tantivy disagrees with exhaustive alive-doc scoring')
    if t['count'] != l['count']:
        raise ValueError(f'{query}: COUNT differs: {t["count"]} vs {l["count"]}')
    t_ids = [doc['id'] for doc in t['top100'][:10]]
    l_ids = [doc['id'] for doc in l['top100'][:10]]
    t_scores = {doc['id']: doc['score'] for doc in t['top100']}
    l_scores = {doc['id']: doc['score'] for doc in l['top100']}
    common = t_scores.keys() & l_scores.keys()
    max_relative_difference = max((abs(t_scores[id] - l_scores[id]) /
        max(abs(t_scores[id]), abs(l_scores[id]), 1e-20) for id in common), default=None)
    if t['count'] and (max_relative_difference is None or max_relative_difference > SCORE_TOLERANCE):
        raise ValueError(f'{query}: score difference {max_relative_difference}')
    validate_tie_equivalent_top10(t, l)
    if set(t_ids) != set(l_ids):
        raise ValueError(f'{query}: frozen-suite top-ten IDs differ: {t_ids!r} vs {l_ids!r}')
    return {'query': query, 'count': t['count'],
        'same_top10_id_set': True, 'same_top10_id_order': t_ids == l_ids,
        'top10_order_matches_within_score_ties': True,
        'common_top100_ids': len(common), 'max_relative_score_difference': max_relative_difference}

def run(command, queries, diagnostics=None):
    result = subprocess.run(command, input=''.join(query + '\n' for query in queries),
                            capture_output=True, text=True, timeout=180)
    if diagnostics is not None:
        diagnostics['stderr'] = result.stderr.strip()
    if result.returncode:
        raise RuntimeError(f'{command[0]} failed: {result.stderr[-4000:]}')
    rows = [json.loads(line) for line in result.stdout.splitlines()]
    if len(rows) != len(queries) or [row['query'] for row in rows] != queries:
        raise ValueError('engine dump is incomplete or query rows differ from the requested suite')
    return rows

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--tantivy-validator', type=Path, required=True)
    parser.add_argument('--tantivy-index', type=Path, required=True)
    parser.add_argument('--lucene-dir', type=Path, required=True)
    parser.add_argument('--lucene-classes', type=Path, default=BENCH/'classes')
    parser.add_argument('--commit', required=True)
    parser.add_argument('--output', type=Path, required=True)
    add_arguments(parser, matched_default=True)
    args = parser.parse_args()
    scoring = configuration(args, parser)
    lucene_dir = args.lucene_dir.resolve()
    queries = [json.loads(line)['query'] for line in (BENCH/'queries-wiki.jsonl').read_text().splitlines()]
    commands = engine_commands(scoring, args.tantivy_validator, args.tantivy_index,
                               lucene_dir, task='dump')
    tantivy_diagnostics, lucene_diagnostics = {}, {}
    tantivy = run(commands['tantivy'], queries, tantivy_diagnostics)
    lucene = run(commands['lucene'], queries, lucene_diagnostics)
    receipts = {}
    if scoring['profile'] is not None:
        receipts = {name: batch_profile(diag['stderr'], scoring['profile'], name)
                    for name, diag in [('tantivy', tantivy_diagnostics), ('lucene', lucene_diagnostics)]}
        validate_pair(receipts)
    comparisons = []
    for query, t, l in zip(queries, tantivy, lucene):
        comparison = compare_results(query, t, l)
        comparisons.append(comparison)
        print(f'{query:22} COUNT={t["count"]} internal=PASS '
              f'top10-id-set={comparison["same_top10_id_set"]} '
              f'common-top100={comparison["common_top100_ids"]} '
              f'max-score-diff={comparison["max_relative_score_difference"]}')
    args.output.write_text(json.dumps({'commit': args.commit,
        **report(scoring),
        'engine_commands':commands,
        **({'profile_receipts':receipts, 'tantivy_diagnostics':tantivy_diagnostics} if receipts else {}),
        'tantivy_index': str(args.tantivy_index),
        'lucene_dir': str(lucene_dir), 'lucene_classes': str(args.lucene_classes.resolve()), 'comparisons': comparisons,
        'tantivy': tantivy, 'lucene': lucene, 'lucene_diagnostics': lucene_diagnostics,
        'provenance':capture(args.tantivy_validator, lucene_dir, args.lucene_classes, scoring['matched_bm25'],
                             native=scoring['native_bm25'], score_scale=scoring['lucene_score_scale'],
                             configured=scoring['profile'] is not None)}, indent=2)+'\n')

if __name__ == '__main__':
    main()
