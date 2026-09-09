from pathlib import Path
import json,hashlib,urllib.request
P=Path(__file__).resolve().parent;G=P/'ties-and-order';H=lambda f:hashlib.sha256(f.read_bytes()).hexdigest()
def bind(p):return {'path':str(p.resolve()),'sha256':H(p)}
def extract(p):
 d={};n=None
 for l in p.read_text().splitlines():
  if l.startswith('CASE|'):n=l[5:];assert n not in d;d[n]=[]
  elif n and l.startswith(('AST|','NODES|','EDGES|','FLOWS|')):d[n].append(l[4:] if l.startswith('AST|') else l)
 return d
raw=extract(G/'live.stdout');cases=json.loads((G/'cases.json').read_text());lr=json.loads((G/'live-run.json').read_text());assert lr['exitCode']==0 and set(raw)==set(cases)
rows=[]
for case,files in cases.items():
 p=G/'input'/case;assert (p/'expected.txt').read_text()=='\n'.join(raw[case])+'\n'
 for f in files:assert H(p/f)==lr['inputHashes'][case][f]
 row={'case':case,'inputs':{f:bind(p/f) for f in files},'reference':bind(p/'expected.txt'),'canonicalLinesIncludingSeparators':len(raw[case]),'nonemptySelectedRecords':sum(bool(x) for x in raw[case])}
 for label in ['baseline-eighth','candidate-v2']:
  row[label]={'fullGraph':bind(G/label/case/'actual.txt'),'completeDifference':bind(G/label/case/'complete.diff')}
 row['liveMacroMethods']=[l for l in raw[case] if l.startswith('METHOD ') and ' CODE=#define ' in l]
 rows.append(row)
revision='d95237aeaf3d12cb4e63336def3a4d9d7315dfb4';checked={}
for f,path in [('MacroHandler.scala','astcreation/MacroHandler.scala'),('AstCreationPass.scala','passes/AstCreationPass.scala'),('AstCreator.scala','astcreation/AstCreator.scala'),('CdtParser.scala','parser/CdtParser.scala')]:
 u=f'https://raw.githubusercontent.com/joernio/joern/{revision}/joern-cli/frontends/c2cpg/src/main/scala/io/joern/c2cpg/'+path
 b=urllib.request.urlopen(u,timeout=20).read();assert hashlib.sha256(b).hexdigest()==H(P/'upstream'/f)
 checked[f]={'commitUrl':u,'sha256':H(P/'upstream'/f),'tagAndCommitBytesEqual':True}
base=json.loads((P/'definition-selection-review.json').read_text())
bs=json.loads((G/'baseline-eighth/replay.json').read_text());cs=json.loads((G/'candidate-v2/replay.json').read_text())
r={'status':'DIAGNOSIS_COMPLETE_REQUIRES_REPAIR_NOT_SOURCE_ACCEPTANCE','priorReceipt':bind(P/'definition-selection-review.json'),'version':{'tag':'v4.0.555','tagObject':'e1f2b03a6400a141181516de3d94c3f86c316f53','commit':revision,'tagResolution':bind(P/'upstream/tag-resolution.txt'),'commitSourceByteChecks':checked},'supplement':{'liveRun':bind(G/'live-run.json'),'raw':bind(G/'live.stdout'),'oracle':bind(G/'oracle.sc'),'beforeReplay':bind(G/'baseline-eighth/replay.json'),'currentReplay':bind(G/'candidate-v2/replay.json'),'rows':rows},'trace':{f:bind(P/'cdt-trace-ties'/f) for f in ['CdtTraceTies.scala','compile.stdout','compile.stderr','run.json','trace.stdout','trace.stderr','first-prepare-failure.txt']},'ties':{'header_event_tie_after':{'numericOffset':41,'rawExpansionOrder':['main.c MODE1 index1','api.h MODE3 index4'],'stableSortedOrderSame':True,'liveSelectedFirstStub':'main.c:MODE:int(0) CODE=#define MODE 1'},'header_event_tie_before':{'numericOffset':57,'rawExpansionOrder':['api.h MODE3 index2','main.c MODE8 index5'],'stableSortedOrderSame':True,'liveSelectedFirstStub':'api.h:MODE:int(0) CODE=#define MODE 3'}},'globalOrder':{'case':'mixed_global_method_order','sourceInvocations':['first() at40 PICK1','global initializer at90 PICK2L','later() at156 PICK3LL'],'liveStubDefinitionsByFullName':rows[2]['liveMacroMethods'],'requirement':'Resolve ownership once in original source declaration order; render from immutable invocation identity, or an equivalent mechanism. Function-first rendering, duplicate method views, and final scaffold rendering must not independently consume the queue.'},'exactRule':[
'Construct one outer macro expansion event stream per CDT translation unit, including active directive conditions and included-header events. Preserve defining-file metadata and file-local numeric expansion offsets.',
'Stably sort events solely by numeric offset; equal offsets preserve the original TU expansion-location list order. Do not add filename ordering or include-position offsets.',
'Only an eligible original macro-expanded AST node consumes. Remove events from the front while offset<=nodeOffset until the first matching simple macro name. Nonmatching earlier events are discarded; later events stay queued.',
'No consumption during phantom/type discovery, synthetic copied arguments, unsupported recovery expansion, or duplicate graph text rendering. Pinned MacroHandler parent-expansion and lone-LOCAL eligibility rules remain material.',
'The selected event supplies defining file and macro definition CODE. The current AST node supplies return type; current MacroArgumentExtractor result count supplies signature arity. Do not copy old formal count or use old replacement text to lower current expression.',
'First registration wins for each resulting complete full name (file:name:returnType(arity)), not simply macro name.'
], 'counts':{'newProjectsTotal':base['totals']['newProjects']+len(rows),'canonicalLinesIncludingSeparators':base['totals']['canonicalLinesIncludingSeparators']+sum(x['canonicalLinesIncludingSeparators'] for x in rows),'nonemptySelectedRecords':base['totals']['nonemptySelectedRecords']+sum(x['nonemptySelectedRecords'] for x in rows),'baselineExact':base['totals']['baselineExact']+sum(x['exact'] for x in bs['results']),'v2Exact':base['totals']['candidateExact']+sum(x['exact'] for x in cs['results'])},'requiredCorrection':'branch_condition_entry_state PICK stub CODE1 was correct in eighth and live, lost in v1/v2; it remains a blocker until verified repaired. New conditional_before_redefine and earlier_function_condition repeat the same property regression. MODE/BAD mismatches predate this patch but have the same source-grounded queue remedy.','limits':['All complete raw/reference/baseline/v2 graphs are retained. No references were rewritten, filtered or relaxed.','Ties are verified for both source-order directions on two small supplied-header cases; broader equal-offset ordering follows the pinned stable-sort source, not a claim of exhaustive parser coverage.','No root/peer production edits, Cargo tests or whole-project producers were performed. The Java observer compiled only in ignored review storage against existing pinned libraries.','This receipt is a repair handoff, not an acceptance of current candidate source or whole-project correctness.']}
(P/'definition-selection-final.json').write_text(json.dumps(r,indent=2)+'\n');print(json.dumps({'sha256':H(P/'definition-selection-final.json'),'counts':r['counts']},indent=2))
