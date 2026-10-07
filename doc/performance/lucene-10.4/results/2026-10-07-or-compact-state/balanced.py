"""Strict gates and balanced, serial native API timing; never alters user processes."""
import contextlib,hashlib,json,math,os,random,select,statistics,struct,subprocess,sys,time
from pathlib import Path
from stage import OUT,ROOT,sha,sources

TINDEX=ROOT/'bench/wiki-1m-native-aligned-v11-oct05.idx'
PINDEX=OUT.parent/'lucene-rs-balanced-oct06/port.idx'
QUERIES=OUT.parent/'lucene-rs-balanced-oct06/queries.jsonl'
BINS={'tantivy':OUT/'target/release/tantivy_worker','port':OUT.parent/'lucene-rs-or-improvement-oct06/target/release/tantivy_worker'}
INDICES={'tantivy':TINDEX,'port':TINDEX}
WORKER_ARGS={}
CASES=[json.loads(line) for line in QUERIES.read_text().splitlines()]

def index_files(path):
    return {str(p.relative_to(path)):sha(p) for p in sorted(path.rglob('*')) if p.is_file() and not p.name.endswith('.lock')}
def identity():
    return {'source':sources(),'indices':{k:index_files(p) for k,p in INDICES.items()},
        'queries':sha(QUERIES),'binaries':{k:sha(p) for k,p in BINS.items()},
        'harness':{str(p.relative_to(OUT)):sha(p) for p in sorted((OUT/'harness').rglob('*')) if p.is_file()},
        'controller':sha(Path(__file__)), 'payload_audit':sha(OUT.parent/'lucene-rs-balanced-oct06/audit-port/stdout.log')}
def telemetry():
    return {'at':time.time(),'loadavg':list(os.getloadavg()),
        'processes':subprocess.check_output(['ps','-eo','pid,comm,pcpu','--sort=-pcpu'],text=True).splitlines()[:13]}

class Worker:
    def __init__(self,engine,folder,label,cpu=4,deadline=None):
        self.engine=engine
        self.expected=None
        self.deadline=deadline
        if deadline is not None and time.monotonic()>=deadline:raise TimeoutError('stage deadline')
        self.stderr=(folder/f'{label}.stderr').open('w')
        self.log=(folder/f'{label}.protocol.jsonl').open('w')
        self.p=subprocess.Popen([str(BINS[engine]),str(INDICES[engine]),str(QUERIES)]+WORKER_ARGS.get(engine,[]),
            stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=self.stderr,text=True,bufsize=1,
            preexec_fn=lambda:os.sched_setaffinity(0,{cpu}))
        try:
            self.ready=self.read(120)
            expected={'max_doc':1000000,'num_docs':1000000,'doc_count':917578,'sum_total_term_freq':294827020,
                'k1_bits':'3f99999a','b_bits':'3f400000','queries':len(CASES),'op':'ready'}
            if WORKER_ARGS.get(engine):
                expected.pop('k1_bits');expected.pop('b_bits')
            for k,v in expected.items():
                if self.ready.get(k)!=v:raise RuntimeError(f'{engine} READY {k}: {self.ready.get(k)} != {v}')
        except BaseException:
            self.close()
            raise
    def read(self,timeout):
        if self.deadline is not None:
            timeout=min(timeout,self.deadline-time.monotonic())
            if timeout<=0:raise TimeoutError('stage deadline')
        if not select.select([self.p.stdout],[],[],timeout)[0]:raise TimeoutError(self.engine)
        line=self.p.stdout.readline()
        if not line:raise RuntimeError(f'{self.engine} exited {self.p.poll()}; inspect stderr')
        value=json.loads(line)
        self.log.write(json.dumps({'response':value},separators=(',',':'))+'\n');self.log.flush()
        return value
    def request(self,request,timeout=10):
        self.log.write(json.dumps({'request':request},separators=(',',':'))+'\n');self.log.flush()
        self.p.stdin.write(json.dumps(request,separators=(',',':'))+'\n');self.p.stdin.flush()
        value=self.read(timeout)
        if value.get('id')!=request['id']:raise RuntimeError('response identity')
        return value
    def close(self):
        if self.p.poll() is None:
            self.p.stdin.close()
            try:self.p.wait(timeout=10)
            except subprocess.TimeoutExpired:self.p.terminate();self.p.wait(timeout=10)
        self.stderr.close();self.log.close()
    def rss(self):
        data=Path(f'/proc/{self.p.pid}/status').read_text().splitlines()
        return {line.split(':')[0]:line.split(':',1)[1].strip() for line in data if line.startswith(('VmRSS:','VmHWM:','VmSize:'))}

