"""Audit the published packet without engines, corpus, or third-party packages."""
import collections
import csv
import gzip
import hashlib
import json
import math
import statistics
import struct
from pathlib import Path

HERE = Path(__file__).resolve().parent

def check(ok, message):
    if not ok:
        raise ValueError(message)

def read(name):
    p = HERE/name
    return gzip.decompress(p.read_bytes()) if p.suffix == '.gz' else p.read_bytes()

def obj(name):
    return json.loads(read(name))

def same(actual, expected, label):
    if isinstance(expected, dict):
        check(actual.keys() == expected.keys(), label+' keys')
        for key, value in expected.items():
            same(actual[key],value,label+'/'+str(key))
    elif isinstance(expected,list):
        check(len(actual)==len(expected),label+' length')
        for i,(a,b) in enumerate(zip(actual,expected)):
            same(a,b,label+'/'+str(i))
    elif isinstance(expected,float):
        check(math.isclose(actual,expected,rel_tol=1e-12,abs_tol=1e-15),label+' numerical mismatch')
    else:
        check(actual==expected,label+' mismatch')

manifest=obj('packet-manifest.json')
check({str(p.relative_to(HERE)) for p in HERE.rglob('*') if p.is_file() and p.name!='packet-manifest.json'}==set(manifest['files']),'complete archive manifest')
for name, info in manifest['files'].items():
    payload=(HERE/name).read_bytes()
    check(len(payload)==info['bytes'] and hashlib.sha256(payload).hexdigest()==info['sha256'],'archive '+name)
for name, info in obj('copy-origins.json').items():
    check(hashlib.sha256(read(name)).hexdigest()==info['original_sha256'],'original '+name)

summary=obj('summary.json')
schedule=obj('measure/schedule.json')
check(schedule['pairs']==[['tantivy','port'],['tantivy','tantivy'],['port','port']] and schedule['rounds']==3 and schedule['samples_per_order']==32,'schedule contract')
queries={q['id']:q for q in map(json.loads,read('queries.jsonl').splitlines())}
check(len(queries)==summary['correctness_queries'] and len(schedule['queries'])==summary['queries_timed']==1221,'summary coverage')
dumps=obj('verify/dumps.json.gz')
check(len(dumps)==len(queries)==1227,'correctness coverage')
verified={}
bits=0
for row in dumps:
    qid=row['case']['id']
    check(qid not in verified and row['case']==queries[qid],'dump identity')
    a,b=row['tantivy'],row['port']
    check(a['count']==b['count'] and a['hits']==b['hits'],'cross-engine results')
    for d in (a,b):
        check(type(d['count']) is int and 0<=d['count']<=1000000,'exact count range')
        check(all(type(doc) is int and 0<=doc<1000000 and type(bits) is int and 0<=bits<2**32 for doc,bits in d['hits']),'hit range')
        check(d['id']==qid and d['count']==d['oracle']['count'] and d['hits']==d['oracle']['hits'],'exhaustive oracle')
        check(len(d['hits'])==min(10,d['count']) and len({h[0] for h in d['hits']})==len(d['hits']),'hit contract')
        scores=[struct.unpack('<f',struct.pack('<I',h[1]))[0] for h in d['hits']]
        check(all(math.isfinite(s) for s in scores),'finite scores')
        keys=[(-s,h[0]) for s,h in zip(scores,d['hits'])]
        check(keys==sorted(keys),'ordered hits')
        report=d['reported']
        if report is not None:
            check(report['relation'] in ('eq','gte'),'hit relation')
            check(report['value']==d['count'] if report['relation']=='eq' else len(d['hits'])<=report['value']<=d['count'],'hit value')
    verified[qid]=row
    bits+=len(a['hits'])
check(bits==11509,'score-bit coverage')
check(set(verified)==set(queries),'dump IDs')
check(obj('verify/receipt.json')['pass'] and obj('verify/failures.json')==[],'correctness receipt')
check(obj('verify/before.json')==obj('verify/after.json')==obj('measure/before.json')==obj('measure/after.json'),'identity guards')
check(obj('audit-port/index-before.json')==obj('audit-port/index-after.json'),'audited index guard')
check(obj('measure/receipt.json')['pass'],'measurement receipt')
same(obj('measure/receipt.json')['wall_seconds'],summary['wall_seconds'],'wall time')

raw=read('measure/samples.jsonl.gz')
check(hashlib.sha256(raw).hexdigest()==obj('measure/receipt.json')['samples_sha256'],'raw timing receipt')
cells=collections.defaultdict(list)
rounds=collections.defaultdict(list)
seen=set()
for line in raw.splitlines():
    r=json.loads(line)
    key=(r['pair'],r['round'],r['order'],r['id'],r['mode'],r['side'])
    check(key not in seen,'duplicate cell')
    seen.add(key)
    engine=schedule['pairs'][r['pair']][0 if r['side']=='A' else 1]
    check(r['engine']==engine,'engine label')
    check(len(r['ns'])==32 and all(type(n) is int and n>0 for n in r['ns']),'sample contract')
    d=verified[r['id']][engine]
    value=d['count'] if r['mode']=='count' else sum(doc^score for doc,score in d['hits'])
    check(r['checksum']==(value*32)%2**64,'timed result checksum')
    cells[(r['pair'],r['order'],r['id'],r['mode'],r['side'])].extend(r['ns'])
    rounds[key].extend(r['ns'])
