"""Record the concrete binaries, classes and workload used by a comparison."""
import hashlib
import platform
import subprocess
from pathlib import Path

HERE = Path(__file__).resolve().parent


def digest(path):
    with Path(path).open('rb') as source:
        return hashlib.file_digest(source, 'sha256').hexdigest()


def capture(binary, lucene_dir, classes, matched=True, *, native=False, score_scale=None):
    if native and matched:
        raise ValueError("Native and matched provenance modes are mutually exclusive")
    binary = Path(binary).resolve()
    lucene_dir = Path(lucene_dir).resolve()
    classes = Path(classes).resolve()
    class_files = ([classes/'DoQueryNative.class', classes/'DumpNativeLuceneResults.class'] if native else
                   [classes/'DoQueryMatched.class', classes/'MatchedStatisticsSearcher.class',
                    classes/'DumpLuceneResults.class'] if matched else
                   [lucene_dir/'build/classes/java/main/DoQuery.class'])
    java = subprocess.run(['java', '-version'], capture_output=True, text=True, check=True)
    return {
        'lucene_mode': 'native' if native else ('matched' if matched else 'adapter'),
        'native_bm25': native, 'matched_collection_statistics': matched,
        'lucene_score_scale': score_scale if score_scale is not None else (2.2 if matched else 1.0),
        'platform': platform.platform(), 'java_version': (java.stdout + java.stderr).strip(),
        'tantivy_binary': str(binary), 'tantivy_binary_sha256': digest(binary),
        'query_suite_sha256': digest(HERE/'queries-wiki.jsonl'),
        'java_classes_sha256': {str(path): digest(path) for path in class_files},
        'lucene_jars_sha256': {path.name: digest(path) for path in sorted((lucene_dir/'build/dependencies').glob('*.jar'))},
        'tool_sources_sha256': {path.name: digest(path) for path in sorted(HERE.iterdir())
                               if path.suffix in ('.py', '.java', '.rs')},
    }
