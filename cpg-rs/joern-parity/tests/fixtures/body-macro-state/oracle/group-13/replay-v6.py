from pathlib import Path
import hashlib,json,subprocess,time,difflib
p=Path(__file__).resolve().parent;roots=Path('/Users/shayaunnejad/vibe-code/.codex-worktrees');v6=roots/'joern-oxidized-astra-body-macro-state/.local/body-macro-state/candidate-v6';sha=lambda f:hashlib.sha256(Path(f).read_bytes()).hexdigest();bind=lambda f:{'path':str(f),'sha256':sha(f),'bytes':Path(f).stat().st_size}
assert sha(v6/'exact.rs')=='c6fbd18f35dc536fa8ba04ba15bea12fb185745fa6e72eb1025e178597383f62'
assert sha(v6/'joern-parity')=='22d651f3daef5ce60908a34ec991d022dad009492c0c6e63e4cba1a23acd98e7'
setup=json.loads((v6/'bindings.json').read_bytes());print(setup,flush=True)
out=p/'v6-independent';out.mkdir();rows=[]
for group in [p,p/'type-position',p/'callee-rescan']:
 label='primary' if group==p else group.name
 for d in sorted((group/'input').iterdir()):
  q=out/label/d.name;q.mkdir(parents=True);cmd=[str(v6/'joern-parity'),str(d/'main.c')];t=time.monotonic()
  with (q/'stdout').open('xb') as stdout,(q/'stderr').open('xb') as stderr:r=subprocess.run(cmd,cwd=q,stdout=stdout,stderr=stderr,timeout=30)
  actual=(q/'stdout').read_bytes();expected=(d/'expected.txt').read_bytes();delta=''.join(difflib.unified_diff([l+'\n' for l in expected.decode().split('\n')[:-1]],[l+'\n' for l in actual.decode().split('\n')[:-1]],fromfile='Joern4.0.555',tofile='V6'))
  (q/'complete.diff').write_bytes(delta.encode());row={'group':label,'case':d.name,'command':cmd,'cwd':str(q),'exitCode':r.returncode,'seconds':time.monotonic()-t,'exact':actual==expected,'input':bind(d/'main.c'),'reference':bind(d/'expected.txt'),'actual':bind(q/'stdout'),'stderr':bind(q/'stderr'),'completeDiff':bind(q/'complete.diff')};rows.append(row);assert r.returncode==0;print(label,d.name,row['exact'],flush=True)
j={'source':bind(v6/'exact.rs'),'binary':bind(v6/'joern-parity'),'ownerFreeze':bind(v6/'bindings.json'),'deltaFromV5':bind(v6/'v5-to-v6.patch'),'results':rows}
with (out/'replay.json').open('x') as f:json.dump(j,f,indent=2);f.write('\n')
print('receipt',sha(out/'replay.json'))
