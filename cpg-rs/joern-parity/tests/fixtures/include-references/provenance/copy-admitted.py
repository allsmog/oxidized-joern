from pathlib import Path
import hashlib, json, shutil

BASE = Path('/Users/shayaunnejad/vibe-code/.codex-worktrees')
OWN = BASE/'joern-oxidized-astra-typedef-existence'
PEER = BASE/'joern-oxidized-astra-typedef-aggregates'
REPO = BASE/'joern-oxidized-astra-sprint'
TARGET = BASE/'joern-oxidized-astra-include-references/cpg-rs/joern-parity/tests/fixtures/include-references'
CANON = OWN/'.local/include-reference-runner-v1/runs/first-complete-six-include-reference-batch'
OBSERVER = PEER/'.local/thirteenth-include-observer-v1/runs/first-complete-six-include-saved-cpg-observation'
ACCEPTED = REPO/'.local/astra-sprint/thirteenth-batch/accepted-capture-v1'
ORIGIN = PEER/'.local/include-reference-preparation-v1'
copied=[]
def sha(path):return hashlib.sha256(path.read_bytes()).hexdigest()
def read(path):return json.loads(path.read_bytes())
def copy(source, relative):
    target=TARGET/relative;target.parent.mkdir(parents=True,exist_ok=True)
    with target.open('xb') as f:f.write(source.read_bytes())
    assert target.read_bytes()==source.read_bytes()
    copied.append({'path':relative,'originalPath':str(source),'sha256':sha(target),'bytes':target.stat().st_size})
assert sha(CANON/'run.json')=='d91a5e412fb4d7ba848ff10a357f9d5c77b1407403913ca4bff9c2fe9af7e495'
assert sha(OBSERVER/'run.json')=='a202d40d8f7f9bf363365fd0857c589c0a031c3a6340f17f7b08b63974c7ed06'
canonical=read(CANON/'run.json');observer=read(OBSERVER/'run.json');baseline=read(ACCEPTED/'run.json')
assert canonical['admittedAsReferences'] and observer['admittedSupplement']
assert canonical['exitCode']==observer['exitCode']==0
assert all(canonical['retainedAnchors'].values()) and len(canonical['retainedAnchors'])==4
assert canonical['before']==canonical['after'] and observer['before']==observer['after']
assert baseline['status']=='COMPLETE_ACCEPTED_RUST_CAPTURE'
for output in observer['outputs']:
    name=output['case'];assert output['canonicalByteIdentical'] and output['supplementAdmitted']
    for source in sorted((CANON/'snapshot/input'/name).iterdir()):copy(source,'cases/'+name+'/input/'+source.name)
    ref=CANON/'expected'/name/'expected.txt';copy(ref,'cases/'+name+'/expected.txt')
    raw=OBSERVER/'admitted-supplement'/name/'supplement.jsonl'
    assert sha(raw)==output['files']['supplement.jsonl']['sha256']
    copy(raw,'cases/'+name+'/joern-supplement.jsonl')
    for source in sorted((OBSERVER/'admitted-supplement'/name).iterdir()):
        if source.name!='supplement.jsonl':copy(source,'provenance/observer/cases/'+name+'/'+source.name)
    for source in sorted((ACCEPTED/name).iterdir()):
        if source.is_file():copy(source,'cases/'+name+'/accepted/'+source.name)
    assert (ACCEPTED/name/'canonical.stdout').read_bytes()==ref.read_bytes()
    assert (ACCEPTED/name/'supplement.stdout').read_bytes()==(ACCEPTED/name/'reopened.stdout').read_bytes()
for name in ['run.json','live.stdout','live.stderr']:
    copy(CANON/name,'provenance/canonical/'+name)
for name in ['oracle.sc','run-oracle.py','prepared.json','checkpoint-release.json']:
    copy(CANON/'snapshot'/name,'provenance/canonical/'+name)
for source in sorted((CANON/'snapshot/retained-anchors').rglob('*')):
    if source.is_file():copy(source,'provenance/canonical/retained-anchors/'+source.relative_to(CANON/'snapshot/retained-anchors').as_posix())
for name in ['run.json','observer.stdout','observer.stderr']:
    copy(OBSERVER/name,'provenance/observer/'+name)
for name in ['observer.sc','run-observer.py','prepared.json','observer-release.json','jobs.json']:
    copy(OBSERVER/'snapshot'/name,'provenance/observer/'+name)
copy(ACCEPTED/'run.json','provenance/accepted-run.json')
copy(REPO/'.local/astra-sprint/thirteenth-batch/capture-accepted-v1.py','provenance/capture-accepted-v1.py')
for source in sorted(ORIGIN.rglob('*')):
    if source.is_file() and 'input' not in source.relative_to(ORIGIN).parts:
        copy(source,'provenance/source-preparation/'+source.relative_to(ORIGIN).as_posix())
copy(OWN/'.local/include-reference-runner-v1/runtime-bindings.json','provenance/runtime-bindings.json')
copy(Path(__file__).resolve(),'provenance/copy-admitted.py')
for source in [OWN/'.local/thirteenth-include-test-design-v1/baseline-review.json',PEER/'.local/thirteenth-include-observation-review-v1/result/review.json',PEER/'.local/thirteenth-include-observation-review-v1/result/facts.json',PEER/'.local/thirteenth-include-observation-review-v1/classification-addendum.json',PEER/'.local/thirteenth-include-observation-review-v1/global-order-proof.json']:
    copy(source,'provenance/reviews/'+source.name)
measurement={'status':'ADMITTED_REFERENCE_FIXTURE_PREPARED_CANDIDATE_PENDING','cases':list(sorted(output['case'] for output in observer['outputs'])),'sourceFiles':11,'newSourceProjects':2,'reusedSourceProjects':4,'freshCanonicalProjects':6,'fullByteHistoricalCanonicalAnchors':4,'fullHistoricalSupplementAnchors':3,'canonicalExactBefore':6,'candidateCanonicalExact':None,'observedIncludePairs':9,'candidateIncludeMetadataExact':None,'copiedUnchangedArtifacts':copied,'canonicalRunSha256':sha(CANON/'run.json'),'observerRunSha256':sha(OBSERVER/'run.json'),'acceptedCaptureSha256':sha(ACCEPTED/'run.json'),'sourcePreparationFreezeSha256':sha(ORIGIN/'freeze.json'),'immutableReferencesEdited':False,'referenceProjectionChanged':False,'candidateBuildRun':False,'scope':'Complete canonical6 and complete observed Import/Dependency property+incident-edge components. Full raw graph differences remain retained; this family does not assert full supplemental parity.'}
with (TARGET/'measurement.json').open('x') as f:json.dump(measurement,f,indent=2);f.write('\n')
print(json.dumps({'copiedFiles':len(copied),'copiedBytes':sum(x['bytes'] for x in copied),'measurementSha256':sha(TARGET/'measurement.json')}))
