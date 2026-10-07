"""Audit this packet using only Python's standard library, without search engines."""
import collections,csv,gzip,hashlib,json,math,re,statistics,struct
from pathlib import Path
HERE=Path(__file__).resolve().parent

def check(ok,label):
 if not ok:raise ValueError(label)
def read(name):
 p=HERE/name
 if not p.exists():p=HERE/(name+'.gz')
 return gzip.decompress(p.read_bytes()) if p.suffix=='.gz' else p.read_bytes()
def obj(name):return json.loads(read(name))
def digest(data):return hashlib.sha256(data).hexdigest()
def same(actual,expected,label):
 if isinstance(expected,dict):
  check(actual.keys()==expected.keys(),label+' keys')
  for k,v in expected.items():same(actual[k],v,label+'/'+str(k))
 elif isinstance(expected,list):
  check(len(actual)==len(expected),label+' length')
  for i,(a,b) in enumerate(zip(actual,expected)):same(a,b,label+'/'+str(i))
 elif isinstance(expected,float):check(math.isclose(actual,expected,rel_tol=1e-12,abs_tol=1e-15),label+' numerical mismatch')
 else:check(actual==expected,label+' mismatch')
def audit_csv(name,rows):
 saved=list(csv.DictReader(read(name).decode().splitlines()))
 check(len(saved)==len(rows),name+' rows')
 for a,b in zip(rows,saved):
  check(a.keys()==b.keys(),name+' columns')
  for k,v in a.items():
   decoded=float(b[k]) if type(v) is float else int(b[k]) if type(v) is int else b[k]=='True' if type(v) is bool else b[k]
   same(v,decoded,name+'/'+k)
def select(rows,s):
 return [r for r in rows if r['set']==s['set'] and ('mode' not in s or r['mode']==s['mode']) and (s['kind']=='ALL' or r['kind']==s['kind'] or s['kind']=='OR_3PLUS' and r['kind']=='OR' and len(r['terms'].split())>=3 or s['kind']=='OR_2' and r['kind']=='OR' and len(r['terms'].split())==2)]
geo=lambda xs:math.exp(statistics.mean(math.log(x) for x in xs))
manifest=obj('packet-manifest.json')
check({str(p.relative_to(HERE)) for p in HERE.rglob('*') if p.is_file() and p.name!='packet-manifest.json'}==set(manifest['files']),'complete manifest')
for name,v in manifest['files'].items():
 data=(HERE/name).read_bytes();check(len(data)==v['bytes'] and digest(data)==v['sha256'],'archive '+name)
for name,v in obj('copy-origins.json').items():
 if 'original_sha256' in v:check(digest(read(name))==v['original_sha256'] and len(read(name))==v['original_bytes'],'original '+name)
queries={q['id']:q for q in map(json.loads,read('queries.jsonl').splitlines())}
check(len(queries)==1227 and set(queries)==set(range(1227)),'query coverage')
references={r['case']['id']:r for r in obj('references/baseline-and-port-dumps.json')}
verified={r['case']['id']:r for r in obj('verify/dumps.json')}
check(len(verified)==1227 and set(verified)==set(queries),'default result coverage')
score_bits=0
for qid,r in verified.items():
 check(r['case']==queries[qid] and references[qid]['case']==queries[qid],'case identity')
 a,b,c=r['tantivy'],r['port'],references[qid]['port']
 check(a['count']==b['count']==c['count'] and a['hits']==b['hits']==c['hits'],'candidate/baseline/fixed-port exact results')
 for d in [a,b,c]:
  check(d['id']==qid and type(d['count']) is int and 0<=d['count']<=1000000,'result identity/count')
  check(d['count']==d['oracle']['count'] and d['hits']==d['oracle']['hits'],'exhaustive oracle')
  check(len(d['hits'])==min(10,d['count']) and len({h[0] for h in d['hits']})==len(d['hits']),'top10 shape')
  check(all(type(doc) is int and 0<=doc<1000000 and type(bits) is int and 0<=bits<2**32 for doc,bits in d['hits']),'hit ranges')
  scores=[struct.unpack('<f',struct.pack('<I',bits))[0] for _,bits in d['hits']]
  check(all(math.isfinite(s) for s in scores),'finite scores')
  keys=[(-s,h[0]) for s,h in zip(scores,d['hits'])];check(keys==sorted(keys),'score/doc ordering')
 score_bits+=len(a['hits'])
