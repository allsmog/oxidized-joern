from pathlib import Path
import hashlib,json,difflib,collections,re,datetime
O=Path(__file__).parent
W=Path('/Users/shayaunnejad/vibe-code/.codex-worktrees/joern-oxidized-astra-body-includes');B=W/'.local/body-includes';F=B/'frozen-v4-dimensions';V=B/'verification-v4-dimensions'
P=W/'cpg-rs/joern-parity/tests/fixtures/array-dimension-macros';P14=P.with_name('body-includes')
checks=[];bindings={}
def h(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def bind(p):
 p=Path(p);r={'path':str(p),'sha256':h(p),'bytes':p.stat().st_size};bindings[str(p)]=r;return r

def ck(n,b):assert b,n;checks.append(n)
def checked(p,d):ck(str(p),bind(p)['sha256']==d)
def j(p):bind(p);return json.loads(p.read_text())
m=j(B/'array-package-v4-freeze-final.json');old=j(B/'package-v2-freeze.json');mb=j(F/'bindings.json');cb=j(F/'bindings-clarified.json');v3=j(B/'frozen-v3-ptf/bindings.json');v=j(V/'checks.json');measure=j(P/'measurement.json')
ck('193 declared files',len(m['files'])==m['fileCount']==193)
for n,d in m['files'].items():checked(W/n,d)
for n,d in old['files'].items():checked(W/n,d)
ck('old760 unchanged',len(old['files'])==760)
for row in measure.get('additionalCopiedEvidence',[]):
 ck('copied '+row['path'],(P/row['path']).read_bytes()==Path(row['originalPath']).read_bytes() and h(P/row['path'])==row['sha256'])
for name,sha in [('measurement.json',m['measurementSha256']),('README.md',m['readmeSha256'])]:checked(P/name,sha)
checked(F/'bindings.json',cb['priorUnmodifiedBuildReceiptSha256'])
ck('clarification only provenance wording',all(cb[k]==mb[k] for k in mb if k!='note'))
ck('196 build input observations',j(F/'before-build.json')==j(F/'after-build.json')==mb['source'] and len(mb['source'])==196)
ck('only existing source change exact', [k for k in v3['source'] if v3['source'][k]!=mb['source'][k]]==['cpg-rs/cpg-lang-c/src/exact.rs'])
ck('one added test input',set(mb['source'])-set(v3['source'])=={'cpg-rs/joern-parity/tests/array_dimension_macros.rs'})
for p in (F/'source').rglob('*'):
 if p.is_file():checked(p,mb['source'][str(p.relative_to(F/'source'))])
for n,r in mb['binaries'].items():checked(F/n,r['sha256'])
checked(F/'build.log',mb['buildLogSha256']);ck('build succeeded',mb['buildStatus']==0 and mb['sourceUnchanged'])
new=(F/'source/cpg-rs/cpg-lang-c/src/exact.rs').read_text()
for tag,patch in [('v3-ptf',F/'v3-v4.patch'),('v2',B/'source-v2-v4.patch')]:
 prior=(B/f'frozen-{tag}'/'source/cpg-rs/cpg-lang-c/src/exact.rs').read_text()
 expected=''.join(difflib.unified_diff(prior.splitlines(True),new.splitlines(True),fromfile='a/cpg-rs/cpg-lang-c/src/exact.rs',tofile='b/cpg-rs/cpg-lang-c/src/exact.rs'))
 ck(tag+' complete patch reconstruction',expected==patch.read_text());bind(patch)
# The V3 delta was already independently proven; no additional semantics in V4.
a=(B/'frozen-v3-ptf/source/cpg-rs/cpg-lang-c/src/exact.rs').read_text()
start=a.index('fn object_decl_suffix(');end=a.index('\n/// CDT\'s nested declarator',start)
s2=new.index('fn object_decl_suffix(');e2=new.index('\n/// CDT\'s nested declarator',s2)
ck('only object suffix function changed',a[:start]==new[:s2] and a[end:]==new[e2:])
ck('explicit identifier shape guard','if sz.kind() == "identifier"' in new[s2:e2] and '} else {\n                        text(sz, b).to_string()' in new[s2:e2])
test=W/'cpg-rs/joern-parity/tests/array_dimension_macros.rs';checked(test,m['newTestSha256']);ck('frozen test bytes',test.read_bytes()==(F/'source/cpg-rs/joern-parity/tests/array_dimension_macros.rs').read_bytes())
patch=Path(m['newTestPatchPath']);checked(patch,m['newTestPatchSha256']);lines=patch.read_text().splitlines(True);body=''.join(l[1:] for l in lines[3:] if l.startswith('+'));ck('corrected test patch exact',body==test.read_text())
ck('whole production equality not selected assertions','project.build(&[("main.c", source)])' in body and 'cpg_lang_c::import::canonical_dump(&project.cpg),\n            expected,' in body and 'standard_pipeline()' in body)
ck('4 test source/reference pairs',len(re.findall(r'include_str!',body))==8)
for rel in re.findall(r'include_str!\("([^"]+)"\)',body):
 pp=test.parent/rel;ck('test reference '+rel,pp.is_file());bind(pp)
# Bound source/test-only prior reviews; original pending-stage bytes remain untouched.
for n in ['package-review.json','static-boundary-checks.json']:bind(O/n)
stage=Path('/Users/shayaunnejad/vibe-code/.codex-worktrees/joern-oxidized-astra-typedef-existence/.local/tenth-array-dimension-package-v1');sm=j(stage/'manifest.json')
for r in sm['files']:checked(Path(sm['stagedRoot'])/r['path'],r['sha256'])
ck('historical README and measurement preserved', (P/'provenance/stage-v1/README.md').read_bytes()==(Path(sm['stagedRoot'])/sm['fixtureRoot']/'README.md').read_bytes() and (P/'provenance/stage-v1/measurement.json').read_bytes()==(Path(sm['stagedRoot'])/sm['fixtureRoot']/'measurement.json').read_bytes())
# Current14+4 complete graph outcomes and full multiset counters.
r14=j(V/'replay14.json');r4=j(B/'array-v4-replay/replay.json');cases=[];union_gain=union_loss=baseexact=currexact=canon=nonempty=0;identities=set()
for r in r14['rows']:
 name=r['case'];d=V/'cases'/name;e=(d/'expected.txt').read_bytes();cur=(d/'actual.txt').read_bytes();prior=(P14/'cases'/name/'baseline/actual.txt').read_bytes()
 ck(name+' status boundV4',r['status']==0 and r['command'][0]==str(F/'joern-parity'))
 for p,dig in [(d/'actual.txt',r['v3Sha256']),(d/'expected.txt',r['expectedSha256']),(d/'v2-actual.txt',r['v2Sha256'])]:checked(p,dig)
 ck(name+' V2 complete identity',cur==(d/'v2-actual.txt').read_bytes())
 for n,dig in r['inputs'].items():checked(d/'input'/n,dig)
 cases.append((name,e,prior,cur,tuple(sorted(r['inputs'].items()))))
for r in r4['rows']:
 name=r['case'];d=B/'array-v4-replay'/name;e=(d/'expected.txt').read_bytes();cur=(d/'actual.txt').read_bytes();prior=(P/'cases'/name/'baseline/actual.txt').read_bytes()
 ck(name+' status boundV4',r['exitCode']==0 and r['command'][0]==str(F/'joern-parity'))
 for p,dig in [(d/'actual.txt',r['actualSha256']),(d/'stderr.txt',r['stderrSha256']),(d/'expected.txt',r['expectedSha256']),(d/'input/main.c',r['inputSha256']),(d/'complete.diff',r['diffSha256'])]:checked(p,dig)
 ck(name+' full exact',cur==e==(P/'cases'/name/'expected.txt').read_bytes())
 ck(name+' shipped output exact',cur==(P/'cases'/name/'candidate-v4/actual.txt').read_bytes())
 cases.append((name,e,prior,cur,(('main.c',r['inputSha256']),)))
for name,e,prior,cur,identity in cases:
 ck(name+' unique source project',identity not in identities);identities.add(identity)
 ec=collections.Counter(e.splitlines());oc=collections.Counter(prior.splitlines());nc=collections.Counter(cur.splitlines());gain=sum(((ec&nc)-(ec&oc)).values());loss=sum(((ec&oc)-(ec&nc)).values());union_gain+=gain;union_loss+=loss
 baseexact+=prior==e;currexact+=cur==e;canon+=len(e.splitlines());nonempty+=sum(bool(l.strip()) for l in e.splitlines())
 ck(name+' no prior exact graph regression',prior!=e or cur==e)
ck('18 final union',len(cases)==18 and (baseexact,currexact,canon,nonempty,union_gain,union_loss)==(6,14,7125,6991,1284,2))
# Prior76: all outputs (including failed empty stdout) match V2; actual statuses preserved separately.
r76=j(B/'ninth-v4-retention/replay.json');prior76=j(B/'ninth-v2-retention/replay.json');checked(B/'ninth-v2-retention/replay.json',r76['priorReplaySha256']);aborts=[];exact76=0;lost76=gained76=0
for r in r76['rows']:
 name=r['case'];d=B/'ninth-v4-retention'/name;e=(d/'expected.txt').read_bytes();cur=(d/'actual.txt').read_bytes();checked(d/'expected.txt',r['expectedSha256']);checked(d/'actual.txt',r['actualSha256']);checked(d/'stderr.txt',r['stderrSha256'])
 for n,dig in r['inputHashes'].items():checked(d/'input'/n,dig)
 ck(name+' current producer',r['command'][0]==str(F/'joern-parity'))
 for ver,compare in r['comparisons'].items():
  p=Path(compare['source']);checked(p,compare['sha256']);oldbytes=p.read_bytes();success=r['status']==0 and compare['oldStatus']==0
  ck(name+ver+' actual pair flags',success==compare['successfulPair'] and (cur==oldbytes)==compare['identicalBytes'])
  if success:
   wanted=collections.Counter(e.splitlines());oldc=collections.Counter(oldbytes.splitlines());newc=collections.Counter(cur.splitlines());lost=sum(((wanted&oldc)-(wanted&newc)).values());gain=sum(((wanted&newc)-(wanted&oldc)).values());ck(name+ver+' full matching counters',(lost,gain)==(compare['matchingLost'],compare['matchingGained']))
   if ver=='accepted-ninth':lost76+=lost;gained76+=gain
  else:ck(name+ver+' no successful graph invented',compare['matchingLost'] is None and compare['matchingGained'] is None)
  if ver=='candidate-v2':ck(name+' allV2stdout identical',cur==oldbytes)
 if r['status']:
  stderr=(d/'stderr.txt').read_text();ck('same duplicate-clinit assertion',name=='duplicate_clinit_tag' and 'duplicate method node properties drift' in stderr and 'a.c:N:int(0)' in stderr and 'b.c:N:int(0)' in stderr and not cur)
  aborts.append({'case':name,'acceptedNinthExit':r['comparisons']['accepted-ninth']['oldStatus'],'v2Exit':r['comparisons']['candidate-v2']['oldStatus'],'v4Exit':r['status'],'stdoutBytes':len(cur)})
 else:exact76+=cur==e
ck('prior76 totals',len(r76['rows'])==76 and exact76==55 and (lost76,gained76)==(0,25) and len(aborts)==1)
for n,dig in [('main308.log',v['main308']['logSha256']),('main308.actual.txt',v['main308']['actualSha256']),('unit-tests.log',v['unitTests']['logSha256']),('array-test.log',v['arrayTests']['logSha256'])]:checked(V/n,dig)
ck('logged gates',sum(s.startswith('PASS  ') for s in (V/'main308.log').read_text().splitlines())==308 and '3 passed; 0 failed' in (V/'unit-tests.log').read_text() and '1 passed; 0 failed' in (V/'array-test.log').read_text())
ck('checks originalcorrect build receipt',v['candidateBindingSha256']==h(F/'bindings.json'))
whole=Path('/Users/shayaunnejad/vibe-code/.codex-worktrees/joern-oxidized-astra-typedef-existence/.local/tenth-whole-review-v4/final-review.json');checked(whole,'5fe842366f331e31305cfaf9fad9f80055915f6be497bebdf248d8e901869c05')
rec={'createdAtUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'verdict':'PASS_BOUNDED_V4_SOURCE_TEST_AND_REFERENCE_PACKAGE','checkpointAccepted':False,'checks':len(checks),'checksPassed':checks,'bindings':list(bindings.values()),'source':{'exactSha256':m['sourceSha256'],'clarifiedBindingSha256':h(F/'bindings-clarified.json'),'originalBuildReceiptSha256':h(F/'bindings.json'),'binaries':mb['binaries'],'rustCargoInputs':196,'scope':'V3→V4 changes only object_decl_suffix size expression branch: expand identifier, preserve source text for other node kinds. Nested shape recursion, parameter/member suffix logic and all V3 PTF code unchanged. One new Rust test gates four complete Project::build/standard-pipeline imported graphs.'},'package':{'deliveredFiles':193,'original135StagePreserved':True,'old760FilesUnchanged':True,'newReferences':4,'fullRawNewLines':2012,'newNonemptyRecords':1977,'anchorAdditionalGate':False},'smallUnion':{'uniqueProjects':18,'acceptedNinthExact':baseexact,'currentExact':currexact,'diagnostics':4,'canonicalLinesIncludingSeparators':canon,'nonemptySelectedRecords':nonempty,'matchingGained':union_gain,'matchingLost':union_loss,'lossClassification':'The same original14 second#12/#13 ordinal collisions; current14 bytes equal V2. Four new graphs are fully exact and lose no prior exact graph.'},'prior76':{'stdoutByteIdenticalV2':76,'successfulPairs':75,'currentExact':55,'matchingGainedVersusAcceptedNinth':gained76,'matchingLostVersusAcceptedNinth':lost76,'failedCase':aborts},'checksEvidence':{'mainBlocks':308,'rdUnitTests':3,'arrayTestFunctions':1,'arrayFullGraphGates':4,'testOrProducerRerunByReviewer':False},'provenanceClarifications':['Original build source/binary observations are correct; clarified receipt supersedes overly narrow reused performance-only note.','replay14 legacy v3Sha256 field and v2-v3 diff filenames refer to V4 current bytes via their actual command and candidate binding.','Corrected inert test patch reconstructs frozen test exactly; prior extra terminal blank patch/provisional manifest preserved.'],'wholeSemanticReview':{'path':str(whole),'sha256':h(whole),'ownedBy':'analysis_scope','scope':'Peer independently proves restoration of all96 zlib raw losses, retained61priorityfacts/19methods/3calls and no matching loss. This review binds that receipt rather than duplicating full source-occurrence analysis.'},'limits':['Final root combined source freeze, full workspace/live gates, repeated resource acceptance and manifest approval remain parent-owned and pending.','Four retained complete diagnostics and prior duplicate-clinit abort remain explicit.','No new producer, source edit, build, reference change or gate weakening by this reviewer.']}
(O/'final-review.json').write_text(json.dumps(rec,indent=2)+'\n');print(json.dumps({'path':str(O/'final-review.json'),'sha256':h(O/'final-review.json'),'checks':len(checks)},indent=2))
