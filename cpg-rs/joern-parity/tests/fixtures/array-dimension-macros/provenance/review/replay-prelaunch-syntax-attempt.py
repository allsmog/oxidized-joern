from pathlib import Path
from collections import Counter
import hashlib,json,subprocess,time,difflib,shutil
P=Path(__file__).resolve().parent
PREP=P.parent/'tenth-array-dimension-controls'
RUN=PREP/'runs/first-complete-array-dimension-reference-batch'
ROOT=Path('/Users/shayaunnejad/vibe-code/.codex-worktrees/joern-oxidized-astra-sprint')
BUILDS={'accepted-ninth':ROOT/'.local/astra-sprint/ninth-batch/reviewed-final-real-differential/bin','held-tenth-v2':ROOT/'.local/astra-sprint/tenth-batch/final-real-differential/bin'}
EXPECTED_BINS={'accepted-ninth':'d9a7ebff8d4bc4a33f9d248fbc9b88409898e9ce12ec62eb37c37a73df65ae17','held-tenth-v2':'522fc62d949b46e3083ba855958e37d9e7a280aa4ada4323ab9a8b9d5aa74d14'}
def sha(p):return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def bind(p):return {'path':str(p),'sha256':sha(p),'bytes':Path(p).stat().st_size}
def write(p,b):
 with p.open('xb') as f:f.write(b)
def save(p,o):write(p,(json.dumps(o,indent=2)+'\n').encode())
def inventory(p):return {str(x.relative_to(p)):bind(x) for x in sorted(p.rglob('*')) if x.is_file()}
def lines(raw):
 v=raw.decode('utf-8').split('\n')
 if v and v[-1]=='':v.pop()
 return v

