import json,os,struct,time
from balanced import OUT,Worker,CASES,validate_dump,BINS,WORKER_ARGS,identity
os.sched_setaffinity(0,{6})
BINS['tantivy']=OUT/'target/release/configured_worker'
for k1,b in [('0.9','0.4'),('2','1')]:
 folder=OUT/f'configured-k{k1}-b{b}';folder.mkdir()
 WORKER_ARGS['tantivy']=[k1,b]
 before=identity();(folder/'before.json').write_text(json.dumps(before,indent=2)+'\n')
 start=time.monotonic();worker=Worker('tantivy',folder,'candidate',deadline=start+1800)
 dumps=[];failures=[]
 def bits(x):return f'{struct.unpack("<I",struct.pack("<f",float(x)))[0]:08x}'
 if worker.ready['k1_bits']!=bits(k1) or worker.ready['b_bits']!=bits(b):raise RuntimeError('configured READY actual stats bits')
 try:
  for q in CASES:
   d=worker.request({'op':'dump','id':q['id']},120);validate_dump(q,d)
   if d['count']!=d['oracle']['count'] or d['hits']!=d['oracle']['hits']:failures.append({'case':q,'dump':d})
   dumps.append({'case':q,'tantivy':d})
   if q['id']%200==0:print('configured',k1,b,q['id'],flush=True)
 finally:worker.close()
 (folder/'dumps.json').write_text(json.dumps(dumps,indent=2)+'\n')
 (folder/'failures.json').write_text(json.dumps(failures,indent=2)+'\n')
 after=identity();(folder/'after.json').write_text(json.dumps(after,indent=2)+'\n')
 r={'unchanged':before==after,'pass':not failures and len(dumps)==len(CASES) and before==after,'queries':len(dumps),'failures':len(failures),'seconds':time.monotonic()-start,'ready':worker.ready}
 (folder/'receipt.json').write_text(json.dumps(r,indent=2)+'\n');print(json.dumps(r),flush=True)
 if not r['pass']:raise RuntimeError('configured correctness failed')
