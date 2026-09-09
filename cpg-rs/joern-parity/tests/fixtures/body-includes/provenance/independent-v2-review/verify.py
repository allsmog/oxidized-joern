from pathlib import Path
from collections import Counter
import hashlib,json,difflib,re
P=Path('/Users/shayaunnejad/vibe-code/.codex-worktrees/joern-oxidized-astra-body-includes/.local/body-includes')
R=Path('/Users/shayaunnejad/vibe-code/.codex-worktrees/joern-oxidized-astra-sprint')
B=R/'.local/astra-sprint/tenth-batch/frozen-baseline-v1'
L=Path('/Users/shayaunnejad/vibe-code/.codex-worktrees/joern-oxidized-astra-body-macro-state/.local/body-include-diagnosis-v4/runs/first-complete-reference-batch')
O=Path(__file__).parent
checks=[]
def ck(n,b):
 assert b,n
 checks.append(n)
def sha(b):return hashlib.sha256(b).hexdigest()
def bind(p):
 b=p.read_bytes();return {'path':str(p),'sha256':sha(b),'bytes':len(b)}
freeze=json.loads((P/'frozen-v2/bindings.json').read_text())
replay=json.loads((P/'replay-v2/replay.json').read_text())
ck('immutable source1b23',sha((P/'frozen-v2/source/cpg-rs/cpg-lang-c/src/exact.rs').read_bytes())=='1b23a6834a9c68ae7c1639c3d5f92116bf2eaeb496af2b9ee32acc6e0605b750')
ck('immutable binary589a',sha((P/'frozen-v2/joern-parity').read_bytes())==freeze['binarySha256']=='589ad4d85fb44e8b0b54c71731bb412bcf9aee271a84a1721d41dc8489c81fb4')
import subprocess
for file,s in freeze['source'].items():
 f=P/'frozen-v2/source'/file
 content=f.read_bytes() if f.exists() else subprocess.check_output(['git','show',freeze['baseline']+':'+file],cwd=P.parents[1])
 ck('frozen or accepted unchanged source '+file,sha(content)==s)
ck('build log',sha((P/'frozen-v2/build.log').read_bytes())==freeze['buildLogSha256'])
ck('build success',freeze['buildExitCode']==0)
ck('replay exact frozen binding',replay['candidate']==freeze)
ck('live receipt',sha((L/'run.json').read_bytes())==replay['oracleReceiptSha256']=='2063ce773a6072d9993e0159d906ec4b94ae8c319e327d01ae5217bc841e7f53')
base_source=(P/'baseline/exact.rs').read_bytes()
ck('accepted source',sha(base_source)=='ad686e57683383c05995cf49ecb76fecb04ccb09c3c207fb178127af9d81d15b')
new_source=(P/'frozen-v2/source/cpg-rs/cpg-lang-c/src/exact.rs').read_bytes()
patch=''.join(difflib.unified_diff(base_source.decode().splitlines(True),new_source.decode().splitlines(True),fromfile='accepted-ad686e',tofile='frozen-v2-96207'))
(O/'source.diff').write_text(patch)
def blocks(data):
 result={};key=None;lines=[]
 for line in data.split('\n'):
  if line.startswith('METHOD '):
   assert key is None
   key=re.split(r' (?:SIGNATURE|ORDER)=',line.rsplit(' FULL_NAME=',1)[1])[0];lines=[line]
  elif key is not None and line:lines.append(line)
  elif key is not None:
   assert key not in result
   result[key]=lines;key=None
 return result
def node_context(block,idx):
 parents=[];path=[]
 for i,line in enumerate(block[:idx+1]):
  depth=(len(line)-len(line.lstrip()))//2
  while parents and parents[-1][0]>=depth:parents.pop()
  path=[{'ordinal':j,'record':l} for _,j,l in parents]+[{'ordinal':i,'record':line}]
  parents.append((depth,i,line))
 return path