check(score_bits==11509,'default raw bits')
check(obj('verify/receipt.json')['pass'] and obj('verify/failures.json')==[],'default gate')
base_guard=obj('verify/after.json')
for name in ['verify/before.json','measure/before.json','measure/after.json']:same(obj(name),base_guard,'identity '+name)
for name in ['src/query/boolean_query/or_maxscore.rs','src/query/boolean_query/boolean_weight.rs','src/query/boolean_query/mod.rs','tests/query_pruning_correctness.rs']:
 compiled_hash=base_guard['source']['tantivy-or-refinement-oct07']['files'][name]
 check(digest(read('compiled-source/'+name))==compiled_hash,'compiled source '+name)
 check(obj('build-reviewed/after.json')['tantivy-or-refinement-oct07']['files'][name]==compiled_hash,'build source '+name)
 check(obj('tests-reviewed/before.json')['tantivy-or-refinement-oct07']['files'][name]==compiled_hash,'tested source '+name)
for name in ['build-reviewed','tests-reviewed','layout','clippy-reviewed','formatting']:
 r=obj(name+'/receipt.json');check(r['returncode']==0 and r['source_unchanged'],'stage '+name)
 check(digest(read(name+'/stdout.log'))==r['stdout_sha256'] and digest(read(name+'/stderr.log'))==r['stderr_sha256'],'stage logs '+name)
 check(obj(name+'/before.json')==obj(name+'/after.json'),'stage guard '+name)
counts=[tuple(map(int,m)) for m in re.findall(rb'test result: ok\. (\d+) passed; \d+ failed; (\d+) ignored;',read('tests-reviewed/stdout.log'))]
check(sum(p for p,_ in counts)==1428 and sum(i for _,i in counts)==7,'test totals')
tr=obj('tests-reviewed/receipt.json');check(tr['returncode']==0 and tr['source_unchanged'],'full tested source')
check(obj('tests-reviewed/before.json')==obj('tests-reviewed/after.json'),'full test guard')
check(b'ClauseState bytes=24; max-32 array payload bytes=1160' in read('layout/stderr.log'),'actual scratch layout')
for name in ['configured-k0.9-b0.4','configured-k2-b1']:
 r=obj(name+'/receipt.json');check(r['pass'] and r['queries']==1227 and r['failures']==0,'configured gate '+name)
 expected_bits=('3f666666','3ecccccd') if name=='configured-k0.9-b0.4' else ('40000000','3f800000')
 check((r['ready']['k1_bits'],r['ready']['b_bits'])==expected_bits,'actual configured parameters')
 check(obj(name+'/before.json')==obj(name+'/after.json'),'configured guard '+name)
 ds=obj(name+'/dumps.json');check(len(ds)==1227,'configured coverage')
 for q,row in zip(queries.values(),ds):
  d=row['tantivy']
  check(row['case']==q and d['id']==q['id'] and d['count']==d['oracle']['count'] and d['hits']==d['oracle']['hits'],'configured exact output')
payload=obj('references/port-payload-audit.json')
check(payload['payload_sha256']=='893a75958e6d15c997d414d5fb5fc47c633aa18c68fcd33bc0f8e13abfa8dbb8' and payload['integrity']=='pass' and payload['positions']==294827020,'full payload proof')
check(obj('references/port-payload-receipt.json')['returncode']==0,'payload stage')
check(obj('baseline-provenance.json')['pass'],'preserved baseline provenance')

