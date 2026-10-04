#!/usr/bin/env python3
"""Run independent old/new logical hashes and require exact payload identity."""
import argparse
import json
from pathlib import Path
import subprocess

parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument("--binary",type=Path,required=True)
parser.add_argument("--old",type=Path,required=True)
parser.add_argument("--new",type=Path,required=True)
parser.add_argument("--output",type=Path,required=True)
args=parser.parse_args()
args.output.mkdir(parents=True,exist_ok=True)
reports={}
for name,path in [("old",args.old),("new",args.new)]:
    result=subprocess.run([str(args.binary.resolve()),str(path.resolve())],text=True,capture_output=True,check=True)
    (args.output/(name+".stderr")).write_text(result.stderr)
    report=json.loads(result.stdout)
    (args.output/(name+".json")).write_text(json.dumps(report,indent=2)+"\n")
    reports[name]=report
same=reports["old"]["logical"]==reports["new"]["logical"]
summary={"payload_identical":same,"metadata_headers_equal":reports["old"]["metadata_headers"]==reports["new"]["metadata_headers"],"old":str(args.old),"new":str(args.new)}
(args.output/"comparison.json").write_text(json.dumps(summary,indent=2)+"\n")
print(json.dumps(summary,indent=2))
raise SystemExit(0 if same else 1)
