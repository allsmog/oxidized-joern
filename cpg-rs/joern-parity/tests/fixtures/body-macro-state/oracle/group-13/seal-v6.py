from pathlib import Path
import json,hashlib,difflib,re,datetime
p=Path(__file__).resolve().parent;g=p/'v6-ownership';out=p/'v6-independent';owner=Path('/Users/shayaunnejad/vibe-code/.codex-worktrees/joern-oxidized-astra-body-macro-state/.local/body-macro-state');v6=owner/'candidate-v6';v5=owner/'candidate-v5'
sha=lambda f:hashlib.sha256(Path(f).read_bytes()).hexdigest();bind=lambda f:{'path':str(f),'sha256':sha(f),'bytes':Path(f).stat().st_size}
def save(f,j):
 with f.open('x') as o:json.dump(j,o,indent=2);o.write('\n')
checks=[]
def check(n,ok):checks.append({'name':n,'pass':bool(ok)});assert ok,n
pair=json.loads((v6/'bindings.json').read_bytes());check('reported successful stable build',pair['build']['exitCode']==0 and pair['build']['sourceStableDuringBuild'])
for k in ['exact.rs','joern-parity','buildLog']:
 a=pair[k];check(k+' hash binding',sha(a['path'])==a['sha256'])
check('build log completion',b'Finished `dev` profile' in Path(pair['buildLog']['path']).read_bytes())
expected_delta=list(difflib.unified_diff(v5.joinpath('exact.rs').read_text().splitlines(True),v6.joinpath('exact.rs').read_text().splitlines(True)))[2:]
actual_delta=v6.joinpath('v5-to-v6.patch').read_text().splitlines(True)[2:];check('frozen delta exactly matches V5 to V6',actual_delta==expected_delta)
prior=json.loads((out/'replay.json').read_bytes());check('12 unique primary references replayed',len(prior['results'])==12 and len({(r['group'],r['case']) for r in prior['results']})==12)
for r in prior['results']:
 check('12 replay status '+r['group']+'/'+r['case'],r['exitCode']==0)
 for k in ['input','reference','actual','stderr','completeDiff']:check('12 hash '+r['group']+'/'+r['case']+'/'+k,sha(r[k]['path'])==r[k]['sha256'])
 check('12 truthful exact '+r['case'],r['exact']==(Path(r['actual']['path']).read_bytes()==Path(r['reference']['path']).read_bytes()))
check('all three exact V4 controls preserved',sum(r['exact'] for r in prior['results'])==3)
sections={};name=None
for l in (g/'oracle-workspace/stdout').read_bytes().split(b'\n'):
 if l.startswith(b'CASE|'):name=l[5:].decode();sections[name]=[]
 elif l.startswith((b'AST|',b'NODES|',b'EDGES|',b'FLOWS|')):sections[name].append(l[4:] if l.startswith(b'AST|') else l)
check('five extra complete cases',len(sections)==5)
rows=[]
for name,lines in sections.items():
 expected=g/'input'/name/'expected.txt';check('full oracle extraction '+name,expected.read_bytes()==b'\n'.join(lines)+b'\n');variants={}
 for v in ['accepted-eighth','candidate-v6']:
  d=g/v/name;r=json.loads((d/'run.json').read_bytes());check('extra successful '+v+'/'+name,r['exitCode']==0)
  for k in ['stdout','stderr']:check('extra raw hash '+v+'/'+name+'/'+k,sha(r[k]['path'])==r[k]['sha256'])
  actual=(d/'stdout').read_bytes();text=''.join(difflib.unified_diff([l+'\n' for l in expected.read_bytes().decode().split('\n')[:-1]],[l+'\n' for l in actual.decode().split('\n')[:-1]],fromfile='Joern4.0.555',tofile=v))
  with (d/'complete-lf.diff').open('x') as f:f.write(text)
  variants[v]={'exact':actual==expected.read_bytes(),'output':bind(d/'stdout'),'run':bind(d/'run.json'),'completeDiff':bind(d/'complete-lf.diff')}
 rows.append({'case':name,'input':bind(g/'input'/name/'main.c'),'reference':bind(expected),'canonicalLines':len(lines),'nonemptyRecords':sum(bool(l) for l in lines),'variants':variants})
def method(f,key='main.c:ARG:int(0)'):
 lines=f.read_bytes().decode().split('\n');start=next(i for i,l in enumerate(lines) if l.startswith('METHOD ') and ' FULL_NAME='+key+' ' in l);end=start+1
 while end<len(lines) and lines[end]:end+=1
 return lines[start:end]
