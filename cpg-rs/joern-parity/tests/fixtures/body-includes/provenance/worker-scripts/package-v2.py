from pathlib import Path
import json,hashlib,shutil,difflib,collections,subprocess
root=Path.cwd();out=root/'cpg-rs/joern-parity/tests/fixtures/body-includes';out.mkdir();work=root/'.local/body-includes';sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
sprint=Path('/Users/shayaunnejad/vibe-code/.codex-worktrees/joern-oxidized-astra-sprint/.local/astra-sprint');old=sprint/'tenth-batch/frozen-baseline-v1'
prior=Path('/Users/shayaunnejad/vibe-code/.codex-worktrees/joern-oxidized-astra-body-macro-state/.local/body-include-diagnosis-v4');loc=Path('/Users/shayaunnejad/vibe-code/.codex-worktrees/joern-oxidized-astra-typedef-existence/.local/tenth-source-locations');locreview=loc.parent/'tenth-source-location-review';peer=Path('/Users/shayaunnejad/vibe-code/.codex-worktrees/joern-oxidized-astra-typedef-aggregates/.local');copies={}
def cp(src,rel):
 dst=out/rel;dst.parent.mkdir(parents=True,exist_ok=True);assert not dst.exists(),dst;shutil.copyfile(src,dst);copies[str(rel)]={'source':str(src),'sha256':sha(src)}
def tree(src,rel):
 for p in sorted(src.rglob('*')):
  if p.is_file():cp(p,Path(rel)/p.relative_to(src))
def lflines(t):
 a=t.split('\n');return [p+'\n' for p in a[:-1]]+([a[-1]] if a[-1] else [])
def delta(a,b):return ''.join(difflib.unified_diff(lflines(a.decode()),lflines(b.decode()),fromfile='complete pinned Joern',tofile='complete producer')).encode()
def extract(raw):
 groups={};current=None
 for line in raw.split(b'\n'):
  if line.startswith(b'CASE|'):
   current=line[5:].decode();assert current not in groups;groups[current]=[]
  elif line.startswith((b'AST|',b'NODES|',b'EDGES|',b'FLOWS|')):
   assert current is not None;groups[current].append(line[4:] if line.startswith(b'AST|') else line)
 return {name:b'\n'.join(lines)+b'\n' for name,lines in groups.items()}
rawgroups={}
for label,prep,runname in [('body-includes',prior,'first-complete-reference-batch'),('source-locations',loc,'canonical-1'),('origin-observations',loc,'origins-1')]:
 run=prep/'runs'/runname
 for name in ['live.stdout','live.stderr','run.json','before.json','after.json','preflight-before.json','started.json']:
  if (run/name).is_file():cp(run/name,Path('oracle')/label/name)
 for p in sorted((run/'snapshot').iterdir()):
  if p.is_file():cp(p,Path('oracle')/label/'snapshot'/p.name)
 cp(prep/'runtime-bindings.json',Path('oracle')/label/'runtime-bindings.json')
 if label!='origin-observations':rawgroups[label]=extract((run/'live.stdout').read_bytes())
rows=[]
for replay,label in [('replay-v2','body-includes'),('replay-origins-v2','source-locations')]:
 info=json.loads((work/replay/'replay.json').read_bytes())
 for row in info['rows']:
  name=row['case'];src=work/replay/name;ref=(src/'expected.txt').read_bytes();assert ref==rawgroups[label][name]
  if any(r['case']==name for r in rows):
   assert ref==(out/'cases'/name/'expected.txt').read_bytes();continue
  case=Path('cases')/name;tree(src/'input',case);cp(src/'expected.txt',case/'expected.txt')
  olddir=old/name if label=='body-includes' else locreview/'accepted-ninth-baseline'/name
  baseline_run=json.loads((olddir/'run.json').read_bytes())
  oldstatus=baseline_run.get('exitCode',baseline_run.get('returnCode'))
  if oldstatus is None:raise ValueError(baseline_run)
  for n in ['actual.txt','stderr.txt','run.json']:cp(olddir/n,case/'baseline'/n)
  for n in ['actual.txt','stderr.txt','complete.diff']:cp(src/n,case/'candidate'/n)
  (out/case/'baseline/complete.diff').write_bytes(delta(ref,(olddir/'actual.txt').read_bytes()))
  (out/case/'candidate/run.json').write_text(json.dumps(row,indent=2)+'\n')
  oc=collections.Counter(x for x in (olddir/'actual.txt').read_text().split('\n') if x);nc=collections.Counter(x for x in (src/'actual.txt').read_text().split('\n') if x);rc=collections.Counter(x for x in ref.decode().split('\n') if x)
  lost=(oc&rc)-(nc&rc);gain=(nc&rc)-(oc&rc)
  rows.append({'case':name,'oracleGroup':label,'oracleCase':name,'inputHashes':row['inputs'],'expectedSha256':sha(src/'expected.txt'),'canonicalLinesIncludingSeparators':len(ref.split(b'\n'))-1,'nonemptySelectedRecords':sum(bool(x) for x in ref.split(b'\n')),'baseline':{'exitCode':oldstatus,'exact':oldstatus==0 and ref==(olddir/'actual.txt').read_bytes(),'actualSha256':sha(olddir/'actual.txt'),'stderrSha256':sha(olddir/'stderr.txt')},'candidate':row,'matchingLost':sum(lost.values()),'matchingGained':sum(gain.values()),'lostRecords':dict(lost),'gainedRecords':dict(gain)})
