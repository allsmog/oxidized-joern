from pathlib import Path
from collections import Counter, defaultdict
import difflib, hashlib, json, re, subprocess

HERE=Path(__file__).resolve().parent
WORK=Path('/Users/shayaunnejad/vibe-code/.codex-worktrees/joern-oxidized-astra-primitive-members')
ROOT=WORK.parent/'joern-oxidized-astra-sprint'
LOCAL=WORK/'.local/primitive-members'
REPLAY=LOCAL/'replay-prior-v1'
PACKAGE=LOCAL/'package-v1'; STAGED=PACKAGE/'staged'
FIXTURE=STAGED/'cpg-rs/joern-parity/tests/fixtures/primitive-members'
REV='f235a0f898c4e19fda90998b903f57741b776ec7'
checks=[]
def sha(p):return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def binding(p):return {'path':str(p),'sha256':sha(p),'bytes':Path(p).stat().st_size}
def check(name,value):
    checks.append({'name':name,'passed':bool(value)});assert value,name
def lf(b):
    r=b.split(b'\n')
    if r and r[-1]==b'':r.pop()
    return r
def keepends(b):
    r=b.split(b'\n');return [x+b'\n' for x in r[:-1]]+([r[-1]] if r[-1] else [])
def diff(a,b,an,bn):return b''.join(difflib.diff_bytes(difflib.unified_diff,keepends(a),keepends(b),fromfile=an,tofile=bn))

replay=json.loads((REPLAY/'replay.json').read_bytes())
planpath=WORK/'.local/primitive-member-implementation-plan-v1/preservation-inventory.json'
plan=json.loads(planpath.read_bytes()); frozen=json.loads((LOCAL/'frozen-v1/bindings.json').read_bytes())
check('retained replay plan and frozen source pair',sha(planpath)==replay['planSha256'] and sha(LOCAL/'frozen-v1/bindings.json')==replay['bindingSha256'])
check('retained replay helper bytes',sha(LOCAL/'scripts/replay-prior-v1.py')==replay['helperSha256'])
check('228 prior plus member_types',replay['caseInstances']==229 and replay['originalGroupInstances']==228 and replay['additionalMemberTypesDiagnostic']==1)
beforebin=ROOT/'.local/astra-sprint/tenth-batch/repaired-final-real-differential/bin/joern-parity'
currentbin=LOCAL/'frozen-v1/joern-parity'
check('both actual frozen replay binaries',sha(beforebin)==replay['beforeBinarySha256']=='ca569f9d75143d9c20e7c8054c8bbf7949640e6eb0213fba806e8224b0994141' and sha(currentbin)==replay['candidateBinarySha256']=='fe5dd8c3036ca44e1112dadc08a748f46165e782894c77c38165ca8c4565cf2c')
groups=plan['groups']+[{'family':'array-initializers-diagnostic','cases':[{'case':'member_types','root':'cpg-rs/joern-parity/tests/fixtures/array-initializers/diagnostics/member_types','inputs':{'arrays.c':'de4911c0e986952e4be1acf1426e2915769d24ba6dca19ff5450f46723e54e4c'},'referenceSha256':'1c115c101d2a696d63efd9fdfdb3fcd8251dd2f5c366c4ceab7db21e9c7132d5'}]}]
planned={(g['family'],c['case']):c for g in groups for c in g['cases']}
check('all229 planned source/reference instances present once',len(planned)==229 and [(r['family'],r['case']) for r in replay['cases']]==list(planned))
gitbindings={}
for c in planned.values():
    gitbindings.update({c['root']+'/'+n:h for n,h in c['inputs'].items()})
    gitbindings[c['root']+'/expected.txt']=c['referenceSha256']
gitbindings.update(plan['mainGate'])
gitbindings.update(plan['memberTypesDiagnostic'])
names=list(gitbindings)
batch=subprocess.check_output(['git','cat-file','--batch'],input=''.join(REV+':'+n+'\n' for n in names).encode(),cwd=ROOT)
pos=0
for n in names:
    end=batch.index(b'\n',pos);header=batch[pos:end].split();check('committed blob present '+n,len(header)==3 and header[1]==b'blob')
    size=int(header[2]);b=batch[end+1:end+1+size];pos=end+1+size+1
    check('committed source/reference bytes '+n,hashlib.sha256(b).hexdigest()==gitbindings[n]==sha(WORK/n))
