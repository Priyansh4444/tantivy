"""Root-owned serial runner: new artifacts only; unchanged strict gates precede timing."""
import hashlib, json, os, subprocess, sys, time
from pathlib import Path

ROOT = Path('/home/pronsh/Coding/playground/search')
REPO = ROOT/'tantivy-pr2937'
BENCH = ROOT/'bench'
TOOLS = REPO/'doc/performance/lucene-10.4'
OUT = BENCH/'native-profile-aligned-oct05'
INDEX = BENCH/'wiki-1m-native-aligned-v11-oct05.idx'
CORPUS = BENCH/'wiki-1m.jsonl'
LUCENE = ROOT/'search-benchmark-game/engines/lucene-10.4.0'
JARS = LUCENE/'build/dependencies'
CLASSES = OUT/'logical-classes'
NATIVE = OUT/'native-classes'
REPLAY = OUT/'aligned.jsonl'
MAP = OUT/'lucene-physical.tsv'
PROFILES = ('default', 'k09-b04', 'k25-b1')
EXPECTED_CORPUS = '2b630549676f1c58a579017b6cd949e25115fe63989f9e75cadadc5c1a1a8238'


def sha(path):
    with Path(path).open('rb') as f:
        return hashlib.file_digest(f, 'sha256').hexdigest()


def head():
    return subprocess.check_output(['git','-C',str(REPO),'rev-parse','HEAD'],text=True).strip()


def snapshot():
    return dict(time_utc=time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),
                loadavg=Path('/proc/loadavg').read_text().strip(),
                processes=subprocess.check_output(['ps','-eo','pid,ppid,comm,pcpu,pmem','--sort=-pcpu'],text=True))


def write_new(path, data):
    with path.open('x') as f:
        f.write(json.dumps(data,indent=2)+'\n')


def run(name, command, *, stdin=None, stdout=None):
    record = dict(command=list(map(str,command)), cwd=str(REPO), before=snapshot())
    print('START '+name,flush=True)
    with (OUT/(name+'.stderr')).open('xb') as err, (stdout or OUT/(name+'.stdout')).open('xb') as dest:
        start = time.monotonic()
        if stdin is None:
            process = subprocess.run(record['command'], cwd=REPO, stdout=dest, stderr=err)
        else:
            with stdin.open('rb') as source:
                process = subprocess.run(record['command'], cwd=REPO, stdin=source, stdout=dest, stderr=err)
    record.update(exit_code=process.returncode, elapsed_seconds=time.monotonic()-start, after=snapshot())
    write_new(OUT/(name+'.command.json'), record)
    if process.returncode:
        raise RuntimeError(f'{name}: exit {process.returncode}; original output retained')
    print('DONE '+name,flush=True)


def state():
    result=json.loads((OUT/'state.json').read_text())
    if result['commit'] != head():
        raise RuntimeError('Integrated revision changed: use a new artifact/run directory')
    artifact=Path(result['artifact'])
    if any(sha(artifact/name)!=digest for name,digest in result['binary_sha256'].items()):
        raise RuntimeError('Immutable binary changed')
    return result,artifact


def retained(name):
    before=json.loads((BENCH/'native-profile-measurements-oct05/retained-artifacts-before-docorder.json').read_text())
    observed={path:dict(bytes=Path(path).stat().st_size,sha256=sha(path)) for path in before}
    if observed != before:
        raise RuntimeError('Retained artifact changed')
    write_new(OUT/(name+'.json'),dict(passed=True,files=observed))


def profile_flags(profile):
    return [] if profile=='default' else ['--bm25-profile',profile]


