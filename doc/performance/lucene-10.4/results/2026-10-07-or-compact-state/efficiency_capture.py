import json,subprocess
from pathlib import Path
from balanced import OUT,identity
from stage import sha
before=json.loads((OUT/'measure/after.json').read_text())
if identity()!=before:raise RuntimeError('final artifact guard changed before publication')
binaries={'baseline':OUT.parent/'lucene-rs-or-improvement-oct06/target/release/tantivy_worker','candidate':OUT/'target/release/tantivy_worker'}
def sections(p):
 result={}
 for line in subprocess.check_output(['size','-A',str(p)],text=True).splitlines():
  fields=line.split()
  if fields and fields[0].startswith('.') and len(fields)>=2:result[fields[0]]=int(fields[1])
 return result
b={k:{'path':str(p),'sha256':sha(p),'file_bytes':p.stat().st_size,'sections':sections(p)} for k,p in binaries.items()}
r={'binaries':b,'candidate_minus_baseline_file_bytes':b['candidate']['file_bytes']-b['baseline']['file_bytes'],'candidate_minus_baseline_text_bytes':b['candidate']['sections']['.text']-b['baseline']['sections']['.text'],'scratch':{'array_payload_formula_bytes':'36*n + 8 (ClauseState=24, prefix=8, contribution=4)','max_clauses':32,'max_array_payload_bytes':1160,'baseline_max_array_payload_bytes':776,'candidate_minus_baseline_array_payload_bytes':384,'vector_header_bytes_x86_64':72,'baseline_vector_header_bytes_x86_64':96,'excludes':'allocator overhead and the existing incoming scorer vector; three once-per-query allocations (baseline four), none per region'},'index_format_changed':False,'all_index_file_hashes_unchanged':True,'warm_rss':{'candidate_baseline':json.loads((OUT/'measure/rss.json').read_text()),'candidate_fixed_port':json.loads((OUT/'port-measure/rss.json').read_text())},'rss_interpretation':'process snapshots after equal warmup, not incremental scratch, allocation rate, or a whole-lifecycle memory proof','source_files':{p:before['source']['tantivy-or-refinement-oct07']['files'][p] for p in ['src/query/boolean_query/or_maxscore.rs','src/query/boolean_query/boolean_weight.rs','src/query/boolean_query/mod.rs','tests/query_pruning_correctness.rs']}}
layout=(OUT/'layout/stderr.log').read_text()
if 'ClauseState bytes=24; max-32 array payload bytes=1160' not in layout:raise RuntimeError('actual scratch layout')
r['layout_receipt_sha256']=sha(OUT/'layout/receipt.json')
(OUT/'efficiency.json').write_text(json.dumps(r,indent=2)+'\n');print(json.dumps({k:v for k,v in r.items() if k not in ('binaries','warm_rss','source_files')}))
