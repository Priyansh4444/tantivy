"""Exercise unchanged comparator and actual Rust/Java public scoring and analysis APIs."""
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys
import urllib.request

OUT = Path(__file__).resolve().parent
ROOT = OUT.parents[1]
PORT = ROOT / 'lucene-rs-review-465ea8f'
MAIN = ROOT / 'tantivy-pr2937'
receipts = {}

def run(args):
    p = subprocess.run(args, text=True, capture_output=True, timeout=120)
    assert p.returncode == 0, p.stderr
    return p.stdout

baseline = 'TERM\tx\t1\t0:1.000000000e+00\n'
(OUT / 'expected.tsv').write_text(baseline)
for kind, content in {
    'different-doc': 'TERM\tx\t1\t1:1.000000000e+00\n',
    'different-score': 'TERM\tx\t1\t0:2.000000000e+00\n',
    'different-hit-count': 'TERM\tx\t2\t0:1.000000000e+00\n',
}.items():
    actual = OUT / (kind + '.tsv')
    actual.write_text(content)
    p = subprocess.run([sys.executable, str(PORT / 'bench/scripts/compare.py'),
                        str(OUT / 'expected.tsv'), str(actual)], capture_output=True, text=True, timeout=10)
    receipts[kind] = {'returncode': p.returncode, 'stdout': p.stdout, 'stderr': p.stderr}
    assert p.returncode == 0

jar_receipts = {}
for artifact in ['lucene-core', 'lucene-analysis-common']:
    name = artifact + '-10.5.2.jar'
    url = 'https://repo.maven.apache.org/maven2/org/apache/lucene/' + artifact + '/10.5.2/' + name
    data = urllib.request.urlopen(url, timeout=30).read()
    expected = urllib.request.urlopen(url + '.sha512', timeout=30).read().decode().strip().split()[0]
    digest = hashlib.sha512(data).hexdigest()
    assert digest == expected
    (OUT / name).write_bytes(data)
    jar_receipts[name] = {'url': url, 'bytes': len(data), 'sha512': digest}
receipts['java_artifacts'] = jar_receipts

bm25 = (MAIN / 'src/query/bm25.rs').read_text()
native = re.search(r'fn native_idf\(.*?\n\}', bm25, re.S).group()
rust = '''use lucene_rs::analysis::{tokenize, StandardAnalyzer};
type Score = f32;
''' + native + '''
fn main() {
    let n = 54505;
    let idf = lucene_rs::sim::idf(n,n);
    let native = native_idf(n,n);
    let sim = lucene_rs::sim::Bm25::for_term(1.0,n,n,n);
    println!("idf={:08x} ours_native_idf={:08x} score={:08x}", idf.to_bits(), native.to_bits(), sim.score(1.0,1).to_bits());
    println!("tokens={:?}",tokenize(&StandardAnalyzer::new().max_token_length(3),"body","abcdef z 😀"));
}
'''
(OUT / 'witness.rs').write_text(rust)
rlibs = list((OUT / 'target/debug/deps').glob('liblucene_rs-*.rlib'))
assert len(rlibs) == 1
receipts['native_idf_source_sha256'] = hashlib.sha256(native.encode()).hexdigest()
run(['rustc', '+1.98.1', '--edition=2024', str(OUT / 'witness.rs'), '-L',
     'dependency=' + str(OUT / 'target/debug/deps'), '--extern', 'lucene_rs=' + str(rlibs[0]),
     '-o', str(OUT / 'witness')])
receipts['rust_witness'] = run([str(OUT / 'witness')])

java = '''import org.apache.lucene.analysis.standard.StandardAnalyzer;
import org.apache.lucene.analysis.tokenattributes.CharTermAttribute;
import org.apache.lucene.analysis.tokenattributes.PositionIncrementAttribute;
import org.apache.lucene.search.similarities.BM25Similarity;
import org.apache.lucene.search.CollectionStatistics;
import org.apache.lucene.search.TermStatistics;
import org.apache.lucene.util.BytesRef;
public class Witness {
  public static void main(String[] args) throws Exception {
    long n = 54505;
    var b = new BM25Similarity();
    var c = new CollectionStatistics("body", n,n,n,n);
    var t = new TermStatistics(new BytesRef("x"), n,n);
    float idf = b.idfExplain(c,t).getValue().floatValue();
    float score = b.scorer(1f,c,t).score(1f,1L);
    System.out.printf("idf=%08x score=%08x%n",Float.floatToRawIntBits(idf),Float.floatToRawIntBits(score));
    try(var a = new StandardAnalyzer()) {
      a.setMaxTokenLength(3);
      try(var s = a.tokenStream("body","abcdef z 😀")) {
        var text = s.addAttribute(CharTermAttribute.class);
        var inc = s.addAttribute(PositionIncrementAttribute.class);
        int pos = -1;
        s.reset();
        while(s.incrementToken()) {pos += inc.getPositionIncrement();System.out.printf("token=%s pos=%d%n",text,pos);}
        s.end();
      }
    }
  }
}
'''
(OUT / 'Witness.java').write_text(java)
cp = ':'.join(str(OUT / name) for name in jar_receipts)
run(['javac','-cp',cp,'-d',str(OUT),str(OUT / 'Witness.java')])
receipts['java_witness'] = run(['java','-cp',str(OUT)+':'+cp,'Witness'])
assert 'idf=3719e737' in receipts['rust_witness']
assert 'ours_native_idf=3719e736' in receipts['rust_witness']
assert 'idf=3719e736' in receipts['java_witness']
(OUT / 'findings-receipt.json').write_text(json.dumps(receipts,indent=2)+'\n')
print(json.dumps(receipts,indent=2))
