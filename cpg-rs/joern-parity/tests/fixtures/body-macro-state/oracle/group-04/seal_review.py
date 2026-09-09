from pathlib import Path
import hashlib,json,re,difflib
P=Path(__file__).resolve().parent
W=Path('/Users/shayaunnejad/vibe-code/.codex-worktrees')
PEER=W/'joern-oxidized-astra-body-macro-state/.local/body-macro-state'
OWN=P.parent/'ninth-body-state-review'
H=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
def bind(p):return {'path':str(p.resolve()),'sha256':H(p)}
def methods(p):
 out={};cur=None
 for l in p.read_text().splitlines():
  if l.startswith('METHOD '):
   cur=l.rsplit(' FULL_NAME=',1)[1].split(' SIGNATURE=',1)[0].split(' ORDER=',1)[0];out[cur]=[l]
  elif cur and l:out[cur].append(l)
  else:cur=None
 return out
r={'status':'REQUIRES_METADATA_SELECTION_REPAIR','scope':'Independent read-only macro METHOD definition-ownership diagnosis on frozen v1/v2; no full source acceptance, no whole-project producers or Rust builds.',
 'candidateV1':{f:bind(PEER/'candidate-v1'/f) for f in ['exact.rs','joern-parity']},'candidateV2':{f:bind(PEER/'candidate-v2'/f) for f in ['exact.rs','joern-parity']},'groups':[],'originalThree':[]}
for group in [P,P/'ownership-boundaries',P/'nested-events',P/'arity-order']:
 live=json.loads((group/'live-run.json').read_text());before=json.loads((group/'baseline-eighth/replay.json').read_text());after=json.loads((group/'candidate-v2/replay.json').read_text())
 assert live['exitCode']==0
 raw={};case=None
 for l in (group/'live.stdout').read_text().splitlines():
  if l.startswith('CASE|'):case=l[5:];assert case not in raw;raw[case]=[]
  elif case and l.startswith(('AST|','NODES|','EDGES|','FLOWS|')):raw[case].append(l[4:] if l.startswith('AST|') else l)
 names=json.loads((group/'cases.json').read_text());assert set(raw)==set(names)
 rows=[]
 for name,fs in names.items():
  inp=group/'input'/name;expected=inp/'expected.txt';assert expected.read_text()=='\n'.join(raw[name])+'\n'
  for f in fs:assert H(inp/f)==live['inputHashes'][name][f]
  bm=methods(group/'baseline-eighth'/name/'actual.txt');am=methods(group/'candidate-v2'/name/'actual.txt');lm=methods(expected)
  macro_rows=[]
  for full,lines in lm.items():
   if ' CODE=#define ' not in lines[0]:continue
   macro_rows.append({'fullName':full,'liveMethod':lines,'baselineMethod':bm.get(full),'candidateMethod':am.get(full),'introducedCorrectMethodRecordLoss':bm.get(full,[None])[0]==lines[0] and am.get(full,[None])[0]!=lines[0]})
  rows.append({'case':name,'inputs':{f:bind(inp/f) for f in fs},'expected':bind(expected),'before':bind(group/'baseline-eighth'/name/'actual.txt'),'current':bind(group/'candidate-v2'/name/'actual.txt'),'completeDifference':bind(group/'candidate-v2'/name/'complete.diff'),'canonicalLinesIncludingSeparators':len(raw[name]),'nonemptySelectedRecords':sum(bool(l) for l in raw[name]),'macroMethods':macro_rows})
 r['groups'].append({'path':str(group),'liveRun':bind(group/'live-run.json'),'raw':bind(group/'live.stdout'),'oracle':bind(group/'oracle.sc'),'beforeReplay':bind(group/'baseline-eighth/replay.json'),'currentReplay':bind(group/'candidate-v2/replay.json'),'baselineExact':sum(x['exact'] for x in before['results']),'candidateExact':sum(x['exact'] for x in after['results']),'rows':rows})
for name,full in [('branch_condition_entry_state','main.c:PICK:int(0)'),('header_after_body_undef','main.c:MODE:int(0)'),('recovery_after_redefinition','main.c:BAD:char*(0)')]:
 files={'live':OWN/'input'/name/'expected.txt','eighth':OWN/'baseline-eighth'/name/'actual.txt','v1':PEER/'replay-v1'/name/'actual.txt','v2':PEER/'replay-v2'/name/'actual.txt'}
 blocks={k:methods(p)[full] for k,p in files.items()}
 q=P/'original-three'/name;q.mkdir(parents=True,exist_ok=True)
 for k,lines in blocks.items():(q/(k+'.method.txt')).write_text('\n'.join(lines)+'\n')
 (q/'eighth-v2-complete.diff').write_text(''.join(difflib.unified_diff(files['eighth'].read_text().splitlines(True),files['v2'].read_text().splitlines(True),fromfile='eighth',tofile='v2')))
 r['originalThree'].append({'case':name,'fullName':full,'fullGraphs':{k:bind(p) for k,p in files.items()},'fullMacroMethodBlocks':blocks,'introducedCorrectCodeLoss':blocks['live'][0]==blocks['eighth'][0] and blocks['live'][0]!=blocks['v2'][0],'completeBeforeCurrentDifference':bind(q/'eighth-v2-complete.diff')})
