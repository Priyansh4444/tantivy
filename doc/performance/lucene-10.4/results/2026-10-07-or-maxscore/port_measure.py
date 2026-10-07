import json,os,random,time
from balanced import OUT,Worker,CASES,native_run,telemetry,identity,BINS,INDICES,PINDEX
os.sched_setaffinity(0,{6})
BINS['port']=OUT.parent/'lucene-rs-balanced-oct06/target/release/port_worker';INDICES['port']=PINDEX
folder=OUT/'port-measure';folder.mkdir();start=time.monotonic()
before=identity();(folder/'before.json').write_text(json.dumps(before,indent=2)+'\n')
work=[q for q in CASES if 'correctness-only' not in q['tags']]
schedule={'pairs':[['tantivy','port'],['port','port']],'rounds':3,'samples_per_order':16,'queries':[q['id'] for q in work],'modes':['top10'],'cpu':4,'warmup':10,'seed':20261006,'deadline_seconds':1800,'labels':{'tantivy':'candidate Tantivy','port':'fixed lucene-rs'}}
(folder/'schedule.json').write_text(json.dumps(schedule,indent=2)+'\n')
proof={r['case']['id']:r for r in json.loads((OUT.parent/'lucene-rs-balanced-oct06/verify/dumps.json').read_text())}
raw=(folder/'samples.jsonl').open('w');loads=[];rss=[]
for pairid,(a,b) in enumerate(schedule['pairs']):
 workers=[]
 try:
  for side,engine in [('A',a),('B',b)]:
   w=Worker(engine,folder,f'pair{pairid}-{side}',deadline=start+1800);w.expected={qid:r[engine] for qid,r in proof.items()};workers.append(w)
  for w in workers:
   for q in work:native_run(w,q,'top10',10)
  rss.append({'pair':pairid,'A':workers[0].rss(),'B':workers[1].rss()});loads.append(telemetry())
  for rnd in range(3):
   cases=work.copy();random.Random(20261006+rnd).shuffle(cases)
   for order in ('AB','BA'):
    for q in cases:
     for side in order:
      w=workers[0 if side=='A' else 1];v=native_run(w,q,'top10',16)
      raw.write(json.dumps({'pair':pairid,'round':rnd,'order':order,'side':side,'engine':w.engine,**v},separators=(',',':'))+'\n')
    raw.flush();loads.append(telemetry());print('port timed',pairid,rnd,order,flush=True)
 finally:
  for w in workers:w.close()
raw.close();after=identity();(folder/'after.json').write_text(json.dumps(after,indent=2)+'\n')
(folder/'rss.json').write_text(json.dumps(rss,indent=2)+'\n');(folder/'telemetry.json').write_text(json.dumps(loads,indent=2)+'\n')
r={'pass':before==after,'unchanged':before==after,'seconds':time.monotonic()-start}
(folder/'receipt.json').write_text(json.dumps(r,indent=2)+'\n');print(json.dumps(r),flush=True)
if before!=after:raise RuntimeError('port timing artifact changed')
