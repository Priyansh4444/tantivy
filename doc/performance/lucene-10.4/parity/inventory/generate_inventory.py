#!/usr/bin/env python3
"""Fetch pinned Lucene documentation/artifacts and inventory API declarations.
No compilation, class execution, repository checkout, or production writes.
"""
import argparse
import concurrent.futures
import csv
import hashlib
from html.parser import HTMLParser
import json
from pathlib import Path
import re
import subprocess
import time
import urllib.error
import urllib.request
import zipfile

VERSION = '10.4.0'
BASE = 'https://lucene.apache.org/core/10_4_0/'
TAG = 'releases/lucene/10.4.0'
ROOT = Path(__file__).resolve().parent

class Catalog(HTMLParser):
    def __init__(self):
        super().__init__(); self.modules = []
    def handle_starttag(self, tag, attrs):
        href = dict(attrs).get('href', '')
        if tag == 'a' and href.endswith('/index.html') and not href.startswith(('http', '../')):
            self.modules.append(href[:-len('/index.html')])

def sha(data): return hashlib.sha256(data).hexdigest()
def git(repo, *args):
    return subprocess.check_output(['git', '-C', str(repo), *args], text=True).strip()
def table(path, rows, fields):
    with path.open('w', newline='') as f:
        w=csv.DictWriter(f, fields, delimiter='\t', lineterminator='\n', extrasaction='ignore'); w.writeheader(); w.writerows(rows)
def read_index(data):
    s=data.decode(); return json.loads(s[s.index('['):s.rindex(']')+1])

