from pathlib import Path
from collections import Counter
import base64, hashlib, json, re

ROOT=Path(__file__).resolve().parent
BATCH=ROOT.parent/'primitive-member-baseline-v1/runs/accepted-tenth-1'
def binding(p):
    p=Path(p); b=p.read_bytes()
    return {'path':str(p.resolve()),'sha256':hashlib.sha256(b).hexdigest(),'bytes':len(b)}
def rows(b):
    r=b.split(b'\n')
    if r and not r[-1]:r.pop()
    return r
def members(b):
    return [{'line':i+1,'rawBase64':base64.b64encode(v).decode(),'text':v.decode()} for i,v in enumerate(rows(b)) if v.lstrip().startswith(b'MEMBER ')]
def raw_counter(c):
    return [{'rawBase64':base64.b64encode(k).decode(),'multiplicity':v} for k,v in sorted(c.items())]
run=json.loads((BATCH/'run.json').read_bytes())
assert run['status']=='COMPLETE_BOUND_ACCEPTED_TENTH_BASELINE' and run['allProducersComplete'] and not run['errors']
assert run['gitBefore']==run['gitAfter']
assert run['caseCount']==19 and run['exactCases']==2 and run['nonexactCases']==17
assert run['referenceReceiptSha256']=='aeef8f13e1f03f46daec5df5d6c8ac9fba026ff71334dc3f59d8d50e3386033f'
cases=[]; artifacts=[binding(BATCH/'run.json'),binding(__file__)]; totals={k:Counter() for k in ('includingSeparators','nonempty')}
for case in run['cases']:
    p=BATCH/case['case']; expected=(p/'expected.txt').read_bytes(); actual=(p/'actual.txt').read_bytes()
    assert case['exitCode']==0 and not case['timedOut'] and not case['processError'] and not case['bindingErrors']
    assert case['inputBefore']==case['inputAfter'] and case['completeProducer']
    assert case['exact']==(expected==actual)
    for field,name in [('expected','expected.txt'),('actual','actual.txt'),('stderr','stderr.txt'),('completeDiff','complete.diff')]:
        assert binding(p/name)==case[field]
        artifacts.append(binding(p/name))
    artifacts.append(binding(p/'run.json'))
    for filename,item in case['inputAfter']['files'].items():
        assert binding(p/'input'/filename)['sha256']==item['sha256']
        artifacts.append(binding(p/'input'/filename))
    for mode,empty in [('includingSeparators',True),('nonempty',False)]:
        e=Counter(x for x in rows(expected) if empty or x); a=Counter(x for x in rows(actual) if empty or x)
        observed={'expected':sum(e.values()),'actual':sum(a.values()),'matching':sum((e&a).values()),'missing':sum((e-a).values()),'extra':sum((a-e).values())}
        assert observed==case['multiplicities'][mode]
        totals[mode].update(observed)
    e=Counter(rows(expected));a=Counter(rows(actual));em=members(expected);am=members(actual)
    assert len(em)==len(am)
    # An ancillary property check only. Complete raw comparisons above remain unmodified.
    for left,right in zip(em,am):
        assert re.sub(r' TYPE_FULL_NAME=.*?(?= ORDER=)','',left['text'])==re.sub(r' TYPE_FULL_NAME=.*?(?= ORDER=)','',right['text'])
    cases.append({'case':case['case'],'exact':case['exact'],'multiplicities':case['multiplicities'],'expectedMembers':em,'baselineMembers':am,'memberNameCodeOrderPreserved':True,'missingCompleteRecords':raw_counter(e-a),'extraCompleteRecords':raw_counter(a-e),'fullRun':binding(p/'run.json')})
assert {x['case'] for x in cases if x['exact']}=={'ordinary_numeric_control','unsigned_char'}
assert dict(totals['nonempty'])=={'expected':3423,'actual':3403,'matching':3277,'missing':146,'extra':126}
assert dict(totals['includingSeparators'])=={'expected':3493,'actual':3473,'matching':3347,'missing':146,'extra':126}
nonprimitive=next(c for c in cases if c['case']=='nonprimitive_scalar_control')
assert [r['text'] for r in nonprimitive['expectedMembers']]==[r['text'] for r in nonprimitive['baselineMembers']]
assert nonprimitive['multiplicities']['nonempty']['missing']==20 and nonprimitive['multiplicities']['nonempty']['extra']==0
summary={
    'status':'PASS_BOUND_COMPLETE_FROZEN_TENTH_BASELINE_MEASUREMENT',
    'baselineRun':binding(BATCH/'run.json'),
    'acceptedSourceAndDocsCommit':'f235a0f898c4e19fda90998b903f57741b776ec7',
    'baselineBinarySha256':'ca569f9d75143d9c20e7c8054c8bbf7949640e6eb0213fba806e8224b0994141',
    'projects':19,'newProjects':17,'retainedAnchorProjects':2,'sourceFiles':20,
    'exactProjects':2,'nonexactProjects':17,'totals':{k:dict(v) for k,v in totals.items()},
    'memberRowsCompared':sum(len(c['expectedMembers']) for c in cases),
    'allMemberNameCodeOrderPreserved':True,
    'findings':[
        'Sixteen nonexact projects differ in MEMBER TYPE_FULL_NAME and corresponding type nodes/relations; full complete graphs remain recorded without filtering.',
        'The nonprimitive scalar control already matches all six MEMBER records but lacks four external TYPE/TYPE_DECL pairs and their twelve relations: Record.Choice, Record.Inner, struct_value, union_value.',
        'const is dropped from measured MEMBER types; volatile is retained. Pointer qualifier location changes CODE but both measured pointer types are shortunsigned*.',
        'Both tiny anchors are full-reference identical to accepted retained references, while each Rust baseline graph still differs by seven known member-type/scaffold records.'
    ],
    'cases':cases,'artifacts':artifacts,
    'scope':'Measured complete references and accepted baseline only; no implementation, producer performance claim, filtered parity gate, or overall port-completion claim.'
}
with (ROOT/'final-review.json').open('x') as f:json.dump(summary,f,indent=2);f.write('\n')
print(json.dumps({'review':binding(ROOT/'final-review.json'),'totals':summary['totals'],'memberRowsCompared':summary['memberRowsCompared']}))
