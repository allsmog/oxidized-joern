from pathlib import Path
from datetime import datetime,timezone
import hashlib,json,os,shutil,subprocess,time
r=Path.cwd();out=r/'.local/primitive-members/validation-v1-final-test';out.mkdir();target=r.parent/'joern-oxidized-astra-nested-casts/.local/target'
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def sources():
 paths=set(subprocess.check_output(['git','ls-files','-co','--exclude-standard'],text=True).splitlines())
 return {p:sha(r/p) for p in sorted(paths) if p.startswith('cpg-rs/') and (p.endswith('.rs') or Path(p).name in ['Cargo.toml','Cargo.lock'])}
before=sources();refs={p.relative_to(r).as_posix():sha(p) for p in (r/'cpg-rs/joern-parity/tests/fixtures/primitive-members/cases').rglob('*') if p.is_file()}
old=json.loads((r/'.local/primitive-members/frozen-v1/bindings.json').read_text())
changes={p:{'frozen':old['sourceInputs'].get(p),'current':h} for p,h in before.items() if old['sourceInputs'].get(p)!=h}
assert list(changes)==['cpg-rs/joern-parity/tests/primitive_members.rs']
shutil.copyfile(r/'cpg-rs/joern-parity/tests/primitive_members.rs',out/'primitive_members.rs')
mainfiles=['cpg-rs/joern-parity/oracle.sc','cpg-rs/joern-parity/oracle_all.txt','cpg-rs/joern-parity/check.sh','cpg-rs/joern-parity/test_check.py']
mainbefore={p:sha(r/p) for p in mainfiles}
env=dict(os.environ,CARGO_TARGET_DIR=str(target),CARGO_INCREMENTAL='0')
tests=['primitive_members','primitive_roles','typedef_aggregates','array_initializers','body_includes','array_dimension_macros','body_macro_state']
commands=[('fmt',['cargo','fmt','--manifest-path','cpg-rs/Cargo.toml','--all','--','--check']),('focused',['cargo','test','--locked','--manifest-path','cpg-rs/Cargo.toml','-p','joern-parity',*[x for name in tests for x in ['--test',name]]]),('main308',['bash','cpg-rs/joern-parity/check.sh','--committed-only']),('clippy',['cargo','clippy','--locked','--manifest-path','cpg-rs/Cargo.toml','-p','cpg-lang-c','-p','joern-parity','--all-targets','--','-D','warnings'])]
rows=[]
for name,cmd in commands:
 t=time.monotonic();started=datetime.now(timezone.utc).isoformat();print('START',name,started,flush=True)
 with (out/(name+'.log')).open('xb') as log:p=subprocess.run(cmd,stdout=log,stderr=log,env=env)
 stable=before==sources() and refs=={p:sha(r/p) for p in refs} and mainbefore=={p:sha(r/p) for p in mainfiles}
 row={'check':name,'command':cmd,'cwd':str(r),'exitCode':p.returncode,'startedAtUtc':started,'seconds':time.monotonic()-t,'sourceInputsUnchanged':stable,'logSha256':sha(out/(name+'.log'))};rows.append(row)
 (out/'checks-running.json').write_text(json.dumps(rows,indent=2)+'\n');print('DONE',name,p.returncode,'stable',stable,flush=True)
 if p.returncode!=0 or not stable:break
testbins={}
if any(x['check']=='focused' and x['exitCode']==0 for x in rows):
 for name in tests:
  candidates=[p for p in (target/'debug/deps').glob(name+'-*') if p.is_file() and p.suffix=='' and os.access(p,os.X_OK)]
  p=max(candidates,key=lambda p:p.stat().st_mtime);testbins[name]={'path':str(p),'sha256':sha(p)}
receipt={'status':'PASS_FOCUSED_MAIN_FMT_CLIPPY' if len(rows)==len(commands) and all(x['exitCode']==0 and x['sourceInputsUnchanged'] for x in rows) else 'HELD_VALIDATION','sourceInputs':before,'sourceInputCount':len(before),'testOnlyChangeSinceFrozenV1':changes,'frozenBindingsSha256':sha(r/'.local/primitive-members/frozen-v1/bindings.json'),'referenceInputs':refs,'mainGateInputs':mainbefore,'environmentOverrides':{'CARGO_TARGET_DIR':str(target),'CARGO_INCREMENTAL':'0'},'checks':rows,'testBinaries':testbins,'helperSha256':sha(Path(__file__))}
(out/'checks.json').write_text(json.dumps(receipt,indent=2)+'\n');print(receipt['status'],sha(out/'checks.json'),flush=True)
raise SystemExit(0 if receipt['status'].startswith('PASS') else 1)
