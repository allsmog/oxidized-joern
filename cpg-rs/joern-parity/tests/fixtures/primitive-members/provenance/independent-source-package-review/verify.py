from pathlib import Path
from collections import Counter
import hashlib, json, re, subprocess, difflib

TASK_ROOT = Path('/Users/shayaunnejad/vibe-code/.codex-worktrees')
W = TASK_ROOT/'joern-oxidized-astra-primitive-members'
P = W/'.local/primitive-members'
OUT = Path(__file__).parent
BASE = 'f235a0f898c4e19fda90998b903f57741b776ec7'
checks=[]
def check(label, ok):
    checks.append({'check':label,'pass':bool(ok)})
    if not ok: raise AssertionError(label)
def digest(b): return hashlib.sha256(b).hexdigest()
def sha(p): return digest(Path(p).read_bytes())
def read(p): return json.loads(Path(p).read_bytes())
def bound(p,h): check(str(p),sha(p)==h)
def lf(b): return b.replace(b'\r\n',b'\n').replace(b'\r',b'\n')
def lines(b): return b.decode().split('\n')
def counters(e,b,c,nonempty):
    E,B,C=[Counter(x for x in lines(v) if x or not nonempty) for v in (e,b,c)]
    old,new=E&B,E&C;g,l=new-old,old-new
    return dict(expected=sum(E.values()),before=sum(B.values()),candidate=sum(C.values()),matchingBefore=sum(old.values()),matchingCandidate=sum(new.values()),gained=sum(g.values()),lost=sum(l.values()),gainedRecords=dict(g),lostRecords=dict(l))
def gitblob(rel): return subprocess.check_output(['git','show',f'{BASE}:{rel}'],cwd=W)

F=P/'frozen-v1';R=P/'replay19-v1';V=P/'validation-v1-final-test'
B=read(F/'bindings.json');RR=read(R/'replay.json');val=read(V/'checks.json');copied=read(P/'copy-admitted.json')
bound(F/'bindings.json','9a036a96c94a34c9bffc85ba101b5db0ea0af40f581f6621cc8c40851cfb98b9')
bound(R/'replay.json','ca8114b68f4e4f31ffbc20e834bc1892db7200f689097956e6ee5a9c13c0e942')
bound(V/'checks.json','1b583646d0dee5c0040033c7ec32b9d7afa23471d2282803134ef15bcd6f5d72')
bound(F/'source.patch',B['sourcePatchSha256']);bound(F/'build.log',B['buildLogSha256'])
bound(P/'scripts/build-v1.py',B['scriptSha256']);bound(P/'scripts/replay19-v1.py',RR['helperSha256'])
bound(P/'copy-admitted.json',RR['copyReceiptSha256']);bound(RR['baselineReceipt'],RR['baselineReceiptSha256'])
bound(copied['runPath'],copied['runSha256']);bound(copied['inputPreparationPath'],copied['inputPreparationSha256'])
check('build accepted baseline/success/stability',B['baselineCommit']==BASE and B['exitCode']==0 and B['allInputsUnchanged'])
check('build before inventory equals successful binding',read(F/'before.json')=={'sources':B['sourceInputs'],'references':B['referenceInputs']})
check('139 frozen inputs',len(B['sourceInputs'])==B['sourceInputCount']==139)
for name,rec in B['binaries'].items():bound(rec['path'],rec['sha256'])
rel='cpg-rs/cpg-lang-c/src/exact.rs';old=gitblob(rel);new=(F/'source'/rel).read_bytes()
before=b'''            let ty = normalize_type(
                &f.child_by_field_name("type")
                    .map(|t| text(t, b).to_string())
                    .unwrap_or("ANY".into()),
            );'''
after=b'            let ty = declaration_type(f, b, TypeRole::Declaration);'
check('one measured member-base replacement reconstructs complete frozen source',old.count(before)==1 and old.replace(before,after)==new)
bound(F/'source'/rel,B['sourceInputs'][rel])
check('no source helper or other production byte changed',old[old.index(b'fn declaration_type('):]==new[new.index(b'fn declaration_type('):])
for rel,h in B['sourceInputs'].items():
    if rel=='cpg-rs/joern-parity/tests/primitive_members.rs':bound(F/'source'/rel,h)
    elif rel=='cpg-rs/cpg-lang-c/src/exact.rs':pass
    else:check('accepted Git source '+rel,digest(gitblob(rel))==h)
