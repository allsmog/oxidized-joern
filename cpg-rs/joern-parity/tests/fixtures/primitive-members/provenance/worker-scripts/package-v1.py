from pathlib import Path
import hashlib,json,shutil,subprocess
r=Path.cwd();local=r/'.local/primitive-members';family=r/'cpg-rs/joern-parity/tests/fixtures/primitive-members';prov=family/'provenance';prov.mkdir()
analysis=r.parent/'joern-oxidized-astra-typedef-existence';parity=r.parent/'joern-oxidized-astra-typedef-aggregates';oldworker=r.parent/'joern-oxidized-astra-body-includes'
copies=[]
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def copy(src,dest):
 assert src.is_file();dest.parent.mkdir(parents=True,exist_ok=True);assert not dest.exists();shutil.copyfile(src,dest);assert sha(src)==sha(dest)
 copies.append({'originalPath':str(src),'relativePath':dest.relative_to(family).as_posix(),'sha256':sha(dest),'bytes':dest.stat().st_size})
def tree(src,dest,skip=None):
 for p in sorted(src.rglob('*')):
  if p.is_file() and not (skip and skip(p.relative_to(src))):copy(p,dest/p.relative_to(src))
run=analysis/'.local/primitive-member-runner-v3/runs/first-complete-primitive-member-reference-batch'
tree(run,prov/'oracle-run',lambda p:p.parts[0]=='workspace')
driver=analysis/'.local/primitive-member-runner-v3'
for name in ['runtime-bindings.json','prepared.json','README.md','cases.json','run-oracle.py','oracle.sc']:
 copy(driver/name,prov/'oracle-driver'/name)
prep=oldworker/'.local/primitive-member-preparation-v2'
for name in ['freeze.json','prepared.json']:
 copy(prep/name,prov/'original-preparation'/name)
base=analysis/'.local/primitive-member-baseline-v1/runs/accepted-tenth-1';tree(base,prov/'accepted-baseline')
for name,dest in [('replay19-v1','replay19'),('replay-prior-v1','prior-replay'),('validation-v1-final-test','validation')]:tree(local/name,prov/dest)
tree(local/'frozen-v1',prov/'frozen-v1',lambda p:len(p.parts)==1 and p.name in ['cpg','joern-parity'])
tree(r/'.local/primitive-member-implementation-plan-v1',prov/'implementation-plan')
for name in ['copy-admitted.json']:
 copy(local/name,prov/name)
tree(local/'scripts',prov/'worker-scripts')
for name in ['reference-admission-review.json','preparation-review.json','live-member-facts.json','live-member-facts.md','review_reference.py','review_runner.py','baseline-helper-review.json','runner-review.json']:
 copy(parity/'.local/eleventh-primitive-preflight'/name,prov/'reviews/reference-admission'/name)
tree(analysis/'.local/primitive-member-baseline-review-v1',prov/'reviews/baseline-facts')
for name in ['source-bindings.json']:
 copy(r.parent/'joern-oxidized-astra-sprint/.local/astra-sprint/tenth-batch/repaired-final-real-differential/bin'/name,prov/'accepted-source'/name)
copy(local/'frozen-v1/source.patch',prov/'source.patch')
test=r/'cpg-rs/joern-parity/tests/primitive_members.rs';quirks=r/'cpg-rs/joern-parity/QUIRKS.md'
testpatch=subprocess.run(['git','diff','--no-index','--','/dev/null',str(test.relative_to(r))],capture_output=True);assert testpatch.returncode==1
(prov/'test.patch').write_bytes(testpatch.stdout)
(prov/'quirks.patch').write_bytes(subprocess.check_output(['git','diff','HEAD','--',str(quirks.relative_to(r))]))
replay=json.loads((local/'replay19-v1/replay.json').read_text());prior=json.loads((local/'replay-prior-v1/replay.json').read_text());checks=json.loads((local/'validation-v1-final-test/checks.json').read_text())
rows=[]
for x in replay['cases']:
 rows.append({'case':x['case'],'retainedAnchor':x['retainedAnchor'],'classification':'complete_exact_gate' if x['candidateExact'] else 'retained_complete_diagnostic','inputs':{p:'cases/'+x['case']+'/'+p for p in x['inputHashes']},'inputHashes':x['inputHashes'],'expected':'cases/'+x['case']+'/expected.txt','expectedSha256':sha(family/'cases'/x['case']/'expected.txt'),'beforeExact':x['beforeExact'],'candidateExact':x['candidateExact'],'beforeCandidateIdentical':x['beforeCandidateIdentical'],'matchingGainsNonempty':x['counts']['nonempty']['gained'],'matchingLossesNonempty':x['counts']['nonempty']['lost'],'replay':'provenance/replay19/'+x['case']+'/run.json'})
