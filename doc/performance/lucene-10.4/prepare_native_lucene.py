#!/usr/bin/env python3
"""Build native Lucene BM25 protocol/dump classes without collection-stat overrides."""
import argparse
import hashlib
import json
import subprocess
from pathlib import Path

HERE = Path(__file__).resolve().parent


def native_source(source, *, configured=False):
    replacements = [
        ('public class DoQuery {', 'public class DoQueryNative {'),
        ('new BM25Similarity(0.9f, 0.4f)', 'new BM25Similarity(1.2f, 0.75f)'),
        ('final Path indexDir = Paths.get(args[0]);',
         'final Path indexDir = Paths.get(args[0]);\n'
         '        final float scoreScale = args.length > 1 ? Float.parseFloat(args[1]) : 1f;\n'
         '        if (!Float.isFinite(scoreScale) || scoreScale <= 0f) {\n'
         '            throw new IllegalArgumentException("Score scale must be finite and positive");\n'
         '        }'),
        ('Query query = queryParser.parse(query_str);',
         'Query query = queryParser.parse(query_str);\n'
         '                if (command.startsWith("TOP_") && scoreScale != 1f) {\n'
         '                    query = new org.apache.lucene.search.BoostQuery(query, scoreScale);\n'
         '                }'),
    ]
    if configured:
        replacements[1] = ('new BM25Similarity(0.9f, 0.4f)',
                           'profile.configured ? profile.similarity : new BM25Similarity(1.2f, 0.75f)')
        replacements[2] = ('final Path indexDir = Paths.get(args[0]);',
                           'final NativeBm25Profile profile = NativeBm25Profile.parse(args);\n'
                           '        final float scoreScale = profile.scoreScale;\n'
                           '        final Path indexDir = Paths.get(args[0]);')
        replacements.append(('final String[] fields = line.trim().split("\\t");',
                             'if (profile.configured && line.equals("BM25_CONFIG\\t")) {\n'
                             '                    System.out.println(NativeBm25Profile.receipt(searcher, scoreScale));\n'
                             '                    System.out.flush();\n'
                             '                    continue;\n'
                             '                }\n'
                             '                final String[] fields = line.trim().split("\\t");'))
    for before, after in replacements:
        if source.count(before) != 1:
            raise ValueError(f'Expected one replacement target in the game adapter: {before!r}')
        source = source.replace(before, after)
    if source.count('new IndexSearcher(reader)') != 1 or source.count('searcher.setQueryCache(null);') != 1:
        raise ValueError('Native adapter requires the original IndexSearcher with disabled query cache')
    if 'MatchedStatisticsSearcher' in source:
        raise ValueError('Native adapter must not override collection statistics')
    return source


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--lucene-dir', type=Path, required=True)
    parser.add_argument('--output-dir', type=Path, default=HERE/'native-classes')
    parser.add_argument('--fresh-output', action='store_true',
                        help='Reject an existing class directory before preparation')
    args = parser.parse_args()
    lucene = args.lucene_dir.resolve()
    output = args.output_dir.resolve()
    output.mkdir(parents=True, exist_ok=not args.fresh_output)
    generated = output/'DoQueryNative.java'
    generated.write_text(native_source((lucene/'src/main/java/DoQuery.java').read_text(), configured=True))
    classpath = f'{lucene/"build/classes/java/main"}:{lucene/"build/dependencies"}/*'
    sources = {generated.name: generated.read_bytes()}
    for name in ('DumpNativeLuceneResults.java', 'NativeBm25Profile.java'):
        sources[name] = (HERE/name).read_bytes()
        (output/name).write_bytes(sources[name])
    command = ['javac', '-cp', classpath, '-d', str(output)] + [str(output/name) for name in sources]
    subprocess.run(command, check=True)
    if any((output/name).read_bytes() != contents for name, contents in sources.items()):
        raise RuntimeError('Native Java sources changed during compilation')
    manifest = {
        'command': command,
        'configured_source': True,
        'sources_sha256': {name: hashlib.sha256(contents).hexdigest() for name, contents in sources.items()},
        'classes_sha256': {name: hashlib.sha256((output/name).read_bytes()).hexdigest()
                           for name in ('DoQueryNative.class', 'DumpNativeLuceneResults.class',
                                        'NativeBm25Profile.class')},
    }
    (output/'native-build.json').write_text(json.dumps(manifest, indent=2) + '\n')
    print(f'Built {output}: native Lucene collection statistics, BM25 1.2/0.75, '
          'TOP score scale 1 by default; COUNT unwrapped; cache disabled.')


if __name__ == '__main__':
    main()
