#!/usr/bin/env python3
from pathlib import Path
import datetime, hashlib, json, os, subprocess
ROOT=Path('/home/pronsh/Coding/playground/search')
REPO=ROOT/'lucene-rs-comparator-fix-oct06'
OUT=ROOT/'bench/lucene-rs-upstream-comparator-oct06'
ENV=os.environ.copy()
ENV['CARGO_TARGET_DIR']=str(OUT/'target')
ENV['CARGO_BUILD_JOBS']='2'
for key in list(ENV):
    if key.startswith('CARGO_PROFILE_') or key in {'CARGO_ENCODED_RUSTFLAGS','RUSTFLAGS'}:
        ENV.pop(key)
def digest(p):
    return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def run(name,args,cwd=REPO):
    start=datetime.datetime.now(datetime.timezone.utc).isoformat()
    r=subprocess.run(args,cwd=cwd,env=ENV,capture_output=True,text=True,timeout=1200)
    (OUT/(name+'.stdout')).write_text(r.stdout)
    (OUT/(name+'.stderr')).write_text(r.stderr)
    receipt={'command':args,'cwd':str(cwd),'returncode':r.returncode,'start':start,'end':datetime.datetime.now(datetime.timezone.utc).isoformat(),'stdout_sha256':digest(OUT/(name+'.stdout')),'stderr_sha256':digest(OUT/(name+'.stderr'))}
    (OUT/(name+'.json')).write_text(json.dumps(receipt,indent=2)+'\n')
    print(name,r.returncode,flush=True)
    if r.returncode:
        raise RuntimeError(name+' failed; inspect retained stderr')
    return r
run('final-python',['python3','-m','unittest','discover','-s','bench/scripts/tests','-v'])
run('rust-identity',['rustc','+1.99.0','-Vv'])
run('cargo-identity',['cargo','+1.99.0','-V'])
run('fmt',['cargo','+1.99.0','fmt','--all','--','--check'])
run('clippy',['cargo','+1.99.0','clippy','--workspace','--all-targets','--all-features','--locked','-j','2','--','-D','warnings'])
run('cargo-tests',['cargo','+1.99.0','test','--workspace','--all-features','--locked','-j','2'])
run('build-fixture-bins',['cargo','+1.99.0','build','-p','lucene-rs-bench','--bins','--locked','-j','2'])
run('render',['python3','bench/scripts/render.py','--check'])
JARS=ROOT/'bench/lucene-rs-review-oct06'
CP=str(JARS/'lucene-core-10.5.2.jar')+':'+str(JARS/'lucene-analysis-common-10.5.2.jar')
CLASSES=OUT/'java-classes';CLASSES.mkdir(exist_ok=False)
run('java-identity',['java','-version'])
run('javac',['javac','-cp',CP,'-d',str(CLASSES),'bench/java/Indexer.java','bench/java/Bench.java'])
FIX=OUT/'fixture';FIX.mkdir(exist_ok=False)
(FIX/'corpus.txt').write_text(('x a b\n'*1205)+'y b\na z\n\n')
(FIX/'queries.tsv').write_text('TERM\tx\nTERM\ty\nTERM\tabsent\nAND\tx a\nOR\tx y\nPHRASE\tx a\n')
run('rust-index',[str(OUT/'target/debug/index'),str(FIX/'corpus.txt'),str(FIX/'rust.idx'),'--positions'])
run('java-index',['java','-Xms128m','-Xmx512m','-cp',str(CLASSES)+':'+CP,'Indexer',str(FIX/'corpus.txt'),str(FIX/'java.idx'),'--positions'])
run('rust-dump',[str(OUT/'target/debug/bench'),str(FIX/'rust.idx'),str(FIX/'queries.tsv'),'dump',str(FIX/'rust.tsv')])
run('java-dump',['java','-Xms128m','-Xmx512m','-cp',str(CLASSES)+':'+CP,'Bench',str(FIX/'java.idx'),str(FIX/'queries.tsv'),'dump',str(FIX/'java.tsv')])
run('fixture-cli',['python3','bench/scripts/compare.py',str(FIX/'java.tsv'),str(FIX/'rust.tsv')])
run('fixture-cli-optimized',['python3','-O','bench/scripts/compare.py',str(FIX/'java.tsv'),str(FIX/'rust.tsv')])
rows=[row.split('\t') for row in (FIX/'rust.tsv').read_text().splitlines()]
if any(len(row)!=5 for row in rows) or set(row[3] for row in rows)!={'eq','gte'}:
    raise RuntimeError('fixture did not exercise both normalized relations')
paths=[REPO/'Cargo.lock',OUT/'target/debug/bench',OUT/'target/debug/index',JARS/'lucene-core-10.5.2.jar',JARS/'lucene-analysis-common-10.5.2.jar',FIX/'java.tsv',FIX/'rust.tsv']
(OUT/'final-artifacts.json').write_text(json.dumps({str(p):digest(p) for p in paths},indent=2)+'\n')
print('PASS fixture both eq and gte',flush=True)