measurement={'status':'WORKER_BOUNDED_VALIDATION_PASS_ROOT_ACCEPTANCE_PENDING','baselineCommit':'f235a0f898c4e19fda90998b903f57741b776ec7','productionSourceSha256':sha(r/'cpg-rs/cpg-lang-c/src/exact.rs'),'frozenBuildBindings':'provenance/frozen-v1/bindings.json','frozenBuildBindingsSha256':sha(local/'frozen-v1/bindings.json'),'binaries':json.loads((local/'frozen-v1/bindings.json').read_text())['binaries'],'cases':rows,'summary':{'projects':19,'newProjects':17,'retainedAnchors':2,'beforeExact':2,'candidateExact':18,'newCompleteGraphGates':16,'retainedAnchorCompleteGraphGates':2,'completeDiagnostics':1,'referenceLfLines':sum(len((family/x['expected']).read_text().splitlines()) for x in rows),'referenceNonemptyRecords':3423,'matchingGainsNonempty':126,'matchingLossesNonempty':0,'counterTerminalEmptyPolicy':'Replay counters split on LF and include a terminal empty entry in includingSeparators; 3512 such entries across19. Nonempty totals3423 and LF line count3493 are distinct.'},'priorCaseInstances':228,'additionalMemberTypesDiagnostic':1,'priorSummary':prior['summary'],'priorReceipt':'provenance/prior-replay/replay.json','priorReceiptSha256':sha(local/'replay-prior-v1/replay.json'),'retainedAbort':{'family':'body-macro-state','case':'duplicate_clinit_tag','beforeExit':-6,'candidateExit':-6,'outputIdentical':True,'includedInSuccessfulMatchingClaims':False},'priorGainOverlap':'The14 prior gains are the same two tiny-fixedtables anchors already included in the19-project gain count. Do not add them twice.','validation':{'receipt':'provenance/validation/checks.json','sha256':sha(local/'validation-v1-final-test/checks.json'),'focusedTests':21,'newRustTests':2,'newTestGroup':1,'mainComparisonBlocks':308,'fmt':True,'strictClippyPackages':['cpg-lang-c','joern-parity'],'rootFinalGatesRunByWorker':False},'test':{'path':'cpg-rs/joern-parity/tests/primitive_members.rs','sha256':sha(test),'initialDraftPreserved':'provenance/frozen-v1/source/cpg-rs/joern-parity/tests/primitive_members.rs','finalTestSnapshot':'provenance/validation/primitive_members.rs','draftChange':'Remove nonprimitive_scalar_control from complete-exact test after replay confirms unchanged scaffold diagnostic; production source/binary unchanged.'},'sourceScope':'One field-base call site reuses declaration_type(..., TypeRole::Declaration); no helper, enum, suffix, clinit, other role, importer, solver, cache or oracle changes.','pending':['independent final source/package review','root integration and final workspace/live/whole/resource acceptance']}
(family/'measurement.json').write_text(json.dumps(measurement,indent=2)+'\n')
(prov/'copy-manifest.json').write_text(json.dumps({'status':'BYTE_IDENTICAL_PROVENANCE_COPIES','copiedFiles':len(copies),'files':copies,'historicalAbsolutePaths':'Original producer receipts preserve original absolute paths. relativePath locates each copied artifact in this portable family.'},indent=2)+'\n')
stage=local/'package-v1/staged';stage.mkdir(parents=True)
paths=[r/'cpg-rs/cpg-lang-c/src/exact.rs',test,quirks,*sorted(p for p in family.rglob('*') if p.is_file())]
files={}
for p in paths:
 rel=p.relative_to(r);dest=stage/rel;dest.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(p,dest);assert sha(p)==sha(dest);files[rel.as_posix()]={'sha256':sha(p),'bytes':p.stat().st_size}
freeze={'status':'FROZEN_WORKER_SOURCE_TEST_AND_PORTABLE_PACKAGE_ROOT_ACCEPTANCE_PENDING','baselineCommit':measurement['baselineCommit'],'stagedRoot':str(stage),'fileCount':len(files),'bytes':sum(x['bytes'] for x in files.values()),'files':files,'sourcePatch':{'path':str(prov/'source.patch'),'sha256':sha(prov/'source.patch')},'testPatch':{'path':str(prov/'test.patch'),'sha256':sha(prov/'test.patch')},'quirksPatch':{'path':str(prov/'quirks.patch'),'sha256':sha(prov/'quirks.patch')},'copiedProvenanceFiles':len(copies),'measurementSha256':sha(family/'measurement.json'),'readmeSha256':sha(family/'README.md'),'helperSha256':sha(Path(__file__))}
(local/'package-v1/freeze.json').write_text(json.dumps(freeze,indent=2)+'\n')
print(json.dumps({k:v for k,v in freeze.items() if k!='files'},indent=2));print('freezeSHA',sha(local/'package-v1/freeze.json'))
