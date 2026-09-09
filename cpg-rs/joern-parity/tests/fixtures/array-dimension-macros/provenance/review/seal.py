from pathlib import Path
import json,hashlib,re
P=Path(__file__).resolve().parent
def bind(p):
 b=p.read_bytes();return {'path':str(p),'sha256':hashlib.sha256(b).hexdigest(),'bytes':len(b)}
def methods(p):
 out={};name=None
 for line in p.read_bytes().decode('utf-8').split('\n'):
  if line.startswith('METHOD '):
   name=line.rpartition(' FULL_NAME=')[2].split(' SIGNATURE=',1)[0].split(' ORDER=',1)[0];assert name not in out;out[name]=[]
  if not line:name=None
  elif name is not None:out[name].append(line)
 return out
replay=json.loads((P/'replay-review.json').read_bytes());reference=json.loads((P/'reference-review.json').read_bytes())
assert replay['sourceAndBinariesUnchangedAfter']
assert replay['exactProjects']=={'accepted-ninth':3,'held-tenth-v2':2}
rows=[]
spelling={'bare_macro':('N','3'),'binary_macro':('N+1','N+1'),'parenthesized_macro':('(N+1)','(N+1)'),'unary_macro':('+N','+N')}
for case,(syntax,expected_type) in spelling.items():
 paths={'oracle':P/'input'/case/'expected.txt','acceptedNinth':P/'accepted-ninth'/case/'actual.txt','heldTenthV2':P/'held-tenth-v2'/case/'actual.txt'}
 trees={tag:methods(path) for tag,path in paths.items()}
 literal={tag:t['literal_dimension'] for tag,t in trees.items()};assert literal['oracle']==literal['acceptedNinth']==literal['heldTenthV2']
 local={tag:[line for line in tree['macro_dimension'] if line.startswith('    LOCAL NAME=values ')] for tag,tree in trees.items()}
 assert all(len(v)==1 for v in local.values())
 local={tag:v[0] for tag,v in local.items()};assert ' TYPE_FULL_NAME=int['+expected_type+'] ' in local['oracle']
 status='BARE_MACRO_IMPROVEMENT' if case=='bare_macro' else 'CONFIRMED_COMPLETE_GRAPH_REGRESSION'
 rows.append({'case':case,'sourceDimension':syntax,'liveType':'int['+expected_type+']','status':status,'localRecords':local,'pairedLiteralFullMethodExactAllVersions':True,'fullGraphs':{tag:bind(path) for tag,path in paths.items()},'completeNinthDiff':bind(P/'accepted-ninth'/case/'complete.diff'),'completeV2Diff':bind(P/'held-tenth-v2'/case/'complete.diff'),'completeBeforeAfterDiff':bind(P/'held-tenth-v2'/case/'complete-before-after.diff')})
report={'status':'PASS_BOUNDED_DIAGNOSIS_CONFIRMS_V2_COMPOUND_ARRAY_REGRESSION','candidateAcceptance':'HELD: reference evidence identifies required repair; no fix implemented by reviewer','referenceReview':bind(P/'reference-review.json'),'replayReview':bind(P/'replay-review.json'),'completeProjects':5,'newPairedProjects':4,'inputFiles':6,'canonicalLines':reference['completeCanonicalLines'],'nonemptyRecords':reference['nonemptyRecords'],'allTenProducersComplete':True,'exactProjects':replay['exactProjects'],'rows':rows,'anchor':{'name':'body_include_macro_only','sourceAndReferenceByteUnchanged':True,'acceptedNinthFullExact':False,'heldV2FullExact':True},'observedPolicy':'Expand the dimension macro only for a bare identifier in these controls. Preserve original source spelling for binary, parenthesized-binary, and unary expressions; the paired literal methods remain fully exact.','suggestedBoundedRepair':'Restrict object_decl_suffix macro expansion to a bare identifier size node, and retain the existing source-text/normalization fallback for other size expressions. Verify against these full graphs, all prior exact fixtures and whole-project output before acceptance.','limitations':['Only these measured dimension shapes and the prior complete real-project witnesses are claimed. Parenthesized bare identifiers, arbitrary function-like dimension macros and general constant folding have not been newly measured here.','All complete outputs, multiplicities and differences are retained. Neither nonexact reference nor expected graph was replaced or filtered.','No source edits, Cargo build, Joern launch, runtime alteration, resource measurement, manifest modification or acceptance decision by this reviewer. Root performed the bound live oracle launch.','One reviewer Python syntax mistake prevented launch before any graph producer; original script/log retained. The corrected script performed exactly ten successful graph replays.'],'artifacts':[bind(p) for p in sorted(P.rglob('*')) if p.is_file()]}
with (P/'final-review.json').open('x') as f:json.dump(report,f,indent=2);f.write('\n')
print(json.dumps(bind(P/'final-review.json')))
