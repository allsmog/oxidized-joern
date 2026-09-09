from pathlib import Path
import ast,copy,json,hashlib,datetime,itertools

P=Path('/Users/shayaunnejad/vibe-code/.codex-worktrees/joern-oxidized-astra-typedef-existence/.local/primitive-member-runner-v3')
BEFORE=P.parent/'tenth-array-dimension-controls'
O=Path(__file__).parent
checked=[]
def c(v,s):
 if not v:raise AssertionError(s)
 checked.append(s)
def bind(p):
 p=Path(p);b=p.read_bytes();return {'path':str(p),'sha256':hashlib.sha256(b).hexdigest(),'bytes':len(b)}
def load(p):return json.loads(Path(p).read_text())
source=(P/'run-oracle.py').read_text();old=(BEFORE/'run-oracle.py').read_text()
c(bind(P/'run-oracle.py')['sha256']=='9e798056560fd244d9ccbc1b9a041152acabc032fa988c400e09f3a6df9bf2ac','V3 runner source frozen')
c(bind(P/'prepared.json')['sha256']=='23302c99c8e4a543b308c04f5f60e72be034b88530fd75638f9e0ff441169894','V3 prepared manifest frozen')
g={'__file__':str(P/'run-oracle.py'),'__name__':'independent_static_review_not_main'}
exec(compile(source,str(P/'run-oracle.py'),'exec'),g)
def no_producer(*a,**k):raise AssertionError('Producer forbidden in read-only review')
g['run_process']=no_producer
newf={n.name:ast.dump(n,include_attributes=False) for n in ast.parse(source).body if isinstance(n,ast.FunctionDef)}
oldf={n.name:ast.dump(n,include_attributes=False) for n in ast.parse(old).body if isinstance(n,ast.FunctionDef)}
changed={n for n in oldf if newf.get(n)!=oldf[n]};added=set(newf)-set(oldf)
c(changed=={'verify_git_checkpoint','verify_release','attempt'},'only3 existing functions adapted')
c(added=={'verify_checkpoint_admission','check_retained_anchors'},'only2 bounded new helpers')
unchanged=sorted(set(oldf)-changed)
for n in unchanged:c(oldf[n]==newf[n],'unchanged function '+n)
manifest,snapshot=g['verify_prepared'](P)
c(manifest['totalProjects']==19 and len(manifest['inputHashes'])==20,'19cases20sources')
original=load('/Users/shayaunnejad/vibe-code/.codex-worktrees/joern-oxidized-astra-body-includes/.local/primitive-member-preparation-v2/prepared.json')
for row in original['cases']+original['anchors']:
 for name,digest in row.get('inputs',row.get('inputHashes',{})).items():c(bind(P/'input'/row['case']/name)['sha256']==digest,'unchanged case '+row['case']+'/'+name)
c(manifest['anchors']==original['anchors'],'full two-anchor metadata unchanged')
c(bind(P/'oracle.sc')['sha256']==bind(BEFORE/'oracle.sc')['sha256']=='56431f14868678e64f9eb41026b44f1d3d7738404d28d2db62711b520c98c742','oracle unchanged')
c(bind(P/'runtime-bindings.json')['sha256']==bind(BEFORE/'runtime-bindings.json')['sha256']=='b15a360470551d7153f9b5db986950ed48168b27deb80455353814b69bd3814d','runtime binding bytes unchanged')
c(manifest['runtime']==load(BEFORE/'prepared.json')['runtime'],'full runtime spec unchanged')
for row in manifest['frozenOriginals']:c(bind(row['path'])['sha256']==row['sha256'],'frozen original '+row['path'])
for row in manifest['frozenOriginalInventories']:c(g['inventory'](row['path'])==row['inventory'],'frozen original inventory '+row['path'])
runtime=g['verify_runtime'](manifest['runtime'])
c({k:len(v['files']) for k,v in runtime['trees'].items()}=={'joern':266,'jdk':527},'actual Joern266 JDK527 runtime files')
c(len(runtime['commands'])==13,'actual13 launcher commands')
template=load(P/'checkpoint-release.template.json');admission=manifest['checkpointAdmission']
g['verify_checkpoint_admission'](template,admission)
checkpoint=g['verify_git_checkpoint'](template)
c(len(checkpoint['frozenInputHashes'])==138 and checkpoint['sourceCommit']==checkpoint['documentationCommit']=='f235a0f898c4e19fda90998b903f57741b776ec7','actual atomic tenth Git/docs138 blobs')
c(template['producerHoldReleased'] is False and template['status']=='TEMPLATE_NOT_APPROVED' and template['preparedManifestSha256'] is None,'template does not release producer')
def rejects(call,label):
 try:call()
 except (RuntimeError,KeyError,TypeError,ValueError):c(True,label);return
 raise AssertionError('Failed to reject '+label)
