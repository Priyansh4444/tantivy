#!/usr/bin/env python3
"""Verify pinned source references and create bounded parity worklists."""
import argparse
from collections import Counter
import csv
import hashlib
import json
from pathlib import Path
import subprocess

ROOT=Path(__file__).resolve().parent
def main():
    p=argparse.ArgumentParser();p.add_argument('--tantivy-repo',required=True);a=p.parse_args()
    manifest=json.loads((ROOT/'manifest.json').read_text());rev=manifest['tantivy_commit']
    families=json.loads((ROOT/'feature_families.json').read_text())
    inventory=json.loads((ROOT/'inventory.json').read_text())
    def git(*args):return subprocess.check_output(['git','-C',a.tantivy_repo,*args])
    paths=set(git('ls-tree','-r','--name-only',rev).decode().splitlines())
    invalid=sorted({s for f in families for s in f['sources'] if s not in paths})
    if invalid:raise ValueError('missing pinned source references: '+repr(invalid))
    evidence={}
    for s in sorted({s for f in families for s in f['sources']}):
        blob=git('show',rev+':'+s)
        evidence[s]={'commit':rev,'sha256':hashlib.sha256(blob).hexdigest(),
                     'bytes':len(blob),'verification':'file content inspected; no behavioral tests executed'}
    module_ids={r['module'] for r in inventory['modules']}
    covered={m for f in families for m in f['modules']}
    if module_ids-covered:raise ValueError('unaccounted modules: '+repr(module_ids-covered))
    if covered-module_ids:raise ValueError('unknown modules: '+repr(covered-module_ids))
    rows=[]
    for f in families:
        rows.append({**f,'modules':';'.join(f['modules']),'sources':';'.join(f['sources']),
                     'java_facade_status':'missing','index_format_status':
                     'incompatible' if f['id'] in ['index-file-compatibility','codec-formats','index-lifecycle','directories','misc-tools','replicator','luke'] else 'not established / not directly applicable',
                     'tested_status':'not tested by this inventory pass',
                     'source_status':'verified source references; family absence scoped to built-in pinned tree'})
    with (ROOT/'feature-parity.tsv').open('w',newline='') as h:
        w=csv.DictWriter(h,list(rows[0]),delimiter='\t', lineterminator='\n');w.writeheader();w.writerows(rows)
    modules=[]
    for m in inventory['modules']:
        matches=[f for f in families if m['module'] in f['modules']]
        modules.append({'module':m['module'],'families':';'.join(f['id'] for f in matches),
                        'accounted':True,'behavioral_parity':'not exhaustively verified',
                        'java_facade':'missing','declared_members':m['declared_public_members']+m['declared_protected_members']})
    with (ROOT/'module-coverage.tsv').open('w',newline='') as h:
        w=csv.DictWriter(h,list(modules[0]),delimiter='\t', lineterminator='\n');w.writeheader();w.writerows(modules)
    # A complete declaration worklist, not an equivalence assertion.
    work=[]
    for r in inventory['members']:
        key='\0'.join([r['module'],r['type'],r['declaration'],r['descriptor']])
        work.append({'api_id':hashlib.sha256(key.encode()).hexdigest()[:20],**r,
                     'behavior_status':'unverified','rust_api_status':'unverified',
                     'java_facade_status':'missing','tested_status':'not tested',
                     'mapping_scope':'family matrix is coarse; member contract not mapped'})
    if len({r['api_id'] for r in work})!=len(work):raise ValueError('duplicate API keys')
    with (ROOT/'api-worklist.tsv').open('w',newline='') as h:
        w=csv.DictWriter(h,list(work[0]),delimiter='\t', lineterminator='\n');w.writeheader();w.writerows(work)
    result={'lucene_commit':manifest['lucene_commit'],'tantivy_commit':rev,'source_evidence':evidence,
            'generator_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
            'family_definitions_sha256':hashlib.sha256((ROOT/'feature_families.json').read_bytes()).hexdigest(),
            'feature_families':families,'module_coverage':modules,
            'remaining_work':{'unaccounted_modules':0,'unextracted_documented_types':manifest['counts']['documented_types_not_binary_extracted'],
                'feature_families_without_exhaustive_behavioral_verification':len(families),
                'type_contracts_without_member_level_parity_review':len(inventory['types']),
                'declared_member_contracts_without_differential_tests':len(work),
                'java_facade_implemented':False,'lucene_index_read_write_implemented':False},
            'family_status_counts':dict(Counter(f['behavior_status'] for f in families)),
            'scope_notes':['Missing means no built-in equivalent identified in the pinned source; external plugins are outside the audit.',
                           'Verified refers only to pinned source evidence. No family is marked verified behavioral parity.',
                           'Per-member worklist includes binary-visible extras and compiler bridges; refine exported/source-reachable scope before compatibility certification.']}
    (ROOT/'parity-matrix.json').write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps({k:result[k] for k in ['remaining_work','family_status_counts']},indent=2))
if __name__=='__main__':main()
