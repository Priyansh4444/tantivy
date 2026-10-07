"""Strictly recompute the candidate/fixed-port schedule, without trimming rows."""
import collections,csv,json,math,statistics
from pathlib import Path
from stage import OUT,sha
folder=OUT/'port-measure';schedule=json.loads((folder/'schedule.json').read_text())
if not json.loads((folder/'receipt.json').read_text())['pass']:raise RuntimeError('port measurement gate')
queries={q['id']:q for q in map(json.loads,(OUT.parent/'lucene-rs-balanced-oct06/queries.jsonl').read_text().splitlines())}
proof={r['case']['id']:r for r in json.loads((OUT.parent/'lucene-rs-balanced-oct06/verify/dumps.json').read_text())}
cells=collections.defaultdict(list);seen=set()
for line in (folder/'samples.jsonl').read_text().splitlines():
 r=json.loads(line);k=(r['pair'],r['round'],r['order'],r['id'],r['side'])
 if k in seen:raise RuntimeError('duplicate cell')
 seen.add(k)
 if r['engine']!=schedule['pairs'][r['pair']][0 if r['side']=='A' else 1] or r['mode']!='top10':raise RuntimeError('wrong engine/mode')
 n=schedule['samples_per_order']
 if len(r['ns'])!=n or any(not isinstance(t,int) or t<=0 for t in r['ns']):raise RuntimeError('invalid timings')
 d=proof[r['id']][r['engine']]
 if r['checksum']!=sum(doc^bits for doc,bits in d['hits'])*n%2**64:raise RuntimeError('incorrect timed checksum')
 cells[r['pair'],r['order'],r['id'],r['side']].extend(r['ns'])
expected={(p,r,o,q,s) for p in range(len(schedule['pairs'])) for r in range(schedule['rounds']) for o in ('AB','BA') for q in schedule['queries'] for s in ('A','B')}
if seen!=expected:raise RuntimeError('missing/extra cells')
rows=[]
for qid in schedule['queries']:
 q=queries[qid];r={'id':qid,'set':'wiki20' if 'wiki20' in q['tags'] else 'port-style','kind':q['kind'],'terms':' '.join(q['terms'])}
 for o in ('AB','BA'):
  r[f'candidate_{o}_ns']=statistics.median(cells[0,o,qid,'A']);r[f'port_{o}_ns']=statistics.median(cells[0,o,qid,'B'])
  r[f'candidate_over_port_{o}']=r[f'candidate_{o}_ns']/r[f'port_{o}_ns']
  r[f'port_control_{o}']=statistics.median(cells[1,o,qid,'A'])/statistics.median(cells[1,o,qid,'B'])
 rows.append(r)
with (OUT/'port-per-query.csv').open('w') as f:
 w=csv.DictWriter(f,fieldnames=list(rows[0]));w.writeheader();w.writerows(rows)
geo=lambda xs:math.exp(statistics.mean(map(math.log,xs)))
summaries=[]
for suite in ('wiki20','port-style'):
 for kind in ('ALL','TERM','AND','OR','OR_3PLUS','OR_2','PHRASE'):
  rs=[r for r in rows if r['set']==suite and (kind=='ALL' or r['kind']==kind or kind=='OR_3PLUS' and r['kind']=='OR' and len(r['terms'].split())>=3 or kind=='OR_2' and r['kind']=='OR' and len(r['terms'].split())==2)]
  if not rs:continue
  s={'set':suite,'kind':kind,'queries':len(rs)}
  for o in ('AB','BA'):
   cm=statistics.mean(r[f'candidate_{o}_ns'] for r in rs);pm=statistics.mean(r[f'port_{o}_ns'] for r in rs)
   s[o]={'candidate_mean_query_median_ns':cm,'port_mean_query_median_ns':pm,'ratio_of_mean_query_medians_candidate_over_port':cm/pm,'geomean_candidate_over_port':geo([r[f'candidate_over_port_{o}'] for r in rs]),'port_control_geomean':geo([r[f'port_control_{o}'] for r in rs])}
  summaries.append(s)
r={'pass':True,'cells':len(seen),'samples':len(seen)*schedule['samples_per_order'],'ratio':'candidate Tantivy / fixed lucene-rs; below1 candidate faster','summaries':summaries,'rss':json.loads((folder/'rss.json').read_text()),'samples_sha256':sha(folder/'samples.jsonl'),'per_query_sha256':sha(OUT/'port-per-query.csv')}
(OUT/'port-summary.json').write_text(json.dumps(r,indent=2)+'\n')
for s in summaries:
 if s['kind'] in ('ALL','OR','OR_3PLUS'):print(json.dumps(s))
