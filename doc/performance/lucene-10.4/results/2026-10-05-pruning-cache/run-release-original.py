from pathlib import Path
import subprocess,json,os,hashlib,time
base=Path('/home/pronsh/Coding/playground/search');repo=base/'tantivy-pr2937';out=base/'bench/native-pruning-cache-release-oct05';out.mkdir()
head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=repo,text=True).strip()
assert head=='e13303b3aaa8c15815ab5ea951a3449111438e81'
records=[]
def run(name,argv,env=None):
 start=time.monotonic()
 with (out/(name+'.stdout')).open('x') as so,(out/(name+'.stderr')).open('x') as se:
  r=subprocess.run(argv,cwd=repo,env=env,stdout=so,stderr=se)
 row={'command':argv,'cwd':str(repo),'commit':head,'exit_code':r.returncode,'seconds':time.monotonic()-start}
 (out/(name+'.command.json')).write_text(json.dumps(row,indent=2)+'\n');records.append(row);assert r.returncode==0,name
 print('DONE',name,flush=True)
run('adapter-build',['python',str(base/'bench/native-perf-adapter-oct03/build.py'),'--expect-commit',head,'--expect-format','11','--jobs','4'])
artifact=base/'bench/native-perf-adapter-oct03/artifacts'/head
env=os.environ.copy();env['CARGO_PROFILE_RELEASE_DEBUG']='0'
run('release-term-scorer',['cargo','test','--offline','--locked','--release','--lib','--jobs','4','query::term_query::term_scorer::tests','--','--nocapture'],env)
for p in ['default','k09-b04','k25-b1']:
 row=json.loads((base/f'bench/native-profile-aligned-oct05/{p}-correctness.command.json').read_text())
 argv=row['command'][:]
 argv[argv.index('--tantivy-validator')+1]=str(artifact/'validate_index')
 argv[argv.index('--commit')+1]=head
 argv[argv.index('--output')+1]=str(out/(p+'-correctness.json'))
 run(p+'-correctness',argv)
 x=json.loads((out/(p+'-correctness.json')).read_text())
 assert len(x['comparisons'])==20 and all(r['same_top10_id_set'] and r['top10_order_matches_within_score_ties'] and r['max_relative_score_difference']<=2e-6 for r in x['comparisons'])
(out/'summary.json').write_text(json.dumps({'passed':True,'commit':head,'records':records,'artifact':str(artifact),'binary_sha256':{n:hashlib.sha256((artifact/n).read_bytes()).hexdigest() for n in ['do_query','validate_index']}},indent=2)+'\n')
print('exact integrated release cache ownership proof and all3 unchanged Wiki20 gates pass',flush=True)
