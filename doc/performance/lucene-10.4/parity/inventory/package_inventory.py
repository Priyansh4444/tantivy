#!/usr/bin/env python3
"""Package compact inventory handoff without JARs or duplicated raw inputs."""
import gzip
import hashlib
import io
from pathlib import Path
import tarfile
ROOT=Path(__file__).resolve().parent
PLAIN=['README.md','generate_inventory.py','map_features.py','package_inventory.py',
       'feature_families.json','manifest.json','parity-matrix.json','modules.tsv',
       'packages.tsv','feature-parity.tsv','module-coverage.tsv','tools.tsv','cli-entrypoints.tsv',
       'unmatched-documented-types.tsv']
COMPRESS=['types.tsv','members.tsv','api-worklist.tsv','documented-member-labels.tsv']
def main():
    files={name:(ROOT/name).read_bytes() for name in PLAIN}
    files.update({name+'.gz':gzip.compress((ROOT/name).read_bytes(),mtime=0) for name in COMPRESS})
    checks=''.join(hashlib.sha256(data).hexdigest()+'  '+name+'\n' for name,data in sorted(files.items()))
    files['SHA256SUMS']=checks.encode()
    buf=io.BytesIO()
    with tarfile.open(fileobj=buf,mode='w') as tar:
        for name,data in sorted(files.items()):
            info=tarfile.TarInfo('lucene-10.4-api-inventory/'+name)
            info.size=len(data);info.mtime=0;info.mode=0o644;tar.addfile(info,io.BytesIO(data))
    out=ROOT/'ship';out.mkdir(exist_ok=True)
    archive=out/'lucene-10.4-api-inventory.tar.gz';archive.write_bytes(gzip.compress(buf.getvalue(),mtime=0))
    print(str(archive),archive.stat().st_size,'bytes',hashlib.sha256(archive.read_bytes()).hexdigest())
if __name__=='__main__':main()
