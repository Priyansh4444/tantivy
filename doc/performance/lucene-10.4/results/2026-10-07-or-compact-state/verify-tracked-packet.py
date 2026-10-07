"""Require an exact tracked packet, then audit the committed Git archive."""
import hashlib,json,subprocess,sys,tempfile,tarfile
from pathlib import Path
here=Path(__file__).resolve().parent
repo=Path(subprocess.check_output(['git','rev-parse','--show-toplevel'],cwd=here,text=True).strip())
rel=here.relative_to(repo)
manifest=json.loads((here/'packet-manifest.json').read_text())
expected={str(rel/name) for name in manifest['files']}|{str(rel/'packet-manifest.json')}
tracked=set(subprocess.check_output(['git','ls-files','-z','--',str(rel)],cwd=repo).decode().strip('\0').split('\0'))
if tracked!=expected:raise RuntimeError(f'tracked inventory missing={sorted(expected-tracked)} extra={sorted(tracked-expected)}')
head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=repo,text=True).strip()
with tempfile.TemporaryDirectory(prefix='or-packet-archive-') as tmp:
 archive=Path(tmp)/'packet.tar'
 with archive.open('wb') as output:subprocess.run(['git','archive',head,str(rel)],cwd=repo,stdout=output,check=True)
 with tarfile.open(archive) as t:t.extractall(tmp,filter='data')
 result=subprocess.run([sys.executable,str(Path(tmp)/rel/'recompute.py')],text=True,capture_output=True,check=True)
 print(result.stdout,end='')
 print(json.dumps({'pass':True,'head':head,'tracked_files':len(tracked),'archive_sha256':hashlib.sha256(archive.read_bytes()).hexdigest(),'clean_git_archive_audit':True}))