def main():
    p=argparse.ArgumentParser(); p.add_argument('--lucene-repo', required=True)
    p.add_argument('--tantivy-repo', required=True); p.add_argument('--tantivy-rev', default='65c4e15e6')
    p.add_argument('--offline', action='store_true'); a=p.parse_args()
    raw=ROOT/'raw'; raw.mkdir(exist_ok=True); jars=ROOT/'jars'; jars.mkdir(exist_ok=True)
    previous=json.loads((ROOT/'manifest.json').read_text()) if (ROOT/'manifest.json').exists() else None
    downloads={}; errors=[]
    def fetch(url, rel, required=True):
        path=ROOT/rel; path.parent.mkdir(parents=True, exist_ok=True)
        if a.offline:
            record=previous['downloads'].get(rel)
            if record is None or record.get('error'):
                downloads[rel]=record or {'url':url,'error':'not present in offline cache'}
                if required: errors.append(downloads[rel]);
                return None
            data=path.read_bytes()
            if sha(data)!=record['sha256']: raise ValueError('cache hash mismatch: '+rel)
            downloads[rel]=record; return data
        record=previous['downloads'].get(rel) if previous else None
        if record and not record.get('error') and record['url']==url and path.exists():
            data=path.read_bytes()
            if sha(data)==record['sha256']:
                downloads[rel]=record; return data
        try:
            req=urllib.request.Request(url, headers={'User-Agent':'Lucene-10.4-public-api-audit/1.0'})
            with urllib.request.urlopen(req, timeout=60) as r: data=r.read(); effective=r.url
            path.write_bytes(data)
            downloads[rel]={'url':url, 'effective_url':effective,'bytes':len(data),'sha256':sha(data)}
            return data
        except (urllib.error.URLError, TimeoutError) as e:
            rec={'url':url,'error':str(e)}; downloads[rel]=rec
            if required: errors.append(rec)
            return None
    catalog=fetch(BASE,'raw/catalog.html'); cat=Catalog(); cat.feed(catalog.decode())
    modules=[{'module':m.replace('/','-'),'doc_path':m,'catalogued':True} for m in cat.modules]
    modules.append({'module':'luke','doc_path':None,'catalogued':False})
    def download_module(m):
        name=m['module']; artifact='lucene-'+name
        if m['doc_path']:
            for file in ['type-search-index.js','member-search-index.js','package-search-index.js']:
                fetch(BASE+m['doc_path']+'/'+file,'raw/'+name+'/'+file)
        fetch(f'https://repo.maven.apache.org/maven2/org/apache/lucene/{artifact}/{VERSION}/{artifact}-{VERSION}.jar',
              'jars/'+artifact+'-'+VERSION+'.jar')
    with concurrent.futures.ThreadPoolExecutor(max_workers=6) as ex:
        list(ex.map(download_module, modules))
    print('downloaded', len(downloads), 'sources; failures',len(errors), flush=True)
    doc_types=[]; doc_members=[]; packages=[]
    for m in modules:
        name=m['module']; d=raw/name
        if (d/'package-search-index.js').exists():
            for r in read_index((d/'package-search-index.js').read_bytes()):
                if r.get('l','').startswith('org.'):
                    packages.append({'module':name,'package':r['l']})
        if (d/'type-search-index.js').exists():
            for r in read_index((d/'type-search-index.js').read_bytes()):
                if not r.get('p'): continue
                q=r['p']+'.'+r['l']
                doc_types.append({'module':name,'type':q,'package':r['p'],'label':r['l'],
                                  'javadoc_url':BASE+m['doc_path']+'/'+r['p'].replace('.','/')+'/'+r['l']+'.html'})
        if (d/'member-search-index.js').exists():
            for r in read_index((d/'member-search-index.js').read_bytes()):
                if not r.get('p') or not r.get('c'): continue
                doc_members.append({'module':name,'type':r['p']+'.'+r['c'],'label':r['l'],
                                    'index_anchor':r.get('u',''),'visibility':'not present in search index'})
    classpath=':'.join(str(x) for x in jars.glob('*.jar'))
    binary_types=[]; members=[]; binary_class_counts={}; javap_failures=[]
    for m in modules:
        name=m['module']; jar=jars/('lucene-'+name+'-'+VERSION+'.jar')
        if not jar.exists(): continue
        with zipfile.ZipFile(jar) as z:
            # Base-version class entries; multi-release overrides are separately counted.
            names=sorted(n[:-6].replace('/','.') for n in z.namelist()
                         if n.endswith('.class') and not n.startswith('META-INF/') and n!='module-info.class')
            binary_class_counts[name]={'base_classes':len(names),'multi_release_classes':sum(
                n.startswith('META-INF/versions/') and n.endswith('.class') for n in z.namelist())}
        texts=[]
        for start in range(0,len(names),250):
            run=subprocess.run(['javap','-protected','-s','-classpath',classpath,*names[start:start+250]],
                               capture_output=True,text=True)
            texts.append(run.stdout)
            if run.returncode or run.stderr.strip():
                javap_failures.append({'module':name,'returncode':run.returncode,'stderr':run.stderr})
        output='\n'.join(texts); (raw/name).mkdir(exist_ok=True); (raw/name/'javap-protected.txt').write_text(output)
        current=None; pending=None
        for line in output.splitlines():
            stripped=line.strip()
            header=re.match(r'^(public|protected)\s+.*?\b(class|interface|enum)\s+([^\s<{]+)',stripped)
            if header:
                q=header.group(3); current=q; pending=None
                binary_types.append({'module':name,'binary_name':q,'type':q.replace('$','.'),
                                     'visibility':header.group(1),'declaration':stripped})
            elif stripped.endswith('{') and not line.startswith(' '): current=None; pending=None
            elif current and re.match(r'^(public|protected)\s', stripped) and stripped.endswith(';'):
                if '(' in stripped:
                    before=stripped.split('(',1)[0].split()[-1]
                    kind='constructor' if before==current else 'method'
                else: kind='field'
                pending={'module':name,'type':current.replace('$','.'),'binary_name':current,
                         'visibility':stripped.split()[0],'kind':kind,'declaration':stripped,'descriptor':''}
                members.append(pending)
            elif pending and stripped.startswith('descriptor:'):
                pending['descriptor']=stripped[len('descriptor:'):].strip(); pending=None
            elif stripped=='}': current=None; pending=None
        print(name, 'classes',len(names),'API types',sum(x['module']==name for x in binary_types), flush=True)
    documented={(x['module'],x['type']):x for x in doc_types}
    binary={(x['module'],x['type']):x for x in binary_types}
    types=[]
    for key in sorted(documented.keys() | binary.keys()):
        d=documented.get(key,{}); b=binary.get(key,{})
        types.append({'module':key[0],'type':key[1],'documented':bool(d),'binary_extracted':bool(b),
                      'visibility':b.get('visibility','javadoc visibility not separated'),
                      'binary_name':b.get('binary_name',''),'declaration':b.get('declaration',''),
                      'javadoc_url':d.get('javadoc_url','')})
    # Explicit launchers are outside the Javadoc module catalog.
    tree=git(a.lucene_repo,'ls-tree','-r','--name-only',TAG).splitlines()
    launchers=[f for f in tree if f.startswith('lucene/distribution/src/') and
               (f.endswith(('.sh','.cmd','.bat')) or '/bin/' in f)]
    tools=[]
    for f in launchers:
        blob=subprocess.check_output(['git','-C',a.lucene_repo,'show',TAG+':'+f])
        target=re.findall(rb'org\.apache\.lucene\.[A-Za-z0-9_.$]+',blob)
        tools.append({'source_path':f,'sha256':sha(blob),'java_targets':';'.join(sorted(set(t.decode() for t in target)))})
    public_count=sum(r['visibility']=='public' for r in members)
    protected_count=sum(r['visibility']=='protected' for r in members)
    module_rows=[]
    for m in modules:
        name=m['module']; row={**m,'artifact':'lucene-'+name,
            'documented_types':sum(r['module']==name for r in doc_types),
            'api_types':sum(r['module']==name for r in types),
            'binary_api_types':sum(r['module']==name for r in binary_types),
            'documented_member_labels':sum(r['module']==name for r in doc_members),
            'declared_public_members':sum(r['module']==name and r['visibility']=='public' for r in members),
            'declared_protected_members':sum(r['module']==name and r['visibility']=='protected' for r in members),
            **binary_class_counts.get(name,{})}
        module_rows.append(row)
    unmatched_doc=[r for r in types if r['documented'] and not r['binary_extracted']]
    counts={'catalogued_modules':len(cat.modules),'additional_tool_modules':1,'accounted_modules':len(modules),
            'documented_types':len(doc_types),'binary_api_types':len(binary_types),'union_api_types':len(types),
            'documented_member_labels':len(doc_members),'declared_public_members':public_count,
            'documented_packages':len(packages),
            'declared_protected_members':protected_count,'declared_members':len(members),
            'member_kinds':{k:sum(r['kind']==k for r in members) for k in ['constructor','method','field']},
            'source_launchers':len(tools),'documented_types_not_binary_extracted':len(unmatched_doc),
            'download_failures':len(errors),'javap_warning_or_failure_batches':len(javap_failures)}
    cli=[r for r in members if r['kind']=='method' and r['descriptor']=='([Ljava/lang/String;)V'
         and re.search(r'\bpublic static void main\(',r['declaration'])]
    counts['public_main_entrypoints']=len(cli)
    manifest={'version':VERSION,'catalog_url':BASE,'lucene_git_tag':TAG,
              'lucene_tag_object':git(a.lucene_repo,'rev-parse',TAG),
              'lucene_commit':git(a.lucene_repo,'rev-parse',TAG+'^{commit}'),
              'tantivy_commit':git(a.tantivy_repo,'rev-parse',a.tantivy_rev+'^{commit}'),
              'tantivy_source_ref':a.tantivy_rev,'java_tool':subprocess.check_output(['javap','-version'],text=True).strip(),
              'generator_sha256':sha(Path(__file__).read_bytes()),
              'extraction':'javap -protected -s, all base-version jar classes; documented search-index cross-check',
              'counts':counts,'downloads':downloads,'errors':errors,'javap_batches_with_stderr':javap_failures,
              'coverage_notes':['Declared members only; inherited declarations appear on declaring classes, not expanded per receiver.',
                  'Compiler-generated public bridges are included. Annotation defaults, runtime annotations and default constant values are not extracted.',
                  'Nested-class source reachability and enclosing-type accessibility are not resolved; binary flags and documented status remain separate.',
                  'Multi-release alternative implementations are counted but not separately disassembled; API baseline is the base jar view.',
                  'External dependency APIs and java.* inherited APIs are outside Lucene artifact inventory.',
                  'Extraction does not demonstrate behavioral, Rust API, Java source/binary, or index-format compatibility.']}
    (ROOT/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
    (ROOT/'inventory.json').write_text(json.dumps({'modules':module_rows,'types':types,'members':members,
                                                  'documented_member_labels':doc_members,'tools':tools},indent=2)+'\n')
    table(ROOT/'modules.tsv',module_rows,['module','doc_path','catalogued','artifact','documented_types','api_types','binary_api_types',
                                         'documented_member_labels','declared_public_members','declared_protected_members','base_classes','multi_release_classes'])
    table(ROOT/'types.tsv',types,['module','type','documented','binary_extracted','visibility','binary_name','declaration','javadoc_url'])
    table(ROOT/'packages.tsv',packages,['module','package'])
    table(ROOT/'members.tsv',members,['module','type','binary_name','visibility','kind','declaration','descriptor'])
    table(ROOT/'documented-member-labels.tsv',doc_members,['module','type','label','index_anchor','visibility'])
    table(ROOT/'tools.tsv',tools,['source_path','sha256','java_targets'])
    table(ROOT/'cli-entrypoints.tsv',cli,['module','type','declaration','descriptor'])
    table(ROOT/'unmatched-documented-types.tsv',unmatched_doc,['module','type','visibility','javadoc_url'])
    print(json.dumps(counts,indent=2),flush=True)

if __name__=='__main__': main()
