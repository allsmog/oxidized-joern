from pathlib import Path
from collections import Counter
import json,hashlib,re,datetime,difflib
p=Path(__file__).resolve().parent
sha=lambda f:hashlib.sha256(Path(f).read_bytes()).hexdigest()
def bind(f):return {'path':str(f),'sha256':sha(f),'bytes':Path(f).stat().st_size}
def save(f,j):
 with f.open('x') as o:json.dump(j,o,indent=2);o.write('\n')
checks=[]
def check(name,ok):
 checks.append({'name':name,'pass':bool(ok)});assert ok,name
rows=[];groups=[]
for group in [p,p/'type-position']:
 setup=json.loads((group/'setup.json').read_bytes())
 for k in ['source','binary','ownerReceipt','buildLog','baseline','script','traceSource','traceRun','upstream']:
  a=setup[k];check(group.name+': '+k+' unchanged',sha(a['path'])==a['sha256'])
 for case,a in setup['inputs'].items():check(group.name+': input '+case,sha(a['path'])==a['sha256'])
 sections={};name=None
 for line in (group/'oracle-workspace/stdout').read_bytes().split(b'\n'):
  if line.startswith(b'CASE|'):name=line[5:].decode();sections[name]=[]
  elif line.startswith((b'AST|',b'NODES|',b'EDGES|',b'FLOWS|')):sections[name].append(line[4:] if line.startswith(b'AST|') else line)
 check(group.name+': exact raw case inventory',set(sections)==set(setup['inputs']))
 for case,lines in sections.items():
  expected=group/'input'/case/'expected.txt';check(group.name+': full extraction '+case,expected.read_bytes()==b'\n'.join(lines)+b'\n')
  variants={}
  for variant in ['accepted-eighth','held-v4']:
   d=group/variant/case;run=json.loads((d/'run.json').read_bytes());actual=d/'stdout'
   check(group.name+': successful '+variant+' '+case,run['exitCode']==0)
   for key in ['stdout','stderr']:check(group.name+': raw hash '+variant+' '+case+' '+key,sha(run[key]['path'])==run[key]['sha256'])
   check(group.name+': argv '+variant+' '+case,run['command']==[setup['baseline' if variant=='accepted-eighth' else 'binary']['path'],str(group/'input'/case/'main.c')])
   variants[variant]={'completeExact':actual.read_bytes()==expected.read_bytes(),'output':bind(actual),'completeDiff':bind(d/'complete-lf.diff'),'run':bind(d/'run.json')}
  rows.append({'group':'primary' if group==p else 'type-position','case':case,'input':bind(group/'input'/case/'main.c'),'reference':bind(expected),'canonicalLines':len(lines),'nonemptyRecords':sum(bool(x) for x in lines),'variants':variants})
 for what in ['oracle-workspace','trace']:
  run=json.loads((group/what/'run.json').read_bytes());check(group.name+': '+what+' exit',run['exitCode']==0)
  for k in ['stdout','stderr']:check(group.name+': '+what+' '+k,sha(run[k]['path'])==run[k]['sha256'])
 groups.append({'name':'primary' if group==p else 'type-position','setup':bind(group/'setup.json'),'oracle':bind(group/'oracle-workspace/run.json'),'trace':bind(group/'trace/run.json'),'rawOracle':bind(group/'oracle-workspace/stdout'),'rawTrace':bind(group/'trace/stdout')})

def methods(path):
 out={};key=None
 for line in path.read_bytes().decode().split('\n'):
  if line.startswith('METHOD '):
   key=re.search(r' FULL_NAME=(.*?)(?: SIGNATURE=| ORDER=)',line).group(1);out[key]=[]
  elif not line or line.startswith(('NODES|','EDGES|','FLOWS|')):key=None
  if key is not None:out[key].append(line)
 return out
case='sizeof_array';g=p/'type-position';key='main.c:ARG:int(0)'
o=methods(g/'input'/case/'expected.txt')[key];b=methods(g/'accepted-eighth'/case/'stdout')[key];c=methods(g/'held-v4'/case/'stdout')[key]
check('regressed macro method baseline exact',o==b)
check('regressed macro method candidate differs',o!=c)
check('regression confined to METHOD CODE in that complete method', '\n'.join(c).replace('CODE=#define ARG 2 ','CODE=#define ARG 1 ')== '\n'.join(o))
proof=p/'regression';proof.mkdir()
for name,lines in [('oracle',o),('accepted-eighth',b),('held-v4',c)]:
 with (proof/(name+'.ast')).open('x') as f:f.write('\n'.join(lines)+'\n')
