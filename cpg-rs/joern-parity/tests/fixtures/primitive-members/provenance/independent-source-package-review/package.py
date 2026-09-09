from pathlib import Path
import hashlib,json,re,subprocess
from collections import Counter
PARENT=Path('/Users/shayaunnejad/vibe-code/.codex-worktrees')
W=PARENT/'joern-oxidized-astra-primitive-members';P=W/'.local/primitive-members'
OUT=Path(__file__).parent
def sha(p):return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def read(p):return json.loads(Path(p).read_bytes())
checks=[]
def ck(label,ok):
    checks.append({'check':label,'pass':bool(ok)})
    if not ok:raise AssertionError(label)
def bound(p,h):ck(str(p),sha(p)==h)
freeze=P/'package-v1/freeze.json';f=read(freeze);stage=Path(f['stagedRoot'])
rel='cpg-rs/joern-parity/tests/fixtures/primitive-members';family=stage/rel
bound(freeze,'d5d9b40efac4f1a36b22eeccbedb180a6a57a5bd41e23a7bae593fc16192995a')
actual={p.relative_to(stage).as_posix() for p in stage.rglob('*') if p.is_file()}
ck('exact 3252-file staged inventory',actual==set(f['files']) and len(actual)==f['fileCount']==3252)
ck('complete byte total',sum((stage/p).stat().st_size for p in actual)==f['bytes']==27434165)
for p,r in f['files'].items():
    bound(stage/p,r['sha256']);ck(p+' bytes',(stage/p).stat().st_size==r['bytes'])
    bound(W/p,r['sha256'])
ck('three non-fixture paths only',actual-{p for p in actual if p.startswith(rel+'/')}=={'cpg-rs/cpg-lang-c/src/exact.rs','cpg-rs/joern-parity/QUIRKS.md','cpg-rs/joern-parity/tests/primitive_members.rs'})
for k in ['sourcePatch','testPatch','quirksPatch']:bound(f[k]['path'],f[k]['sha256'])
copy=read(family/'provenance/copy-manifest.json')
ck('3205 provenance copies',len(copy['files'])==copy['copiedFiles']==f['copiedProvenanceFiles']==3205)
ck('unique portable copied paths',len({r['relativePath'] for r in copy['files']})==3205)
for r in copy['files']:
    bound(r['originalPath'],r['sha256']);bound(family/r['relativePath'],r['sha256'])
    ck(r['relativePath']+' copied size',(family/r['relativePath']).stat().st_size==r['bytes'])
m=read(family/'measurement.json');bound(family/'measurement.json',f['measurementSha256']);bound(family/'README.md',f['readmeSha256'])
source=read(OUT/'source-review.json');bound(OUT/'source-review.json','c661e055259b242337c477bb3a44040de1a59502a9433fd8e6a82fc9aeb92855')
ck('source SHA binding',m['productionSourceSha256']==sha(stage/'cpg-rs/cpg-lang-c/src/exact.rs'))
bound(family/m['frozenBuildBindings'],m['frozenBuildBindingsSha256']);bound(family/m['priorReceipt'],m['priorReceiptSha256'])
bound(family/m['validation']['receipt'],m['validation']['sha256'])
rows={r['case']:r for r in source['cases']};gates=set()
for r in m['cases']:
    n=r['case'];a=rows[n];bound(family/r['expected'],r['expectedSha256'])
    for key,h in r['inputHashes'].items():bound(family/r['inputs'][key],h)
    ck(n+' measured exact/retention claims',all(r[k]==a[k] for k in ['beforeExact','candidateExact','beforeCandidateIdentical']))
    ck(n+' measured gains/losses',r['matchingGainsNonempty']==a['gained'] and r['matchingLossesNonempty']==a['lost'])
    rr=read(family/r['replay']);ck(n+' portable complete row',rr==read(P/'replay19-v1'/n/'run.json'))
    ck(n+' gate classification',r['classification']==('complete_exact_gate' if a['candidateExact'] else 'retained_complete_diagnostic'))
    if a['candidateExact']:gates.add(n)
