from pathlib import Path
import collections, difflib, hashlib, json, shutil, subprocess, time

r=Path.cwd(); frozen=r/'.local/primitive-members/frozen-v1'
out=r/'.local/primitive-members/replay-prior-v1';out.mkdir()
plan=r/'.local/primitive-member-implementation-plan-v1/preservation-inventory.json'
inventory=json.loads(plan.read_text());binding=json.loads((frozen/'bindings.json').read_text())
beforebin=r.parent/'joern-oxidized-astra-sprint/.local/astra-sprint/tenth-batch/repaired-final-real-differential/bin/joern-parity'
currentbin=frozen/'joern-parity'
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def lf(b):return b.replace(b'\r\n',b'\n').replace(b'\r',b'\n')
def delta(a,b,an,bn):return ''.join(difflib.unified_diff(a.decode().splitlines(True),b.decode().splitlines(True),fromfile=an,tofile=bn))
assert sha(beforebin)=='ca569f9d75143d9c20e7c8054c8bbf7949640e6eb0213fba806e8224b0994141'
assert sha(currentbin)=='fe5dd8c3036ca44e1112dadc08a748f46165e782894c77c38165ca8c4565cf2c'
sourcebefore={p:sha(r/p) for p in binding['sourceInputs']};assert sourcebefore==binding['sourceInputs']
groups=inventory['groups']+ [{'family':'array-initializers-diagnostic','cases':[{'case':'member_types','root':'cpg-rs/joern-parity/tests/fixtures/array-initializers/diagnostics/member_types','inputs':{'arrays.c':'de4911c0e986952e4be1acf1426e2915769d24ba6dca19ff5450f46723e54e4c'},'referenceSha256':'1c115c101d2a696d63efd9fdfdb3fcd8251dd2f5c366c4ceab7db21e9c7132d5'}]}]
rows=[]
for group in groups:
 for case in group['cases']:
  name=group['family']+'/'+case['case']; d=out/name;(d/'input').mkdir(parents=True)
  fixture=r/case['root']
  for rel,h in case['inputs'].items():
   assert sha(fixture/rel)==h
   p=d/'input'/rel;p.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(fixture/rel,p)
  expected=(fixture/'expected.txt').read_bytes();assert sha(fixture/'expected.txt')==case['referenceSha256'];(d/'expected.txt').write_bytes(expected)
  runs={};outputs={}
  for variant,binary in [('accepted-tenth',beforebin),('candidate-v1',currentbin)]:
   cmd=[str(binary),'--production',*sorted(case['inputs'])];start=time.monotonic();error=None;timeout=False
   try:
    p=subprocess.run(cmd,cwd=d/'input',capture_output=True,timeout=120);stdout,stderr,status=p.stdout,p.stderr,p.returncode
   except subprocess.TimeoutExpired as e:stdout,stderr,status=e.stdout or b'',e.stderr or b'',None;timeout=True;error=str(e)
   (d/(variant+'.stdout')).write_bytes(stdout);(d/(variant+'.stderr')).write_bytes(stderr);actual=lf(stdout);(d/(variant+'.txt')).write_bytes(actual)
   (d/(variant+'.diff')).write_text(delta(expected,actual,'live',variant));outputs[variant]=actual
   runs[variant]={'command':cmd,'cwd':str(d/'input'),'exitCode':status,'timeout':timeout,'error':error,'seconds':time.monotonic()-start,'exact':status==0 and actual==expected}
  old,current=outputs.values();(d/'before-current.diff').write_text(delta(old,current,'accepted-tenth','candidate-v1'))
  complete=all(v['exitCode']==0 for v in runs.values());measures={}
  for label,nonempty in [('includingSeparators',False),('nonempty',True)]:
   e,b,c=[collections.Counter(x for x in v.decode().split('\n') if not nonempty or x) for v in (expected,old,current)]
   gain=(e&c)-(e&b);loss=(e&b)-(e&c)
   measures[label]={'expected':sum(e.values()),'before':sum(b.values()),'candidate':sum(c.values()),'matchingBefore':sum((e&b).values()),'matchingCandidate':sum((e&c).values()),'gained':sum(gain.values()),'lost':sum(loss.values()),'gainedRecords':dict(gain),'lostRecords':dict(loss),'successfulPair':complete}
  assert {rel:sha(d/'input'/rel) for rel in case['inputs']}==case['inputs']
  row={'case':case['case'],'family':group['family'],'inputHashes':case['inputs'],'runs':runs,'successfulPair':complete,'sameOutput':old==current,'counts':measures,'files':{p.relative_to(d).as_posix():sha(p) for p in sorted(d.rglob('*')) if p.is_file()}}
  (d/'run.json').write_text(json.dumps(row,indent=2)+'\n');rows.append(row)
  if not row['sameOutput'] or not complete: print(name,[(k,v['exitCode'],v['exact']) for k,v in runs.items()], 'gain/loss',measures['nonempty']['gained'],measures['nonempty']['lost'],flush=True)
 print('completed',group['family'],len(group['cases']),flush=True)
assert sourcebefore=={p:sha(r/p) for p in binding['sourceInputs']}
summary=[]
for group in groups:
 group_rows=[x for x in rows if x['family']==group['family']];success=[x for x in group_rows if x['successfulPair']]
 summary.append({'family':group['family'],'cases':len(group_rows),'beforeExact':sum(x['runs']['accepted-tenth']['exact'] for x in group_rows),'candidateExact':sum(x['runs']['candidate-v1']['exact'] for x in group_rows),'successfulPairs':len(success),'sameOutputs':sum(x['sameOutput'] for x in group_rows),'matchingGains':sum(x['counts']['nonempty']['gained'] for x in success),'matchingLosses':sum(x['counts']['nonempty']['lost'] for x in success)})
result={'status':'COMPLETE_FROZEN_REPLAY_WITH_FAILURES_RETAINED','planSha256':sha(plan),'bindingSha256':sha(frozen/'bindings.json'),'beforeBinarySha256':sha(beforebin),'candidateBinarySha256':sha(currentbin),'helperSha256':sha(Path(__file__)),'sourceInputsUnchanged':True,'caseInstances':len(rows),'originalGroupInstances':228,'additionalMemberTypesDiagnostic':1,'summary':summary,'cases':rows}
(out/'replay.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(summary,indent=2));print('receipt',sha(out/'replay.json'))
