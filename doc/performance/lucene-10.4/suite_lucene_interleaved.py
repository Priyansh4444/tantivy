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

BENCH = Path(__file__).resolve().parent
QUERIES = [json.loads(line)['query'] for line in
           (Path(__file__).parent / 'queries-wiki.jsonl').read_text().splitlines()]

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
    ap.add_argument('--matched-bm25', action='store_true')
    args = ap.parse_args()
    postings_files = list(args.tantivy_index.glob('*.idx'))
    assert len(postings_files) == 1, 'Statistics matcher requires the frozen one-segment text index'
    with postings_files[0].open('rb') as postings:
        token_total = int.from_bytes(postings.read(8), 'little')
    lucene = args.lucene_dir.resolve()
    cp = f'{lucene / "build/classes/java/main"}:{lucene / "build/dependencies"}/*'
    if args.matched_bm25:
        cp = f'{args.lucene_classes.resolve()}:{cp}'
    lucene_class = 'DoQueryMatched' if args.matched_bm25 else 'DoQuery'
    pin = [] if args.cpu_core is None else ['taskset', '-c', str(args.cpu_core)]
    commands = {
        'tantivy': pin + [str(args.tantivy_binary),str(args.tantivy_index)],
        'lucene': pin + ['java','-XX:+UseParallelGC',
                   '--add-modules','jdk.incubator.vector','--enable-native-access=ALL-UNNAMED',
                   '-cp',cp,lucene_class,str(lucene/'idx')] + ([str(token_total)] if args.matched_bm25 else []),
    }
    names = ['lucene','tantivy'] if args.lucene_first else ['tantivy','lucene']
    procs = {name: start(commands[name]) for name in names}
    samples = {name: {q: [] for q in QUERIES} for name in names}
    counts = {}
    try:
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
            proc.stdin.close()
            proc.wait(timeout=10)
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
        'matched_bm25':args.matched_bm25,
        'matched_collection_statistics': args.matched_bm25, 'token_total': token_total,
        'tantivy_index':str(args.tantivy_index),
        'lucene_dir':str(lucene), 'lucene_classes':str(args.lucene_classes.resolve()),
        'cpu_core':args.cpu_core, 'engine_commands':commands,
        'provenance':capture(args.tantivy_binary, lucene, args.lucene_classes, args.matched_bm25)},indent=2)+'\n')

if __name__ == '__main__':
    main()
