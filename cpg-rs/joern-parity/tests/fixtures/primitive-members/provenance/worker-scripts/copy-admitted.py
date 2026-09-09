from pathlib import Path
import json,hashlib,shutil
r=Path.cwd();peer=r.parent/'joern-oxidized-astra-typedef-existence';run=peer/'.local/primitive-member-runner-v3/runs/first-complete-primitive-member-reference-batch';prep=r.parent/'joern-oxidized-astra-body-includes'/'.local/primitive-member-preparation-v2';out=r/'cpg-rs/joern-parity/tests/fixtures/primitive-members';out.mkdir()
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
runmeta=json.loads((run/'run.json').read_text());assert runmeta['admittedAsReferences'] and runmeta['exitCode']==0 and not runmeta['holdReasons']
assert sha(run/'run.json')=='aeef8f13e1f03f46daec5df5d6c8ac9fba026ff71334dc3f59d8d50e3386033f'
old=json.loads((prep/'prepared.json').read_text());rows=[]
for case in sorted((prep/'input').iterdir()):
 dest=out/'cases'/case.name;dest.mkdir(parents=True)
 inputs={}
 for p in sorted(case.rglob('*')):
  if p.is_file():
   rel=p.relative_to(case);q=dest/rel;q.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(p,q);assert sha(q)==sha(p);inputs[rel.as_posix()]=sha(p)
 ref=run/'expected'/case.name/'expected.txt';shutil.copyfile(ref,dest/'expected.txt');assert sha(ref)==sha(dest/'expected.txt')
 rows.append({'case':case.name,'retainedAnchor':case.name.startswith('tiny_fixedtables_'),'inputHashes':inputs,'expectedSha256':sha(ref),'referencePath':str(ref)})
assert len(rows)==19 and sum(x['retainedAnchor'] for x in rows)==2
meta={'status':'ADMITTED_COMPLETE_REFERENCE_COPIES_CANDIDATE_PENDING','runPath':str(run/'run.json'),'runSha256':sha(run/'run.json'),'inputPreparationPath':str(prep/'prepared.json'),'inputPreparationSha256':sha(prep/'prepared.json'),'cases':rows,'noExpectedRewrite':True}
(r/'.local/primitive-members/copy-admitted.json').write_text(json.dumps(meta,indent=2)+'\n')
print('Copied19 admitted graphs and20 unchanged input files; two retained anchors separated.')
