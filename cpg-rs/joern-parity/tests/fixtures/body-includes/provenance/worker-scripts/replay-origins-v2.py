from pathlib import Path
import subprocess,json,hashlib,sys,time,difflib
root=Path(__file__).resolve().parents[1];freeze=root/sys.argv[1];out=root/sys.argv[2];out.mkdir()
source=Path('/Users/shayaunnejad/vibe-code/.codex-worktrees/joern-oxidized-astra-typedef-existence/.local/tenth-source-locations')
refs=source/'runs/canonical-1/expected';sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
def lf_lines(text):
 parts=text.split('\n')
 return [part+'\n' for part in parts[:-1]]+([parts[-1]] if parts[-1] else [])
rows=[]
for case in sorted((source/'input').iterdir()):
 d=out/case.name;d.mkdir();paths=[]
 for p in sorted(case.rglob('*')):
  if p.is_file():
   dst=d/'input'/p.relative_to(case);dst.parent.mkdir(parents=True,exist_ok=True);dst.write_bytes(p.read_bytes());paths.append(str(dst))
 command=[str(freeze/'joern-parity'),*paths];start=time.monotonic()
 with (d/'actual.txt').open('xb') as stdout,(d/'stderr.txt').open('xb') as stderr:
  try:
   result=subprocess.run(command,stdout=stdout,stderr=stderr,cwd=d,timeout=120);status=result.returncode
  except subprocess.TimeoutExpired:status='timeout'
 actual=(d/'actual.txt').read_bytes();expected=(refs/case.name/'expected.txt').read_bytes()
 (d/'expected.txt').write_bytes(expected)
 delta=''.join(difflib.unified_diff(lf_lines(expected.decode()),lf_lines(actual.decode()),fromfile='complete pinned Joern',tofile='complete candidate',lineterm='\n'))
 (d/'complete.diff').write_text(delta)
 rows.append({'case':case.name,'exitCode':status,'seconds':time.monotonic()-start,'exact':status==0 and actual==expected,'command':command,'inputs':{str(p.relative_to(d/'input')):sha(p) for p in (d/'input').rglob('*') if p.is_file()},'expectedSha256':sha(d/'expected.txt'),'actualSha256':sha(d/'actual.txt'),'stderrSha256':sha(d/'stderr.txt'),'diffSha256':sha(d/'complete.diff')})
 print(case.name,status,rows[-1]['exact'],flush=True)
(out/'replay.json').write_text(json.dumps({'status':'COMPLETE_CANDIDATE_REPLAY_DIAGNOSTICS_RETAINED','scriptSha256':sha(Path(__file__)), 'candidate':json.loads((freeze/'bindings.json').read_bytes()),'oracleReceiptSha256':sha(source/'runs/canonical-1/run.json'),'projects':len(rows),'exact':sum(r['exact'] for r in rows),'rows':rows},indent=2)+'\n')
