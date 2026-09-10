from pathlib import Path
from datetime import datetime,timezone
import subprocess,hashlib,json,os,time
r=Path.cwd();out=r/'.local/primitive-members/frozen-v1';out.mkdir();target=r.parent/'joern-oxidized-astra-nested-casts'/'.local/target'
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def source_inputs():
 paths=set(subprocess.check_output(['git','ls-files','-co','--exclude-standard'],text=True).splitlines())
 return {rel:sha(r/rel) for rel in sorted(paths) if rel.startswith('cpg-rs/') and (rel.endswith('.rs') or Path(rel).name in ['Cargo.toml','Cargo.lock'])}
before=source_inputs();refs={p.relative_to(r).as_posix():sha(p) for p in sorted((r/'cpg-rs/joern-parity/tests/fixtures/primitive-members/cases').rglob('*')) if p.is_file()}
(out/'before.json').write_text(json.dumps({'sources':before,'references':refs},indent=2)+'\n')
for rel in ['cpg-rs/cpg-lang-c/src/exact.rs','cpg-rs/cpg-lang-c/src/import.rs','cpg-rs/cpg-lang-c/src/lib.rs','cpg-rs/cpg-cli/src/workspace.rs','cpg-rs/joern-parity/tests/primitive_members.rs']:
 dest=out/'source'/rel;dest.parent.mkdir(parents=True,exist_ok=True);dest.write_bytes((r/rel).read_bytes())
(out/'source.patch').write_bytes(subprocess.check_output(['git','diff','HEAD','--','cpg-rs/cpg-lang-c/src/exact.rs']))
cmd=['cargo','build','--locked','--release','--manifest-path','cpg-rs/Cargo.toml','-p','cpg-cli','-p','joern-parity'];env=dict(os.environ,CARGO_TARGET_DIR=str(target),CARGO_INCREMENTAL='0');started=datetime.now(timezone.utc).isoformat();t=time.monotonic()
with (out/'build.log').open('xb') as log:result=subprocess.run(cmd,stdout=log,stderr=log,env=env)
after=source_inputs();refafter={rel:sha(r/rel) for rel in refs};stable=before==after and refs==refafter
receipt={'status':'FROZEN_BUILD_PAIR' if stable and result.returncode==0 else 'HELD_BUILD_OR_INPUT_DRIFT','baselineCommit':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),'startedAtUtc':started,'completedAtUtc':datetime.now(timezone.utc).isoformat(),'seconds':time.monotonic()-t,'command':cmd,'exitCode':result.returncode,'environmentOverrides':{'CARGO_TARGET_DIR':str(target),'CARGO_INCREMENTAL':'0'},'sourceInputs':before,'sourceInputCount':len(before),'referenceInputs':refs,'allInputsUnchanged':stable,'buildLogSha256':sha(out/'build.log'),'sourcePatchSha256':sha(out/'source.patch'),'scriptSha256':sha(Path(__file__)),'binaries':{}}
if stable and result.returncode==0:
 for name in ['cpg','joern-parity']:
  src=target/'release'/name;dest=out/name;dest.write_bytes(src.read_bytes());dest.chmod(0o755);receipt['binaries'][name]={'path':str(dest),'sha256':sha(dest)}
(out/'bindings.json').write_text(json.dumps(receipt,indent=2)+'\n')
print(json.dumps({k:v for k,v in receipt.items() if k not in ['sourceInputs','referenceInputs']},indent=2),flush=True)
raise SystemExit(0 if stable and result.returncode==0 else 1)
