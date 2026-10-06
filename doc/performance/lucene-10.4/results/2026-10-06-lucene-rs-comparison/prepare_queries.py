"""Freeze Wiki20 and port-style SBG queries without changing corpus tokens."""
import hashlib,json,random,re
from pathlib import Path

OUT=Path(__file__).resolve().parent
ROOT=OUT.parents[1]
wiki=ROOT/'tantivy-pr2937/doc/performance/lucene-10.4/queries-wiki.jsonl'
sbg=ROOT/'search-benchmark-game/queries.txt'
cases=[]
def add(kind,terms,tags):
    cases.append(dict(id=len(cases),kind=kind,terms=terms,tags=tags))
for line in wiki.read_text().splitlines():
    q=json.loads(line); text=q['query']
    kind='PHRASE' if text.startswith('"') else 'AND' if text.startswith('+') else 'TERM' if len(text.split())==1 else 'OR'
    add(kind,text.replace('"','').replace('+','').split(),['wiki20',*q['tags']])
norm=lambda s:[t for t in re.sub('[^a-z0-9]+',' ',s.lower()).split() if len(t)<=255]
qs=[];phrases=[];terms=set()
for line in sbg.read_text().splitlines():
    q=json.loads(line);tag=q['tags'][0];toks=norm(q['query'].replace('+',' '))
    if len(toks)<2:continue
    if tag=='phrase':
        if toks not in phrases:phrases.append(toks)
    elif tag in ('intersection','union'):
        qs.append(('AND' if tag=='intersection' else 'OR',toks));terms.update(toks)
for term in random.Random(42).sample(sorted(terms),300):add('TERM',[term],['port-style'])
for kind,toks in qs:add(kind,toks,['port-style'])
for toks in phrases:add('PHRASE',toks,['port-style'])
for kind,toks in [('TERM',['zzzzzzmissingtermzzzzzz']),('AND',['the','zzzzzzmissingtermzzzzzz']),('OR',['the','zzzzzzmissingtermzzzzzz']),('PHRASE',['the','the']),('AND',['the','the']),('OR',['the','the'])]:
    add(kind,toks,['correctness-only'])
path=OUT/'queries.jsonl'
with path.open('x') as f:
    for q in cases:f.write(json.dumps(q,separators=(',',':'))+'\n')
receipt={'queries':len(cases),'wiki20':20,'port_style':len(cases)-26,'correctness_only':6,
    'inputs':{str(p):hashlib.sha256(p.read_bytes()).hexdigest() for p in (wiki,sbg)},
    'output_sha256':hashlib.sha256(path.read_bytes()).hexdigest()}
(OUT/'queries-receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
print(json.dumps(receipt))
