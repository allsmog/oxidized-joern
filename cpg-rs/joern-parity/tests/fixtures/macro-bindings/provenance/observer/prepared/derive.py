"""Prepare only the bounded successor source; do not launch an observer."""
from pathlib import Path
import difflib
here=Path(__file__).resolve().parent
prior=here.parent/'thirteenth-include-observer-v1'
old=(prior/'run-observer.py').read_text()
replacements={
    "['baseRunner','basePrepared','canonicalOracle','historicalObservation']": "['baseRunner','basePrepared','canonicalOracle','historicalObservation','sourcePreparationFreeze','acceptedCheckpointBinding']",
    "len(cfg['newCases'])==6,'Six fresh cases differ'": "len(cfg['newCases'])==4,'Four fresh cases differ'",
    "cfg['anchorCases']==['duplicate_supplied_macro_local','tiny_fixedtables_include','tiny_fixedtables_inline'],'Three historical supplemental anchors differ'": "cfg['anchorCases']==['repeated_direct_include'],'One historical supplemental anchor differs'",
    "PARENT_APPROVED_INCLUDE_REFERENCE_OBSERVER_RELEASE": "PARENT_APPROVED_MACRO_BINDING_OBSERVER_RELEASE",
    "len(names)==len(set(names))==11,'Historical eleven-case inventory differs'": "len(names)==len(set(names))==6,'Historical six-case inventory differs'",
    "len({j['case'] for j in jobs})==6,'Six unique fresh observer jobs required'": "len({j['case'] for j in jobs})==4,'Four unique fresh observer jobs required'",
    "'newCaseCount':6,'retainedCaseCount':0,'historicalSupplementAnchorCount':3,'newSourceCaseCount':2,'reusedSourceCaseCount':4": "'newCaseCount':4,'retainedCaseCount':0,'historicalSupplementAnchorCount':1,'newSourceCaseCount':3,'reusedSourceCaseCount':1",
}
text=old
for before,after in replacements.items():
    assert text.count(before)==1,before
    text=text.replace(before,after)
marker="    verify_historical_anchors(cfg,base,manifest)\n    return cfg,base,manifest"
addition="""    checkpoint=read(cfg['acceptedCheckpointBinding']['path'])
    require(checkpoint['acceptedCommit']==cfg['checkpointAdmission']['sourceCommit']==cfg['checkpointAdmission']['documentationCommit'],'Accepted source/document checkpoint differs')
    require(checkpoint['inputPreparation']==cfg['sourcePreparationFreeze'],'Accepted source preparation differs')
    for key in ['path','sha256']:
        require(checkpoint['frozenBuild'][key]==cfg['checkpointAdmission']['frozenBuild'][key],'Accepted frozen build differs: '+key)
    require(checkpoint['sourceFiles']==151 and checkpoint['fixtureFiles']==11821 and checkpoint['producerReleased'] is False,'Preparation-only checkpoint boundary differs')
    verify_historical_anchors(cfg,base,manifest)
    return cfg,base,manifest"""
assert text.count(marker)==1
text=text.replace(marker,addition)
with (here/'run-observer.py').open('x')as f:f.write(text)
with (here/'observer.sc').open('xb')as f:f.write((prior/'observer.sc').read_bytes())
with (here/'predecessor-to-v1.patch').open('x')as f:f.write(''.join(difflib.unified_diff(old.splitlines(True),text.splitlines(True),fromfile='thirteenth/run-observer.py',tofile='fourteenth/run-observer.py')))
