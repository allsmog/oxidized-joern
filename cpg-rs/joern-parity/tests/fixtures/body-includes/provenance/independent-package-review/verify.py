from pathlib import Path
from collections import Counter
import hashlib,json,re,subprocess
O=Path(__file__).parent
W=Path('/Users/shayaunnejad/vibe-code/.codex-worktrees/joern-oxidized-astra-body-includes')
P=W/'.local/body-includes';F=W/'cpg-rs/joern-parity/tests/fixtures/body-includes'
checks=[]
def ck(n,b):
 assert b,n
 checks.append(n)
def h(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def bind(p):return {'path':str(p),'sha256':h(p),'bytes':p.stat().st_size}
m=json.loads((P/'package-v2-freeze.json').read_text());d=json.loads((F/'measurement.json').read_text())
ck('freeze exact',h(P/'package-v2-freeze.json')=='5c111a73e64cf54b5f6cb0a84bcede39bc14278b2b732d129aeabdb1393308dc')
ck('760 file freeze',m['fileCount']==len(m['files'])==760)
for p,s in m['files'].items():ck('frozen '+p,h(W/p)==s)
for p,row in d['copiedEvidence'].items():ck('copy '+p,h(F/p)==row['sha256']==h(Path(row['source'])))
ck('same candidate frozen binding',d['candidateBindings']==json.loads((P/'frozen-v2/bindings.json').read_text()))
for key,file in [('productionBindingsSha256','frozen-v2/bindings.json'),('sourcePatchSha256','source-v2.patch'),('testPatchSha256','test-v2.patch'),('quirksPatchSha256','quirks-v2.patch'),('newTestsReceiptSha256','new-package-tests-v2-final.json')]:ck('freeze '+key,h(P/file)==m[key])
for key,p in [('measurementSha256',F/'measurement.json'),('READMEsha256',F/'README.md'),('newTestSourceSha256',W/'cpg-rs/joern-parity/tests/body_includes.rs')]:ck('freeze '+key,h(p)==m[key])
raw={}
for group in ['body-includes','source-locations']:
 current=None;items={}
 for line in (F/'oracle'/group/'live.stdout').read_text().split('\n'):
  if line.startswith('CASE|'):current=line[5:];ck(group+' unique CASE '+current,current not in items);items[current]=[]
  elif line.startswith(('AST|','NODES|','EDGES|','FLOWS|')):
   ck(group+' selected belongs to CASE',current is not None)
   items[current].append(line[4:] if line.startswith('AST|') else line)
 raw[group]={key:('\n'.join(lines)+'\n').encode() for key,lines in items.items()}
rows=[]
for r in d['cases']:
 c=r['case'];p=F/'cases'/c;e=(p/'expected.txt').read_bytes();b=(p/'baseline/actual.txt').read_bytes();a=(p/'candidate/actual.txt').read_bytes()
 ck(c+' complete raw extraction',e==raw[r['oracleGroup']][r['oracleCase']] and h(p/'expected.txt')==r['expectedSha256'])
 selected_sources={str(f.relative_to(p)):h(f) for f in p.rglob('*') if f.is_file() and f.suffix in ['.c','.h']}
 ck(c+' actual test source inventory',selected_sources==r['inputHashes'])
 for version,contents in [('baseline',b),('candidate',a)]:
  rr=r[version];ck(c+' '+version+' output',h(p/version/'actual.txt')==rr['actualSha256']);ck(c+' '+version+' stderr',h(p/version/'stderr.txt')==rr['stderrSha256']);ck(c+' '+version+' exact',rr['exact']==(rr['exitCode']==0 and contents==e));ck(c+' '+version+' success',rr['exitCode']==0)
  if 'diffSha256' in rr:ck(c+' '+version+' diff',h(p/version/'complete.diff')==rr['diffSha256'])
 ec,bc,ac=(Counter(x for x in text.decode().split('\n') if x) for text in [e,b,a]);lost=(bc&ec)-ac;gained=(ac&ec)-bc
 ck(c+' exact loss multiplicity',dict(lost)==r['lostRecords'] and sum(lost.values())==r['matchingLost'])
 ck(c+' exact gain multiplicity',dict(gained)==r['gainedRecords'] and sum(gained.values())==r['matchingGained'])
 ck(c+' canonical line counts',len(e.decode().split('\n'))-1==r['canonicalLinesIncludingSeparators'] and sum(ec.values())==r['nonemptySelectedRecords'])
 rows.append({'case':c,'baselineExact':b==e,'candidateExact':a==e,'matchingGained':sum(gained.values()),'matchingLost':sum(lost.values()),'canonical':r['canonicalLinesIncludingSeparators'],'nonempty':sum(ec.values()),'expected':bind(p/'expected.txt'),'candidate':bind(p/'candidate/actual.txt'),'baseline':bind(p/'baseline/actual.txt')})
ck('14 unique projects',len(rows)==len(set(x['case'] for x in rows))==14)
for key,value in [('baselineExact',sum(x['baselineExact'] for x in rows)),('candidateExact',sum(x['candidateExact'] for x in rows)),('rawMatchingGained',sum(x['matchingGained'] for x in rows)),('rawMatchingLost',sum(x['matchingLost'] for x in rows)),('canonicalLinesIncludingSeparators',sum(x['canonical'] for x in rows)),('nonemptySelectedRecords',sum(x['nonempty'] for x in rows))]:ck(key,d[key]==value)
t=(W/'cpg-rs/joern-parity/tests/body_includes.rs').read_text();gate=t.split('fn body_includes_match_complete_live_graphs()')[1].split('] {')[0];names=re.findall(r'^\s*"([a-z_]+)",',gate,re.M)
ck('10 complete production gates equal all exact projects',len(names)==10 and set(names)=={x['case'] for x in rows if x['candidateExact']})
ck('full production graph equality', 'cpg_lang_c::import::canonical_dump(&build_case(name)),\n            expected_case(name),' in t)
ck('no source hidden filtering in graph gates','project.build(&borrowed)' in t and 'cpg_analysis::standard_pipeline()' in t)
ck('3 separate subset assertions', all(x in t for x in ['("tiny_fixedtables_include", "fixedtables")','("tiny_fixedtables_inline", "fixedtables")','("repeated_include_context", "first")']))
ck('2 method owners separate assertions','("main.c:N:int(0)", "main.c")' in t and '("nested/table.h:N:int(0)", "nested/table.h")' in t)
prior=json.loads((P/'ninth-v2-retention/replay.json').read_text());ck('prior76 receipt unchanged',d['priorFamilyReplay']=={k:v for k,v in prior.items() if k!='rows'})
loss=d['rawMatchingLossClassification'];ck('collision receipt binding',h(F/loss['receipt'])==loss['sha256'])
result={'status':'PASS_BOUNDED_PACKAGE_INTEGRITY_AND_ACCEPTANCE_DISTINCTION','checks':len(checks),'checkLabels':checks,'packageFreeze':bind(P/'package-v2-freeze.json'),'fileCount':760,'copiedEvidenceFilesVerifiedAgainstOriginals':len(d['copiedEvidence']),'projects':14,'baselineExact':3,'candidateExact':10,'fullGraphProductionGates':10,'separateMethodSubtreeAssertions':3,'separateCompleteMacroMethodAndOwnershipAssertions':2,'retainedCompleteDiagnostics':4,'rawMatchingGained':1263,'rawMatchingLost':2,'rawMatchingLossClassification':'Two previously reviewed ordinal collisions, raw total retained.','canonicalLinesIncludingSeparators':5113,'nonemptySelectedRecords':5014,'rows':rows,'test':bind(W/'cpg-rs/joern-parity/tests/body_includes.rs'),'README':bind(F/'README.md'),'measurement':bind(F/'measurement.json'),'quirks':bind(W/'cpg-rs/joern-parity/QUIRKS.md'),'sourceReview':bind(O.parent/'tenth-body-include-v2-review/final-review.json'),'validation':bind(P/'new-package-tests-v2-final.json'),'noProducerBuildOrSourceEdits':True,'limits':['No test executable rerun: existing bound production results and immutable test source reviewed.','The 3 subset and 2 METHOD assertions are additional structural checks, not 5 full-project gates.','No root whole-project/resource/final checkpoint acceptance.']}
(O/'verification.json').write_text(json.dumps(result,indent=2)+'\n')
print({k:result[k] for k in ['status','checks','fileCount','projects','baselineExact','candidateExact','rawMatchingGained','rawMatchingLost']})
