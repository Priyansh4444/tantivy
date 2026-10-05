import json,subprocess,time,os
from pathlib import Path
root=Path('/home/pronsh/Coding/playground/search')
repo=root/'tantivy-pr2937'
bench=root/'bench'
sha='872c9f55dcc91034dcf8c15fe060dd0b1cfd3ccd'
out=bench/'native-v11-configured-measurements-oct04'
out.mkdir(exist_ok=True)
artifact=bench/'native-perf-adapter-oct03/artifacts'/sha
index=bench/'wiki-1m-native-equivalent-v11-oct03.idx'
lucene=root/'search-benchmark-game/engines/lucene-10.4.0'
classes=bench/'native-lucene-tools-oct03/lucene-native-classes-oct03'
def snapshot():
 return {'time_utc':time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),'loadavg':Path('/proc/loadavg').read_text().strip(),'cpuinfo':Path('/proc/cpuinfo').read_text(),'processes':subprocess.check_output(['ps','-eo','pid,ppid,comm,pcpu,pmem','--sort=-pcpu'],text=True)}
common=['--tantivy-binary',str(artifact/'do_query'),'--tantivy-index',str(index),'--lucene-dir',str(lucene),'--lucene-classes',str(classes),'--native-bm25','--cpu-core','4']
wiki=json.loads((bench/'native-v11-configured-wiki-correctness-oct04.json').read_text())
assert wiki['native_bm25'] and len(wiki['comparisons'])==20
assert all(x['same_top10_id_set'] and x['top10_order_matches_within_score_ties'] and x['max_relative_score_difference'] <= 2e-6 for x in wiki['comparisons'])
assert json.loads((repo/'target/parity-native-configured-oct04/report.json').read_text())['passed']
for mode in ['COUNT','TOP_10']:
 for lucene_first in [False,True]:
  name=mode.lower()+('-lucene-first' if lucene_first else '-tantivy-first')
  cmd=['python',str(repo/'doc/performance/lucene-10.4/suite_lucene_interleaved.py'),*common,'--command',mode,'--warmup-seconds','40','--iterations','256','--output',str(out/(name+'.json'))]
  if lucene_first:cmd.append('--lucene-first')
  receipt={'command':cmd,'before':snapshot()}
  print('START '+name,flush=True)
  with (out/(name+'.log')).open('w') as log:subprocess.run(cmd,cwd=repo,stdout=log,stderr=subprocess.STDOUT,check=True)
  receipt['after']=snapshot()
  (out/(name+'.host.json')).write_text(json.dumps(receipt,indent=2)+'\n')
  result=json.loads((out/(name+'.json')).read_text())
  print('DONE '+name+' T/L='+str(result['geomean_ratio']),flush=True)
cmd=['python',str(repo/'doc/performance/lucene-10.4/compare_process_memory.py'),*common,'--warmup-seconds','20','--output',str(out/'memory.json')]
receipt={'command':cmd,'before':snapshot()}
with (out/'memory.log').open('w') as log:subprocess.run(cmd,cwd=repo,stdout=log,stderr=subprocess.STDOUT,check=True)
receipt['after']=snapshot()
(out/'memory.host.json').write_text(json.dumps(receipt,indent=2)+'\n')
print('DONE memory',flush=True)
sizes={}
for name,directory in [('tantivy',index),('lucene',lucene/'idx')]:
 files={str(p.relative_to(directory)):p.stat().st_size for p in directory.rglob('*') if p.is_file()}
 sizes[name]={'directory':str(directory),'total_bytes':sum(files.values()),'files':files}
(out/'storage.json').write_text(json.dumps(sizes,indent=2)+'\n')
