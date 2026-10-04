#!/usr/bin/env python3
"""Actual-index proof of the bounded named analyzer and its query registration."""
import argparse
import json
from pathlib import Path
import subprocess

parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument("--binaries",type=Path,required=True)
parser.add_argument("--output",type=Path,required=True)
parser.add_argument("--jars",type=Path,required=True)
args=parser.parse_args()
bins=args.binaries.resolve()
output=args.output.resolve();output.mkdir(parents=True)
words=["a"*39,"b"*40,"c"*219,"d"*255]
docs=[{"id":"fixture0","text":" ".join(words),"sort_field":9},
      {"id":"fixture1","text":"alpha beta","sort_field":8},
      {"id":"fixture2","text":"alpha  beta","sort_field":7}]
data="".join(json.dumps(doc)+"\n" for doc in docs)
(output/"valid.jsonl").write_text(data)
index=output/"valid.idx"
built=subprocess.run([str(bins/"build_index"),str(index),"--fixture-docs","3"],input=data,text=True,capture_output=True,check=True)
(output/"build.stdout").write_text(built.stdout);(output/"build.stderr").write_text(built.stderr)
metadata=json.loads(built.stdout)
assert metadata["documents"]==3 and metadata["tokens"]==8 and metadata["segments"]==1
terms=subprocess.run([str(bins/"dump_termtotals"),str(index),"--positions"],text=True,capture_output=True,check=True)
(output/"terms.tsv").write_text(terms.stdout);(output/"term-statistics.json").write_text(terms.stderr)
observed={}
for row in terms.stdout.splitlines():
    word,df,ttf,positions=row.split("\t")
    observed[word]=(int(df),int(ttf),json.loads(positions))
expected={word:(1,1,[[0,[position]]]) for position,word in enumerate(words)}
expected.update({"alpha":(2,2,[[1,[0]],[2,[0]]]),"beta":(2,2,[[1,[1]],[2,[1]]])})
assert observed==expected,"accepted terms or actual posting positions differ"
queries=words+["\""+" ".join(words)+"\"","\"alpha beta\""]
protocol="".join("COUNT\t"+q+"\n" for q in queries)
queried=subprocess.run([str(bins/"do_query"),str(index)],input=protocol,text=True,capture_output=True,check=True)
assert [int(x) for x in queried.stdout.splitlines()]==[1,1,1,1,1,2]
validated=subprocess.run([str(bins/"validate_index"),str(index)],input="\n".join(queries)+"\n",text=True,capture_output=True,check=True)
(output/"validation.jsonl").write_text(validated.stdout)
assert all(row["count_matches_exhaustive"] and row["ranking_matches_exhaustive"] for row in map(json.loads,validated.stdout.splitlines()))
rejections=[]
for name,row,fixture,expected_error in [
    ("length256",{**docs[0],"text":"x"*256},True,"longer than 255"),
    ("uppercase",{**docs[0],"text":"Alpha"},True,"outside [a-z ]"),
    ("punctuation",{**docs[0],"text":"alpha-beta"},True,"outside [a-z ]"),
    ("unknown_key",{**docs[0],"extra":1},True,"exactly id/text/sort_field"),
    ("negative_sort",{**docs[0],"sort_field":-1},True,"sort_field must be a u64"),
    ("missing_text",{k:v for k,v in docs[0].items() if k!="text"},True,"exactly id/text/sort_field"),
    ("default_short",docs[0],False,"expected 1000000 documents"),
]:
    argv=[str(bins/"build_index"),str(output/(name+".idx"))]
    if fixture: argv += ["--fixture-docs","1"]
    result=subprocess.run(argv,input=json.dumps(row)+"\n",text=True,capture_output=True)
    assert result.returncode!=0 and expected_error in result.stderr,(name,result.stderr)
    assert not (output/(name+".idx.build.json")).exists()
    (output/(name+".stderr")).write_text(result.stderr)
    rejections.append(name)
# Exercise signed norm promotion and zero norms on two actual native indexes.
high_rows=[{"id":"empty","text":"","sort_field":0},{"id":"high","text":"alpha "*65000,"sort_field":1}]
high_data="".join(json.dumps(row)+"\n" for row in high_rows)
high_tantivy=output/"high-norm-tantivy.idx"
subprocess.run([str(bins/"build_index"),str(high_tantivy),"--fixture-docs","2"],input=high_data,text=True,capture_output=True,check=True)
classes=output/"norm-classes";classes.mkdir()
root=Path(__file__).resolve().parent
subprocess.run(["javac","-cp",str(args.jars.resolve()/"*"),"-d",str(classes),str(root/"BuildWikiFixture.java"),str(root/"DumpWikiLogical.java")],check=True)
high_lucene=output/"high-norm-lucene.idx"
subprocess.run(["java","-cp",str(classes)+":"+str(args.jars.resolve()/"*"),"BuildWikiFixture",str(high_lucene)],check=True)
tmap=subprocess.check_output([str(bins/"dump_docmap"),str(high_tantivy)],text=True)
lmap=subprocess.check_output(["java","-cp",str(classes)+":"+str(args.jars.resolve()/"*"),"DumpWikiLogical",str(high_lucene),"documents"],text=True)
(output/"high-norm-tantivy.tsv").write_text(tmap);(output/"high-norm-lucene.tsv").write_text(lmap)
assert tmap==lmap,"native norm/sort/ID inventories differ"
high_norm=int([line for line in tmap.splitlines() if line.startswith("high\t")][0].split("\t")[2])
assert high_norm>=128,"high encoded norm control did not exercise signed-byte domain"
assert "empty\t0\t0\n" in tmap,"empty text norm must canonicalize to zero"
summary={"passed":True,"native_high_norm":high_norm,"empty_norm":0,"accepted_lengths":[39,40,219,255],"positions_checked":True,"named_query_registration":True,"rejected":rejections}
(output/"summary.json").write_text(json.dumps(summary,indent=2)+"\n")
print(json.dumps(summary,indent=2))