rejects(lambda:g['verify_release'](P/'checkpoint-release.template.json',bind(P/'checkpoint-release.template.json')['sha256'],bind(P/'prepared.json')['sha256'],admission),'actual false template rejected before producer')
for key,value in [('sourceCommit',None),('sourceCommit','0'*40),('documentationCommit','f'*40),('repository','/tmp/not-this-checkpoint')]:
 q=copy.deepcopy(template);q[key]=value;rejects(lambda q=q:g['verify_checkpoint_admission'](q,admission),'reject incorrect '+key+repr(value))
q=copy.deepcopy(template);q['documentationBlobs']=q['documentationBlobs'][:1];rejects(lambda:g['verify_checkpoint_admission'](q,admission),'require both committed report and metrics')
q=copy.deepcopy(template);q['frozenBuild']['sha256']='0'*64;rejects(lambda:g['verify_checkpoint_admission'](q,admission),'require exact released build hash')
q=copy.deepcopy(template);q['acceptance']['repositoryPath']='cpg-rs/PROGRESS.md';rejects(lambda:g['verify_checkpoint_admission'](q,admission),'require actual tenth acceptance path')
q=copy.deepcopy(template);q['acceptance']['sha256']='0'*64;rejects(lambda:g['verify_git_checkpoint'](q),'actual committed acceptance bytes verified')
selected={x['case']:(P/x['referencePath']).read_bytes() for x in manifest['anchors']}
c(all(g['check_retained_anchors'](P,selected,manifest['anchors']).values()),'both full anchors match')
for row in manifest['anchors']:
 q=dict(selected);q.pop(row['case']);c(not all(g['check_retained_anchors'](P,q,manifest['anchors']).values()),'missing full anchor blocks '+row['case'])
 q=dict(selected);q[row['case']]+=b'AST|extra\n';c(not all(g['check_retained_anchors'](P,q,manifest['anchors']).values()),'extra anchor record blocks '+row['case'])
rejects(lambda:g['check_retained_anchors'](P,selected,[]),'empty anchor list rejected')
rejects(lambda:g['check_retained_anchors'](P,selected,[manifest['anchors'][0]]*2),'duplicate anchor names rejected')
raw=b'log\nCASE|sample\nAST|METHOD CODE=left\x0bmiddle\r right  \nAST|\nNODES|TYPE NAME=int\nEDGES|CFG x#0 -> x#1\nEDGES|CFG x#0 -> x#1\nFLOWS|REACHING_DEF[a\\nb] x#0 -> x#1\n'
projection,kinds,complete=g['extract_selected'](raw,['sample'])
expected=b'METHOD CODE=left\x0bmiddle\r right  \n\nNODES|TYPE NAME=int\nEDGES|CFG x#0 -> x#1\nEDGES|CFG x#0 -> x#1\nFLOWS|REACHING_DEF[a\\nb] x#0 -> x#1\n'
c(complete and projection['sample']==expected,'LF-only raw preservation including CR VT spaces duplicate edge escaped label')
rejects(lambda:g['extract_selected'](raw+b'CASE|sample\n',['sample']),'duplicate CASE rejected')
rejects(lambda:g['extract_selected'](b'AST|METHOD\nCASE|sample\n',['sample']),'selected record before CASE rejected')
c(not g['extract_selected'](raw,['sample','missing'])[2],'missing case incomplete')
for exit_ok,timed,error,comp,anchors,drift,git_drift in itertools.product([False,True],repeat=7):
 process={'exitCode':0 if exit_ok else 1,'timedOut':timed,'processError':'error' if error else None}
 yes,reasons=g['admissibility'](process,comp,anchors,[],{'bound':1},{'bound':2 if drift else 1},{'git':1},{'git':2 if git_drift else 1})
 c(yes==(exit_ok and not timed and not error and comp and anchors and not drift and not git_drift),'admission truth table '+str((exit_ok,timed,error,comp,anchors,drift,git_drift)))