def prepare():
    release_receipt=BENCH/'native-docorder-release-acceptance-oct05.json'
    acceptance=json.loads(release_receipt.read_text())
    if acceptance['passed'] is not True or acceptance['commit'] != head():
        raise RuntimeError('Exact integrated release fixture acceptance is required')
    OUT.mkdir()
    if INDEX.exists() or INDEX.with_name(INDEX.name+'.build.json').exists():
        raise FileExistsError('New aligned index/receipt path required')
    revision=head()
    artifact=BENCH/'native-perf-adapter-oct03/artifacts'/revision
    provenance=json.loads((artifact/'provenance.json').read_text())
    assert provenance['commit']==revision and provenance['index_format']==11
    subprocess.run(['git','-C',str(REPO),'diff','--quiet','HEAD'],check=True)
    for name,value in provenance['sources_sha256'].items():
        assert sha(TOOLS/name)==value, name
    write_new(OUT/'state.json',dict(commit=revision,artifact=str(artifact),index=str(INDEX),
        lucene_index=str(LUCENE/'idx'),corpus=str(CORPUS),corpus_sha256=EXPECTED_CORPUS,
        binary_sha256=provenance['binaries_sha256'],batch_size=25000,memory_bytes=500000000,
        release_fixture_receipt_sha256=sha(release_receipt),
        cpuinfo=Path('/proc/cpuinfo').read_text(),start=snapshot()))
    state()
    CLASSES.mkdir()
    run('compile-logical',['javac','-cp',str(JARS/'*'),'-d',str(CLASSES),str(TOOLS/'DumpWikiLogical.java')])
    write_new(OUT/'logical-build.json',dict(source_sha256=sha(TOOLS/'DumpWikiLogical.java'),
        classes_sha256={p.name:sha(p) for p in sorted(CLASSES.glob('*.class'))}))
    run('compile-native',['python',str(TOOLS/'prepare_native_lucene.py'),'--lucene-dir',str(LUCENE),
                         '--output-dir',str(NATIVE),'--fresh-output'])
    run('export-lucene-physical',['java','-Xmx512m','-cp',str(CLASSES)+':'+str(JARS/'*'),
                                'DumpWikiLogical',str(LUCENE/'idx'),'documents-physical'],stdout=MAP)
    run('replay',['python',str(TOOLS/'wiki_docorder.py'),'replay','--corpus',str(CORPUS),
                 '--physical-map',str(MAP),'--output',str(REPLAY)])
    run('build-aligned',[str(artifact/'build_index'),str(INDEX),'--ordered-batches','25000',
        '--physical-map',str(MAP),'--replay-receipt',str(REPLAY)+'.replay.json','--memory-bytes','500000000'],stdin=REPLAY)
    receipt=json.loads(INDEX.with_name(INDEX.name+'.build.json').read_text())
    assert receipt['documents']==1000000 and receipt['tokens']==294827020
    assert receipt['ordered_replay']['physical_verified'] and len(receipt['ordered_replay']['batches'])==40
    write_new(OUT/'prepare-passed.json',dict(passed=True,commit=revision,build_receipt=receipt,
        build_receipt_sha256=sha(INDEX.with_name(INDEX.name+'.build.json'))))


def proof():
    s,artifact=state()
    assert json.loads((OUT/'prepare-passed.json').read_text())['passed']
    tmap=OUT/'tantivy-physical.tsv'
    tp=OUT/'tantivy-payload.json'; lp=OUT/'lucene-postings.json'
    run('export-tantivy-physical',[str(artifact/'dump_docmap'),str(INDEX),'--physical'],stdout=tmap)
    run('tantivy-full-payload',[str(artifact/'payload_identity'),str(INDEX)],stdout=tp)
    run('lucene-full-postings',['java','-Xmx512m','-cp',str(CLASSES)+':'+str(JARS/'*'),
                              'DumpWikiLogical',str(LUCENE/'idx'),'postings-digest'],stdout=lp)
    run('compare-physical-payload',['python',str(TOOLS/'wiki_docorder.py'),'compare',
        '--tantivy-physical',str(tmap),'--lucene-physical',str(MAP),'--tantivy-payload',str(tp),
        '--lucene-postings',str(lp),'--output',str(OUT/'physical-payload-comparison.json')])
    run('compare-logical',['python',str(TOOLS/'compare_wiki_logical.py'),'--binaries',str(artifact),
        '--tantivy-index',str(INDEX),'--lucene-index',str(LUCENE/'idx'),'--jars',str(JARS),
        '--output',str(OUT/'logical-comparison')])
    assert json.loads((OUT/'physical-payload-comparison.json').read_text())['postings_verified']
    assert json.loads((OUT/'logical-comparison/summary.json').read_text())['passed']
    retained('retained-after-proof')
    write_new(OUT/'proof-passed.json',dict(passed=True,commit=s['commit']))


