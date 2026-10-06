#!/usr/bin/env python3
"""Retain the command, source/lock identity, output, and status of each check."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time

ROOT = Path('/home/pronsh/Coding/playground/search/lucene-rs-idf-fix-oct06')
OUT = Path(__file__).resolve().parent
stage, *args = sys.argv[1:]
run = OUT / stage
run.mkdir()
env = dict(os.environ)
for key in list(env):
    if key.startswith('CARGO_PROFILE_') or key == 'CARGO_ENCODED_RUSTFLAGS':
        del env[key]
env['RUSTUP_TOOLCHAIN'] = '1.99.0'
env['CARGO_TARGET_DIR'] = str(OUT / 'target')
env['CARGO_BUILD_JOBS'] = '2'

def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def identity():
    files = subprocess.check_output(['git', 'ls-files', '-z'], cwd=ROOT).split(b'\0')
    return {
        'head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
        'status': subprocess.check_output(['git', 'status', '--porcelain=v1'], cwd=ROOT, text=True),
        'files': {os.fsdecode(path): sha(ROOT / os.fsdecode(path)) for path in files if path},
        'lock_sha256': sha(ROOT / 'Cargo.lock'),
    }

before = identity()
command = ['cargo', *args]
(run / 'command.json').write_text(json.dumps(command, indent=2) + '\n')
(run / 'before.json').write_text(json.dumps(before, indent=2) + '\n')
compiler = subprocess.check_output(['rustc', '-Vv'], env=env, cwd=ROOT, text=True)
(run / 'compiler.txt').write_text(compiler)
start = time.monotonic()
with (run / 'stdout.log').open('w') as stdout, (run / 'stderr.log').open('w') as stderr:
    result = subprocess.run(command, cwd=ROOT, env=env, stdout=stdout, stderr=stderr)
after = identity()
(run / 'after.json').write_text(json.dumps(after, indent=2) + '\n')
receipt = {
    'command': command,
    'returncode': result.returncode,
    'wall_seconds': time.monotonic() - start,
    'source_lock_unchanged': before == after,
    'toolchain': env['RUSTUP_TOOLCHAIN'],
    'jobs': env['CARGO_BUILD_JOBS'],
    'target_dir': env['CARGO_TARGET_DIR'],
    'cargo_profile_overrides': {key: value for key, value in env.items() if key.startswith('CARGO_PROFILE_')},
    'stdout_sha256': sha(run / 'stdout.log'),
    'stderr_sha256': sha(run / 'stderr.log'),
}
(run / 'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
print(json.dumps(receipt))
if not receipt['source_lock_unchanged']:
    raise SystemExit('source or lock changed during check')
raise SystemExit(result.returncode)
