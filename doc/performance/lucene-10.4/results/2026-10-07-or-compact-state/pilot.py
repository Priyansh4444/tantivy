import json,os,random,statistics,time
from balanced import OUT,Worker,CASES,native_run,telemetry,identity
folder=OUT/'pilot-final';folder.mkdir()
os.sched_setaffinity(0,{6})
start=time.monotonic();workers=[];raw=[]
expected={r['case']['id']:r['tantivy'] for r in json.loads((OUT.parent/'lucene-rs-balanced-oct06/verify/dumps.json').read_text())}
# Full 301 broad OR set plus Wiki20 and a seeded sample of every other query kind.
work=[q for q in CASES if 'correctness-only' not in q['tags'] and (q['kind']=='OR' or 'wiki20' in q['tags'] or q['id']%17==0)]
before=identity();loads=[telemetry()]
try:
 for engine in ('tantivy','port'):
  w=Worker(engine,folder,engine,deadline=start+600);w.expected=expected;workers.append(w)
 for w in workers:
  for q in work:native_run(w,q,'top10',4)
 for order in ('AB','BA'):
  cases=work.copy();random.Random(20261006).shuffle(cases)
  for q in cases:
   for side in order:
    w=workers[0 if side=='A' else 1]
    v=native_run(w,q,'top10',12)
    raw.append({'order':order,'side':side,'engine':w.engine,**v})
finally:
 for w in workers:w.close()
(folder/'samples.json').write_text(json.dumps(raw)+'\n')
lookup={(r['order'],r['side'],r['id']):statistics.median(r['ns']) for r in raw}
rows=[{**q,**{o:lookup[o,'A',q['id']]/lookup[o,'B',q['id']] for o in ('AB','BA')}} for q in work]
summary=[]
for label,predicate in [('all',lambda q:True),('or3plus',lambda q:q['kind']=='OR' and len(q['terms'])>=3),('or2',lambda q:q['kind']=='OR' and len(q['terms'])==2)]:
 rs=[r for r in rows if predicate(r)]
 if not rs:continue
 summary.append({'set':label,'queries':len(rs),**{o:{'candidate_mean_ns':statistics.mean(lookup[o,'A',q['id']] for q in rs),'baseline_mean_ns':statistics.mean(lookup[o,'B',q['id']] for q in rs),'ratio_mean':sum(lookup[o,'A',q['id']] for q in rs)/sum(lookup[o,'B',q['id']] for q in rs),'median_ratio':statistics.median(q[o] for q in rs)} for o in ('AB','BA')}})
loads.append(telemetry());after=identity()
receipt={'seconds':time.monotonic()-start,'unchanged':before==after,'summary':summary,'worst_regressions':sorted(rows,key=lambda q:min(q['AB'],q['BA']),reverse=True)[:20],'loads':loads}
(folder/'before.json').write_text(json.dumps(before,indent=2)+'\n');(folder/'after.json').write_text(json.dumps(after,indent=2)+'\n');(folder/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
print(json.dumps(receipt),flush=True)
if before!=after:raise RuntimeError('pilot changed artifacts')