with (proof/'method.diff').open('x') as f:f.writelines(difflib.unified_diff([x+'\n' for x in o],[x+'\n' for x in c],fromfile='Joern=accepted-eighth',tofile='held-v4'))
trace=(g/'trace/stdout').read_bytes().decode().split('\n');section=trace[trace.index('CASE|sizeof_array'):trace.index('CASE|sizeof_array_plain')]
pairs=[l for l in section if l.startswith('PAIR|')]
check('pinned CDT distinct callee and argument events',[(int(l.split('|')[1]),l.rsplit('|',1)[1]) for l in pairs]==[(88,'#define CALL consume'),(104,'#define ARG 1'),(144,'#define ARG 2')])
trace2=(p/'trace/stdout').read_bytes().decode().split('\n');start=trace2.index('CASE|object_function_alias');end=trace2.index('CASE|object_nested_argument');aliaspairs=[l for l in trace2[start:end] if l.startswith('PAIR|')]
check('object alias to function macro consumes invocation',len(aliaspairs)==2 and aliaspairs[0].endswith('#define CALL WRAP') and aliaspairs[1].endswith('#define ARG 2'))
plain=next(r for r in rows if r['case']=='sizeof_array_plain');check('ordinary callee control complete exact',plain['variants']['held-v4']['completeExact'])
peer=Path('/Users/shayaunnejad/vibe-code/.codex-worktrees/joern-oxidized-astra-typedef-aggregates/.local/ninth-macro-definition-review/cdt-trace')
compile=json.loads((peer/'compile.json').read_bytes());check('observer successful compile provenance',compile['compilerExit']==0 and sha(peer/'CdtTrace.scala')==compile['sourceSha256'])
for f,h in compile['classpathJars'].items():check('pinned observer classpath '+f,sha(f)==h)
source=Path(json.loads((p/'setup.json').read_bytes())['source']['path']);(proof/'held-v4-exact.rs').write_bytes(source.read_bytes())
receipt={'status':'CONFIRMED_INTRODUCED_MACRO_METHOD_CODE_REGRESSION_HELD_V4','createdAtUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'reviewScope':'Object-like callee macro versus function-like invocation argument expansion ownership only; no production edits or builds.','source':bind(source),'binary':json.loads((p/'setup.json').read_bytes())['binary'],'checks':checks,'checkCount':len(checks),'groups':groups,'projects':rows,'totals':{'uniqueProjects':len(rows),'canonicalLines':sum(r['canonicalLines'] for r in rows),'nonemptyRecords':sum(r['nonemptyRecords'] for r in rows),'acceptedEighthCompleteExact':sum(r['variants']['accepted-eighth']['completeExact'] for r in rows),'heldV4CompleteExact':sum(r['variants']['held-v4']['completeExact'] for r in rows)},'finding':{'location':{'path':str(source),'line':976},'consumerLocation':{'path':str(source),'line':1428},'minimalSource':bind(g/'input/sizeof_array/main.c'),'triggerLine':4,'laterUseLine':7,'message':'The unconditional return after any macro-named callee drops preprocessing expansion events in arguments of object-like aliases to ordinary functions. An ARG occurrence inside sizeof(int[ARG]) has no corresponding emitted macro wrapper, so its definition event must remain available to the later return. V4 selects new ARG2 metadata instead of the old ARG1 selected by pinned Joern.','completeMethod':key,'baselineWasExactMethod':True,'projectWasExact':False,'oracleHeader':o[0],'acceptedHeader':b[0],'candidateHeader':c[0],'methodEvidence':[bind(proof/(n+'.ast')) for n in ['oracle','accepted-eighth','held-v4']],'methodDiff':bind(proof/'method.diff'),'cdtPairs':pairs,'recommendation':'Distinguish a callee object expansion that resolves to an ordinary callee from one whose rescan consumes the argument list as a function-like macro. Preserve argument events for the former and suppress them for the latter; retain ordinary callee and function-wrapper controls. Validate against these complete references without accepting the unrelated unresolved-callee AST gaps.'},'upstreamContract':{'path':str(peer.parent/'upstream/MacroHandler.scala'),'sha256':sha(peer.parent/'upstream/MacroHandler.scala'),'lines':[22,67,72,78],'description':'Pinned MacroHandler builds an offset-sorted destructive stream from CDT TU macro expansion locations and pops through matching names. This queue makes omitted earlier events observably affect later METHOD definition metadata.'},'observer':{'compile':bind(peer/'compile.json'),'source':bind(peer/'CdtTrace.scala'),'classes':[bind(f) for f in sorted((peer/'classes').rglob('*')) if f.is_file()],'extraControlCases':'Existing observer also prints three fixed previously supplied controls; those raw lines are retained and not included in this ten-project graph inventory.'},'limitations':['Held V4 is not accepted; this review finds a concrete blocking metadata regression.','All ten full references and all divergences are retained. Seven current projects remain nonexact; object callee call-name/expansion differences already existed before.','The full sizeof_array project was not exact on accepted eighth; only its complete ARG macro METHOD was exact and is now regressed.','No root writes, source implementation, Cargo build, whole-project rerun, scanner, resource gate, or manifest change.','Original complete.diff files have condensed diff formatting; supplementary complete-lf.diff files render every record with LF. Both are retained; expected/output bytes were never edited.']}
save(p/'final-review.json',receipt)
print(json.dumps({'receipt':str(p/'final-review.json'),'sha256':sha(p/'final-review.json'),'checks':len(checks),'totals':receipt['totals']},indent=2))
