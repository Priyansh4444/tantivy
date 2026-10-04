#!/usr/bin/env python3
"""Check real-index sensitivity before the frozen corpus walk."""
import argparse
import json
from pathlib import Path
import subprocess

parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument("--binaries",type=Path,required=True)
parser.add_argument("--output",type=Path,required=True)
args=parser.parse_args()
binaries=args.binaries.resolve()
output=args.output.resolve()
# Absent output prevents accidental overwriting of prior fixtures/evidence.
output.mkdir(parents=True)
fixtures=output/"fixtures"
subprocess.run([str(binaries/"payload_fixtures"),str(fixtures)],check=True)
subprocess.run([str(binaries/"reencode_index"),str(fixtures/"baseline"),str(fixtures/"reencoded")],check=True)
reports={}
for name in ["baseline","reencoded","postings","frequency","positions","fieldnorm","stored","fast"]:
    result=subprocess.run([str(binaries/"payload_identity"),str(fixtures/name)],capture_output=True,text=True,check=True)
    report=json.loads(result.stdout)
    (output/(name+".json")).write_text(json.dumps(report,indent=2)+"\n")
    reports[name]=report["logical"]
base=reports["baseline"]
assert reports["reencoded"]==base,"identical reencode changed logical payload"
fields=lambda report: {entry["name"]:entry for entry in report["fields"]}
changed={}
for name,component in [("postings","postings"),("frequency","postings"),("positions","postings"),("fieldnorm","fieldnorms_sha256"),("fast","fast_u64_sha256")]:
    field="sort_field" if name=="fast" else "text"
    assert fields(reports[name])[field][component]!=fields(base)[field][component],name+" perturbation was missed"
    assert reports[name]["schema_sha256"]==base["schema_sha256"]
    changed[name]=component
assert reports["stored"]["stored_documents_sha256"]!=base["stored_documents_sha256"]
changed["stored"]="stored_documents_sha256"
# These changes must be isolated from unrelated payload components.
for name in ["positions","fieldnorm","stored","fast"]:
    assert reports[name]["max_doc"]==base["max_doc"]
    if name!="stored": assert reports[name]["stored_documents_sha256"]==base["stored_documents_sha256"]
    for field in base["fields"]:
        expected=field.copy()
        altered=fields(reports[name])[field["name"]].copy()
        component=changed[name]
        if component in expected:
            expected.pop(component); altered.pop(component)
        assert expected==altered,(name,field["name"],"unrelated field component changed")
rejections={}
for name,message in [("unsupported_mixed_json","unsupported mixed JSON field"),("unsupported_fast_string","unsupported fast-field type")]:
    result=subprocess.run([str(binaries/"payload_identity"),str(fixtures/name)],capture_output=True,text=True)
    assert result.returncode!=0 and message in result.stderr,(name,"unsupported layout was not rejected",result.stderr)
    (output/(name+".stderr")).write_text(result.stderr)
    rejections[name]=message
summary={"passed":True,"identical_reencode":True,"detected_components":changed,"unsupported_rejections":rejections}
(output/"summary.json").write_text(json.dumps(summary,indent=2)+"\n")
print(json.dumps(summary,indent=2))
