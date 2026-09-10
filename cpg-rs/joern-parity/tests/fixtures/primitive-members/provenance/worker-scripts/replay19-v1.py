from pathlib import Path
import collections, difflib, hashlib, json, shutil, subprocess, time

r = Path.cwd()
base = r.parent / 'joern-oxidized-astra-typedef-existence/.local/primitive-member-baseline-v1/runs/accepted-tenth-1'
frozen = r / '.local/primitive-members/frozen-v1'
out = r / '.local/primitive-members/replay19-v1'
out.mkdir()
def sha(p): return hashlib.sha256(p.read_bytes()).hexdigest()
def lf(b): return b.replace(b'\r\n', b'\n').replace(b'\r', b'\n')
def diff(a, b, an, bn):
    return ''.join(difflib.unified_diff(a.decode().splitlines(True), b.decode().splitlines(True), fromfile=an, tofile=bn))
def counter(b, nonempty):
    return collections.Counter(x for x in b.decode().split('\n') if not nonempty or x)
bind = json.loads((frozen/'bindings.json').read_text())
copied = json.loads((r/'.local/primitive-members/copy-admitted.json').read_text())
base_meta = json.loads((base/'run.json').read_text())
baseline = {x['case']:x for x in base_meta['cases']}
binary = frozen/'joern-parity'
assert sha(binary) == 'fe5dd8c3036ca44e1112dadc08a748f46165e782894c77c38165ca8c4565cf2c'
before = {p:sha(r/p) for p in bind['sourceInputs']}
assert before == bind['sourceInputs']
rows = []
for case in copied['cases']:
    name = case['case']; d = out/name; (d/'input').mkdir(parents=True)
    fixture = r/'cpg-rs/joern-parity/tests/fixtures/primitive-members/cases'/name
    for rel, h in case['inputHashes'].items():
        p = fixture/rel; assert sha(p)==h
        q = d/'input'/rel; q.parent.mkdir(parents=True,exist_ok=True); shutil.copyfile(p,q)
    expected = (fixture/'expected.txt').read_bytes(); assert sha(fixture/'expected.txt')==case['expectedSha256']
    bp = baseline[name]; assert bp['exitCode']==0
    oldp = Path(bp['actual']['path']); assert sha(oldp)==bp['actual']['sha256']
    old = oldp.read_bytes()
    assert sha(Path(bp['expected']['path'])) == case['expectedSha256']
    for rel,h in case['inputHashes'].items(): assert bp['inputBefore']['files'][rel]['sha256']==h
    (d/'expected.txt').write_bytes(expected); (d/'before.txt').write_bytes(old)
    command = [str(binary), '--production', *sorted(case['inputHashes'])]
    start=time.monotonic(); error=None; timeout=False
    try:
        p=subprocess.run(command,cwd=d/'input',capture_output=True,timeout=120)
        stdout,stderr,status=p.stdout,p.stderr,p.returncode
    except subprocess.TimeoutExpired as e:
        stdout,stderr,status=e.stdout or b'',e.stderr or b'',None; timeout=True;error=str(e)
    (d/'stdout.txt').write_bytes(stdout); (d/'stderr.txt').write_bytes(stderr)
    actual=lf(stdout); (d/'actual.txt').write_bytes(actual)
    (d/'expected-before.diff').write_text(diff(expected,old,'live','accepted-tenth'))
    (d/'expected-candidate.diff').write_text(diff(expected,actual,'live','candidate-v1'))
    (d/'before-candidate.diff').write_text(diff(old,actual,'accepted-tenth','candidate-v1'))
    measures={}
    for label,nonempty in [('includingSeparators',False),('nonempty',True)]:
        e,b,c=map(lambda x:counter(x,nonempty),(expected,old,actual))
        lost=(e&b)-(e&c); gained=(e&c)-(e&b)
        measures[label]={'expected':sum(e.values()),'before':sum(b.values()),'candidate':sum(c.values()),'matchingBefore':sum((e&b).values()),'matchingCandidate':sum((e&c).values()),'gained':sum(gained.values()),'lost':sum(lost.values()),'lostRecords':dict(lost),'gainedRecords':dict(gained)}
    row={'case':name,'retainedAnchor':case['retainedAnchor'],'command':command,'cwd':str(d/'input'),'seconds':time.monotonic()-start,'exitCode':status,'timeout':timeout,'error':error,'beforeExact':old==expected,'candidateExact':status==0 and actual==expected,'beforeCandidateIdentical':old==actual,'inputHashes':case['inputHashes'],'counts':measures}
    assert {rel:sha(d/'input'/rel) for rel in case['inputHashes']}==case['inputHashes']
    row['files']={p.relative_to(d).as_posix():sha(p) for p in sorted(d.rglob('*')) if p.is_file()}
    (d/'run.json').write_text(json.dumps(row,indent=2)+'\n');rows.append(row)
    print(name,status,row['beforeExact'],'->',row['candidateExact'],measures['nonempty']['gained'],measures['nonempty']['lost'],flush=True)
after={p:sha(r/p) for p in bind['sourceInputs']};assert before==after
result={'status':'COMPLETE_FROZEN_CANDIDATE_REPLAY','frozenBindings':str(frozen/'bindings.json'),'frozenBindingsSha256':sha(frozen/'bindings.json'),'binarySha256':sha(binary),'baselineReceipt':str(base/'run.json'),'baselineReceiptSha256':sha(base/'run.json'),'copyReceiptSha256':sha(r/'.local/primitive-members/copy-admitted.json'),'helperSha256':sha(Path(__file__)),'sourceInputsUnchanged':True,'cases':rows,'summary':{'caseCount':len(rows),'beforeExact':sum(x['beforeExact'] for x in rows),'candidateExact':sum(x['candidateExact'] for x in rows),'successful':sum(x['exitCode']==0 for x in rows),'nonemptyGained':sum(x['counts']['nonempty']['gained'] for x in rows),'nonemptyLost':sum(x['counts']['nonempty']['lost'] for x in rows)}}
(out/'replay.json').write_text(json.dumps(result,indent=2)+'\n')
print(result['summary']);print('receipt',sha(out/'replay.json'))