def main():
 receipt=json.loads((RUN/'run.json').read_bytes());prep=json.loads((PREP/'prepared.json').read_bytes())
 assert sha(PREP/'prepared.json')=='75117abb78dc7662784ad363fd114755249a39422561393ca0b6f741d8ba0cf1'
 assert sha(RUN/'run.json')=='72ae0ca16446071ea627caf918ab2d9ea9574125d6ac7c0385a1a03340e3c5a4'
 assert receipt['status']=='COMPLETE_RAW_REFERENCE_BATCH' and receipt['exitCode']==0 and not receipt['timedOut'] and not receipt['holdReasons']
 assert receipt['before']==receipt['after'] and receipt['gitBefore']==receipt['gitAfter']
 assert receipt['before']['runtime']==prep['runtime']['expectedSnapshot']
 assert receipt['retainedAnchorByteIdentical'] and receipt['allCasesComplete']
 release=ROOT/'.local/astra-sprint/tenth-batch/array-dimensions-release.json'
 assert sha(release)==receipt['parentApprovedReceiptSha256']=='d17e5f104f5acadeaaf3c6ae270e54d0d72557976aa28fe1c05065aa2dfc98ee'
 for name in ['live.stdout','live.stderr']:
  field='rawStdout' if name.endswith('stdout') else 'rawStderr';assert sha(RUN/name)==receipt[field]['sha256']
 for x in prep['files']:
  assert sha(PREP/x['path'])==x['sha256'] and (PREP/x['path']).stat().st_size==x['bytes']
 assert {n:bind(PREP/'input'/n)['sha256'] for n in prep['inputHashes']}==prep['inputHashes']
 assert inventory(PREP/'input').keys()==inventory(RUN/'snapshot/input').keys()
 for n in prep['inputHashes']:
  assert (PREP/'input'/n).read_bytes()==(RUN/'snapshot/input'/n).read_bytes()
 for n in ['oracle.sc','run-oracle.py','prepared.json']:
  assert (PREP/n).read_bytes()==(RUN/'snapshot'/n).read_bytes()
 cases={};selected=None
 for raw in (RUN/'live.stdout').read_bytes().split(b'\n'):
  if raw.startswith(b'CASE|'):
   name=raw[5:].decode();assert name not in cases and name in prep['cases'];selected=[];cases[name]=selected
  elif raw.startswith((b'AST|',b'NODES|',b'EDGES|',b'FLOWS|')):
   assert selected is not None;selected.append(raw[4:] if raw.startswith(b'AST|') else raw)
 assert set(cases)==set(prep['cases'])
 outrows={x['case']:x for x in receipt['outputs']}; assert set(outrows)==set(cases)
 inputs=P/'input';inputs.mkdir()
 for name,records in sorted(cases.items()):
  expected=b'\n'.join(records)+b'\n'; ep=RUN/'expected'/name/'expected.txt'
  assert expected==ep.read_bytes() and sha(ep)==outrows[name]['sha256']
  assert len(records)==outrows[name]['canonicalLinesIncludingSeparators']
  assert sum(bool(r) for r in records)==outrows[name]['nonemptySelectedRecords']
  d=inputs/name;shutil.copytree(PREP/'input'/name,d);write(d/'expected.txt',expected)
 assert (inputs/'body_include_macro_only/expected.txt').read_bytes()==(PREP/'retained-anchor/expected.txt').read_bytes()
 rawreview={'status':'PASS_ALL5_COMPLETE_RAW_REFERENCES','run':bind(RUN/'run.json'),'raw':bind(RUN/'live.stdout'),'rawStderr':bind(RUN/'live.stderr'),'prepared':bind(PREP/'prepared.json'),'release':bind(release),'inputHashes':prep['inputHashes'],'allBeforeAfterBindingsEqual':True,'runtimeSnapshotMatchesPrepared':True,'anchorByteIdentical':True,'completeCanonicalLines':sum(len(r) for r in cases.values()),'nonemptyRecords':sum(sum(bool(r) for r in rows) for rows in cases.values()),'outputs':outrows}
 save(P/'reference-review.json',rawreview)
 buildbindings={};frozen_before={}
 for tag,b in BUILDS.items():
  meta=json.loads((b/'source-bindings.json').read_bytes());assert meta['allInputsUnchangedDuringBuild']
  assert sha(b/'joern-parity')==meta['binaries']['joern-parity']==EXPECTED_BINS[tag]
  files={'metadata':bind(b/'source-bindings.json'),'binary':bind(b/'joern-parity')};proofs=[]
  for n,h in meta['sources'].items():
   source=b/'source'/n
   if not source.exists():source=b/'source'/Path(n).relative_to('cpg-rs')
   assert sha(source)==h;files[n]=bind(source)
   if tag=='accepted-ninth':
    old=subprocess.run(['git','cat-file','blob','00c37613b3074025d3d65aa632cbd79af95d0c46:'+n],cwd=ROOT,capture_output=True,check=True).stdout
    assert hashlib.sha256(old).hexdigest()==h
   proofs.append({'path':n,'frozen':bind(source),'acceptedCommitByteMatch':tag=='accepted-ninth'})
  buildbindings[tag]={'metadata':files['metadata'],'binary':files['binary'],'sourceProofs':proofs}
  frozen_before[tag]=files
 results={}
 for tag,b in BUILDS.items():
  dest=P/tag;dest.mkdir();rows=[]
  for name in sorted(cases):
   inp=inputs/name;srcs=sorted(x.name for x in inp.iterdir() if x.suffix in {'.h','.c'});out=dest/name;out.mkdir()
   before=inventory(inp);argv=[str(b/'joern-parity'),'--production',*srcs];start=time.monotonic();timed=False
   with (out/'actual.txt').open('xb') as stdout,(out/'stderr.txt').open('xb') as stderr:
    proc=subprocess.Popen(argv,cwd=inp,stdout=stdout,stderr=stderr)
    try:status=proc.wait(timeout=60)
    except subprocess.TimeoutExpired:timed=True;proc.kill();status=proc.wait()
   after=inventory(inp);actual=(out/'actual.txt').read_bytes();expected=(inp/'expected.txt').read_bytes();ac=Counter(lines(actual));ec=Counter(lines(expected))
   assert before==after and sha(b/'joern-parity')==EXPECTED_BINS[tag]
   # Keep every line and its terminating LF in readable complete diffs.
   diff=''.join(difflib.unified_diff([x+'\n' for x in lines(expected)],[x+'\n' for x in lines(actual)],fromfile='full-live-expected',tofile=tag+'-production'))
   write(out/'complete.diff',diff.encode('utf-8'))
   row={'case':name,'status':'COMPLETE_PRODUCTION_COMPARISON' if status==0 and not timed else 'HELD_INCOMPLETE_PRODUCER','command':argv,'cwd':str(inp),'exitCode':status,'timedOut':timed,'seconds':time.monotonic()-start,'inputBefore':before,'inputAfter':after,'expected':bind(inp/'expected.txt'),'actual':bind(out/'actual.txt'),'stderr':bind(out/'stderr.txt'),'completeDiff':bind(out/'complete.diff'),'byteExact':actual==expected,'matchingRecordMultiplicities':sum((ec&ac).values()),'missingRecordMultiplicities':sum((ec-ac).values()),'extraRecordMultiplicities':sum((ac-ec).values())}
   save(out/'run.json',row);rows.append(row)
  results[tag]=rows
  save(dest/'review.json',{'status':'COMPLETE_REPLAY' if all(x['exitCode']==0 and not x['timedOut'] for x in rows) else 'HELD_PRODUCER_FAILURE','build':buildbindings[tag],'cases':rows,'exactCases':sum(x['byteExact'] for x in rows)})
  assert all(x['exitCode']==0 and not x['timedOut'] for x in rows)
 for tag,files in frozen_before.items():
  assert all(bind(Path(row['path']))==row for row in files.values())
  assert all(inventory(inputs/r['case'])==r['inputBefore'] for r in results[tag])
 for name in sorted(cases):
  a=(P/'accepted-ninth'/name/'actual.txt').read_bytes();b=(P/'held-tenth-v2'/name/'actual.txt').read_bytes()
  diff=''.join(difflib.unified_diff([x+'\n' for x in lines(a)],[x+'\n' for x in lines(b)],fromfile='accepted-ninth',tofile='held-tenth-v2'))
  write(P/'held-tenth-v2'/name/'complete-before-after.diff',diff.encode())
 save(P/'replay-review.json',{'status':'COMPLETE10_BOUND_READ_ONLY_PRODUCTION_REPLAYS','referenceReview':bind(P/'reference-review.json'),'builds':buildbindings,'results':{t:bind(P/t/'review.json') for t in results},'exactProjects':{t:sum(x['byteExact'] for x in rows) for t,rows in results.items()},'sourceAndBinariesUnchangedAfter':True,'script':bind(Path(__file__)),'scope':'Fresh full-reference comparisons; no builds or source edits. Incidental elapsed times are not resource-gate measurements.'})
 print(json.dumps({'receipt':bind(P/'replay-review.json'),'results':{t:[{k:x[k] for k in ['case','byteExact','missingRecordMultiplicities','extraRecordMultiplicities']} for x in rows] for t,rows in results.items()}))
if __name__=='__main__':main()