# Every failed first-candidate graph remains intact, with an additive readable
# LF-only diff. The first producer's original malformed presentation is kept too.
for src in sorted((work/'replay-v1').iterdir()):
 if src.is_dir():
  for n in ['actual.txt','stderr.txt','complete.diff']:cp(src/n,Path('held/v1')/src.name/n)
  (out/'held/v1'/src.name/'complete-readable.diff').write_bytes(delta((src/'expected.txt').read_bytes(),(src/'actual.txt').read_bytes()))
for n in ['bindings.json','before-build.json','build.log','checks.json','ninth-gates.log','main308.log']:cp(work/'frozen-v1'/n,Path('held/v1')/n)
cp(work/'replay-v1/replay.json','held/v1/replay.json')
for p in (work/'frozen-v1/source').rglob('*.rs'):cp(p,Path('held/v1/source')/p.relative_to(work/'frozen-v1/source'))
for n in ['bindings.json','before-build.json','build.log','checks.json','origin-lines.log','ninth-gates.log','main308.log']:cp(work/'frozen-v2'/n,Path('provenance/candidate')/n)
for n in ['replay-v2','replay-origins-v2']:cp(work/n/'replay.json',Path('provenance')/(n+'.json'))
for p in sorted((work/'scripts').glob('*.py')):cp(p,Path('provenance/worker-scripts')/p.name)
# Actual preceding-family runs (including its known nonzero importer exit),
# with references supplied unchanged by the already committed prior family.
for d in sorted((work/'ninth-v2-retention').iterdir()):
 if d.is_dir():
  for label in ['accepted-ninth','candidate-v2']:tree(d/label,Path('provenance/prior-family-replay')/d.name/label)
 else:cp(d,Path('provenance/prior-family-replay')/d.name)
for n in ['final-review.json','review.md','all-decoded-and-bridged.json','focused-source-positions.json','bridge-review.json']:cp(locreview/n,Path('provenance/source-location-review')/n)
for n in ['final-review.json','ownership-regression.json','review.md']:cp(peer/'tenth-body-include-v1-review'/n,Path('provenance/v1-review')/n)
tree(peer/'tenth-body-include-v1-review/pass-order','provenance/pass-order')
for n in ['final-review.json','diagnosis.md']:cp(peer/'tenth-body-include-reference-review'/n,Path('provenance/reference-review')/n)
for src,name in [(sprint/'tenth-batch/body-include-v4-release.json','parent-release.json'),(sprint/'ninth-batch/closeout.json','ninth-closeout.json'),(old/'runs.json','baseline-runs.json')]:cp(src,Path('provenance')/name)
sourcepatch=subprocess.check_output(['git','diff','--','cpg-rs/cpg-lang-c/src/exact.rs','cpg-rs/cpg-lang-c/src/import.rs','cpg-rs/cpg-lang-c/src/lib.rs']);(work/'source-v2.patch').write_bytes(sourcepatch)
prior_replay=json.loads((work/'ninth-v2-retention/replay.json').read_bytes())
meta={'status':'FROZEN_V2_REPLAY_COMPLETE_REVIEW_PENDING','joernVersion':'4.0.555','joernCommit':'d95237aeaf3d12cb4e63336def3a4d9d7315dfb4','baselineSourceCommit':'00c37613b3074025d3d65aa632cbd79af95d0c46','baselineDocsCommit':'1962cd7681b1d414c75e1c9c8de039b25b850228','candidateBindings':json.loads((work/'frozen-v2/bindings.json').read_bytes()),'sourcePatchSha256':sha(work/'source-v2.patch'),'projectCount':len(rows),'baselineExact':sum(r['baseline']['exact'] for r in rows),'candidateExact':sum(r['candidate']['exact'] for r in rows),'fullGraphProductionGates':sum(r['candidate']['exact'] for r in rows),'retainedNonexactProjects':sum(not r['candidate']['exact'] for r in rows),'canonicalLinesIncludingSeparators':sum(r['canonicalLinesIncludingSeparators'] for r in rows),'nonemptySelectedRecords':sum(r['nonemptySelectedRecords'] for r in rows),'rawMatchingLost':sum(r['matchingLost'] for r in rows),'rawMatchingGained':sum(r['matchingGained'] for r in rows),'priorFamilyReplay':{k:v for k,v in prior_replay.items() if k!='rows'},'cases':rows,'copiedEvidence':copies}
(out/'measurement.json').write_text(json.dumps(meta,indent=2)+'\n');print({k:v for k,v in meta.items() if k not in ['candidateBindings','copiedEvidence','cases','priorFamilyReplay']})
