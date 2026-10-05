#!/usr/bin/env python3
"""Tiny real-index physical order, canonical postings and corruption witnesses."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import struct

import wiki_docorder as order
from compare_wiki_correctness import compare_results

HERE = Path(__file__).resolve().parent


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binaries', type=Path, required=True)
    parser.add_argument('--jars', type=Path, required=True)
    parser.add_argument('--reverse-witness', type=Path, required=True,
                        help='Actual indexes from the build_index reversed-merge Rust test')
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    output = args.output.resolve(); output.mkdir(parents=True)
    bins = args.binaries.resolve(); jars = args.jars.resolve()
    records = []

    def run(name, command, stdin=None, *, success=True):
        result = subprocess.run(list(map(str, command)), input=stdin, capture_output=True, timeout=180)
        row = {'name': name, 'command': list(map(str, command)), 'returncode': result.returncode,
               'stdout': result.stdout.decode(), 'stderr': result.stderr.decode()}
        if stdin is not None:
            row['stdin_sha256'] = hashlib.sha256(stdin).hexdigest()
            row['stdin_bytes'] = len(stdin)
            (output/(name+'.stdin')).write_bytes(stdin)
        records.append(row)
        if success != (result.returncode == 0):
            raise AssertionError(row)
        return result

    def save(name, raw):
        path = output/name; path.write_bytes(raw); return path

    def no_receipt(index):
        if index.with_name(index.name+'.build.json').exists():
            raise AssertionError('failed builder emitted a success receipt')

    classes = output/'classes'; classes.mkdir()
    java_sources = ['BuildWikiFixture.java', 'DumpWikiLogical.java',
                    'DumpNativeLuceneResults.java', 'NativeBm25Profile.java']
    run('java-build', ['javac', '-cp', jars/'*', '-d', classes, *[HERE/name for name in java_sources]])
    java = ['java', '--add-modules', 'jdk.incubator.vector', '--enable-native-access=ALL-UNNAMED',
            '-cp', f'{classes}:{jars}/*']
    tie_docs = [{'id': f'tie{i:02}', 'text': 'alpha beta', 'sort_field': 9} for i in range(16)]
    high = {'id': 'high', 'text': 'alpha '*65000, 'sort_field': 2**64-1}
    empty = {'id': 'empty', 'text': '', 'sort_field': 2**63+1}
    positions = {'id': 'positions', 'text': 'a b a', 'sort_field': 2**63+7}
    reference = [tie_docs[7], empty, tie_docs[3], high, positions] + [row for row in tie_docs if row['id'] not in {'tie07', 'tie03'}]
    source_rows = [high, *tie_docs[::-1], positions, empty]
    source_bytes = b''.join((json.dumps(row, separators=(', ', ': ')) + ('\r\n' if i % 2 else '\n')).encode()
                            for i, row in enumerate(source_rows))
    source = save('original.jsonl', source_bytes)
    reference_bytes = b''.join((json.dumps(row)+'\n').encode() for row in reference)
    lucene = output/'lucene.idx'
    run('java-reference-build', [*java, 'BuildWikiFixture', lucene, '--jsonl'], reference_bytes)
    lphysical = save('lucene.physical.tsv', run('java-physical', [*java, 'DumpWikiLogical', lucene, 'documents-physical']).stdout)
    lsorted = save('lucene.sorted.tsv', run('java-documents-legacy', [*java, 'DumpWikiLogical', lucene, 'documents']).stdout)
    lterms = save('lucene.terms.tsv', run('java-terms-legacy', [*java, 'DumpWikiLogical', lucene, 'terms']).stdout)
    lpostings = save('lucene.postings.json', run('java-postings', [*java, 'DumpWikiLogical', lucene, 'postings-digest']).stdout)
    map_rows = list(order.physical_rows(lphysical))
    assert len(map_rows) == len(reference) == 19
    assert next(row for row in map_rows if row.external_id == 'high').norm == 135
    assert next(row for row in map_rows if row.external_id == 'empty').norm == 0
    assert next(row for row in map_rows if row.external_id == 'high').sort == 2**64-1
    assert next(row for row in map_rows if row.external_id == 'empty').sort > 2**63
    replay = output/'ordered.jsonl'
    replay_result = order.replay(source, lphysical, replay, expected_docs=19,
                                expected_sha256=order.digest(source), fixture=True)
    replay_receipt = replay.with_name(replay.name+'.replay.json')
    by_id = {order.corpus_row(raw)[0]: raw for raw in source_bytes.splitlines(keepends=True)}
    assert replay.read_bytes() == b''.join(by_id[row.external_id] for row in map_rows)
    tantivy = output/'ordered.idx'
    built = run('ordered-build', [bins/'build_index', tantivy, '--fixture-docs', '19', '--ordered-batches', '7',
                                 '--physical-map', lphysical, '--replay-receipt', replay_receipt], replay.read_bytes())
    completion = json.loads(built.stdout)
    assert [row['documents'] for row in completion['ordered_replay']['batches']] == [7, 7, 5]
    assert completion['ordered_replay']['actual_raw_input_sha256'] == order.digest(replay)
    assert completion['ordered_replay']['physical_verified']
    tphysical = save('tantivy.physical.tsv', run('rust-physical', [bins/'dump_docmap', tantivy, '--physical']).stdout)
    tsorted = save('tantivy.sorted.tsv', run('rust-documents-legacy', [bins/'dump_docmap', tantivy]).stdout)
    tterms = save('tantivy.terms.tsv', run('rust-terms-legacy', [bins/'dump_termtotals', tantivy]).stdout)
    tpayload = save('tantivy.payload.json', run('rust-payload', [bins/'payload_identity', tantivy]).stdout)
    assert tsorted.read_bytes() == lsorted.read_bytes()
    assert tterms.read_bytes() == lterms.read_bytes()
    physical_proof = order.compare_physical(tphysical, lphysical)
    payload_proof = order.compare_payload(tpayload, lpostings)
    assert physical_proof['documents'] == payload_proof['documents'] == 19
    assert physical_proof['field_doc_count'] == payload_proof['field_doc_count'] == 18
    assert payload_proof['derived_tokens'] == payload_proof['positions'] == 65035
    save('physical-comparison.json', (json.dumps(physical_proof, indent=2)+'\n').encode())
    save('payload-comparison.json', (json.dumps(payload_proof, indent=2)+'\n').encode())

    # Real >10 exact ties use actual physical docIDs in all three native profiles.
    tied = {}
    for name, bits in [('DEFAULT', []), ('k09-b04', ['3f666666', '3ecccccd']), ('k25-b1', ['40200000', '3f800000'])]:
        t = run('tie-rust-'+name, [bins/'validate_index', tantivy, *bits], b'beta\n')
        l = run('tie-java-'+name, [*java, 'DumpNativeLuceneResults', lucene, '1', *bits], b'beta\n')
        tr, lr = json.loads(t.stdout), json.loads(l.stdout)
        comparison = compare_results('beta', tr, lr)
        assert tr['count'] == 16 and lr['count'] == 16
        tbits = {struct.pack('!f', row['score']).hex() for row in tr['top100']}
        lbits = {struct.pack('!f', row['score']).hex() for row in lr['top100']}
        assert len(tbits) == len(lbits) == 1 and tbits == lbits
        assert [row['id'] for row in tr['top100'][:10]] == [row['id'] for row in lr['top100'][:10]]
        tied[name] = {'count': 16, 'exact_score_bits': next(iter(tbits)), 'comparison': comparison}

    rejected = []
    for name, mutation in [('position', 'a a b'), ('frequency', 'a b b'), ('norm', 'a b a a')]:
        changed = [dict(row, text=mutation) if row['id'] == 'positions' else row for row in reference]
        data = b''.join((json.dumps(row)+'\n').encode() for row in changed)
        path = output/(name+'.idx')
        run(name+'-build', [bins/'build_index', path, '--fixture-docs', '19'], data)
        physical = save(name+'.physical.tsv', run(name+'-physical', [bins/'dump_docmap', path, '--physical']).stdout)
        payload = save(name+'.payload.json', run(name+'-payload', [bins/'payload_identity', path]).stdout)
        if name == 'norm':
            try: order.compare_physical(physical, lphysical)
            except ValueError: pass
            else: raise AssertionError('changed norm was not detected')
        else:
            order.compare_physical(physical, lphysical)
        try: order.compare_payload(payload, lpostings)
        except ValueError: pass
        else: raise AssertionError('changed TF/positions were not detected')
        if name == 'position':
            assert run('position-term-statistics', [bins/'dump_termtotals', path]).stdout == tterms.read_bytes()
        rejected.append(name)
    changed = [dict(row, sort_field=1) if row['id'] == 'positions' else row for row in reference]
    path = output/'sort.idx'
    run('sort-build', [bins/'build_index', path, '--fixture-docs', '19'], b''.join((json.dumps(row)+'\n').encode() for row in changed))
    physical = save('sort.physical.tsv', run('sort-physical', [bins/'dump_docmap', path, '--physical']).stdout)
    try: order.compare_physical(physical, lphysical)
    except ValueError: rejected.append('sort')
    else: raise AssertionError('changed sort was not detected')

    # The Rust test built both actual merge orders through the public API.
    ordered_index, reversed_index = args.reverse_witness/'ordered.idx', args.reverse_witness/'reversed.idx'
    ordered_sorted = run('reverse-control-sorted', [bins/'dump_docmap', ordered_index]).stdout
    reversed_sorted = run('reverse-witness-sorted', [bins/'dump_docmap', reversed_index]).stdout
    assert ordered_sorted == reversed_sorted
    first = save('merge-control.physical.tsv', run('reverse-control-physical', [bins/'dump_docmap', ordered_index, '--physical']).stdout)
    second = save('merge-reversed.physical.tsv', run('reverse-witness-physical', [bins/'dump_docmap', reversed_index, '--physical']).stdout)
    try: order.compare_physical(first, second)
    except ValueError: rejected.append('reversed-merge')
    else: raise AssertionError('reversed merge was not detected')

    bad_cases = [('--ordered-batches', '0'), ('--memory-bytes', '5368709120')]
    common = ['--ordered-batches', '7', '--physical-map', lphysical, '--replay-receipt', replay_receipt]
    for i, (flag, value) in enumerate(bad_cases):
        path = output/f'bad-option-{i}.idx'
        options = common.copy()
        if flag in options: options[options.index(flag)+1] = value
        else: options += [flag, value]
        run('bad-option-'+str(i), [bins/'build_index', path, '--fixture-docs', '19', *options], replay.read_bytes(), success=False)
        assert not path.exists(); no_receipt(path)
    for name, data in [('changed-bytes', replay.read_bytes().replace(b'alpha', b'gamma', 1)),
                       ('changed-ending', replay.read_bytes().replace(b'\r\n', b'\n'))]:
        path = output/(name+'.idx')
        run(name, [bins/'build_index', path, '--fixture-docs', '19', *common], data, success=False)
        no_receipt(path); rejected.append(name)
    run('reused-index', [bins/'build_index', tantivy, '--fixture-docs', '19', *common], replay.read_bytes(), success=False)
    try: order.replay(source, lphysical, replay, expected_docs=19, expected_sha256=order.digest(source), fixture=True)
    except FileExistsError: rejected.append('reused-replay')
    else: raise AssertionError('reused replay accepted')

    # An oversized single row forces a batch to flush twice at the legal minimum.
    def word(value):
        letters = []
        for _ in range(5): letters.append(chr(97 + value % 26)); value //=26
        return ''.join(letters)
    heavy = [{'id': 'heavy', 'text': ' '.join(word(i) for i in range(250000)), 'sort_field': 0},
             {'id': 'tail', 'text': 'tail', 'sort_field': 1}]
    raw = b''.join((json.dumps(row)+'\n').encode() for row in heavy)
    heavy_source = save('heavy.jsonl', raw)
    heavy_map = save('heavy.map.tsv', f'0\theavy\t0\t{order.norm_byte(250000)}\n1\ttail\t1\t1\n'.encode())
    heavy_replay = output/'heavy-ordered.jsonl'
    order.replay(heavy_source, heavy_map, heavy_replay, expected_docs=2, expected_sha256=order.digest(heavy_source), fixture=True)
    heavy_index = output/'heavy.idx'
    failed = run('early-multiple-flush', [bins/'build_index', heavy_index, '--fixture-docs', '2', '--ordered-batches', '2',
                 '--physical-map', heavy_map, '--replay-receipt', heavy_replay.with_name(heavy_replay.name+'.replay.json'),
                 '--memory-bytes', '15000000'], heavy_replay.read_bytes(), success=False)
    assert b'exactly one new segment' in failed.stderr; no_receipt(heavy_index)
    rejected.append('early-multiple-flush')

    # Existing two-document Java fixture/default builder path still matches.
    legacy_java = output/'legacy-java.idx'
    run('legacy-java-fixture', [*java, 'BuildWikiFixture', legacy_java])
    legacy_t = output/'legacy-tantivy.idx'
    run('legacy-rust-fixture', [bins/'build_index', legacy_t, '--fixture-docs', '2'],
        b''.join((json.dumps(row)+'\n').encode() for row in [dict(empty, sort_field=0), dict(high, sort_field=1)]))
    assert run('legacy-java-map', [*java, 'DumpWikiLogical', legacy_java, 'documents']).stdout == run('legacy-rust-map', [bins/'dump_docmap', legacy_t]).stdout
    result = {'passed': True, 'fixture_documents': 19, 'field_doc_count': 18, 'tokens': 65035,
              'batch_sizes': [7, 7, 5], 'high_norm': 135, 'empty_norm': 0,
              'unsigned_max': 2**64-1, 'sort_ties': True, 'raw_crlf_preserved': True,
              'exact_tie_profiles': tied, 'physical_proof': physical_proof, 'payload_proof': payload_proof,
              'replay_receipt': replay_result, 'rejected': rejected, 'commands': records,
              'source_sha256': {name: order.digest(HERE/name) for name in [*java_sources, 'build_index.rs', 'dump_docmap.rs', 'shared/wiki_physical.rs', 'wiki_docorder.py', 'verify_wiki_docorder.py']},
              'binaries_sha256': {name: order.digest(bins/name) for name in ['build_index', 'dump_docmap', 'payload_identity', 'dump_termtotals', 'validate_index']},
              'classes_sha256': {path.name: order.digest(path) for path in classes.glob('*.class')},
              'jars_sha256': {path.name: order.digest(path) for path in jars.glob('*.jar')}}
    (output/'summary.json').write_text(json.dumps(result, indent=2)+'\n')
    print(json.dumps({key: result[key] for key in ('passed', 'fixture_documents', 'batch_sizes', 'physical_proof', 'payload_proof', 'rejected')}, indent=2))


if __name__ == '__main__':
    main()