ck('18 exact, one full diagnostic',len(gates)==18 and len(m['cases'])==19)
ck('literal LF and nonempty counts',m['summary']['referenceLfLines']==source['totals']['expectedLfLines']==3493 and m['summary']['referenceNonemptyRecords']==source['totals']['expectedNonempty']==3423)
ck('no additional records or split-terminal gain',m['summary']['matchingGainsNonempty']==126 and m['summary']['matchingLossesNonempty']==0)
ck('prior summary copied without reclassification',m['priorSummary']==read(family/m['priorReceipt'])['summary'])
q=stage/'cpg-rs/joern-parity/QUIRKS.md';oldq=subprocess.check_output(['git','show','f235a0f898c4e19fda90998b903f57741b776ec7:cpg-rs/joern-parity/QUIRKS.md'],cwd=W)
ck('QUIRKS preserves whole accepted prefix',q.read_bytes().startswith(oldq))
ck('QUIRKS only added reviewed section',q.read_bytes()[len(oldq):].decode().startswith('\n### Primitive MEMBER declaration spelling\n'))
test=stage/'cpg-rs/joern-parity/tests/primitive_members.rs';bound(test,source['testClassification']['finalTestSha256'])
patch=(family/'provenance/test.patch').read_text().split('\n')
ck('test patch creates full measured test bytes',''.join(x[1:]+'\n' for x in patch if x.startswith('+') and not x.startswith('+++')).encode()==test.read_bytes())
readme=(family/'README.md').read_text()
for target in re.findall(r'\]\(([^)]+)\)',readme):
    ck('README relative link '+target,(family/target).resolve().is_file())
ck('explicit pending root acceptance','Root integration, whole-project comparisons and final resource acceptance are\npending.' in readme)
ck('explicit unchanged diagnostic','nonprimitive_scalar_control` remains a complete, byte-identical diagnostic' in readme)
ck('explicit failed run exclusion','failed\nruns are excluded from successful-pair matching-record claims' in readme)
ck('explicit overlap exclusion','must not be added again to the 19-project gain count' in readme)
# Establish the retained diagnostic's exact complete missing-record count.
d=family/'provenance/replay19/nonprimitive_scalar_control'
e=Counter(x for x in (d/'expected.txt').read_bytes().split(b'\n') if x);a=Counter(x for x in (d/'actual.txt').read_bytes().split(b'\n') if x)
ck('nonprimitive diagnostic 20 absent records and no extras',sum((e-a).values())==20 and not (a-e))
ck('nonprimitive all MEMBER records exact',Counter({k:v for k,v in e.items() if b'MEMBER NAME=' in k})==Counter({k:v for k,v in a.items() if b'MEMBER NAME=' in k}))
report={'status':'PASS_IMMUTABLE_PORTABLE_PACKAGE','checks':checks,'checkCount':len(checks),'freeze':{'path':str(freeze),'sha256':sha(freeze)},'targetHashes':{str(p.relative_to(stage)):sha(p) for p in [family/'README.md',family/'measurement.json',test,q]},'sourceReview':{'path':str(OUT/'source-review.json'),'sha256':sha(OUT/'source-review.json')},'summary':{'stagedFiles':3252,'stagedBytes':27434165,'byteIdenticalProvenanceCopies':3205,'completeReferences':19,'expectedLfLines':3493,'expectedNonemptyRecords':3423,'exactGraphsBefore':2,'exactGraphsCandidate':18,'fullGraphTestGates':18,'fullDiagnostic':1,'matchingNonemptyGains':126,'matchingNonemptyLosses':0},'limits':['No source edits, builds, oracle runs or candidate producer executions by this reviewer.','Raw 229-case retention audit belongs to analysis_scope; complete portable files byte-bound here and prior summary copied faithfully.','Root integration, whole-project and resource acceptance remain pending.','The 3423 nonempty records count the complete reference corpus; the nonprimitive candidate retains a 20-record scaffold deficit.']}
(OUT/'package-review.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps({'checks':len(checks),'sha256':sha(OUT/'package-review.json'),'status':report['status']}))