summary=obj('summary.json');schedule=obj('measure/schedule.json')
check(schedule['pairs']==[['tantivy','port'],['tantivy','tantivy'],['port','port']] and schedule['rounds']==3 and schedule['samples_per_order']==16,'main schedule contract')
check(schedule['queries']==list(range(1221)),'timed corpus coverage')
raw=read('measure/samples.jsonl');check(digest(raw)==obj('measure/receipt.json')['samples_sha256'],'main sample hash')
cells=collections.defaultdict(list);rounds={};seen=set()
for line in raw.splitlines():
 r=json.loads(line);k=(r['pair'],r['round'],r['order'],r['id'],r['mode'],r['side'])
 check(k not in seen,'duplicate main cell');seen.add(k)
 check(r['engine']==schedule['pairs'][r['pair']][0 if r['side']=='A' else 1],'main engine')
 check(len(r['ns'])==16 and all(type(n) is int and n>0 for n in r['ns']),'main timings')
 d=verified[r['id']][r['engine']];v=d['count'] if r['mode']=='count' else sum(doc^bits for doc,bits in d['hits'])
 check(r['checksum']==v*16%2**64,'main checksum')
 cells[r['pair'],r['order'],r['id'],r['mode'],r['side']].extend(r['ns']);rounds[k]=r['ns']
expected={(p,r,o,q,m,s) for p in range(3) for r in range(3) for o in ['AB','BA'] for q in schedule['queries'] for m in ['count','top10'] for s in ['A','B']}
check(seen==expected and len(seen)==summary['complete_cells']==87912 and len(seen)*16==summary['raw_samples']==1406592,'main complete schedule')
rows=[]
for qid in schedule['queries']:
 q=queries[qid]
 for mode in ['count','top10']:
  r={'id':qid,'kind':q['kind'],'terms':' '.join(q['terms']),'set':'wiki20' if 'wiki20' in q['tags'] else 'port-style','mode':mode,'exact_count':verified[qid]['tantivy']['count']}
  for o in ['AB','BA']:
   for side,engine in [('A','tantivy'),('B','port')]:r[f'{engine}_{o}_ns']=statistics.median(cells[0,o,qid,mode,side])
   r[f'T_over_P_{o}']=r[f'tantivy_{o}_ns']/r[f'port_{o}_ns']
   for pair,label in [(1,'TT'),(2,'PP')]:r[f'{label}_{o}']=statistics.median(cells[pair,o,qid,mode,'A'])/statistics.median(cells[pair,o,qid,mode,'B'])
  noise=max(abs(math.log(r[f'{label}_{o}'])) for label in ['TT','PP'] for o in ['AB','BA']);r['control_log_spread']=noise
  r['raw_T_wins_both']=all(r[f'T_over_P_{o}']<1 for o in ['AB','BA']);r['raw_P_wins_both']=all(r[f'T_over_P_{o}']>1 for o in ['AB','BA'])
  r['T_wins_beyond_controls']=all(math.log(r[f'T_over_P_{o}'])<-noise for o in ['AB','BA']);r['P_wins_beyond_controls']=all(math.log(r[f'T_over_P_{o}'])>noise for o in ['AB','BA']);rows.append(r)
audit_csv('per-query.csv',rows)
for s in summary['summaries']:
 rs=select(rows,s);check(len(rs)==s['queries'] and sum(r['exact_count']>0 for r in rs)==s['nonzero_queries'],'main group size')
 for saved,k in [('T_wins_both_orders','raw_T_wins_both'),('P_wins_both_orders','raw_P_wins_both'),('T_wins_beyond_observed_controls','T_wins_beyond_controls'),('P_wins_beyond_observed_controls','P_wins_beyond_controls')]:check(sum(r[k] for r in rs)==s[saved],'group wins')
 for o in ['AB','BA']:
  t=statistics.mean(r[f'tantivy_{o}_ns'] for r in rs);p=statistics.mean(r[f'port_{o}_ns'] for r in rs)
  same({'geomean_T_over_P':geo([r[f'T_over_P_{o}'] for r in rs]),'mean_query_median_T_ns':t,'mean_query_median_P_ns':p,'ratio_of_mean_query_medians':t/p,'controls':{label:geo([r[f'{label}_{o}'] for r in rs]) for label in ['TT','PP']}},s[o],'main aggregate')
 for saved in s.get('per_round',[]):
  rnd,o=saved['round'],saved['order'];ratios=lambda pair:[statistics.median(rounds[pair,rnd,o,r['id'],s['mode'],'A'])/statistics.median(rounds[pair,rnd,o,r['id'],s['mode'],'B']) for r in rs]
  same({'round':rnd,'order':o,'geomean_T_over_P':geo(ratios(0)),'controls':{label:geo(ratios(pair)) for pair,label in [(1,'TT'),(2,'PP')]}},saved,'main round')
