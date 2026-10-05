#!/usr/bin/env python3
"""Snapshot warmed query-process memory, including mapped index pages."""
import argparse
import hashlib
import json
import subprocess
import time
from pathlib import Path
from lucene_mode import (add_arguments, configuration, report, engine_commands,
                         request_profile, validate_pair, stop_process)
from provenance import capture


def fields(path):
    return {key: value.strip() for line in path.read_text().splitlines()
            if ':' in line for key, value in [line.split(':', 1)]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--tantivy-binary', type=Path, required=True)
    parser.add_argument('--tantivy-index', type=Path, required=True)
    parser.add_argument('--lucene-dir', type=Path, required=True)
    parser.add_argument('--lucene-classes', type=Path, required=True)
    parser.add_argument('--query-file', type=Path, default=Path(__file__).with_name('queries-wiki.jsonl'))
    parser.add_argument('--cpu-core', default='4')
    parser.add_argument('--warmup-seconds', type=float, default=20)
    parser.add_argument('--output', type=Path, required=True)
    add_arguments(parser, matched_default=True)
    args = parser.parse_args()
    scoring = configuration(args, parser)
    queries = [json.loads(line)['query'] for line in args.query_file.read_text().splitlines()]
    prefix = [] if args.cpu_core == 'none' else ['taskset', '-c', args.cpu_core]
    commands = engine_commands(scoring, args.tantivy_binary, args.tantivy_index,
                               args.lucene_dir, pin=prefix)
    receipts = {}
    result = {
        'method': 'COUNT and TOP_10 over the same query suite, warmed serially per engine; '
                  'snapshot includes mmap pages; default JVM heap; query cache disabled. '
                  'This is not an indexing or universal peak-memory measurement.',
        'warmup_seconds': args.warmup_seconds,
        **report(scoring),
        'provenance': capture(args.tantivy_binary, args.lucene_dir, args.lucene_classes,
                              scoring['matched_bm25'], native=scoring['native_bm25'],
                              score_scale=scoring['lucene_score_scale'],
                             configured=scoring['profile'] is not None),
        'tantivy_binary_sha256': hashlib.sha256(args.tantivy_binary.read_bytes()).hexdigest(),
        'query_file_sha256': hashlib.sha256(args.query_file.read_bytes()).hexdigest(),
        'loadavg_start': Path('/proc/loadavg').read_text().strip(),
        'engine_commands': commands,
        'engines': {},
    }
    for name, command in commands.items():
        process = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                   stderr=subprocess.DEVNULL, text=True, bufsize=1)
        try:
            if scoring['profile'] is not None:
                receipts[name] = request_profile(process, scoring['profile'], name)
                if len(receipts) == 2:
                    validate_pair(receipts)
            start = time.monotonic()
            rounds = 0
            while time.monotonic() - start < args.warmup_seconds:
                for mode in ['COUNT', 'TOP_10']:
                    for query in queries:
                        process.stdin.write(f'{mode}\t{query}\n')
                        process.stdin.flush()
                        response = process.stdout.readline()
                        if not response:
                            raise RuntimeError(f'{name} exited during warmup')
                        # Reject delayed duplicate/control output rather than
                        # treating an arbitrary nonempty line as a query reply.
                        int(response)
                rounds += 1
            status = fields(Path(f'/proc/{process.pid}/status'))
            result['engines'][name] = {
                'rounds': rounds,
                'status': {key: status[key] for key in ['VmRSS', 'VmHWM', 'VmSize', 'RssAnon', 'RssFile']},
                'smaps_rollup': fields(Path(f'/proc/{process.pid}/smaps_rollup')),
            }
        finally:
            stop_process(process)
    if receipts:
        result['profile_receipts'] = receipts
    result['loadavg_end'] = Path('/proc/loadavg').read_text().strip()
    args.output.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps({name: data['status'] for name, data in result['engines'].items()}, indent=2))


if __name__ == '__main__':
    main()
