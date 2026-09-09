#!/usr/bin/env python3
"""Capture the accepted writer on frozen include inputs; no Joern reference admission."""
import hashlib,json,pathlib,shutil,subprocess,time
P=pathlib.Path
ROOT=P('/Users/shayaunnejad/vibe-code/.codex-worktrees/joern-oxidized-astra-sprint')
PREP=P('/Users/shayaunnejad/vibe-code/.codex-worktrees/joern-oxidized-astra-typedef-aggregates/.local/include-reference-preparation-v1')
BUILD=ROOT/'.local/astra-sprint/twelfth-batch/lint-corrected/final-real-differential/bin'
OUT=ROOT/'.local/astra-sprint/thirteenth-batch/accepted-capture-v1'
COMMIT='afe23fc4bedbbf6b669f2bd6dfed0f0af141ecfc'
FROZEN_SHA='33f4740b78f60a142a615b1a07b2a2ea448ed7955e9464395e72fe6a8af91dda'
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def write(p,x):
 with p.open('x') as f:json.dump(x,f,indent=2);f.write('\n')
def obs(p):
 try:return {'path':str(p),'bytes':p.stat().st_size,'sha256':sha(p),'error':None}
 except Exception as e:return {'path':str(p),'error':repr(e)}
def inventory(p):return {str(q.relative_to(p)):sha(q) for q in sorted(p.rglob('*')) if q.is_file()}
def run(cmd,folder,stem):
 r={'command':cmd,'cwd':str(folder),'exitCode':None,'error':None,'timedOut':False};start=time.monotonic()
 try:
  with (folder/(stem+'.stdout')).open('xb') as out,(folder/(stem+'.stderr')).open('xb') as err:
   r['exitCode']=subprocess.run(cmd,cwd=folder,stdout=out,stderr=err,timeout=120).returncode
 except subprocess.TimeoutExpired:r['timedOut']=True
 except Exception as e:r['error']=repr(e)
 r.update(seconds=time.monotonic()-start,stdout=obs(folder/(stem+'.stdout')),stderr=obs(folder/(stem+'.stderr')))
 write(folder/(stem+'.run.json'),r);return r
def ok(r):return r['exitCode']==0 and not r['timedOut'] and r['error'] is None and r['stdout']['error'] is None and r['stderr']['error'] is None
OUT.mkdir(exist_ok=False)
r={'status':'HELD','acceptedCommit':COMMIT,'cases':[],'errors':[],'producer':'accepted Rust build only','expectedOutputsGuessed':False,'noJoernProducer':True}
tracked={};input_before=None
try:
 assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT).strip().decode()==COMMIT
 assert not subprocess.check_output(['git','status','--porcelain'],cwd=ROOT)
 assert sha(BUILD/'source-bindings.json')==FROZEN_SHA
 frozen=json.loads((BUILD/'source-bindings.json').read_bytes());prep=json.loads((PREP/'preparation.json').read_bytes())
 for p in [P(__file__).resolve(),BUILD/'source-bindings.json',PREP/'preparation.json',ROOT/'.local/astra-sprint/twelfth-batch/closeout.json']:
  tracked[str(p)]=sha(p)
 for name in ('cpg','joern-parity'):
  assert sha(BUILD/name)==frozen['binaries'][name];tracked[str(BUILD/name)]=sha(BUILD/name)
 for rel,digest in frozen['allRustAndCargoInputs'].items():
  assert sha(ROOT/rel)==digest
 r['verifiedSourceFiles']=len(frozen['allRustAndCargoInputs'])
 input_before=inventory(PREP/'input');assert input_before==prep['inputHashes'] and len(input_before)==11
 r['sourceInventoryBefore']=input_before
 for case in prep['cases']:
  name=case['name'];assert name==P(name).name and name not in ('.','..')
  folder=OUT/name;folder.mkdir();shutil.copytree(PREP/'input'/name,folder/'input')
  cr={'case':name,'commands':{},'errors':[]};r['cases'].append(cr)
  try:
   before=inventory(folder/'input');assert before==case['files'];cr['sources']=before
   paths=sorted(str(p.resolve()) for p in (folder/'input').rglob('*') if p.is_file() and p.suffix in ('.c','.h'))
   commands=[('canonical',[str(BUILD/'joern-parity'),'--production',*paths]),('supplement',[str(BUILD/'joern-parity'),'--production-supplemental',*paths]),('persist',[str(BUILD/'cpg'),'build',str(folder/'input'),'--lang','c','-o',str(folder/'accepted-v2.cpg')]),('reopened',[str(BUILD/'joern-parity'),'--supplemental-cpg',str(folder/'accepted-v2.cpg')])]
   for stem,cmd in commands:
    cr['commands'][stem]=run(cmd,folder,stem)
    if not ok(cr['commands'][stem]):cr['errors'].append(stem+' command failed')
   cr['savedGraph']=obs(folder/'accepted-v2.cpg')
   cr['actualVersionTwoHeader']=(folder/'accepted-v2.cpg').read_bytes()[:6]==b'CPG2\x02\x00'
   cr['completeSupplementSurvivesReopen']=(folder/'supplement.stdout').read_bytes()==(folder/'reopened.stdout').read_bytes()
   assert cr['actualVersionTwoHeader'] and cr['completeSupplementSurvivesReopen']
   if case.get('historicalCanonicalReference'):
    ref=P(case['historicalCanonicalReference']['path']);assert sha(ref)==case['historicalCanonicalReference']['sha256']
    tracked[str(ref)]=sha(ref);cr['historicalCanonicalExact']=(folder/'canonical.stdout').read_bytes()==ref.read_bytes()
   cr['sourceInventoryAfter']=inventory(folder/'input');assert cr['sourceInventoryAfter']==before
  except Exception as e:cr['errors'].append(repr(e))
  cr['complete']=not cr['errors'];write(folder/'case.json',cr)
  if cr['errors']:r['errors'].append(name+': '+', '.join(cr['errors']))
except Exception as e:r['errors'].append(repr(e))
try:
 r['sourceInventoryAfter']=inventory(PREP/'input');assert input_before is not None and r['sourceInventoryAfter']==input_before
 r['bindingObservationsAfter']={p:obs(P(p)) for p in tracked}
 assert all(r['bindingObservationsAfter'][p].get('sha256')==digest for p,digest in tracked.items())
 assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT).strip().decode()==COMMIT
 assert not subprocess.check_output(['git','status','--porcelain'],cwd=ROOT)
 if 'frozen' in globals():
  assert all(sha(ROOT/rel)==digest for rel,digest in frozen['allRustAndCargoInputs'].items())
 assert len(r['cases'])==6 and all(x['complete'] for x in r['cases'])
except Exception as e:r['errors'].append('final: '+repr(e))
r['bindings']=tracked
if not r['errors']:r['status']='COMPLETE_ACCEPTED_RUST_CAPTURE'
write(OUT/'run.json',r)
print(json.dumps({'status':r['status'],'cases':len(r['cases']),'commands':sum(len(x['commands']) for x in r['cases']),'errors':r['errors'],'receipt':str(OUT/'run.json'),'sha256':sha(OUT/'run.json')}))
raise SystemExit(bool(r['errors']))