r['totals']={'newProjects':sum(len(g['rows']) for g in r['groups']),'canonicalLinesIncludingSeparators':sum(x['canonicalLinesIncludingSeparators'] for g in r['groups'] for x in g['rows']),'nonemptySelectedRecords':sum(x['nonemptySelectedRecords'] for g in r['groups'] for x in g['rows']),'baselineExact':sum(g['baselineExact'] for g in r['groups']),'candidateExact':sum(g['candidateExact'] for g in r['groups']),'newIntroducedCorrectMacroMethodLossCases':[x['case'] for g in r['groups'] for x in g['rows'] if any(y['introducedCorrectMethodRecordLoss'] for y in x['macroMethods'])]}
r['upstream']={}
for f,path in [('MacroHandler.scala','astcreation/MacroHandler.scala'),('AstCreationPass.scala','passes/AstCreationPass.scala'),('AstCreator.scala','astcreation/AstCreator.scala'),('CdtParser.scala','parser/CdtParser.scala')]:r['upstream'][f]={**bind(P/'upstream'/f),'url':'https://raw.githubusercontent.com/joernio/joern/v4.0.555/joern-cli/frontends/c2cpg/src/main/scala/io/joern/c2cpg/'+path}
r['sourceRules']=[{'source':'MacroHandler.scala','lines':'21-28','fact':'TU macro expansion locations sorted by numeric file-local offset only; filename is not a sort key.'},{'source':'MacroHandler.scala','lines':'61-76','fact':'Eligible node consumes and discards prior nonmatching events until first same macro name at offset <= node offset.'},{'source':'MacroHandler.scala','lines':'97-110,130-169','fact':'Selected definition supplies CODE and defining file; current node and MacroArgumentExtractor supply result type and actual argument count.'},{'source':'AstCreationPass.scala','lines':'34-36','fact':'getOrElseUpdate preserves first method registration by complete full name.'},{'source':'AstCreator.scala','lines':'76-93','fact':'Joern creates source declaration ASTs in source order; Rust function-before-global rendering must not reorder metadata selection.'}]
r['trace']={f:bind(P/'cdt-trace'/f) for f in ['CdtTrace.scala','compile.json','compile.stdout','compile.stderr','run.json','trace.stdout','trace.stderr','nested-run.json','nested.stdout','nested.stderr']}
r['trace']['classes']={str(f.relative_to(P/'cdt-trace')):bind(f) for f in (P/'cdt-trace/classes').rglob('*.class')}
r['trace']['failedDirectScriptAttempt']={f:bind(P/f) for f in ['trace.sc','trace-run.json','trace.stdout','trace.stderr','trace-producer.log']}
r['trace']['failureExplanation']='Initial normal Joern REPL classpath lacked C2Cpg/CDT classes, so the trace script failed to compile; retained unchanged. Successful trace compiled a local observer against the already pinned distribution jars; no producer/library/source changes.'
r['trace']['selectedObservations']=[
'MODE3 header condition appears at api.h file-local offset4 before main.c MODE1@41: selected old header event explains live main.c MODE CODE3 despite before() expansion literal1.',
'PICK1 condition event precedes later PICK2 body event; first normal wrapper consumes PICK1, fixing the introduced CODE regression.',
'BAD malformed first initializer has events BAD@176 and VALUE@181 but recovery creates no wrappers to consume them; later normal BAD@241 can consume earlier BAD definition.',
'WRAP(ARG) contributes only outer WRAP event, excluding macros nested in argument/replacement.',
'defined(FLAG) && PICK(FLAG) contributes only PICK; defined operand and nested argument are excluded.',
'#if FLAG within inactive #if0 contributes no event; active #if0 && FLAG does contribute FLAG despite its false result.'
]
r['requiredFixes']=['Restore previously correct PICK stub CODE in branch_condition_entry_state and the fresh conditional/earlier-function controls; do not label this introduced loss a retained diagnostic.','Keep expression snapshots and metadata ownership separate. Build outer expansion events in each TU, including active directive conditions and supplied-header local offsets; replay destructive name-matching only for eligible original macro wrappers.','Use selected definition CODE/file only: current invocation argument count and current expansion result type determine signature. Retain first registration by full name.','Preserve original recovery/copy/temporary overrides and their non-consumption behavior; do not consume events in phantom/type discovery or duplicate rendering.','Validate source declaration ordering independently of functions-before-global output, and retain malformed/function-condition/transport diagnostics without filtering.']
r['limits']=['This is a definition-ownership diagnosis, not final acceptance of frozen v2.','The old malformed function-like #if selection gap in formal_list_redefined/condition_function_and_defined remains a full graph diagnostic; supported object_to_function isolates arity.','No full port percentage, whole-project safety, memory budget or source-location parity is inferred.','All original17/first7 references and producer binaries remained unchanged.']
(P/'definition-selection-review.json').write_text(json.dumps(r,indent=2)+'\n')
print(json.dumps({'sha256':H(P/'definition-selection-review.json'),'totals':r['totals']},indent=2))