for p in (F/'source').rglob('*'):
    if p.is_file():bound(p,B['sourceInputs'][p.relative_to(F/'source').as_posix()])
for rel,h in B['referenceInputs'].items():bound(W/rel,h)
check('only final test classification changes after source freeze',[k for k in B['sourceInputs'] if B['sourceInputs'][k]!=val['sourceInputs'][k]]==['cpg-rs/joern-parity/tests/primitive_members.rs'])
for rel,h in val['sourceInputs'].items():bound(W/rel,h)
test=(W/'cpg-rs/joern-parity/tests/primitive_members.rs').read_text()
oldtest=(F/'source/cpg-rs/joern-parity/tests/primitive_members.rs').read_text()
check('final test removes only measured nonexact case',test.replace('    // nonprimitive_scalar_control retains a complete scaffold diagnostic;\n    // its MEMBER rows and full output are preserved in the recorded replay.\n','')==oldtest.replace('        "nonprimitive_scalar_control",\n',''))
gate_names=re.findall(r'^        "([a-z_]+)",?$',test,re.M)
# The two retained anchors appear on one inline array line.
gate_names+=['tiny_fixedtables_include','tiny_fixedtables_inline']
check('final test has 18 full graph gates',len(gate_names)==18 and len(set(gate_names))==18)
check('test compares complete canonical production graph','cpg_lang_c::import::canonical_dump(&project.cpg)' in test and 'root.join("expected.txt")' in test)
for rec in val['checks']:
    bound(V/(rec['check']+'.log'),rec['logSha256'])
    check('gate success '+rec['check'],rec['exitCode']==0 and rec['sourceInputsUnchanged'])
for rec in val['testBinaries'].values():bound(rec['path'],rec['sha256'])
focused=(V/'focused.log').read_text();groups=re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;',focused)
check('21 focused tests no failed/ignored',sum(int(a) for a,b,c in groups)==21 and all(b==c=='0' for a,b,c in groups))
check('main committed 308 pass','308/308' in (V/'main308.log').read_text())

baseline=read(RR['baselineReceipt']);basecases={x['case']:x for x in baseline['cases']};copycases={x['case']:x for x in copied['cases']}
check('complete unique 19 inventory',len(RR['cases'])==19 and len(copycases)==19 and {x['case'] for x in RR['cases']}==set(copycases))
total=Counter();rows=[]
for row in RR['cases']:
    n=row['case'];d=R/n;bc=basecases[n];cc=copycases[n]
    check(n+' saved row',read(d/'run.json')==row)
    for rel,h in row['files'].items():bound(d/rel,h)
    for rel,h in row['inputHashes'].items():
        bound(d/'input'/rel,h);check(n+' admitted source '+rel,cc['inputHashes'][rel]==h and bc['inputBefore']['files'][rel]['sha256']==h)
    e=(d/'expected.txt').read_bytes();b=(d/'before.txt').read_bytes();c=(d/'actual.txt').read_bytes()
    bound(cc['referencePath'],cc['expectedSha256']);check(n+' full admitted reference',e==Path(cc['referencePath']).read_bytes())
    bound(bc['actual']['path'],bc['actual']['sha256']);check(n+' accepted old full output',b==Path(bc['actual']['path']).read_bytes())
    check(n+' candidate stdout LF only',c==lf((d/'stdout.txt').read_bytes()))
    check(n+' command and successful status',row['command']==[B['binaries']['joern-parity']['path'],'--production',*sorted(row['inputHashes'])] and row['exitCode']==0 and not row['timeout'] and row['error'] is None)
    check(n+' exact status correct',row['beforeExact']==(b==e) and row['candidateExact']==(c==e) and row['beforeCandidateIdentical']==(b==c))
    for label,ne in [('includingSeparators',False),('nonempty',True)]:
        own=counters(e,b,c,ne);check(n+' '+label+' counters',own==row['counts'][label])
    for target,a,z,an,bn in [('expected-before.diff',e,b,'live','accepted-tenth'),('expected-candidate.diff',e,c,'live','candidate-v1'),('before-candidate.diff',b,c,'accepted-tenth','candidate-v1')]:
        diff=''.join(difflib.unified_diff(a.decode().splitlines(True),z.decode().splitlines(True),fromfile=an,tofile=bn)).encode()
        check(n+' complete diff '+target,(d/target).read_bytes()==diff)
    check(n+' flow bytes unchanged',[x for x in lines(b) if x.startswith('FLOWS|')]==[x for x in lines(c) if x.startswith('FLOWS|')])
    def astwithouttypes(v):
        return [re.sub(r' TYPE_FULL_NAME=.*?(?= ORDER=|$)','',x) for x in lines(v) if not x.startswith(('NODES|','EDGES|','FLOWS|'))]
    check(n+' AST topology and non-type properties unchanged',astwithouttypes(b)==astwithouttypes(c))
    check(n+' no old matching loss',row['counts']['nonempty']['lost']==0)
    total.update({'cases':1,'beforeExact':b==e,'candidateExact':c==e,'gained':row['counts']['nonempty']['gained'],'lost':row['counts']['nonempty']['lost'],'expectedLfLines':e.count(b'\n'),'expectedNonempty':sum(bool(x) for x in e.split(b'\n'))})
    rows.append({'case':n,'beforeExact':b==e,'candidateExact':c==e,'beforeCandidateIdentical':b==c,'expectedSha256':digest(e),'beforeSha256':digest(b),'candidateSha256':digest(c),'gained':row['counts']['nonempty']['gained'],'lost':row['counts']['nonempty']['lost']})