check('Git batch consumed fully',pos==len(batch))

summaries=defaultdict(Counter); changed=[]; failures=[]; record_totals=Counter(); actual_lf_totals=Counter(); verified_replay_files=0
for row in replay['cases']:
    key=(row['family'],row['case']);c=planned[key];d=REPLAY/row['family']/row['case'];dest=FIXTURE/'provenance/prior-replay'/row['family']/row['case']
    check('per-case receipt identity '+str(key),json.loads((d/'run.json').read_bytes())==row)
    expected=(d/'expected.txt').read_bytes()
    check('complete accepted expected and input selection '+str(key),sha(d/'expected.txt')==c['referenceSha256'] and row['inputHashes']==c['inputs'])
    for name,h in row['files'].items():
        check('raw+portable file '+str(key)+'/'+name,sha(d/name)==h==sha(dest/name));verified_replay_files+=1
    check('portable complete receipt '+str(key),(d/'run.json').read_bytes()==(dest/'run.json').read_bytes())
    check('copied source filename set '+str(key),{p.relative_to(d/'input').as_posix() for p in (d/'input').rglob('*') if p.is_file()}==set(c['inputs']))
    for n,h in c['inputs'].items():check('copied source bytes '+str(key)+'/'+n,sha(d/'input'/n)==h)
    outputs={}
    for label,binary in [('accepted-tenth',beforebin),('candidate-v1',currentbin)]:
        run=row['runs'][label];raw=(d/(label+'.stdout')).read_bytes();outputs[label]=raw
        check('exact raw transport '+str(key)+' '+label,raw==(d/(label+'.txt')).read_bytes() and b'\r' not in raw)
        check('full LF-only reference diff '+str(key)+' '+label,(d/(label+'.diff')).read_bytes()==diff(expected,raw,b'live',label.encode()))
        check('actual producer argv and cwd '+str(key)+' '+label,run['command']==[str(binary),'--production',*sorted(c['inputs'])] and run['cwd']==str(d/'input'))
        check('no unreported timeout/process error '+str(key)+' '+label,not run['timeout'] and run['error'] is None)
        check('full exact status '+str(key)+' '+label,run['exact']==(run['exitCode']==0 and raw==expected))
    old=outputs['accepted-tenth'];current=outputs['candidate-v1'];success=all(x['exitCode']==0 for x in row['runs'].values())
    check('successful pair and output identity '+str(key),row['successfulPair']==success and row['sameOutput']==(old==current))
    check('complete LF before-current diff '+str(key),(d/'before-current.diff').read_bytes()==diff(old,current,b'accepted-tenth',b'candidate-v1'))
    if old!=current:changed.append('/'.join(key))
    if not success:failures.append(row)
    # Reproduce the stored scheme, including its terminal-empty sentinel; final totals below use actual LF records.
    for mode,nonempty in [('includingSeparators',False),('nonempty',True)]:
        e,b,a=[Counter(x for x in v.decode().split('\n') if not nonempty or x) for v in (expected,old,current)]
        gain=(e&a)-(e&b);loss=(e&b)-(e&a)
        observed={'expected':sum(e.values()),'before':sum(b.values()),'candidate':sum(a.values()),'matchingBefore':sum((e&b).values()),'matchingCandidate':sum((e&a).values()),'gained':sum(gain.values()),'lost':sum(loss.values()),'gainedRecords':dict(gain),'lostRecords':dict(loss),'successfulPair':success}
        check('complete multiplicity counters '+str(key)+' '+mode,observed==row['counts'][mode])
        if success and nonempty:record_totals.update({k:v for k,v in observed.items() if isinstance(v,int) and not isinstance(v,bool)})
    if success:
        e,b,a=map(lambda v:Counter(lf(v)),(expected,old,current))
        actual_lf_totals.update({'expected':sum(e.values()),'before':sum(b.values()),'candidate':sum(a.values()),'matchingBefore':sum((e&b).values()),'matchingCandidate':sum((e&a).values()),'gained':sum(((e&a)-(e&b)).values()),'lost':sum(((e&b)-(e&a)).values())})
    s=summaries[row['family']];s.update({'cases':1,'beforeExact':int(row['runs']['accepted-tenth']['exact']),'candidateExact':int(row['runs']['candidate-v1']['exact']),'successfulPairs':int(success),'sameOutputs':int(old==current),'matchingGains':row['counts']['nonempty']['gained'] if success else 0,'matchingLosses':row['counts']['nonempty']['lost'] if success else 0})