check(summary['rss']==obj('measure/rss.json') and summary['wall_seconds']==obj('measure/receipt.json')['wall_seconds'],'main metadata')
for name,value in summary['files_sha256'].items():
 if name.endswith('telemetry.json'):check(value==obj('copy-origins.json')['measure/telemetry-summary.json']['derived_from_sha256'],'original telemetry hash')
 else:
  short='measure/'+Path(name).name if '/measure/' in name else Path(name).name
  check(digest(read(short))==value,'summary file '+short)

affected_ids=[i for i in schedule['queries'] if queries[i]['kind']=='OR' and len(queries[i]['terms'])>=3]
check(len(affected_ids)==103,'affected scope')
affected_rounds=[]
for rnd in range(3):
 for o in ['AB','BA']:
  def group(pair):
   a=statistics.mean(statistics.median(rounds[pair,rnd,o,i,'top10','A']) for i in affected_ids)
   b=statistics.mean(statistics.median(rounds[pair,rnd,o,i,'top10','B']) for i in affected_ids)
   return {'A_mean_ns':a,'B_mean_ns':b,'ratio':a/b,'geo':geo([statistics.median(rounds[pair,rnd,o,i,'top10','A'])/statistics.median(rounds[pair,rnd,o,i,'top10','B']) for i in affected_ids])}
  affected_rounds.append({'round':rnd,'order':o,'candidate_vs_baseline':group(0),'candidate_self':group(1),'baseline_self':group(2)})
same(affected_rounds,obj('affected-rounds.json'),'affected round aggregates')
minimum_gain=min(1-r['candidate_vs_baseline']['ratio'] for r in affected_rounds)
maximum_control=max(abs(1-r[k]['ratio']) for r in affected_rounds for k in ['candidate_self','baseline_self'])
check(minimum_gain>maximum_control,'affected group gain exceeds observed group controls')

ps=obj('port-summary.json');sc=obj('port-measure/schedule.json');check(sc['pairs']==[['tantivy','port'],['port','port']] and sc['rounds']==3 and sc['samples_per_order']==16 and sc['queries']==list(range(1221)),'port schedule')
check(obj('port-measure/receipt.json')['pass'] and obj('port-measure/before.json')==obj('port-measure/after.json'),'port guard')
pg=obj('port-measure/after.json');check(pg['indices']['tantivy']==base_guard['indices']['tantivy'] and pg['indices']['port']==obj('references/port-index.json'),'both index identities')
check(pg['source']==base_guard['source'] and pg['binaries']['tantivy']==base_guard['binaries']['tantivy'],'candidate shared identity')
raw=read('port-measure/samples.jsonl');check(digest(raw)==ps['samples_sha256'],'port sample hash')
cells=collections.defaultdict(list);seen=set()
for line in raw.splitlines():
 r=json.loads(line);k=(r['pair'],r['round'],r['order'],r['id'],r['side']);check(k not in seen,'duplicate port cell');seen.add(k)
 check(r['engine']==sc['pairs'][r['pair']][0 if r['side']=='A' else 1] and r['mode']=='top10','port engine/mode')
 check(len(r['ns'])==16 and all(type(n) is int and n>0 for n in r['ns']),'port timings')
 d=references[r['id']][r['engine']];check(r['checksum']==sum(doc^bits for doc,bits in d['hits'])*16%2**64,'port checksum')
 cells[r['pair'],r['order'],r['id'],r['side']].extend(r['ns'])