rows=[];data={}
for row in replay['rows']:
 c=row['case'];cp=P/'replay-v2'/c
 expected=(L/'expected'/c/'expected.txt').read_bytes();before=(B/c/'actual.txt').read_bytes();after=(cp/'actual.txt').read_bytes()
 ck(c+' exit0',row['exitCode']==0)
 ck(c+' expected unchanged',expected==(cp/'expected.txt').read_bytes() and sha(expected)==row['expectedSha256'])
 ck(c+' actual bound',sha(after)==row['actualSha256'])
 ck(c+' stderr bound',sha((cp/'stderr.txt').read_bytes())==row['stderrSha256'])
 ck(c+' full diff bound',sha((cp/'complete.diff').read_bytes())==row['diffSha256'])
 ck(c+' full exact claimed',row['exact']==(expected==after))
 for path,s in row['inputs'].items():
  ck(c+'/'+path+' inputs',sha((cp/'input'/path).read_bytes())==s==sha((L/'snapshot/input'/c/path).read_bytes())==sha((B/'input'/c/path).read_bytes()))
 e,b,a=[Counter(x.decode().split('\n')) for x in (expected,before,after)]
 losses=(b&e)-a;gains=(a&e)-b
 data[c]={'live':expected.decode(),'baseline':before.decode(),'candidate':after.decode()}
 lossfile=O/(c+'.record-delta.json');lossfile.write_text(json.dumps({'matchingLost':list(losses.elements()),'matchingGained':list(gains.elements()),'allAdded':list((a-b).elements()),'allRemoved':list((b-a).elements())},indent=2)+'\n')
 rows.append({'case':c,'baselineExact':expected==before,'candidateExact':expected==after,'matchingGained':sum(gains.values()),'matchingLost':sum(losses.values()),'allAdded':sum((a-b).values()),'allRemoved':sum((b-a).values()),'live':bind(L/'expected'/c/'expected.txt'),'baseline':bind(B/c/'actual.txt'),'candidate':bind(cp/'actual.txt'),'fullCandidateDiff':bind(cp/'complete.diff'),'independentRecordDelta':bind(lossfile)})

ck('12 cases and8exact',len(rows)==12 and sum(r['candidateExact'] for r in rows)==8)
ck('two baseline exact retained',sum(r['baselineExact'] for r in rows)==2 and all(not r['baselineExact'] or r['candidateExact'] for r in rows))
c='repeated_include_context';method='nested/table.h:N:int(0)'
bs={side:blocks(text) for side,text in data[c].items()}
v1=(P/'replay-v1'/c/'actual.txt').read_text()
ck('same complete macro method',bs['live'][method]==bs['baseline'][method]==bs['candidate'][method])
old=['EDGES|CONTAINS D:nested/table.h:<global> -> nested/table.h:N:int(0)#0','EDGES|SOURCE_FILE nested/table.h:N:int(0)#0 -> F:nested/table.h']
for l in old:ck('restored owner '+l,all(l in data[c][s].split('\n') for s in ['live','baseline','candidate']) and l not in v1.split('\n'))
ck('first complete method now exact',bs['live']['first']==bs['candidate']['first'])
collision={str(idx):{side:node_context(b['second'],idx) for side,b in bs.items()} for idx in [12,13]}
closure={'case':c,'restoredOwnershipEdges':old,'completeMacroMethod':bs['live'][method],'firstCompleteMethodExact':True,'rawOrdinalCollisionContexts':collision}
(O/'ownership-closure.json').write_text(json.dumps(closure,indent=2)+'\n')
for n in ['exact.rs','import.rs','lib.rs']:
 a=(P/'baseline'/n).read_text();b=(P/'frozen-v2/source/cpg-rs/cpg-lang-c/src'/n).read_text()
 (O/(n+'.diff')).write_text(''.join(difflib.unified_diff(a.splitlines(True),b.splitlines(True),fromfile='accepted/'+n,tofile='frozen-v2/'+n)))
res={'checks':len(checks),'checkLabels':checks,'sourceSha256':freeze['source']['cpg-rs/cpg-lang-c/src/exact.rs'],'binarySha256':freeze['binarySha256'],'projects':12,'baselineExact':2,'candidateExact':8,'formerlyExactLost':0,'rawMatchingGained':sum(r['matchingGained'] for r in rows),'rawMatchingLost':sum(r['matchingLost'] for r in rows),'rows':rows,'bindings':[bind(P/'frozen-v2/bindings.json'),bind(P/'replay-v2/replay.json'),bind(P/'frozen-v2/build.log'),bind(O/'ownership-closure.json')],'noProducersOrBuilds':True}
(O/'verification.json').write_text(json.dumps(res,indent=2)+'\n')
print(json.dumps({k:res[k] for k in ['checks','baselineExact','candidateExact','rawMatchingGained','rawMatchingLost']},indent=2))
for r in rows:
 print(r['case'],r['candidateExact'],'matching+',r['matchingGained'],'-',r['matchingLost'])