original=method(p/'type-position/input/sizeof_array/expected.txt');restored=method(out/'type-position/sizeof_array/stdout');check('original complete ARG METHOD restored',original==restored)
expected=method(g/'input/disabled_function_tail/expected.txt');before=method(g/'accepted-eighth/disabled_function_tail/stdout');after=method(g/'candidate-v6/disabled_function_tail/stdout')
check('disabled-tail complete METHOD previously exact',expected==before)
check('disabled-tail new CODE-only loss',expected!=after and '\n'.join(expected)== '\n'.join(after).replace('CODE=#define ARG 2 ','CODE=#define ARG 1 '))
for name in ['object_cycle','trailing_comment','transitive_function','transitive_ordinary']:check('adjacent ARG method exact '+name,method(g/'input'/name/'expected.txt')==method(g/'candidate-v6'/name/'stdout'))
proof=out/'disabled-terminal-regression';proof.mkdir()
for v,m in [('oracle',expected),('accepted-eighth',before),('candidate-v6',after)]:
 with (proof/(v+'.ast')).open('x') as f:f.write('\n'.join(m)+'\n')
with (proof/'method.diff').open('x') as f:f.writelines(difflib.unified_diff([l+'\n' for l in expected],[l+'\n' for l in after],fromfile='Joern=accepted-eighth',tofile='V6'))
pairs={};name=None
for l in (g/'trace/stdout').read_bytes().decode().split('\n'):
 if l.startswith('CASE|'):name=l[5:];pairs[name]=[]
 elif l.startswith('PAIR|'):pairs[name].append(l)
check('disabled-function CDT events',[(int(l.split('|')[1]),l.rsplit('|',1)[1]) for l in pairs['disabled_function_tail']]==[(94,'#define CALL F(0)'),(110,'#define ARG 1'),(150,'#define ARG 2')])
summary={'status':'ORIGINAL_BLOCKER_CLOSED_RELATED_DISABLED_TERMINAL_METADATA_BLOCKER_REMAINS','createdAtUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'source':bind(v6/'exact.rs'),'binary':bind(v6/'joern-parity'),'ownerPair':bind(v6/'bindings.json'),'buildLog':bind(Path(pair['buildLog']['path'])),'v5Baseline':bind(v5/'exact.rs'),'eventOnlyDelta':bind(v6/'v5-to-v6.patch'),'checkCount':len(checks),'checks':checks,'twelveReferenceReplay':bind(out/'replay.json'),'originalBlocker':{'closed':True,'fullMethod':'main.c:ARG:int(0)','input':bind(p/'type-position/input/sizeof_array/main.c'),'reference':bind(p/'type-position/input/sizeof_array/expected.txt'),'v6Output':bind(out/'type-position/sizeof_array/stdout'),'fullMethodByteIdentical':True},'extraFiveProjects':rows,'extraOracleRun':bind(g/'oracle-workspace/run.json'),'extraRawOracle':bind(g/'oracle-workspace/stdout'),'extraTraceRun':bind(g/'trace/run.json'),'extraRawTrace':bind(g/'trace/stdout'),'remainingFinding':{'sourceLocation':{'path':str(v6/'exact.rs'),'line':1038},'producerLocation':{'path':str(v6/'exact.rs'),'line':980},'input':bind(g/'input/disabled_function_tail/main.c'),'triggerLine':5,'laterUseLine':8,'cdtPairs':pairs['disabled_function_tail'],'oracleHeader':expected[0],'acceptedEighthHeader':before[0],'v6Header':after[0],'completeMethodEvidence':[bind(proof/(n+'.ast')) for n in ['oracle','accepted-eighth','candidate-v6']],'methodDiff':bind(proof/'method.diff'),'cause':'expand_declaration_tokens returns plain text F after expanding F(0) while F is disabled; macro_consumes_arguments then tests only the final spelling in macros and marks F available. Pinned CDT keeps that F unavailable, so the original parenthesized arguments remain separate expansion events. The omitted old ARG event causes later METHOD CODE to select the new definition.','recommendation':'Carry terminal-token expansion eligibility/provenance through the existing bounded rescanner, so a function macro disabled by its own expansion cannot consume the original arguments. Preserve the now-passing direct/transitive/comment/parenthesized controls; avoid a second evaluator or macro-name exception.'},'sourceReview':{'scope':'Only one new ownership helper and two call-site predicate changes versus V5. No expansion emission, CFG, RD, scanner, declaration rendering or oracle changes in this delta.','bounds':'Uses existing 64-level disabled-name bound and 65,536 output budget. Ordinary object cycles terminate and their complete ARG metadata matches; no stress/performance claim.','commentsAndTokens':'Full fresh trailing-comment and transitive aliases select correct ARG METHOD metadata. Existing final-token controls prove trailing WRAP consumes args; parenthesized WRAP does not.','unresolved':'Disabled function terminal availability remains a confirmed compatibility defect.'},'limits':['All17 complete selected graphs and every divergence retained; three complete exact controls remain exact, fourteen are full diagnostics.','The disabled-tail whole graph was already nonexact; the complete ARG:int(0) METHOD was exact before and loses only CODE.','This review closes the original simple case but does not approve V6 or replace whole-project/resource acceptance.','No production/root source edits or builds; only frozen CLI replays and two bounded compiler-oracle/trace runs for the requested ownership controls.']}
save(out/'final-review.json',summary);print(json.dumps({'receipt':str(out/'final-review.json'),'sha256':sha(out/'final-review.json'),'checks':len(checks),'extraCanonical':sum(r['canonicalLines'] for r in rows),'extraRecords':sum(r['nonemptyRecords'] for r in rows)},indent=2))