observed_summary=[{'family':g['family'],**dict(summaries[g['family']])} for g in groups]
check('all family summaries independently reproduced',observed_summary==replay['summary'])
check('only duplicate anchors change',changed==['body-includes/tiny_fixedtables_include','body-includes/tiny_fixedtables_inline'])
check('one actual retained abort',len(failures)==1 and failures[0]['case']=='duplicate_clinit_tag')
failure=failures[0];fp=REPLAY/'body-macro-state/duplicate_clinit_tag'
for label in ['accepted-tenth','candidate-v1']:
    check('abort -6 and no graph '+label,failure['runs'][label]['exitCode']==-6 and (fp/(label+'.stdout')).read_bytes()==b'')
    check('complete assertion transcript '+label,b'assertion `left == right` failed: duplicate method node properties drift' in (fp/(label+'.stderr')).read_bytes())
check('successful retention +14/0',record_totals['gained']==14 and record_totals['lost']==0)

validation=LOCAL/'validation-v1-final-test';v=json.loads((validation/'checks.json').read_bytes())
check('requested final validation receipt',sha(validation/'checks.json')=='1b583646d0dee5c0040033c7ec32b9d7afa23471d2282803134ef15bcd6f5d72')
check('validation status and complete4 checks',v['status']=='PASS_FOCUSED_MAIN_FMT_CLIPPY' and [c['check'] for c in v['checks']]==['fmt','focused','main308','clippy'])
check('validation helper bound',sha(LOCAL/'scripts/validate-v1-final-test.py')==v['helperSha256'])
for n,h in v['sourceInputs'].items():check('final validated source byte '+n,sha(WORK/n)==h)
for n,h in v['referenceInputs'].items():check('validated complete reference byte '+n,sha(WORK/n)==h)
for n,h in v['mainGateInputs'].items():check('main oracle/checker unchanged '+n,sha(WORK/n)==h==plan['mainGate'][n])
check('test-only change since frozen compilation',v['testOnlyChangeSinceFrozenV1']=={n:{'frozen':frozen['sourceInputs'][n],'current':h} for n,h in v['sourceInputs'].items() if frozen['sourceInputs'][n]!=h} and list(v['testOnlyChangeSinceFrozenV1'])==['cpg-rs/joern-parity/tests/primitive_members.rs'])
for c in v['checks']:
    check('completed immutable validation '+c['check'],c['exitCode']==0 and c['sourceInputsUnchanged'] and sha(validation/(c['check']+'.log'))==c['logSha256'])
    check('portable validation log '+c['check'],sha(FIXTURE/'provenance/validation'/(c['check']+'.log'))==c['logSha256'])
focused=(validation/'focused.log').read_text();results=re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; \d+ measured; (\d+) filtered out',focused)
check('21 focused tests in7 groups',len(results)==7 and sum(int(x[0]) for x in results)==21 and all(x[1:]==('0','0','0') for x in results))
check('committed main308 full match',(validation/'main308.log').read_text().rstrip().endswith('308/308 comparison blocks byte-identical to Joern'))
check('fmt empty and strict Clippy complete',(validation/'fmt.log').read_bytes()==b'' and 'Finished ' in (validation/'clippy.log').read_text() and v['checks'][3]['command'][-3:]==['--','-D','warnings'] and '--all-targets' in v['checks'][3]['command'])
test=(STAGED/'cpg-rs/joern-parity/tests/primitive_members.rs').read_bytes()
check('final test snapshot and only2 new tests',test==(validation/'primitive_members.rs').read_bytes() and test.count(b'#[test]')==2)
for name,row in v['testBinaries'].items():check('bound compiled focused test '+name,sha(row['path'])==row['sha256'])

