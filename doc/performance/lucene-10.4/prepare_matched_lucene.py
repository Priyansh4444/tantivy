#!/usr/bin/env python3
"""Build the scoring-matched Lucene protocol adapter outside the source tree."""
import argparse
import subprocess
from pathlib import Path

HERE = Path(__file__).resolve().parent


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--lucene-dir', type=Path, required=True)
    parser.add_argument('--output-dir', type=Path, default=HERE/'classes')
    args = parser.parse_args()
    lucene = args.lucene_dir.resolve()
    output = args.output_dir.resolve()
    output.mkdir(parents=True, exist_ok=True)
    source = (lucene/'src/main/java/DoQuery.java').read_text()
    replacements = [
        ('public class DoQuery {', 'public class DoQueryMatched {'),
        ('new BM25Similarity(0.9f, 0.4f)', 'new BM25Similarity(1.2f, 0.75f)'),
        ('new IndexSearcher(reader)',
         'new MatchedStatisticsSearcher(reader, Long.parseLong(args[1]))'),
        ('Query query = queryParser.parse(query_str);',
         'Query query = queryParser.parse(query_str);\n'
         '                if (command.startsWith("TOP_")) {\n'
         '                    query = new org.apache.lucene.search.BoostQuery(query, 2.2f);\n'
         '                }'),
    ]
    for before, after in replacements:
        if source.count(before) != 1:
            raise ValueError(f'Expected one replacement target in the game adapter: {before!r}')
        source = source.replace(before, after)
    generated = output/'DoQueryMatched.java'
    generated.write_text(source)
    classpath = f'{lucene/"build/classes/java/main"}:{lucene/"build/dependencies"}/*'
    subprocess.run(['javac', '-cp', classpath, '-d', str(output), str(generated),
                    str(HERE/'DumpLuceneResults.java'),
                    str(HERE/'MatchedStatisticsSearcher.java')], check=True)
    print(f'Built {output}: BM25 1.2/0.75, matched collection statistics, '
          'TOP_* score scale 2.2; COUNT unwrapped; cache disabled.')


if __name__ == '__main__':
    main()
