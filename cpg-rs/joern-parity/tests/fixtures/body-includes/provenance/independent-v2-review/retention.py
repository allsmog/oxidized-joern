from pathlib import Path
from collections import Counter
import hashlib,json
O=Path(__file__).parent
P=Path('/Users/shayaunnejad/vibe-code/.codex-worktrees/joern-oxidized-astra-body-includes/.local/body-includes')
R=Path('/Users/shayaunnejad/vibe-code/.codex-worktrees/joern-oxidized-astra-sprint')
L=Path('/Users/shayaunnejad/vibe-code/.codex-worktrees/joern-oxidized-astra-typedef-existence/.local/tenth-source-location-review')
F=R/'cpg-rs/joern-parity/tests/fixtures/body-macro-state'
checks=[]
def ck(n,b):
 assert b,n
 checks.append(n)
def sha(b):return hashlib.sha256(b).hexdigest()
def bind(p):
 b=p.read_bytes();return {'path':str(p),'sha256':sha(b),'bytes':len(b)}
def counter(b):return Counter(x for x in b.decode().split('\n') if x)
def verify_checks():
 d=json.loads((P/'frozen-v2/checks.json').read_text())
 ck('checks frozen binding',d['bindingsSha256']==sha((P/'frozen-v2/bindings.json').read_bytes()))
 for r in d['checks']:
  f=P/'frozen-v2'/(r['name']+'.log');ck('check log '+r['name'],sha(f.read_bytes())==r['logSha256']);ck('check successful '+r['name'],r['exitCode']==0 and r['sourceUnchanged'])
 return {'receipt':bind(P/'frozen-v2/checks.json'),'checks':d['checks']}
d=json.loads((P/'ninth-v2-retention/replay.json').read_text());package=json.loads((F/'measurement.json').read_text());pmap={x['case']:x for x in package['cases']}
ck('retention final bindings',d['bindingsSha256']==sha((P/'frozen-v2/bindings.json').read_bytes()))
ck('current binary',d['candidateBinarySha256']==sha((P/'frozen-v2/joern-parity').read_bytes()))
ck('accepted binary',d['baselineBinarySha256']==sha((R/'.local/astra-sprint/ninth-batch/reviewed-final-real-differential/bin/joern-parity').read_bytes()))
ck('retention 76 source identities',len(d['rows'])==76 and set(x['case'] for x in d['rows'])==set(pmap))
rows=[]
for r in d['rows']:
 c=r['case'];q=P/'ninth-v2-retention'/c;e=(q/'expected.txt').read_bytes();prior=pmap[c]
 ck(c+' live original expected binding',sha(e)==r['expectedSha256']==prior['expectedSha256'])
 ck(c+' exact source set',r['inputs']==prior['inputHashes'])
 for f,h in r['inputs'].items():ck(c+' input '+f,sha((q/'input'/f).read_bytes())==h)
 contents={};runs={}
 for version,run in r['runs'].items():
  v=q/version
  for f,key in [('actual.txt','actualSha256'),('stderr.txt','stderrSha256'),('complete.diff','diffSha256')]:ck(c+' '+version+' '+f,sha((v/f).read_bytes())==run[key])
  a=(v/'actual.txt').read_bytes();contents[version]=a
  ck(c+' '+version+' exact classification',run['exact']==(run['exitCode']==0 and a==e))
  runs[version]={'exitCode':run['exitCode'],'exact':run['exact'],'actual':bind(v/'actual.txt'),'stderr':bind(v/'stderr.txt'),'fullDiff':bind(v/'complete.diff')}
 successful=all(x['exitCode']==0 for x in r['runs'].values())
 ck(c+' success comparison only',successful==r['matchingCountersComparableSuccessfulRuns'])
 lost,gained=None,None
 if successful:
  ec=counter(e);b=counter(contents['accepted-ninth']);a=counter(contents['candidate-v2'])
  lost=(b&ec)-a;gained=(a&ec)-b
  ck(c+' exact matching multiplicity',dict(lost)==r['rawLostRecords'] and dict(gained)==r['rawGainedRecords'] and sum(lost.values())==r['matchingLost'] and sum(gained.values())==r['matchingGained'])
  ck(c+' no old exact loss',not runs['accepted-ninth']['exact'] or runs['candidate-v2']['exact'])
 else:
  ck(c+' both failures retained',all(x['exitCode']!=0 for x in runs.values()) and all(not x for x in contents.values()))
  ck(c+' same duplicate method assertion',all('duplicate method node properties drift' in (q/v/'stderr.txt').read_text() and 'a.c:N:int(0)' in (q/v/'stderr.txt').read_text() and 'b.c:N:int(0)' in (q/v/'stderr.txt').read_text() for v in runs))
 rows.append({'case':c,'runs':runs,'expected':bind(q/'expected.txt'),'matchingLost':dict(lost) if successful else None,'matchingGained':dict(gained) if successful else None,'comparable':successful})
