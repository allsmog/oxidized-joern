from pathlib import Path
import json,subprocess,hashlib,shutil,time,difflib,collections
root=Path.cwd();base=root/'cpg-rs/joern-parity/tests/fixtures/body-macro-state';meta=json.loads((base/'measurement.json').read_bytes());out=root/'.local/body-includes/ninth-v2-retention';out.mkdir();freeze=root/'.local/body-includes/frozen-v2';before=Path('/Users/shayaunnejad/vibe-code/.codex-worktrees/joern-oxidized-astra-sprint/.local/astra-sprint/ninth-batch/reviewed-final-real-differential/bin/joern-parity');after=freeze/'joern-parity';sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
def lines(s):
 parts=s.split('\n');return [x+'\n' for x in parts[:-1]]+([parts[-1]] if parts[-1] else [])
rows=[]
for row in meta['cases']:
 name=row['case'];src=base/'cases'/name;d=out/name;d.mkdir();paths=[]
 for rel,h in row['inputHashes'].items():
  assert sha(src/rel)==h;dst=d/'input'/rel;dst.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(src/rel,dst);paths.append(str(dst))
 assert sha(src/'expected.txt')==row['expectedSha256'];ref=(src/'expected.txt').read_bytes();(d/'expected.txt').write_bytes(ref);refc=collections.Counter(x for x in ref.decode().split('\n') if x)
 results={};counters={}
 for label,binary in [('accepted-ninth',before),('candidate-v2',after)]:
  stage=d/label;stage.mkdir();cmd=[str(binary),*sorted(paths)];start=time.monotonic()
  with (stage/'actual.txt').open('xb') as stdout,(stage/'stderr.txt').open('xb') as stderr:
   try:r=subprocess.run(cmd,stdout=stdout,stderr=stderr,cwd=d,timeout=120);status=r.returncode
   except subprocess.TimeoutExpired:status='timeout'
  actual=(stage/'actual.txt').read_bytes();(stage/'complete.diff').write_text(''.join(difflib.unified_diff(lines(ref.decode()),lines(actual.decode()),fromfile='complete pinned Joern',tofile=label)))
  results[label]={'exitCode':status,'exact':status==0 and actual==ref,'seconds':time.monotonic()-start,'command':cmd,'actualSha256':sha(stage/'actual.txt'),'stderrSha256':sha(stage/'stderr.txt'),'diffSha256':sha(stage/'complete.diff')};counters[label]=collections.Counter(x for x in actual.decode().split('\n') if x)
 matching={k:v&refc for k,v in counters.items()};loss=(matching['accepted-ninth']-matching['candidate-v2']);gain=(matching['candidate-v2']-matching['accepted-ninth']);valid=all(x['exitCode']==0 for x in results.values())
 item={'case':name,'inputs':row['inputHashes'],'expectedSha256':row['expectedSha256'],'runs':results,'matchingCountersComparableSuccessfulRuns':valid,'matchingLost':sum(loss.values()) if valid else None,'matchingGained':sum(gain.values()) if valid else None,'rawLostRecords':dict(loss),'rawGainedRecords':dict(gain)};rows.append(item);print(name,results['accepted-ninth']['exitCode'],results['candidate-v2']['exitCode'],results['accepted-ninth']['exact'],results['candidate-v2']['exact'],'lost',item['matchingLost'],flush=True)
 (out/'replay.json').write_text(json.dumps({'status':'FULL_PRIOR_FAMILY_REPLAY_DIAGNOSTICS_RETAINED','scriptSha256':sha(Path(__file__)),'baselineBinarySha256':sha(before),'candidateBinarySha256':sha(after),'bindingsSha256':sha(freeze/'bindings.json'),'rows':rows,'completeProjects':len(rows),'priorExact':sum(r['runs']['accepted-ninth']['exact'] for r in rows),'currentExact':sum(r['runs']['candidate-v2']['exact'] for r in rows),'matchingLost':sum(r['matchingLost'] or 0 for r in rows),'matchingGained':sum(r['matchingGained'] or 0 for r in rows)},indent=2)+'\n')
