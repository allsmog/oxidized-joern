from pathlib import Path
import subprocess,hashlib,json,os,sys,datetime
root=Path.cwd();out=root/'.local/body-includes'/sys.argv[1];out.mkdir()
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
paths=[p for p in subprocess.check_output(['git','ls-files'],text=True).split('\n') if p and (p.endswith('.rs') or Path(p).name in ['Cargo.toml','Cargo.lock'])]
before={p:sha(root/p) for p in paths}
for p in ['cpg-rs/cpg-lang-c/src/exact.rs','cpg-rs/cpg-lang-c/src/import.rs','cpg-rs/cpg-lang-c/src/lib.rs']:
 dst=out/'source'/p;dst.parent.mkdir(parents=True,exist_ok=True);dst.write_bytes((root/p).read_bytes())
(out/'before-build.json').write_text(json.dumps(before,indent=2)+'\n')
cmd=['cargo','build','--locked','--manifest-path','cpg-rs/Cargo.toml','-p','joern-parity'];env=dict(os.environ,CARGO_TARGET_DIR='/Users/shayaunnejad/vibe-code/.codex-worktrees/joern-oxidized-astra-nested-casts/.local/target')
with (out/'build.log').open('xb') as log:r=subprocess.run(cmd,stdout=log,stderr=log,env=env)
after={p:sha(root/p) for p in paths};unchanged=before==after
receipt={'status':'FROZEN_AFTER_SUCCESSFUL_BUILD_NO_SOURCE_DRIFT' if r.returncode==0 and unchanged else 'HELD_BUILD_OR_SOURCE_DRIFT','createdUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'baseline':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),'source':before,'sourceUnchanged':unchanged,'buildCommand':cmd,'target':env['CARGO_TARGET_DIR'],'buildExitCode':r.returncode,'buildLogSha256':sha(out/'build.log')}
if r.returncode==0 and unchanged:
 src=Path(env['CARGO_TARGET_DIR'])/'debug/joern-parity';dst=out/'joern-parity';dst.write_bytes(src.read_bytes());dst.chmod(0o755);receipt['binarySha256']=sha(dst)
(out/'bindings.json').write_text(json.dumps(receipt,indent=2)+'\n');print(json.dumps({k:v for k,v in receipt.items() if k!='source'}),flush=True)
sys.exit(0 if r.returncode==0 and unchanged else 1)
