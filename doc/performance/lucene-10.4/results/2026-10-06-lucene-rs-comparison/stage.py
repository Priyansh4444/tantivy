"""Exclusive bounded benchmark preparation stages with command/source receipts."""
import hashlib,json,os,subprocess,sys,time
from pathlib import Path
OUT=Path(__file__).resolve().parent
ROOT=OUT.parents[1]
def sha(p):
    h=hashlib.sha256()
    with p.open('rb') as f:
        for b in iter(lambda:f.read(1<<20),b''):h.update(b)
    return h.hexdigest()
def sources():
    state={}
    for name in ('tantivy-pr2937','lucene-rs-idf-fix-oct06'):
        repo=ROOT/name
        files=subprocess.check_output(['git','ls-files','-z'],cwd=repo).split(b'\0')
        state[name]={'head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=repo,text=True).strip(),
            'status':subprocess.check_output(['git','status','--porcelain'],cwd=repo,text=True),
            'files':{os.fsdecode(p):sha(repo/os.fsdecode(p)) for p in files if p}}
    state['balanced_harness']={str(p.relative_to(OUT)):sha(p) for p in sorted((OUT/'harness').rglob('*')) if p.is_file()}
    return state
def environment():
    env=dict(os.environ)
    for k in list(env):
        if k.startswith('CARGO_PROFILE_') or k in ('CARGO_ENCODED_RUSTFLAGS','RUSTFLAGS','RUSTUP_TOOLCHAIN','CARGO_TARGET_DIR'):
            del env[k]
    env.update(RUSTUP_TOOLCHAIN='1.99.0',RUSTFLAGS='-C target-cpu=native',CARGO_BUILD_JOBS='2',CARGO_TARGET_DIR=str(OUT/'target'))
    return env
if __name__=='__main__':
    stage,timeout,*command=sys.argv[1:]
    folder=OUT/stage;folder.mkdir()
    before=sources();start=time.monotonic()
    audited_index=Path(command[2]) if len(command)>2 and command[1]=='audit' else None
    def index_guard():
        return {str(p.relative_to(audited_index)):sha(p) for p in sorted(audited_index.rglob('*')) if p.is_file() and not p.name.endswith('.lock')} if audited_index else None
    index_before=index_guard()
    (folder/'before.json').write_text(json.dumps(before,indent=2)+'\n')
    (folder/'command.json').write_text(json.dumps(command,indent=2)+'\n')
    env=environment()
    (folder/'compiler.txt').write_text(subprocess.check_output(['rustc','-Vv'],env=env,text=True))
    with (folder/'stdout.log').open('w') as stdout,(folder/'stderr.log').open('w') as stderr:
        try:p=subprocess.run(command,cwd=OUT/'harness',env=env,stdout=stdout,stderr=stderr,timeout=int(timeout));rc=p.returncode
        except subprocess.TimeoutExpired:rc=124
    after=sources();(folder/'after.json').write_text(json.dumps(after,indent=2)+'\n')
    index_after=index_guard()
    if audited_index:
        (folder/'index-before.json').write_text(json.dumps(index_before,indent=2)+'\n')
        (folder/'index-after.json').write_text(json.dumps(index_after,indent=2)+'\n')
    receipt={'command':command,'returncode':rc,'wall_seconds':time.monotonic()-start,'source_unchanged':before==after,
        'compiler':'1.99.0','rustflags':env['RUSTFLAGS'],'jobs':2,'profile':dict(opt_level=3,lto='fat',codegen_units=1,overflow_checks=False,panic='abort',debug=False),
        'stdout_sha256':sha(folder/'stdout.log'),'stderr_sha256':sha(folder/'stderr.log'),'audited_index_unchanged':index_before==index_after}
    (folder/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
    print(json.dumps(receipt),flush=True)
    if before!=after:raise RuntimeError('source changed')
    if index_before!=index_after:raise RuntimeError('audited index changed')
    sys.exit(rc)
