#!/usr/bin/env python3
"""Offline full-term and external-ID/sort/norm equality on the bounded wiki corpus."""
import argparse
import hashlib
import json
from pathlib import Path
import struct
import subprocess

ROOT=Path(__file__).resolve().parent
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument("--binaries",type=Path,required=True)
parser.add_argument("--tantivy-index",type=Path,required=True)
parser.add_argument("--lucene-index",type=Path,required=True)
parser.add_argument("--jars",type=Path,required=True)
parser.add_argument("--output",type=Path,required=True)
args=parser.parse_args()
output=args.output.resolve();output.mkdir(parents=True)
classes=output/"classes";classes.mkdir()
subprocess.run(["javac","-cp",str(args.jars.resolve()/"*"),"-d",str(classes),str(ROOT/"DumpWikiLogical.java")],check=True)
commands={
    "tantivy_terms":[str(args.binaries.resolve()/"dump_termtotals"),str(args.tantivy_index.resolve())],
    "tantivy_documents":[str(args.binaries.resolve()/"dump_docmap"),str(args.tantivy_index.resolve())],
    "lucene_terms":["java","-Xmx512m","-cp",str(classes)+":"+str(args.jars.resolve()/"*"),"DumpWikiLogical",str(args.lucene_index.resolve()),"terms"],
    "lucene_documents":["java","-Xmx512m","-cp",str(classes)+":"+str(args.jars.resolve()/"*"),"DumpWikiLogical",str(args.lucene_index.resolve()),"documents"],
}
for name,command in commands.items():
    with (output/(name+".tsv")).open("wb") as stream, (output/(name+".stderr")).open("wb") as errors:
        subprocess.run(command,stdout=stream,stderr=errors,check=True)

def term_digest(path):
    digest=hashlib.sha256();previous=None;count=0
    with path.open("rb") as stream:
        for line in stream:
            term,df,ttf=line.rstrip(b"\n").split(b"\t")
            if previous is not None and term<=previous: raise ValueError("term order/uniqueness violation")
            previous=term;count+=1
            if int(df)<1 or int(ttf)<int(df): raise ValueError("invalid term statistics")
            digest.update(line)
    return {"sha256":digest.hexdigest(),"terms":count}

def document_digest(path):
    digest=hashlib.sha256();previous=None;count=0
    with path.open("rb") as stream:
        for line in stream:
            external_id,sort,norm=line.rstrip(b"\n").split(b"\t")
            if previous is not None and external_id<=previous: raise ValueError("ID order/uniqueness violation")
            previous=external_id;count+=1
            digest.update(struct.pack("<Q",len(external_id)));digest.update(external_id)
            digest.update(struct.pack("<QB",int(sort),int(norm)))
    return {"sha256":digest.hexdigest(),"documents":count}

def statistics(path):
    rows=[json.loads(line) for line in path.read_text().splitlines() if line.startswith("{")]
    if len(rows)!=1: raise ValueError("missing or ambiguous term statistics")
    return rows[0]

results={engine:{"terms":term_digest(output/(engine+"_terms.tsv")),"documents":document_digest(output/(engine+"_documents.tsv")),
                 "statistics":statistics(output/(engine+"_terms.stderr"))} for engine in ["tantivy","lucene"]}
passed=results["tantivy"]==results["lucene"]
summary={"passed":passed,"results":results,"java_source_sha256":hashlib.sha256((ROOT/"DumpWikiLogical.java").read_bytes()).hexdigest()}
(output/"summary.json").write_text(json.dumps(summary,indent=2)+"\n")
print(json.dumps(summary,indent=2))
raise SystemExit(0 if passed else 1)