expected={(p,r,o,q,m,s) for p in range(3) for r in range(3) for o in ('AB','BA') for q in schedule['queries'] for m in ('count','top10') for s in ('A','B')}
check(seen==expected,'complete schedule')
check(len(seen)==summary['complete_cells']==87912 and len(seen)*32==summary['raw_samples']==2813184,'sample totals')
geo=lambda xs:math.exp(statistics.mean(math.log(x) for x in xs))
rows=[]
for qid in schedule['queries']:
    q=queries[qid]
    for mode in ('count','top10'):
        row={'id':qid,'kind':q['kind'],'terms':' '.join(q['terms']),'set':'wiki20' if 'wiki20' in q['tags'] else 'port-style','mode':mode,'exact_count':verified[qid]['tantivy']['count']}
        for order in ('AB','BA'):
            for side,engine in (('A','tantivy'),('B','port')):
                row[f'{engine}_{order}_ns']=statistics.median(cells[(0,order,qid,mode,side)])
            row[f'T_over_P_{order}']=row[f'tantivy_{order}_ns']/row[f'port_{order}_ns']
            for pair,label in ((1,'TT'),(2,'PP')):
                row[f'{label}_{order}']=statistics.median(cells[(pair,order,qid,mode,'A')])/statistics.median(cells[(pair,order,qid,mode,'B')])
        noise=max(abs(math.log(row[f'{label}_{order}'])) for label in ('TT','PP') for order in ('AB','BA'))
        row['control_log_spread']=noise
        row['raw_T_wins_both']=all(row[f'T_over_P_{o}']<1 for o in ('AB','BA'))
        row['raw_P_wins_both']=all(row[f'T_over_P_{o}']>1 for o in ('AB','BA'))
        row['T_wins_beyond_controls']=all(math.log(row[f'T_over_P_{o}'])<-noise for o in ('AB','BA'))
        row['P_wins_beyond_controls']=all(math.log(row[f'T_over_P_{o}'])>noise for o in ('AB','BA'))
        rows.append(row)
published=list(csv.DictReader(read('per-query.csv').decode().splitlines()))
check(len(published)==len(rows)==2442,'CSV coverage')
for actual,saved in zip(rows,published):
    check(actual.keys()==saved.keys(),'CSV keys')
    for key,value in actual.items():
        same(value,float(saved[key]) if type(value) is float else int(saved[key]) if type(value) is int else saved[key]=='True' if type(value) is bool else saved[key],'CSV/'+key)
for s in summary['summaries']:
    rs=[r for r in rows if r['set']==s['set'] and r['mode']==s['mode'] and (s['kind']=='ALL' or r['kind']==s['kind'])]
    same(len(rs),s['queries'],'queries')
    same(sum(r['exact_count']>0 for r in rs),s['nonzero_queries'],'nonzero queries')
    for saved,key in [('T_wins_both_orders','raw_T_wins_both'),('P_wins_both_orders','raw_P_wins_both'),('T_wins_beyond_observed_controls','T_wins_beyond_controls'),('P_wins_beyond_observed_controls','P_wins_beyond_controls')]:
        same(sum(r[key] for r in rs),s[saved],saved)
    for order in ('AB','BA'):
        t=statistics.mean(r[f'tantivy_{order}_ns'] for r in rs)
        p=statistics.mean(r[f'port_{order}_ns'] for r in rs)
        same({'geomean_T_over_P':geo([r[f'T_over_P_{order}'] for r in rs]),'mean_query_median_T_ns':t,'mean_query_median_P_ns':p,'ratio_of_mean_query_medians':t/p,'controls':{label:geo([r[f'{label}_{order}'] for r in rs]) for label in ('TT','PP')}},s[order],'aggregate')
    for saved in s.get('per_round',[]):
        rnd,order=saved['round'],saved['order']
        ratios=lambda pair:[statistics.median(rounds[(pair,rnd,order,r['id'],s['mode'],'A')])/statistics.median(rounds[(pair,rnd,order,r['id'],s['mode'],'B')]) for r in rs]
        same({'round':rnd,'order':order,'geomean_T_over_P':geo(ratios(0)),'controls':{label:geo(ratios(pair)) for pair,label in ((1,'TT'),(2,'PP'))}},saved,'round')
same(obj('measure/rss.json'),summary['rss'],'RSS receipt')
for name in ('measure/samples.jsonl','per-query.csv','measure/schedule.json','queries.jsonl'):
    compressed=name+'.gz' if not (HERE/name).exists() else name
    check(hashlib.sha256(read(compressed)).hexdigest()==summary['files_sha256'][name],'summary file hash '+name)
print(json.dumps({'pass':True,'queries':len(queries),'matched_score_bits':bits,'cells':len(seen),'samples':len(seen)*32,'csv_rows':len(rows),'groups':len(summary['summaries'])}))