def gates():
    s,artifact=state()
    assert json.loads((OUT/'proof-passed.json').read_text())['passed']
    common=['--tantivy-index',str(INDEX),'--lucene-dir',str(LUCENE),'--lucene-classes',str(NATIVE),'--native-bm25']
    results={}
    for profile in PROFILES:
        name=profile+'-correctness'
        run(name,['python',str(TOOLS/'compare_wiki_correctness.py'),'--tantivy-validator',str(artifact/'validate_index'),
            *common,*profile_flags(profile),'--commit',s['commit'],'--output',str(OUT/(name+'.json'))])
        result=json.loads((OUT/(name+'.json')).read_text())
        assert len(result['comparisons'])==20
        assert all(row['same_top10_id_set'] and row['top10_order_matches_within_score_ties']
                   and row['max_relative_score_difference']<=2e-6 for row in result['comparisons'])
        results[profile]=result
    default={(r['query'],d['id']):d['score'] for r in results['default']['tantivy'] for d in r['top100']}
    changed={p:sum((r['query'],d['id']) in default and d['score']!=default[(r['query'],d['id'])]
        for r in results[p]['tantivy'] for d in r['top100']) for p in PROFILES[1:]}
    assert all(v>0 for v in changed.values())
    write_new(OUT/'sensitivity.json',dict(passed=True,changed_common_scores=changed,commit=s['commit']))
    write_new(OUT/'gates-passed.json',dict(passed=True,commit=s['commit'],profiles=list(PROFILES)))


def measure():
    s,artifact=state()
    assert json.loads((OUT/'gates-passed.json').read_text())['commit']==s['commit']
    assert json.loads((OUT/'sensitivity.json').read_text())['passed']
    common=['--tantivy-index',str(INDEX),'--lucene-dir',str(LUCENE),'--lucene-classes',str(NATIVE),'--native-bm25']
    for profile in PROFILES:
        for command in ('COUNT','TOP_10'):
            for reverse in (False,True):
                name=profile+'-'+command.lower()+('-lucene-first' if reverse else '-tantivy-first')
                run(name,['python',str(TOOLS/'suite_lucene_interleaved.py'),
                    '--tantivy-binary',str(artifact/'do_query'),*common,*profile_flags(profile),
                    '--cpu-core','4','--command',command,'--warmup-seconds','40','--iterations','256',
                    '--output',str(OUT/(name+'.json')),*(['--lucene-first'] if reverse else [])])
                print(name+' T/L='+str(json.loads((OUT/(name+'.json')).read_text())['geomean_ratio']),flush=True)
        name=profile+'-memory'
        run(name,['python',str(TOOLS/'compare_process_memory.py'),'--tantivy-binary',str(artifact/'do_query'),
            *common,*profile_flags(profile),'--cpu-core','4','--warmup-seconds','20','--output',str(OUT/(name+'.json'))])
    retained('retained-after-measurement')
    write_new(OUT/'storage.json',{engine:{p.name:p.stat().st_size for p in sorted(path.iterdir()) if p.is_file()}
                               for engine,path in [('tantivy',INDEX),('lucene',LUCENE/'idx')]})
    write_new(OUT/'measure-passed.json',dict(passed=True,commit=s['commit'],end=snapshot()))

if __name__=='__main__':
    {'prepare':prepare,'proof':proof,'gate':gates,'measure':measure}[sys.argv[1]]()
