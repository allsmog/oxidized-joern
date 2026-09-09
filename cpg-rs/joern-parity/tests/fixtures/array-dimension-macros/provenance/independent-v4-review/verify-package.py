from pathlib import Path
import json,hashlib,difflib,collections,datetime
P=Path('/Users/shayaunnejad/vibe-code/.codex-worktrees/joern-oxidized-astra-typedef-existence/.local/tenth-array-dimension-package-v1')
O=Path(__file__).parent
checks=[];files={}
def h(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def ck(n,x):assert x,n;checks.append(n)
def bind(p):
 p=Path(p);r={'path':str(p),'sha256':h(p),'bytes':p.stat().st_size};files[str(p)]=r;return r
m=json.loads((P/'manifest.json').read_text());bind(P/'manifest.json');F=Path(m['stagedRoot'])/m['fixtureRoot']
listed={r['path'] for r in m['files']};actual={str(p.relative_to(Path(m['stagedRoot']))) for p in F.rglob('*') if p.is_file()}
ck('exact135-file inventory',len(listed)==135 and actual==listed)
for r in m['files']:
 p=Path(m['stagedRoot'])/r['path'];ck(r['path'],bind(p)['sha256']==r['sha256'] and p.stat().st_size==r['bytes'])
ck('full package size',sum(r['bytes'] for r in m['files'])==m['bytes']==12810939)
j=json.loads((F/'measurement.json').read_text())
for r in j['copiedEvidence']:
 p=F/r['path'];old=Path(r['originalPath']);ck('copy '+r['path'],p.read_bytes()==old.read_bytes() and h(old)==r['sha256'])
raw=F/'oracle/array-dimension-controls/live.stdout';run=json.loads((raw.parent/'run.json').read_text());ck('raw bound and complete',h(raw)==run['rawStdout']['sha256'] and run['allCasesComplete'] and run['admittedAsReferences'] and run['exitCode']==0 and not run['timedOut'])
sections={};case=None
for line in raw.read_bytes().splitlines(keepends=True):
 if line.startswith(b'CASE|'):
  case=line[5:].decode().rstrip('\n');ck('unique marker '+case,case not in sections);sections[case]=bytearray()
 elif line.startswith(b'AST|'):sections[case].extend(line[4:])
 elif line.startswith((b'NODES|',b'EDGES|',b'FLOWS|')):sections[case].extend(line)
ck('all five live cases retained',len(sections)==5)
lines=nonempty=0;baseexact=heldexact=0
for r in j['cases']:
 name=r['case'];c=F/'cases'/name;expected=(c/'expected.txt').read_bytes()
 ck(name+' full raw extraction',bytes(sections[name])==expected==(raw.parent/'extracted'/name/'expected.txt').read_bytes())
 ck(name+' raw input', (c/'main.c').read_bytes()==(raw.parent/'snapshot/input'/name/'main.c').read_bytes())
 ck(name+' source hash',h(c/'main.c')==r['inputHashes']['main.c'])
 ls=expected.splitlines(keepends=True);nn=sum(bool(l.strip()) for l in ls);lines+=len(ls);nonempty+=nn
 ck(name+' counters',len(ls)==r['canonicalLinesIncludingSeparators'] and nn==r['nonemptySelectedRecords'])
 for v in ['baseline','held-v2']:
  obs=r['observations'][v];vdir=c/v;vr=json.loads((vdir/'run.json').read_text());actual=(vdir/'actual.txt').read_bytes();eq=actual==expected
  ck(name+v+' receipt',vr['exitCode']==0 and vr['inputBefore']==vr['inputAfter'] and obs['exitCode']==0 and obs['exact']==eq and h(vdir/'actual.txt')==obs['actualSha256'])
  ck(name+v+' bound full artifacts',h(vdir/'actual.txt')==vr['actual']['sha256'] and h(vdir/'stderr.txt')==vr['stderr']['sha256'] and h(vdir/'complete.diff')==vr['completeDiff']['sha256'])
  if v=='baseline':baseexact+=eq
  else:heldexact+=eq
  # Complete diffs may use historical absolute filenames; compare all hunk bytes after their two headers.
  d=(vdir/'complete.diff').read_bytes();generated=''.join(difflib.unified_diff(expected.decode().splitlines(True),actual.decode().splitlines(True))).encode()
  ck(name+v+' complete difference', d.split(b'\n',2)[-1] == generated.split(b'\n',2)[-1])
  records=collections.Counter(actual.splitlines(keepends=True));want=collections.Counter(expected.splitlines(keepends=True))
  ck(name+v+' full counters',sum((records&want).values())==vr['matchingRecordMultiplicities'] and sum((want-records).values())==vr['missingRecordMultiplicities'] and sum((records-want).values())==vr['extraRecordMultiplicities'])
 ck(name+' repaired evidence honestly pending',r['finalRepairedCandidate'] is None)
ck('four exact status and counts',baseexact==3 and heldexact==1 and lines==2012 and nonempty==1977)
anchor=bytes(sections['body_include_macro_only']);ck('anchor provenance only',hashlib.sha256(anchor).hexdigest()==j['anchor']['expectedSha256'] and not j['anchor']['productionGateDuplicated'] and (raw.parent/'snapshot/anchor-expected.txt').read_bytes()==anchor)
ck('complete batch counters',sum(len(bytes(b).splitlines(True)) for b in sections.values())==2135 and sum(sum(bool(l.strip()) for l in bytes(b).splitlines(True)) for b in sections.values())==2096)
rec={'createdAtUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'status':'PASS_FROZEN_REFERENCE_AND_HELD_REGRESSION_PACKAGE','checks':len(checks),'checksPassed':checks,'bindings':list(files.values()),'scope':{'files':135,'bytes':12810939,'newReferences':4,'anchorProvenanceOnly':True,'rawSelectedCanonicalLines':2012,'rawSelectedNonempty':1977,'baselineExact':3,'heldV2Exact':1,'repairedCandidatePendingInThisImmutablePackage':True,'productionTestNotInThisPackage':True},'noProducersOrSourceEdits':True,'veracity':'The package is a historical reference/regression package. V4 source/test/results will be separately bound rather than silently rewriting its pending status.'}
(O/'package-review.json').write_text(json.dumps(rec,indent=2)+'\n');print(json.dumps({'path':str(O/'package-review.json'),'sha256':h(O/'package-review.json'),'checks':len(checks)},indent=2))