expected={(p,r,o,q,s) for p in range(2) for r in range(3) for o in ['AB','BA'] for q in sc['queries'] for s in ['A','B']}
check(seen==expected and len(seen)==ps['cells']==29304 and len(seen)*16==ps['samples']==468864,'complete port schedule')
rows=[]
for qid in sc['queries']:
 q=queries[qid];r={'id':qid,'set':'wiki20' if 'wiki20' in q['tags'] else 'port-style','kind':q['kind'],'terms':' '.join(q['terms'])}
 for o in ['AB','BA']:
  r[f'candidate_{o}_ns']=statistics.median(cells[0,o,qid,'A']);r[f'port_{o}_ns']=statistics.median(cells[0,o,qid,'B'])
  r[f'candidate_over_port_{o}']=r[f'candidate_{o}_ns']/r[f'port_{o}_ns'];r[f'port_control_{o}']=statistics.median(cells[1,o,qid,'A'])/statistics.median(cells[1,o,qid,'B'])
 rows.append(r)
audit_csv('port-per-query.csv',rows);check(digest(read('port-per-query.csv'))==ps['per_query_sha256'],'port CSV hash')
for s in ps['summaries']:
 rs=select(rows,s);check(len(rs)==s['queries'],'port group size')
 for o in ['AB','BA']:
  c=statistics.mean(r[f'candidate_{o}_ns'] for r in rs);p=statistics.mean(r[f'port_{o}_ns'] for r in rs)
  same({'candidate_mean_query_median_ns':c,'port_mean_query_median_ns':p,'ratio_of_mean_query_medians_candidate_over_port':c/p,'geomean_candidate_over_port':geo([r[f'candidate_over_port_{o}'] for r in rs]),'port_control_geomean':geo([r[f'port_control_{o}'] for r in rs])},s[o],'port aggregate')
check(ps['rss']==obj('port-measure/rss.json'),'port RSS')
for prefix in ['baseline','candidate']:
 receipt=obj(prefix+'-profile-receipt.json');check(receipt['returncode']==0 and receipt['correctness_checksums_match'],'profile '+prefix)
 engine='port' if prefix=='baseline' else 'tantivy'
 check(receipt['binary_sha256']==base_guard['binaries'][engine],'profile binary identity')
 replies=obj(prefix+'-profile-replies.json');check(len(replies)==21,'profile requests')
 for v in replies[1:]:
  n=len(v['ns']);d=verified[v['id']]['tantivy'];check(v['checksum']==sum(doc^bits for doc,bits in d['hits'])*n%2**64,'profile checksum')
efficiency=obj('efficiency.json')
for label,engine in [('baseline','port'),('candidate','tantivy')]:check(efficiency['binaries'][label]['sha256']==base_guard['binaries'][engine],'efficiency binary identity')
for label,key in [('file_bytes','candidate_minus_baseline_file_bytes'),('text','candidate_minus_baseline_text_bytes')]:
 values=[efficiency['binaries'][b]['file_bytes'] if label=='file_bytes' else efficiency['binaries'][b]['sections']['.text'] for b in ['candidate','baseline']]
 check(values[0]-values[1]==efficiency[key],'code size delta')
check(efficiency['warm_rss']['candidate_baseline']==summary['rss'] and efficiency['warm_rss']['candidate_fixed_port']==ps['rss'],'efficiency RSS')
check(efficiency['scratch']['max_array_payload_bytes']==1160 and efficiency['scratch']['baseline_max_array_payload_bytes']==776 and efficiency['scratch']['vector_header_bytes_x86_64']==72,'scratch accounting')
print(json.dumps({'pass':True,'correctness_queries':1227,'default_matched_score_bits':score_bits,'configured_profiles':2,'tests_passed':1428,'tests_ignored':7,'main_cells':87912,'main_samples':1406592,'port_cells':29304,'port_samples':468864,'all_published_csv_and_summary_values_recomputed':True}))
