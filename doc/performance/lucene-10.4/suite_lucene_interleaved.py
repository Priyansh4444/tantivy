#!/usr/bin/env python3
"""Alternating Tantivy and Lucene requests over the fixed Wiki 1M query suite."""
import argparse
import json
import math
import random
import statistics
import subprocess
import time
from pathlib import Path
from provenance import capture
from lucene_mode import (add_arguments, configuration, report, engine_commands,
                         request_profile, validate_pair, stop_process)

BENCH = Path(__file__).resolve().parent

def cpu_core(value):
    if value.lower() == 'none':
        return None
    core = int(value)
    if core < 0:
        raise argparse.ArgumentTypeError('CPU core must be nonnegative or none')
    return core

def start(cmd):
    return subprocess.Popen(cmd, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                            stderr=subprocess.DEVNULL, text=True, bufsize=1)

def query(proc, q, command):
    begin = time.perf_counter_ns()
    proc.stdin.write(f'{command}\t{q}\n')
    proc.stdin.flush()
    count = int(proc.stdout.readline())
    return count, (time.perf_counter_ns() - begin) / 1000

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--tantivy-binary', type=Path, required=True)
    ap.add_argument('--tantivy-index', type=Path, required=True)
    ap.add_argument('--lucene-dir', type=Path, required=True)
    ap.add_argument('--lucene-classes', type=Path, default=BENCH/'classes')
    ap.add_argument('--cpu-core', type=cpu_core, default=4, metavar='CORE|none')
    ap.add_argument('--output', type=Path, required=True)
    ap.add_argument('--warmup-seconds', type=int, default=40)
    ap.add_argument('--iterations', type=int, default=128)
    ap.add_argument('--lucene-first', action='store_true')
    ap.add_argument('--command', choices=['COUNT', 'TOP_10'], required=True)
    add_arguments(ap)
    args = ap.parse_args()
    scoring = configuration(args, ap)
    QUERIES = [json.loads(line)['query'] for line in
               (BENCH/'queries-wiki.jsonl').read_text().splitlines()]
    lucene = args.lucene_dir.resolve()
    pin = [] if args.cpu_core is None else ['taskset', '-c', str(args.cpu_core)]
    commands = engine_commands(scoring, args.tantivy_binary, args.tantivy_index, lucene, pin=pin)
    names = ['lucene','tantivy'] if args.lucene_first else ['tantivy','lucene']
    procs = {}
    receipts = {}
    samples = {name: {q: [] for q in QUERIES} for name in names}
    counts = {}
    try:
        for name in names:
            procs[name] = start(commands[name])
            if scoring['profile'] is not None:
                receipts[name] = request_profile(procs[name], scoring['profile'], name)
        if receipts:
            validate_pair(receipts)
        i = 0
        until = time.monotonic() + args.warmup_seconds
        while time.monotonic() < until:
            name = names[i % 2]
            q = QUERIES[(i // 2) % len(QUERIES)]
            count, _ = query(procs[name], q, args.command)
            if q in counts:
                assert counts[q] == count, (q, counts[q], count)
            counts[q] = count
            i += 1
        rng = random.Random(23)
        for iteration in range(args.iterations):
            order = QUERIES.copy()
            rng.shuffle(order)
            for j, q in enumerate(order):
                pair = names if (iteration + j) % 2 == 0 else names[::-1]
                for name in pair:
                    count, us = query(procs[name], q, args.command)
                    assert count == counts[q], (q, name, count)
                    samples[name][q].append(us)
    finally:
        for proc in procs.values():
            stop_process(proc)
    medians = {name:{q:statistics.median(v) for q,v in values.items()}
               for name,values in samples.items()}
    ratios=[]
    for q in QUERIES:
        t=medians['tantivy'][q]; l=medians['lucene'][q]; ratio=t/l
        ratios.append(ratio)
        print(f'{q:20} Tantivy={t:.1f} Lucene={l:.1f} T/L={ratio:.3f}')
    overall=math.exp(sum(map(math.log,ratios))/len(ratios))
    print('geomean T/L',overall)
    args.output.write_text(json.dumps({'queries':QUERIES,'counts':counts,'command':args.command,'medians_us':medians,
        'samples_us':samples,'geomean_ratio':overall,'iterations':args.iterations,
        'warmup_seconds':args.warmup_seconds,'lucene_first':args.lucene_first,
        **report(scoring),
        **({'profile_receipts':receipts} if receipts else {}),
        'tantivy_index':str(args.tantivy_index),
        'lucene_dir':str(lucene), 'lucene_classes':str(args.lucene_classes.resolve()),
        'cpu_core':args.cpu_core, 'engine_commands':commands,
        'provenance':capture(args.tantivy_binary, lucene, args.lucene_classes, scoring['matched_bm25'],
                             native=scoring['native_bm25'], score_scale=scoring['lucene_score_scale'],
                             configured=scoring['profile'] is not None)},indent=2)+'\n')

if __name__ == '__main__':
    main()
