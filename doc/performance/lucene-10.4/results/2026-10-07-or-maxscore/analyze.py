"""Check the complete frozen schedule and summarize all untrimmed native samples."""
import collections,csv,json,math,statistics
from pathlib import Path
from stage import OUT,sha

folder=OUT/'measure'
receipt=json.loads((folder/'receipt.json').read_text())
if not receipt['pass'] or sha(folder/'samples.jsonl')!=receipt['samples_sha256']:raise RuntimeError('timing receipt')
schedule=json.loads((folder/'schedule.json').read_text())
queries={q['id']:q for q in map(json.loads,(OUT.parent/'lucene-rs-balanced-oct06/queries.jsonl').read_text().splitlines())}
dumps={row['case']['id']:row for row in json.loads((OUT/'verify/dumps.json').read_text())}
cells=collections.defaultdict(list);rounds=collections.defaultdict(list);seen=set()
for line in (folder/'samples.jsonl').read_text().splitlines():
    r=json.loads(line)
    key=(r['pair'],r['round'],r['order'],r['id'],r['mode'],r['side'])
    if key in seen:raise RuntimeError('duplicate schedule cell')
    seen.add(key)
    if r['engine']!=schedule['pairs'][r['pair']][0 if r['side']=='A' else 1]:raise RuntimeError('wrong engine')
    if len(r['ns'])!=schedule['samples_per_order'] or any(not isinstance(n,int) or n<=0 for n in r['ns']):raise RuntimeError('bad samples')
    dump=dumps[r['id']][r['engine']]
    total=dump['count'] if r['mode']=='count' else sum(doc^bits for doc,bits in dump['hits'])
    if r['checksum']!=(total*schedule['samples_per_order'])%2**64:raise RuntimeError('incorrect timed checksum')
    cells[(r['pair'],r['order'],r['id'],r['mode'],r['side'])].extend(r['ns'])
    rounds[(r['pair'],r['round'],r['order'],r['id'],r['mode'],r['side'])].extend(r['ns'])
expected={(p,r,o,q,m,s) for p in range(3) for r in range(3) for o in ('AB','BA') for q in schedule['queries'] for m in ('count','top10') for s in ('A','B')}
if seen!=expected:raise RuntimeError('missing/extra schedule cells')
geo=lambda xs:math.exp(statistics.mean(math.log(x) for x in xs))
rows=[]
for qid in schedule['queries']:
    q=queries[qid]
    for mode in ('count','top10'):
        row={'id':qid,'kind':q['kind'],'terms':' '.join(q['terms']),'set':'wiki20' if 'wiki20' in q['tags'] else 'port-style','mode':mode,'exact_count':dumps[qid]['tantivy']['count']}
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
with (OUT/'per-query.csv').open('w') as f:
    writer=csv.DictWriter(f,fieldnames=list(rows[0]));writer.writeheader();writer.writerows(rows)
summaries=[]
for query_set in ('wiki20','port-style'):
    for mode in ('count','top10'):
        for kind in ('ALL','TERM','AND','OR','OR_3PLUS','OR_2','PHRASE'):
            rs=[r for r in rows if r['set']==query_set and r['mode']==mode and (kind=='ALL' or r['kind']==kind or (kind=='OR_3PLUS' and r['kind']=='OR' and len(r['terms'].split())>=3) or (kind=='OR_2' and r['kind']=='OR' and len(r['terms'].split())==2))]
            if not rs:continue
            result={'set':query_set,'mode':mode,'kind':kind,'queries':len(rs),'nonzero_queries':sum(r['exact_count']>0 for r in rs),
                'T_wins_both_orders':sum(r['raw_T_wins_both'] for r in rs),'P_wins_both_orders':sum(r['raw_P_wins_both'] for r in rs),
                'T_wins_beyond_observed_controls':sum(r['T_wins_beyond_controls'] for r in rs),'P_wins_beyond_observed_controls':sum(r['P_wins_beyond_controls'] for r in rs)}
            for order in ('AB','BA'):
                result[order]={'geomean_T_over_P':geo([r[f'T_over_P_{order}'] for r in rs]),
                    'mean_query_median_T_ns':statistics.mean(r[f'tantivy_{order}_ns'] for r in rs),
                    'mean_query_median_P_ns':statistics.mean(r[f'port_{order}_ns'] for r in rs),
                    'ratio_of_mean_query_medians':statistics.mean(r[f'tantivy_{order}_ns'] for r in rs)/statistics.mean(r[f'port_{order}_ns'] for r in rs),
                    'controls':{label:geo([r[f'{label}_{order}'] for r in rs]) for label in ('TT','PP')}}
            if kind=='ALL':
                result['per_round']=[{'round':rnd,'order':order,
                    'geomean_T_over_P':geo([statistics.median(rounds[(0,rnd,order,r['id'],mode,'A')])/statistics.median(rounds[(0,rnd,order,r['id'],mode,'B')]) for r in rs]),
                    'controls':{label:geo([statistics.median(rounds[(pair,rnd,order,r['id'],mode,'A')])/statistics.median(rounds[(pair,rnd,order,r['id'],mode,'B')]) for r in rs]) for pair,label in ((1,'TT'),(2,'PP'))}}
                    for rnd in range(3) for order in ('AB','BA')]
            summaries.append(result)
data={'protocol':'or-improvement-native-api-v1','ratio':'candidate / preserved baseline Tantivy; below1 means candidate faster. port/P labels are baseline Tantivy for script compatibility',
    'complete_cells':len(seen),'raw_samples':len(seen)*schedule['samples_per_order'],'queries_timed':len(schedule['queries']),
    'correctness_queries':len(queries),'full_postings_and_positions_identity':True,
    'no_trim_no_cherry_pick':True,'summaries':summaries,'rss':json.loads((folder/'rss.json').read_text()),
    'wall_seconds':receipt['wall_seconds'],'source_binary_index_guard':True,
    'files_sha256':{str(p):sha(p) for p in [folder/'samples.jsonl',OUT/'per-query.csv',folder/'schedule.json',folder/'telemetry.json',OUT.parent/'lucene-rs-balanced-oct06/queries.jsonl']}}
(OUT/'summary.json').write_text(json.dumps(data,indent=2)+'\n')
for s in summaries:
    if s['kind']=='ALL':print(json.dumps(s))
print(json.dumps({'cells':len(seen),'samples':data['raw_samples'],'rss':data['rss']}))