ck('54→55',sum(x['runs']['accepted-ninth']['exact'] for x in rows)==54 and sum(x['runs']['candidate-v2']['exact'] for x in rows)==55)
ck('all76 zero loss plus25',sum(sum((x['matchingLost'] or {}).values()) for x in rows)==0 and sum(sum((x['matchingGained'] or {}).values()) for x in rows)==25)
prior_result={'cases':76,'baselineExact':54,'currentExact':55,'aborts':[x['case'] for x in rows if not x['comparable']],'matchingGained':25,'matchingLost':0,'rows':rows,'bindings':[bind(P/'ninth-v2-retention/replay.json'),bind(F/'measurement.json')]}
(O/'ninth-retention.json').write_text(json.dumps(prior_result,indent=2)+'\n')
origin_receipt=json.loads((L/'final-review.json').read_text());ck('origin reviewed receipt',sha((L/'final-review.json').read_bytes())=='36da9f7ee5a527db5929c6e0af4b6f9d05f8f639c1e9fa949e9709de73fcc838')
for item in origin_receipt['artifacts']:ck('origin evidence '+item['path'],sha(Path(item['path']).read_bytes())==item['sha256'])
od=json.loads((P/'replay-origins-v2/replay.json').read_text());ck('origin candidate binary',od['candidate']['binarySha256']==d['candidateBinarySha256'])
ck('origin raw receipt',od['oracleReceiptSha256']==sha((L/'raw/canonical-1/run.json').read_bytes()))
originrows=[]
for r in od['rows']:
 c=r['case'];p=P/'replay-origins-v2'/c
 ck(c+' origin original reference', (p/'expected.txt').read_bytes()==(L/'input'/c/'expected.txt').read_bytes())
 for f,k in [('expected.txt','expectedSha256'),('actual.txt','actualSha256'),('stderr.txt','stderrSha256'),('complete.diff','diffSha256')]:ck(c+' origin '+f,sha((p/f).read_bytes())==r[k])
 for f,h in r['inputs'].items():ck(c+' origin input '+f,sha((p/'input'/f).read_bytes())==h==sha((L/'input'/c/f).read_bytes()))
 ck(c+' origin whole exact',r['exitCode']==0 and (p/'actual.txt').read_bytes()==(p/'expected.txt').read_bytes())
 originrows.append({'case':c,'actual':bind(p/'actual.txt'),'expected':bind(p/'expected.txt'),'exact':True})
result={'checks':len(checks),'checkLabels':checks,'prior76':{k:v for k,v in prior_result.items() if k!='rows'},'origins3':originrows,'originReferenceReceipt':bind(L/'final-review.json'),'sourceLineValidation':verify_checks(),'noProducersOrBuilds':True}
result['checks']=len(checks)
(O/'retention-verification.json').write_text(json.dumps(result,indent=2)+'\n')
print({k:v for k,v in result['prior76'].items() if k not in ['rows','bindings']});print('checks',len(checks));print('origin3 full exact')
