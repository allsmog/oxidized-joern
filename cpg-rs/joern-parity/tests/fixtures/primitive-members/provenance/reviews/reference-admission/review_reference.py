from pathlib import Path
import json,hashlib,datetime,collections

P=Path('/Users/shayaunnejad/vibe-code/.codex-worktrees/joern-oxidized-astra-typedef-existence/.local/primitive-member-runner-v3')
RUN=P/'runs/first-complete-primitive-member-reference-batch'
R=Path('/Users/shayaunnejad/vibe-code/.codex-worktrees/joern-oxidized-astra-sprint')
O=Path(__file__).parent
release=R/'.local/astra-sprint/eleventh-batch/primitive-member-release.json'
release_sha='73e4723d51b1b134b32bbf6bd81469ee224861ae59920a3ad3b1794cf0441d02'
checks=[];bound=[]
def c(v,s):
 if not v:raise AssertionError(s)
 checks.append(s)
def bind(p):
 p=Path(p);b=p.read_bytes();return {'path':str(p),'sha256':hashlib.sha256(b).hexdigest(),'bytes':len(b)}
def load(p):return json.loads(Path(p).read_bytes())
def verify(p,h,label):
 b=bind(p);c(b['sha256']==h,label);bound.append(b)
j=load(RUN/'run.json');manifest=load(P/'prepared.json');start=bind(RUN/'run.json')
c(start['sha256']=='aeef8f13e1f03f46daec5df5d6c8ac9fba026ff71334dc3f59d8d50e3386033f','immutable completed run')
c(j['status']=='COMPLETE_RAW_REFERENCE_BATCH' and j['admittedAsReferences'] and j['allCasesComplete'] and j['extractedProjectionComplete'],'completed admitted receipt')
c(j['exitCode']==0 and not j['timedOut'] and j['processError'] is None and not j['holdReasons'],'successful process no ignored hold')
c(j['producerStarted'] is True,'actual launch recorded')
verify(release,release_sha,'parent approved release bytes')
c(j['parentApprovedReceiptSha256']==release_sha,'receipt uses approved parent release')
verify(P/'prepared.json','23302c99c8e4a543b308c04f5f60e72be034b88530fd75638f9e0ff441169894','reviewed prepared bytes')
verify(P/'run-oracle.py','9e798056560fd244d9ccbc1b9a041152acabc032fa988c400e09f3a6df9bf2ac','reviewed driver bytes')
for k in ['rawStdout','rawStderr']:
 verify(j[k]['path'],j[k]['sha256'],'full '+k);c(Path(j[k]['path']).stat().st_size==j[k]['bytes'],'raw byte length '+k)

# Independent extraction: preserve each raw LF-delimited selected record in
# encounter order, removing exactly AST| and retaining duplicate/blank records.
raw=(RUN/'live.stdout').read_bytes();frames=[];selected={};raw_selected={};kinds={};current=None
for line in raw.split(b'\n'):
 if line[:5]==b'CASE|':
  current=line[5:].decode('utf8');c(current not in selected,'unique CASE '+current)
  frames.append(current);selected[current]=[];raw_selected[current]=[];kinds[current]=set()
 else:
  prefix=next((p for p in [b'AST|',b'NODES|',b'EDGES|',b'FLOWS|'] if line.startswith(p)),None)
  if prefix is None:continue
  c(current is not None,'selected line has CASE owner')
  raw_selected[current].append(line);kinds[current].add(prefix[:-1].decode())
  selected[current].append(line[4:] if prefix==b'AST|' else line)
c(frames==manifest['cases']==sorted(frames) and len(frames)==19,'all19 sorted complete CASE frames')
c(len(j['outputs'])==19 and len({x['case'] for x in j['outputs']})==19,'exact19 distinct admitted outputs')
expected_paths={f"{x}/expected.txt" for x in frames}
c({str(p.relative_to(RUN/'expected')) for p in (RUN/'expected').rglob('*') if p.is_file()}==expected_paths,'only19 expected graph files')
counts=[];outputs={x['case']:x for x in j['outputs']}
for name in frames:
 data=b'\n'.join(selected[name])+b'\n';row=outputs[name];p=Path(row['path'])
 c(data==p.read_bytes(),'complete raw extraction equals admitted '+name)
 c(kinds[name]=={'AST','NODES','EDGES','FLOWS'}==set(row['sectionsSeen']),'all4 selected sections '+name)
 c(row['completeReference'] is True and p==RUN/'expected'/name/'expected.txt','proper admitted output '+name)
 verify(p,row['sha256'],'reference hash '+name)
 c(data.count(b'\n')==row['canonicalLinesIncludingSeparators'],'canonical LF count '+name)
 c(sum(bool(x) for x in selected[name])==row['nonemptySelectedRecords'],'nonempty count '+name)
 c(collections.Counter(data[:-1].split(b'\n'))==collections.Counter(selected[name]),'selected multiplicity '+name)
 counts.append({'case':name,'canonicalLinesIncludingSeparators':len(selected[name]),'nonemptySelectedRecords':sum(bool(x) for x in selected[name]),'rawSelectedRecords':len(raw_selected[name]),'sha256':row['sha256']})
for row in manifest['anchors']:
 name=row['case'];expected=(P/row['referencePath']).read_bytes();c((b'\n'.join(selected[name])+b'\n')==expected,'full raw anchor equality '+name)
 verify(P/row['referencePath'],row['expectedSha256'],'retained anchor '+name)
c(j['retainedAnchorByteIdentical'] is True and j['retainedAnchors']=={x['case']:True for x in manifest['anchors']},'both anchor receipt states')

