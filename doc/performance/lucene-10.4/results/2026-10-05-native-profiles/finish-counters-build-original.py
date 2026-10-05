from pathlib import Path
import subprocess,os,shutil,json,hashlib
base=Path('/home/pronsh/Coding/playground/search');out=base/'bench/native-profile-pruning-counters-oct05';crate=out/'adapter'
m=crate/'Cargo.toml';m.write_text(m.read_text().replace('[profile.release]','serde_json="1.0"\n[profile.release]'))
shutil.copy2(base/'bench/native-perf-adapter-oct03/Cargo.lock',crate/'Cargo.lock')
argv=json.loads((out/'build-command.json').read_text());env=os.environ.copy();env['RUSTFLAGS']='-C target-cpu=native'
with (out/'build-final.log').open('w') as f:subprocess.run(argv,env=env,stdout=f,stderr=subprocess.STDOUT,check=True)
shutil.copy2(base/'bench/native-perf-adapter-oct03/target/release/do_query',out/'do_query-counters')
(out/'binary-sha256.json').write_text(json.dumps({'binary':hashlib.sha256((out/'do_query-counters').read_bytes()).hexdigest(),'manifest':hashlib.sha256(m.read_bytes()).hexdigest(),'cargo_lock':hashlib.sha256((crate/'Cargo.lock').read_bytes()).hexdigest()})+'\n')
print('ready')