freeze=json.loads((PACKAGE/'freeze.json').read_bytes());measurement=json.loads((FIXTURE/'measurement.json').read_bytes())
check('requested sealed package',sha(PACKAGE/'freeze.json')=='d5d9b40efac4f1a36b22eeccbedb180a6a57a5bd41e23a7bae593fc16192995a')
check('all3252 staged file names',set(freeze['files'])=={p.relative_to(STAGED).as_posix() for p in STAGED.rglob('*') if p.is_file()} and len(freeze['files'])==3252)
for n,row in freeze['files'].items():check('sealed package byte '+n,sha(STAGED/n)==row['sha256'] and (STAGED/n).stat().st_size==row['bytes'])
check('portable metadata preserves reviewed prior results',measurement['priorSummary']==observed_summary and measurement['priorReceiptSha256']==sha(REPLAY/'replay.json')==sha(FIXTURE/'provenance/prior-replay/replay.json'))
check('portable validation claim exact',measurement['validation']['sha256']==sha(validation/'checks.json') and measurement['validation']['focusedTests']==21 and measurement['validation']['newRustTests']==2 and measurement['validation']['newTestGroup']==1 and measurement['validation']['mainComparisonBlocks']==308 and measurement['validation']['rootFinalGatesRunByWorker'] is False)
readme=(FIXTURE/'README.md').read_text()
check('README retains actual failure and no duplicate-gain claim','`-6` on both release producers' in readme and 'must not be added again' in readme and 'Root integration, whole-project comparisons and final resource acceptance are\npending.' in readme)
result={'status':'PASS_BOUNDED_PRIOR_RETENTION_VALIDATION_AND_AGGREGATE_REVIEW','checkCount':len(checks),'checks':checks,'sourceCommitBaseline':REV,'replay':binding(REPLAY/'replay.json'),'validation':binding(validation/'checks.json'),'packageFreeze':binding(PACKAGE/'freeze.json'),'reviewScript':binding(__file__),'caseInstances':229,'plannedPriorInstances':228,'additionalDiagnostic':1,'successfulPairs':228,'beforeExact':sum(s['beforeExact'] for s in summaries.values()),'candidateExact':sum(s['candidateExact'] for s in summaries.values()),'rawOutputFilesVerified':458,'allRawStdoutIdenticalToStoredText':True,'rawTransportNote':'Replay helper contains CR/CRLF normalization, but every actual raw stdout equals stored .txt byte for byte; independent complete diffs were recomputed using LF-only splitting. No input/reference/actual graph was altered in this evidence.','nonemptySuccessfulPairTotals':dict(record_totals),'lfSuccessfulPairTotals':dict(actual_lf_totals),'storedSeparatorCounterNote':'Worker includingSeparators counters also count the sole terminal split sentinel. They were independently reproduced; lfSuccessfulPairTotals remove that sentinel and represent physical LF records. Failure empty stdout is not a graph and is excluded from successful-pair totals.','families':observed_summary,'changedOutputs':changed,'retainedAbort':{'case':'body-macro-state/duplicate_clinit_tag','exits':[-6,-6],'stdoutBytes':[0,0],'sameAssertion':True,'stderrByteIdentical':False,'includedInSuccessfulCounters':False,'beforeStderr':binding(fp/'accepted-tenth.stderr'),'candidateStderr':binding(fp/'candidate-v1.stderr')},'priorGainsOverlapNew19':14,'rootInventoryExpectation':{'acceptedWorkspaceTests':468,'addedRustTests':2,'expectedWorkspaceTests':470,'acceptedWorkspaceGroups':76,'addedGroup':1,'expectedWorkspaceGroups':77,'status':'Expected from reviewed inventory; root final execution still required.'},'validationScope':'Worker21 focused tests, committed308, fmt and strict Clippy for cpg-lang-c/joern-parity all targets. No worker full-workspace, fresh-live, whole-project or resource acceptance claim.','completeGitBlobsVerified':len(gitbindings),'verifiedPriorArtifactFiles':verified_replay_files,'packageFilesVerified':3252,'blockers':[],'producerRuns':0,'rootWrites':False}
with (HERE/'final-review.json').open('x') as out:json.dump(result,out,indent=2);out.write('\n')
print(json.dumps({'review':binding(HERE/'final-review.json'),'checkCount':len(checks),'beforeExact':result['beforeExact'],'candidateExact':result['candidateExact'],'nonempty':result['nonemptySuccessfulPairTotals']}))
