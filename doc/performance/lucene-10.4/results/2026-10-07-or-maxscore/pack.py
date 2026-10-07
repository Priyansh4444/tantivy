"""Publish retained evidence only; no binaries, targets, or corpus/index payloads."""
import gzip,hashlib,json,shutil,subprocess
from pathlib import Path
OUT=Path(__file__).resolve().parent
REPO=OUT.parents[1]/'tantivy-or-maxscore-oct06'
DEST=REPO/'doc/performance/lucene-10.4/results/2026-10-07-or-maxscore'
DEST.mkdir(parents=True,exist_ok=False)
origins={}
def sha(data):return hashlib.sha256(data).hexdigest()
def copy(source,name=None):
 source=Path(source);name=name or str(source.relative_to(OUT));data=source.read_bytes()
 # Keep scripts/source and readable short receipts plain. Large retained data
 # is deterministic gzip; original content hashes remain independently bound.
 compressed=source.suffix not in ('.py','.rs','.md','.toml','.lock') and len(data)>65536
 target_name=name+'.gz' if compressed else name
 target=DEST/target_name;target.parent.mkdir(parents=True,exist_ok=True)
 target.write_bytes(gzip.compress(data,compresslevel=6,mtime=0) if compressed else data)
 origins[target_name]={'original_sha256':sha(data),'original_bytes':len(data),'original_path':str(source)}
root_names=['analyze.py','analyze_port.py','balanced.py','configured_gate.py','pilot.py','port_measure.py','profile.py','stage.py','pack.py','recompute.py','efficiency_capture.py','baseline-provenance.json','baseline-perf.data','baseline-perf-report.txt','baseline-perf.stderr','baseline-profile-receipt.json','baseline-profile-replies.json','candidate-perf.data','candidate-perf-report.txt','candidate-perf.stderr','candidate-profile-receipt.json','candidate-profile-replies.json','profile-cases.json','grounding.md','design-essential.md','design-windows.md','judge.md','synthesis.md','attempt1-decision.md','summary.json','per-query.csv','port-summary.json','port-per-query.csv','efficiency.json','results-review.md']
for name in root_names:copy(OUT/name)
copy(OUT/'write_report.py')
for stage in ['harness','build-final','unit-tests','tests','guard-regression','clippy','formatting','verify','configured-k0.9-b0.4','configured-k2-b1','measure','port-measure','pilot-final']:
 for source in sorted((OUT/stage).rglob('*')):
  if source.is_file():
   if source.name=='telemetry.json':
    entries=json.loads(source.read_text());name=str(source.relative_to(OUT)).replace('telemetry.json','telemetry-summary.json');target=DEST/name;target.parent.mkdir(parents=True,exist_ok=True)
    target.write_text(json.dumps([{'at':v['at'],'loadavg':v['loadavg']} for v in entries],indent=2)+'\n')
    origins[name]={'derived_from_sha256':sha(source.read_bytes()),'omitted':'process inventory retained locally'}
   else:copy(source)
for attempt in ['attempt1','attempt2']:
 for source in sorted((OUT/attempt).rglob('*')):
  if source.is_file() and (source.name=='or_maxscore.rs' or source.name=='decision.md' or source.name.endswith('receipt.json') or source.name=='failures.json' or source.name.endswith('perf-report.txt')):copy(source)
old=OUT.parent/'lucene-rs-balanced-oct06'
for name,target in [('queries.jsonl','queries.jsonl'),('verify/dumps.json','references/baseline-and-port-dumps.json'),('audit-port/stdout.log','references/port-payload-audit.json'),('audit-port/receipt.json','references/port-payload-receipt.json'),('audit-port/index-after.json','references/port-index.json'),('harness/src/bin/port_worker.rs','references/port_worker.rs')]:copy(old/name,target)
for name in ['src/query/boolean_query/or_maxscore.rs','src/query/boolean_query/boolean_weight.rs','src/query/boolean_query/mod.rs','tests/query_pruning_correctness.rs']:copy(REPO/name,'compiled-source/'+name)
copy(REPO/'Cargo.lock','test-Cargo.lock')
copy(OUT/'README.md','README.md')
(DEST/'copy-origins.json').write_text(json.dumps(origins,indent=2,sort_keys=True)+'\n')
head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=REPO,text=True).strip()
manifest={'core_commit':head,'baseline_commit':'c0efbc99c2bcb8e6e7c0d6679a8dc9b7071744ac','fixed_port_commit':'e5d1f81bff42c87886b12770f3c79648bb1963ed','files':{str(p.relative_to(DEST)):{'bytes':p.stat().st_size,'sha256':sha(p.read_bytes())} for p in sorted(DEST.rglob('*')) if p.is_file()}}
(DEST/'packet-manifest.json').write_text(json.dumps(manifest,indent=2,sort_keys=True)+'\n')
print(json.dumps({'files':len(manifest['files'])+1,'bytes':sum(v['bytes'] for v in manifest['files'].values()),'destination':str(DEST),'core_commit':head}))
