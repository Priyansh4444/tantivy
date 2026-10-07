import hashlib,json,os,select,subprocess,time
from pathlib import Path
from stage import OUT,ROOT,sha
old=OUT.parent/'lucene-rs-balanced-oct06'
cases=json.loads((OUT/'profile-cases.json').read_text())
proof={r['case']['id']:r['tantivy']['hits'] for r in json.loads((old/'verify/dumps.json').read_text())}
binary=OUT/'target/release/tantivy_worker'
command=['perf','record','-e','cycles:u','-F','499','-o',str(OUT/'candidate-perf.data'),'--',str(binary),str(ROOT/'bench/wiki-1m-native-aligned-v11-oct05.idx'),str(old/'queries.jsonl')]
os.sched_setaffinity(0,{6});start=time.monotonic();replies=[]
with (OUT/'candidate-perf.stderr').open('w') as stderr:
 p=subprocess.Popen(command,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=stderr,text=True,bufsize=1,preexec_fn=lambda:os.sched_setaffinity(0,{4}))
 def read():
  if not select.select([p.stdout],[],[],60)[0]:raise TimeoutError('profile worker')
  line=p.stdout.readline()
  if not line:raise RuntimeError('perf worker exited')
  value=json.loads(line);replies.append(value);return value
 def run(q,n):
  p.stdin.write(json.dumps({'op':'run','id':q['id'],'mode':'top10','iterations':n})+'\n');p.stdin.flush();v=read()
  if v['checksum']!=sum(doc^bits for doc,bits in proof[q['id']])*n%2**64:raise RuntimeError('profile checksum')
 read()
 for q in cases:run(q,10)
 for _ in range(3):
  for q in cases:run(q,100)
 p.stdin.close();rc=p.wait(timeout=10)
(OUT/'candidate-profile-replies.json').write_text(json.dumps(replies,indent=2)+'\n')
r={'command':command,'returncode':rc,'seconds':time.monotonic()-start,'binary_sha256':sha(binary),'correctness_checksums_match':True,'cases':cases,'first_preparation_and_warmup_included_in_trace':True}
(OUT/'candidate-profile-receipt.json').write_text(json.dumps(r,indent=2)+'\n')
if rc:raise RuntimeError('profile failure')
report=subprocess.check_output(['perf','report','--stdio','-i',str(OUT/'candidate-perf.data'),'--no-children','--sort','symbol','--percent-limit','0.5'],text=True,stderr=subprocess.PIPE)
(OUT/'candidate-perf-report.txt').write_text(report);print(report[:12000])
