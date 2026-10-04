#!/usr/bin/env python3
"""Build native Lucene BM25 protocol/dump classes without collection-stat overrides."""
import argparse
import subprocess
from pathlib import Path

HERE = Path(__file__).resolve().parent


def native_source(source):
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
    args = parser.parse_args()
    lucene = args.lucene_dir.resolve()
    output = args.output_dir.resolve()
    output.mkdir(parents=True, exist_ok=True)
    generated = output/'DoQueryNative.java'
    generated.write_text(native_source((lucene/'src/main/java/DoQuery.java').read_text()))
    classpath = f'{lucene/"build/classes/java/main"}:{lucene/"build/dependencies"}/*'
    subprocess.run(['javac', '-cp', classpath, '-d', str(output), str(generated),
                    str(HERE/'DumpNativeLuceneResults.java')], check=True)
    print(f'Built {output}: native Lucene collection statistics, BM25 1.2/0.75, '
          'TOP score scale 1 by default; COUNT unwrapped; cache disabled.')


if __name__ == '__main__':
    main()
