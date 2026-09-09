from pathlib import Path
import hashlib,json
P=Path('/Users/shayaunnejad/vibe-code/.codex-worktrees');own=P/'joern-oxidized-astra-typedef-aggregates';peer=P/'joern-oxidized-astra-typedef-existence';root=P/'joern-oxidized-astra-sprint';body=P/'joern-oxidized-astra-body-macro-state';out=Path(__file__).parent
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest();read=lambda p:json.loads(p.read_bytes())
def row(p):return {'path':str(p),'sha256':sha(p),'bytes':p.stat().st_size}
obs=read(own/'.local/twelfth-identity-observer-v2/runs/first-complete-eleven-identity-saved-cpg-observation/run.json')
prior={}
for name in ['duplicate_supplied_macro_local','tiny_fixedtables_include','tiny_fixedtables_inline']:
 j=next(j for j in obs['jobs']if j['case']==name);ref=Path(j['reference']['path']);run=ref.parents[2];prior[name]=(run/'snapshot/input'/name,ref,run/'run.json')
run=body/'.local/body-include-diagnosis-v4/runs/first-complete-reference-batch';prior['inactive_body_include']=(run/'snapshot/input/inactive_body_include',run/'expected/inactive_body_include/expected.txt',run/'run.json')
files={};provenance={}
for name,(src,ref,run)in prior.items():
 j=read(run);assert j['status']=='COMPLETE_RAW_REFERENCE_BATCH'and j['admittedAsReferences']and j['exitCode']==0
 srcfiles={str(p.relative_to(src)):p.read_bytes()for p in sorted(src.rglob('*'))if p.is_file()};expected={k[len(name)+1:]:v for k,v in read(run.parent/'snapshot/prepared.json')['inputHashes'].items()if k.startswith(name+'/')};assert {k:hashlib.sha256(v).hexdigest()for k,v in srcfiles.items()}==expected
 files[name]=srcfiles;provenance[name]={'kind':'byte-identical reuse','sourceRoot':str(src),'sourceFiles':{k:row(src/k)for k in srcfiles},'historicalCanonicalReference':row(ref),'historicalCanonicalRun':row(run),'historicalSupplementAvailable':name!='inactive_body_include'}
files['repeated_direct_include']={'main.c':b'#include "repeat.h"\n#include "repeat.h"\nint entry(void) { return INCLUDED_VALUE; }\n','repeat.h':b'#ifndef REPEAT_H\n#define REPEAT_H\n#define INCLUDED_VALUE 7\n#endif\n'}
files['unresolved_include_pair']={'main.c':b'#include "missing-local.h"\n#include <missing-system.h>\nint entry(void) { return 0; }\n'}
for n in ['repeated_direct_include','unresolved_include_pair']:provenance[n]={'kind':'new proposed input','historicalCanonicalReference':None,'historicalSupplementAvailable':False}
coverage={
'duplicate_supplied_macro_local':(['direct file-scope include','same header in two caller files'],['Are two separate import/dependency pairs retained for the two physical include occurrences despite identical header names and dependency properties?','Do physical caller namespace ownership, import properties, CODE/line/column/order and every incident edge reproduce the already admitted full observation?']),
'tiny_fixedtables_include':(['body include'],['Does the body include remain attached to the caller global NAMESPACE_BLOCK in the fresh full graph?','Are import metadata and the included array declarations both preserved, without equating their source locations or scopes?']),
'tiny_fixedtables_inline':(['inline/no-include control'],['Does the fresh full graph retain the existing complete canonical projection and no include-reference nodes/edges?']),
'inactive_body_include':(['inactive include in body','include inside inactive function','no macro/declaration leakage control'],['Which of the two lexically present inactive include directives appear in the CDT include inventory and full import/dependency graph?','How are their AST parent, source coordinates, CODE and order represented relative to active source declarations?','Does import inventory coexist with the preserved inactive declaration/macro behavior, without LEAK changing active branches?']),
'repeated_direct_include':(['repeated direct file-scope include','header guard'],['Are both directives represented separately when the second header body is guarded out?','Are dependency nodes distinct or shared, and how do exact properties, namespace AST order, IMPORTS endpoints and multiplicity appear?','Does the full expression graph retain the first header macro effect without a second body expansion?']),
'unresolved_include_pair':(['quoted unresolved include candidate','system unresolved include candidate'],['With no supplied header files, does the pinned parser leave both include requests unresolved?','Does each missing quoted/system include still produce import/dependency records, and what are their exact CODE/name/properties/locations/edges and ordering?','Are there diagnostics, external fallback content or failure statuses that prevent a complete reference? Retain them; do not substitute guessed empty graphs.'])}
input_hashes={};cases=[]
for name,fs in files.items():
 for rel,data in fs.items():
  assert data.endswith(b'\n')and b'\r'not in data
  p=out/'input'/name/rel;p.parent.mkdir(parents=True,exist_ok=True)
  with p.open('xb')as f:f.write(data)
  input_hashes[name+'/'+rel]=sha(p)
 cases.append({'name':name,'coverage':coverage[name][0],'questionsExpectedUnknown':coverage[name][1],'files':{rel:input_hashes[name+'/'+rel]for rel in fs},'freshCanonicalExpected':None,'freshSupplementExpected':None,'provenance':provenance[name]})