g={'__file__':str(P/'run-oracle.py'),'__name__':'read_only_admission_review'}
exec(compile((P/'run-oracle.py').read_text(),str(P/'run-oracle.py'),'exec'),g)
def forbidden(*a,**k):raise AssertionError('No producer may run in review')
g['run_process']=forbidden
verified_manifest,current_prepared=g['verify_prepared'](P)
c(verified_manifest==manifest,'complete prepared directory matches reviewed manifest')
release_data,checkpoint=g['verify_release'](release,release_sha,bind(P/'prepared.json')['sha256'],manifest['checkpointAdmission'])
c(checkpoint==j['gitBefore']==j['gitAfter'],'current actual accepted Git/138source/docs/binary release equals both recorded snapshots')
c(checkpoint['sourceCommit']==checkpoint['documentationCommit']=='f235a0f898c4e19fda90998b903f57741b776ec7' and len(checkpoint['frozenInputHashes'])==138,'actual committed tenth and138inputs')
external={manifest['joern']['executable']['path']:manifest['joern']['executable']['sha256'],**{r['path']:r['sha256'] for r in manifest['frozenOriginals']},**{r['path']:r['inventory'] for r in manifest['frozenOriginalInventories']},str(release.resolve()):release_sha}
external.update(checkpoint['externalBindings'])
for p,v in external.items():g['require_bound'](Path(p),v);c(True,'actual original/runtime dependency '+p)
actual=g['relevant_snapshot'](P,RUN,external,manifest['runtime'])
c(j['before']==j['after']==actual,'all before/after/current prepared copied runtime external inventories equal')
c(actual['runtime']==manifest['runtime']['expectedSnapshot'],'current complete runtime equals reviewed expected runtime')
c(actual['runtime']==j['preflightBefore']['runtime'],'runtime unchanged from earliest preflight through completed current snapshot')
c({k:len(v['files']) for k,v in actual['runtime']['trees'].items()}=={'joern':266,'jdk':527},'complete266Joern527JDK inventories')
c(len(actual['runtime']['commands'])==13,'13actual launcher commands bound')
c(all(not x['nonempty'] for x in actual['runtime']['overrideEnvironment'].values()),'all JVM/shell/loader overrides remain absent or empty')
for name in ['oracle.sc','prepared.json','run-oracle.py']:
 c((RUN/'snapshot'/name).read_bytes()==(P/name).read_bytes(),'snapshot exact '+name)
c((RUN/'snapshot/checkpoint-release.json').read_bytes()==release.read_bytes(),'snapshot exact approved release')
inputs=g['inventory'](RUN/'snapshot/input')
c(inputs==g['inventory'](P/'input') and len(inputs['files'])==20,'all20 copied source bytes/names unchanged')
for name,row in inputs['files'].items():
 c(row['sha256']==manifest['inputHashes'][name],'input prepared hash '+name)
 c(b'\r' not in (RUN/'snapshot/input'/name).read_bytes(),'input LF only '+name)
c(j['command']==[manifest['joern']['executable']['path'],'--script',str(RUN/'snapshot/oracle.sc'),'--param','inputPath='+str(RUN/'snapshot/input')],'actual producer command frozen oracle and input')
c((RUN/'workspace').is_dir(),'unique producer workspace retained')
for file,key in [('preflight-before.json','preflightBefore'),('before.json','before'),('after.json','after')]:c(load(RUN/file)==j[key],'standalone snapshot receipt '+file)
c(bind(RUN/'run.json')==start,'run receipt stable during review')
record={'status':'PASS_COMPLETE19_REFERENCE_ADMISSION_REVIEW','createdAtUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'checks':len(checks),'passed':checks,'run':start,'release':bind(release),'prepared':bind(P/'prepared.json'),'driver':bind(P/'run-oracle.py'),'oracle':bind(P/'oracle.sc'),'rawStdout':bind(RUN/'live.stdout'),'rawStderr':bind(RUN/'live.stderr'),'cases':counts,'totals':{'newProjects':17,'retainedFullAnchors':2,'admittedCompleteProjects':19,'sourceFiles':20,'canonicalLinesIncludingSeparators':sum(x['canonicalLinesIncludingSeparators'] for x in counts),'nonemptySelectedRecords':sum(x['nonemptySelectedRecords'] for x in counts)},'process':{k:j[k] for k in ['seconds','exitCode','timedOut','processError','producerStarted']},'priorStaticReview':bind(O/'runner-review.json'),'boundFiles':bound,'findings':['All19 raw CASE frames and every selected AST/NODES/EDGES/FLOWS record independently extracted byte-for-byte; no line trimming, deduplication, scalar substitution or graph filtering.','Both complete retained anchor outputs equal prior references; both raw datasets and all20 input hashes are preserved.','Actual parent release73e4723d binds reviewed prepared23302 and acceptedf235 source/docs with138frozen Git blobs and frozen cpg/parity executables.','Complete Joern/JDK, launcher, override, source/oracle/driver/copied and external before/after inventories match actual current inventories.','Successful raw reference admission is distinct from Rust parity or implementation acceptance. No new expected spelling was guessed; later Rust baseline must consume these live complete bytes.'],'requiredFixes':[],'sourceEdits':False,'reviewerProducerRuns':0,'rustParityClaim':False,'failureRetention':'Existing V1/V2 offline schema failure and prior frozen preparations remain hash-bound; this run exited0 with no hold reasons. Inherited nonzero/timeout/drift paths were statically reviewed, not artificially rerun.'}
(O/'reference-admission-review.json').write_text(json.dumps(record,indent=2)+'\n')
print(json.dumps({'receipt':bind(O/'reference-admission-review.json'),'checks':len(checks),'totals':record['totals']},indent=2))
