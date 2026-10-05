from pathlib import Path
import json,subprocess,struct,hashlib,datetime
base=Path('/home/pronsh/Coding/playground/search');out=base/'bench/native-profile-pruning-counters-oct05'
binary=out/'do_query-counters';names=json.loads((out/'counter-map.json').read_text())
queries=json.loads((base/'bench/native-profile-aligned-oct05/default-top_10-tantivy-first.json').read_text())['queries']
bits=lambda f: struct.pack('>f',f).hex()
profiles={'default':[bits(1.2),bits(.75)], 'k09-b04':[bits(.9),bits(.4)], 'k25-b1':[bits(2.5),bits(1)]}
summary={}
for p,args in profiles.items():
 argv=[str(binary),str(base/'bench/wiki-1m-native-aligned-v11-oct05.idx'),*args]
 requests=['BM25_CONFIG\t']+['TOP_10\t'+q for _ in range(2) for q in queries]+['COUNT\t'+q for q in queries]+['VALIDATE_TOP_10\t'+q for q in queries]
 payload='\n'.join(requests)+'\n'
 run=subprocess.run(argv,input=payload,text=True,capture_output=True,check=True)
 for suffix,data in [('stdin',payload),('stdout',run.stdout),('stderr',run.stderr)]:
  (out/(p+'.'+suffix)).write_text(data)
 lines=run.stdout.splitlines();receipt=json.loads(lines[0]);assert len(lines)==81
 assert all(x=='1' for x in lines[1:41]);assert all(x=='1' for x in lines[61:])
 entries=[]
 for line in run.stderr.splitlines():
  if not line.startswith('COUNTERS\t'):continue
  _,mode,q,values=line.split('\t');entries.append(dict(mode=mode,query=q,counters=dict(zip(names,json.loads(values),strict=True))))
 assert len(entries)==80
 assert entries[:20]==entries[20:40],p
 summary[p]=dict(command=argv,receipt=receipt,top10=entries[:20],count=entries[40:60],validation_passed=True,repeats_identical=True)
 (out/(p+'.json')).write_text(json.dumps(summary[p],indent=2)+'\n')
(out/'summary.json').write_text(json.dumps({'untimed':True,'commit':'0eb1af15df2fac22b5af46b040382c2e14f5e125','time':datetime.datetime.now(datetime.timezone.utc).isoformat(),'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'profiles':summary},indent=2)+'\n')
for p,x in summary.items():
 for e in x['top10']:
  if e['query'] in ['the','of','american','+the +of','the of','+the +american','the american','+the +saxophone']:
   c=e['counters'];print(p,e['query'],{k:v for k,v in c.items() if v})