assert len(cases)==6 and len(input_hashes)==11
(up:=out/'upstream').mkdir()
uproot=root/'cpg-rs/joern-parity/tests/fixtures/body-macro-state/provenance/upstream'
for name in ['AstCreatorHelper.scala','AstCreator.scala','tag-resolution.txt']:(up/name).write_bytes((uproot/name).read_bytes())
runner=peer/'.local/fixedtables-identity-runner-v3';observer=own/'.local/twelfth-identity-observer-v2'
plan={'status':'PREPARED_INPUT_SCOPE_ONLY_NOT_RELEASED','projects':6,'sourceFiles':11,'reusedProjects':4,'newInputProjects':2,'inputHashes':dict(sorted(input_hashes.items())),'cases':cases,'sourceTexts':{name:{rel:data.decode()for rel,data in fs.items()}for name,fs in files.items()},'scope':'Direct caller include-reference nodes/properties/edges. No parser semantics change, source implementation, new expected graph, or producer.','historicalAnchors':'Four exact-source canonical references are available for full-byte comparison after fresh generation; they are historical evidence, not newly generated outputs. Three also have admitted complete saved-CPG supplements.','upstream':{'tag':'v4.0.555','commit':'d95237aeaf3d12cb4e63336def3a4d9d7315dfb4','files':{n:row(uproot/n)for n in ['AstCreatorHelper.scala','AstCreator.scala','tag-resolution.txt']},'observedRule':'AstCreatorHelper.scala179-189 builds import/dependency pairs from getIncludeDirectives filtered only by physical caller file; AstCreator.scala70-74 places them before the translation-unit declaration AST. Whether each inactive/unresolved directive reaches that inventory remains an observation question.'},'unchangedProducersToReuse':{'canonicalOracle':row(runner/'oracle.sc'),'canonicalRunnerPredecessor':row(runner/'run-oracle.py'),'historicalRuntimeInventory':row(runner/'runtime-bindings.json'),'supplementalObserverScala':row(observer/'observer.sc'),'supplementalDriverPredecessor':row(observer/'run-observer.py')},'futureAdmission':{'acceptedTwelfthCommit':None,'acceptedTwelfthDocumentationCommit':None,'parentExecutionRelease':None,'currentFrozenSourceForLaterAdmission':row(root/'.local/astra-sprint/twelfth-batch/lint-corrected/final-real-differential/bin/source-bindings.json'),'runtimeMustBeReattestedBeforeAndAfter':True,'historicalRuntimeInventoryIsNotFreshAttestation':True},'newExpectedGraphs':0,'producerRuns':0,'productionSourceOrTestEdits':False}
with (out/'preparation.json').open('x')as f:json.dump(plan,f,indent=2);f.write('\n')
print(json.dumps({'sha256':sha(out/'preparation.json'),'projects':len(cases),'sourceFiles':len(input_hashes)}))