def validate_dump(case,dump):
    if dump['id']!=case['id']:raise RuntimeError('dump id')
    hits=dump['hits'];count=dump['count']
    if not isinstance(count,int) or count<0 or len(hits)!=min(10,count):raise RuntimeError('hit length/count')
    if len({h[0] for h in hits})!=len(hits):raise RuntimeError('duplicate hits')
    prev=None
    for doc,bits in hits:
        if not isinstance(doc,int) or not 0<=doc<1000000 or not isinstance(bits,int) or not 0<=bits<2**32:raise RuntimeError('hit range')
        score=struct.unpack('<f',struct.pack('<I',bits))[0]
        if not math.isfinite(score):raise RuntimeError('nonfinite')
        order=(-score,doc)
        if prev is not None and order<prev:raise RuntimeError('hit order')
        prev=order
    report=dump['reported']
    if report is not None:
        if report['relation']=='eq' and report['value']!=count:raise RuntimeError('incorrect exact report')
        elif report['relation']=='gte' and not len(hits)<=report['value']<=count:raise RuntimeError('invalid lower bound')
        elif report['relation'] not in ('eq','gte'):raise RuntimeError('unknown relation')

def verify(folder):
    start=time.monotonic()
    ruler=json.loads((ROOT/'bench/native-profile-aligned-oct05/frozen-ruler.json').read_text())
    expected={Path(p).name:v['sha256'] for p,v in ruler['artifacts'].items() if str(TINDEX)+'/' in p and not p.endswith('.lock')}
    if index_files(TINDEX)!=expected:raise RuntimeError('Tantivy full-payload frozen index changed')
    audit=json.loads((OUT.parent/'lucene-rs-balanced-oct06/audit-port/stdout.log').read_text())
    if audit['payload_sha256']!='893a75958e6d15c997d414d5fb5fc47c633aa18c68fcd33bc0f8e13abfa8dbb8' or audit['positions']!=294827020 or audit['integrity']!='pass':raise RuntimeError('full payload audit required')
    if json.loads((OUT.parent/'lucene-rs-balanced-oct06/audit-port/receipt.json').read_text())['returncode']!=0:raise RuntimeError('payload stage failed')
    if index_files(PINDEX)!=json.loads((OUT.parent/'lucene-rs-balanced-oct06/audit-port/index-after.json').read_text()):raise RuntimeError('audited port index changed')
    before=identity();(folder/'before.json').write_text(json.dumps(before,indent=2)+'\n')
    failures=[];dumps=[];workers=[]
    try:
        workers.append(Worker('tantivy',folder,'candidate',deadline=start+3600))
        frozen={r['case']['id']:r for r in json.loads((OUT.parent/'lucene-rs-balanced-oct06/verify/dumps.json').read_text())}
        for case in CASES:
            value=workers[0].request({'op':'dump','id':case['id']},120)
            validate_dump(case,value)
            reference=frozen[case['id']]['tantivy']
            port_reference=frozen[case['id']]['port']
            if value['count']!=value['oracle']['count'] or value['hits']!=value['oracle']['hits']:
                failures.append({'id':case['id'],'reason':'optimized != exhaustive','actual':value})
            if value['count']!=reference['count'] or value['hits']!=reference['hits']:
                failures.append({'id':case['id'],'reason':'candidate != baseline','actual':value})
            if value['count']!=port_reference['count'] or value['hits']!=port_reference['hits']:
                failures.append({'id':case['id'],'reason':'candidate != fixed port','actual':value})
            dumps.append({'case':case,'tantivy':value,'port':reference})
            if case['id']%100==0:print('verified',case['id'],flush=True)
    finally:
        for worker in workers:worker.close()
        (folder/'dumps.json').write_text(json.dumps(dumps,indent=2)+'\n')
        (folder/'failures.json').write_text(json.dumps(failures,indent=2)+'\n')
    after=identity();(folder/'after.json').write_text(json.dumps(after,indent=2)+'\n')
    receipt={'queries':len(dumps),'failures':len(failures),'unchanged':before==after,'pass':not failures and len(dumps)==len(CASES) and before==after,'wall_seconds':time.monotonic()-start}
    (folder/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n');print(json.dumps(receipt),flush=True)
    if not receipt['pass']:raise RuntimeError('correctness gate failed')

def native_run(worker,case,mode,n):
    value=worker.request({'op':'run','id':case['id'],'mode':mode,'iterations':n})
    if value.get('mode')!=mode or len(value.get('ns',[]))!=n or any(not isinstance(t,int) or t<=0 for t in value['ns']):raise RuntimeError('timing response')
    if worker.expected is not None:
        dump=worker.expected[case['id']]
        total=dump['count'] if mode=='count' else sum(doc^bits for doc,bits in dump['hits'])
        if value['checksum']!=(total*n)%2**64:raise RuntimeError(f'{worker.engine} timed checksum differs: {case["id"]}/{mode}')
    return value

def measure(folder):
    for stage in ('build-reviewed', 'tests-reviewed', 'layout', 'clippy-reviewed', 'formatting'):
        r=json.loads((OUT/stage/'receipt.json').read_text())
        if r['returncode']!=0 or not r['source_unchanged']:raise RuntimeError(f'preparation stage failed: {stage}')
    gate=json.loads((OUT/'verify/receipt.json').read_text())
    if not gate['pass']:raise RuntimeError('verification required')
    if json.loads((OUT/'verify/after.json').read_text())!=identity():raise RuntimeError('verified artifacts changed')
    before=identity();(folder/'before.json').write_text(json.dumps(before,indent=2)+'\n')
    workload=[q for q in CASES if 'correctness-only' not in q['tags']]
    schedule={'warmup':10,'samples_per_order':16,'rounds':3,'cpu':4,'pairs':[['tantivy','port'],['tantivy','tantivy'],['port','port']],
        'queries':[q['id'] for q in workload],'query_order_seed':20261006,'primary':'candidate vs baseline; identical native API/prebuilt AST/no totals; port engine label means preserved baseline Tantivy','total_deadline_seconds':3600}
    (folder/'schedule.json').write_text(json.dumps(schedule,indent=2)+'\n')
    start=time.monotonic();raw=(folder/'samples.jsonl').open('w');loads=[];rss=[]
    for pairid,(a,b) in enumerate(schedule['pairs']):
        workers=[]
        try:
            workers.append(Worker(a,folder,f'pair{pairid}-A',deadline=start+3600))
            workers.append(Worker(b,folder,f'pair{pairid}-B',deadline=start+3600))
            verified=json.loads((OUT/'verify/dumps.json').read_text())
            for worker in workers:worker.expected={row['case']['id']:row[worker.engine] for row in verified}
            loads.append(telemetry())
            for worker in workers:
                for case in workload:
                    for mode in ('count','top10'):native_run(worker,case,mode,10)
            rss.append({'pair':pairid,'A':workers[0].rss(),'B':workers[1].rss()})
            for roundid in range(3):
                cases=workload.copy();random.Random(20261006+roundid).shuffle(cases)
                for order in ('AB','BA'):
                    for case in cases:
                        for mode in ('count','top10'):
                            for side in order:
                                w=workers[0 if side=='A' else 1]
                                value=native_run(w,case,mode,16)
                                record={'pair':pairid,'round':roundid,'order':order,'side':side,'engine':w.engine,**value}
                                raw.write(json.dumps(record,separators=(',',':'))+'\n')
                            if time.monotonic()-start>3600:raise TimeoutError('total timing deadline')
                    raw.flush();loads.append(telemetry());print('timed',pairid,roundid,order,flush=True)
        finally:
            for worker in workers:worker.close()
    raw.close();after=identity();(folder/'after.json').write_text(json.dumps(after,indent=2)+'\n')
    (folder/'telemetry.json').write_text(json.dumps(loads,indent=2)+'\n')
    (folder/'rss.json').write_text(json.dumps(rss,indent=2)+'\n')
    receipt={'pass':before==after,'source_index_binary_unchanged':before==after,'wall_seconds':time.monotonic()-start,'samples_sha256':sha(folder/'samples.jsonl')}
    (folder/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n');print(json.dumps(receipt),flush=True)
    if before!=after:raise RuntimeError('artifact changed during timing')

if __name__=='__main__':
    op=sys.argv[1]
    if op not in ('verify','measure'):raise ValueError('verify|measure')
    allowed=os.sched_getaffinity(0)
    if 4 not in allowed:raise RuntimeError('CPU4 unavailable')
    os.sched_setaffinity(0,{6 if 6 in allowed else min(allowed-{4})})
    folder=OUT/op;folder.mkdir()
    try:
        (verify if op=='verify' else measure)(folder)
    except BaseException as exc:
        (folder/'error.json').write_text(json.dumps({'type':type(exc).__name__,'message':str(exc)},indent=2)+'\n')
        raise