check('independent 19-case totals',dict(total)==dict(cases=19,beforeExact=2,candidateExact=18,gained=126,lost=0,expectedLfLines=3493,expectedNonempty=3423))
check('exact gates equal successful full-exact cases',set(gate_names)=={r['case'] for r in rows if r['candidateExact']})
check('nonprimitive retained complete diagnostic',next(r for r in rows if r['case']=='nonprimitive_scalar_control')['beforeCandidateIdentical'])

priorpath=P/'replay-prior-v1/replay.json';prior=read(priorpath)
bound(priorpath,'1ec133c54770652fc9384877ed88a0785f8f6e245c851fa8395fb2e14602fa31')
check('prior receipt pair binding',prior['beforeBinarySha256']==baseline['binarySha256'] if 'binarySha256' in baseline else prior['candidateBinarySha256']==B['binaries']['joern-parity']['sha256'])

report={'status':'PASS_BOUNDED_FROZEN_SOURCE_AND_COMPLETED_REPLAYS','checkCount':len(checks),'checks':checks,'bindings':{str(p):sha(p) for p in [F/'bindings.json',F/'source.patch',F/'source/cpg-rs/cpg-lang-c/src/exact.rs',R/'replay.json',V/'checks.json',priorpath]},'sourceScope':'One MEMBER base-type call now uses existing TypeRole::Declaration. The renderer and nonprimitive normalize_type fallback are unchanged; CODE, suffixes, order, enum branch and clinit logic are byte-identical. No importer, solver, checker or reference changes.','totals':dict(total),'cases':rows,'priorReplay':{'status':'Raw review assigned independently to analysis_scope; only receipt identity bound here','instancesReported':229},'testClassification':{'initialFrozenTestIncorrectlyIncluded':'nonprimitive_scalar_control','initialFrozenTestPreserved':True,'finalTestSha256':sha(W/'cpg-rs/joern-parity/tests/primitive_members.rs'),'fullGraphGates':18,'finalTestOnlyRemovesMeasuredDiagnosticFromExactGateAndAddsExplanation':True},'limits':['Existing outputs independently recomputed; no reviewer Rust/Joern producer or Cargo execution.','The 19 complete canonical references contain 3493 LF lines and 3423 nonempty records. The replay includingSeparators counters additionally count the terminal split empty item; no gain/loss effect.','Nonprimitive scalar control retains missing external nested/tag/member scaffold, byte-identical to accepted tenth.','Portable package not sealed/reviewed yet. Root integration, full-project and resource acceptance remain separate.']}
(OUT/'source-review.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps({'status':report['status'],'checks':len(checks),'receipt':str(OUT/'source-review.json'),'sha256':sha(OUT/'source-review.json'),'totals':dict(total)}))