c(g['prepared_snapshot'](P)==snapshot,'prepared inventory unchanged after offline checks')
c(g['runtime_snapshot'](manifest['runtime'])==runtime,'actual runtime unchanged after offline checks')
handoff=P.parent/'primitive-member-runner-v3-handoff.json';author=P/'offline-checks/preflight/review.json'
c(bind(author)['sha256']=='4b9e9dcdb522beeab4993430cc3078ec87935126cd4071590a5a67a73b194821','author offline preflight82 fixed')
c(bind(handoff)['sha256']=='42ef1318050eecf9db847a142d1ec2e67759e23ca0bce67976248641ecdd1c44','immutable handoff fixed')
record={'status':'PASS_STATIC_RUNTIME_AND_ADMISSION_PREFLIGHT_NOT_PRODUCER_RELEASE','createdAtUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'checks':len(checked),'passed':checked,'prepared':bind(P/'prepared.json'),'runner':bind(P/'run-oracle.py'),'fullPatch':bind(P/'array-runner-to-primitive.patch'),'runtime':bind(P/'runtime-bindings.json'),'oracle':bind(P/'oracle.sc'),'handoff':bind(handoff),'authorOfflineChecks':bind(author),'preparationReview':bind(O/'preparation-review.json'),'readOnlyGitCheckpoint':checkpoint,'unchangedFunctions':unchanged,'staticFindings':['Only checkpoint schema/admission and complete two-anchor handling differ from reviewed74cf predecessor.','All19 cases require full selected extraction; both retained full anchors must match before promotion. No guessed new expected files exist.','Runtime verifies complete266-file Joern and527-file JDK trees,13 launcher commands, modes/symlinks and absent JVM/shell/loader overrides before and after; inherited host dylib/framework boundary remains explicit.','Raw stdout/stderr and exit/timeout/error receipts remain written before admission; selected failed output goes to held-selected, never expected. New run directory uses exist_ok=False.','Both source/docs IDs are pinned to actual atomic tenthf235; accepted document blobs, passed gates,138source/doc frozen file hashes and sibling frozen executables are verified.','False/null release template stays unapproved; exact separately parent-approved release SHA and explicit CLI release are still required.','No main(), attempt(), run_process(), Joern, Rust, Cargo or new oracle projection was invoked. In-memory synthetic protocol/predicate tests are not reference graphs.'],'requiredFixes':[],'sourceEdits':False,'producerStarted':False,'parentProducerReleaseIssuedByReviewer':False,'limits':['Inherited selected projection/escaping is unchanged and is not full schema or injective source transport.','Read-only runtime and actual Git admission verification does not authorize launch; parent owns the separate release and unique production attempt.','V1/V2 and offline binary-schema failure remain preserved.']}
(O/'runner-review.json').write_text(json.dumps(record,indent=2)+'\n')
print(json.dumps({'receipt':bind(O/'runner-review.json'),'checks':len(checked)},indent=2))
