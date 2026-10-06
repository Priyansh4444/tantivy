#!/usr/bin/env python3
import hashlib
import json
from pathlib import Path
import subprocess

OUT = Path(__file__).resolve().parent
ROOT = Path('/home/pronsh/Coding/playground/search/lucene-rs-idf-fix-oct06')
BASE = '465ea8f77f4397e35de56b7f7d202158d59e5871'
FIX = 'e5d1f81bff42c87886b12770f3c79648bb1963ed'
stages = {}
for stage in sorted(OUT.iterdir()):
    path = stage / 'receipt.json'
    if not path.is_file():
        continue
    receipt = json.loads(path.read_text())
    before = json.loads((stage / 'before.json').read_text())
    after = json.loads((stage / 'after.json').read_text())
    assert before == after
    assert receipt['source_lock_unchanged']
    for stream in ('stdout', 'stderr'):
        actual = hashlib.sha256((stage / f'{stream}.log').read_bytes()).hexdigest()
        assert actual == receipt[f'{stream}_sha256']
    stages[stage.name] = {'head': before['head'], **receipt}

assert stages['regression-before']['returncode'] == 101
for name in ('fmt-final', 'clippy-final', 'debug-final', 'release-final'):
    assert stages[name]['returncode'] == 0
    assert stages[name]['head'] == FIX
head = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
status = subprocess.check_output(['git', 'status', '--porcelain=v1'], cwd=ROOT, text=True)
assert head == FIX and not status
changed = subprocess.check_output(['git', 'diff', '--name-only', BASE, 'HEAD'], cwd=ROOT, text=True).splitlines()
assert changed == ['src/sim.rs']
patch = subprocess.check_output(['git', 'diff', BASE, 'HEAD', '--', 'src/sim.rs'], cwd=ROOT)
(OUT / 'source.diff').write_bytes(patch)
summary = {
    'base': BASE,
    'regression_commit': '298fc4820c85ac4abf8244238760bbdff8931c9f',
    'fix_commit': FIX,
    'clean': True,
    'changed_files': changed,
    'diff_sha256': hashlib.sha256(patch).hexdigest(),
    'sim_rs_sha256': hashlib.sha256((ROOT / 'src/sim.rs').read_bytes()).hexdigest(),
    'lock_sha256': hashlib.sha256((ROOT / 'Cargo.lock').read_bytes()).hexdigest(),
    'stages': stages,
}
(OUT / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')
print(json.dumps({'head': head, 'changed_files': changed, 'all_final_checks_pass': True}))
