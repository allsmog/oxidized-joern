from pathlib import Path
from datetime import datetime, timezone
import hashlib, json, subprocess
r=Path.cwd();out=r/'.local/primitive-member-implementation-plan-v1';old=r.parent/'joern-oxidized-astra-body-includes';prep=old/'.local/primitive-member-preparation-v2'
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def write(name,v):(out/name).write_text(json.dumps(v,indent=2)+'\n')
def git(*a):return subprocess.check_output(['git',*a],cwd=r)
head=git('rev-parse','HEAD').decode().strip();status=git('status','--porcelain').decode()
assert head=='f235a0f898c4e19fda90998b903f57741b776ec7' and not status
pf=json.loads((prep/'freeze.json').read_text())
for rel,h in pf['files'].items():assert sha(prep/rel)==h
prepared=json.loads((prep/'prepared.json').read_text());assert prepared['newExpectedGraphs']==0 and prepared['producerRuns']==0
source=r/'cpg-rs/cpg-lang-c/src/exact.rs';assert sha(source)=='7ea06ac7361e2580b9d6fc77fdf6fa7e06cbb4b9bb294aa57787c3e8f9ce2da8'
source_lines=source.read_text().splitlines()
source_refs=[]
for line,term,meaning in [(2611,'fn emit_type_decl','Aggregate MEMBER emission; enum path is separate.'),(2654,'let ty = normalize_type','Only proposed base-type selection call site.'),(2674,'tfn: Some','Combines chosen base with existing member_decl_suffix.'),(6931,'fn member_decl_suffix','Retain suffix and macro dimension rules unchanged.'),(7085,'enum TypeRole','Existing roles remain distinct.'),(7123,'fn declaration_type','Potential declaration-role entry, conditional on live qualifier evidence.'),(7132,'fn specifier_type_for_role','Qualifiers and nonprimitive fallback shared with other roles.'),(7157,'fn primitive_type','Existing primitive token renderer; do not change globally for MEMBER-specific evidence.'),(7318,'fn normalize_type','Nonprimitive fallback and historical unsigned-long mapping remain unchanged.')]:
 assert term in source_lines[line-1],(line,source_lines[line-1])
 source_refs.append({'path':'cpg-rs/cpg-lang-c/src/exact.rs','sha256':sha(source),'line':line,'text':source_lines[line-1],'meaning':meaning})
fixture_root=r/'cpg-rs/joern-parity/tests/fixtures'
groups=[]
for name,folder,count,test in [('primitive-roles','primitive-roles',39,'primitive_roles.rs'),('typedef-aggregates','typedef-aggregates/cases',26,'typedef_aggregates.rs'),('array-initializers','array-initializers/cases',69,'array_initializers.rs'),('body-includes','body-includes/cases',14,'body_includes.rs'),('array-dimension-macros','array-dimension-macros/cases',4,'array_dimension_macros.rs'),('body-macro-state','body-macro-state/cases',76,'body_macro_state.rs')]:
 cases=[]
 for directory in sorted((fixture_root/folder).iterdir()):
  if not directory.is_dir() or not (directory/'expected.txt').is_file():continue
  inputs=[p for p in sorted(directory.rglob('*')) if p.is_file() and p.suffix in ['.c','.h']]
  assert inputs,directory
  inputmap={p.relative_to(directory).as_posix():sha(p) for p in inputs}
  # Bind every input byte and exact path; no source text normalization.
  for p in inputs:assert p.read_bytes()==git('show',head+':'+p.relative_to(r).as_posix())
  ref=directory/'expected.txt';assert ref.read_bytes()==git('show',head+':'+ref.relative_to(r).as_posix())
  cases.append({'case':directory.name,'root':directory.relative_to(r).as_posix(),'inputs':inputmap,'referenceSha256':sha(ref)})
 assert len(cases)==count,(name,len(cases))
 testpath=r/'cpg-rs/joern-parity/tests'/test
 groups.append({'family':name,'caseCount':count,'test':testpath.relative_to(r).as_posix(),'testSha256':sha(testpath),'plannedCompleteReplay':True,'cases':cases})
diag=fixture_root/'array-initializers/diagnostics/member_types'
diagfiles={p.relative_to(r).as_posix():sha(p) for p in sorted(diag.iterdir()) if p.is_file()}
for rel,h in diagfiles.items():assert hashlib.sha256(git('show',head+':'+rel)).hexdigest()==h
main={p.relative_to(r).as_posix():sha(p) for p in [r/'cpg-rs/joern-parity/oracle.sc',r/'cpg-rs/joern-parity/oracle_all.txt',r/'cpg-rs/joern-parity/check.sh',r/'cpg-rs/joern-parity/test_check.py']}
write('preservation-inventory.json',{'status':'SOURCE_AND_REFERENCE_BINDINGS_ONLY_NO_REPLAYS','baseline':head,'groups':groups,'groupCaseInstances':sum(x['caseCount'] for x in groups),'memberTypesDiagnostic':diagfiles,'mainGate':main,'caseCountsAreNotGateCounts':True,'diagnosticRule':'Replay all retained references against accepted-tenth baseline and future candidate. Do not call every case exact; body-includes/body-macro-state retain diagnostics beyond their exact production gates.'})
write('source-map.json',source_refs)
plan='''# Primitive MEMBER implementation and validation plan

Preparation only. The new worktree starts at accepted tenth checkpoint `f235a0f898c4e19fda90998b903f57741b776ec7`. Its tracked state is clean and `exact.rs` is the accepted `7ea06ac7…` source. The prior body-includes worktree, all frozen candidates and all fixture packages remain untouched. No source, test, expected graph or runtime driver is edited here. No compiler, graph producer or oracle has run for this unit.

## Decision required from the fresh references

The preserved preparation has 17 new projects and two unchanged tiny-fixedtables anchors, with 20 source files. Ten isolate primitive order, width, sign and scalar qualifier spellings. Seven separate numeric primitives, scalar struct/union/typedef fallbacks, pointer and array suffixes, base-versus-pointer qualification, and multiple declarators. Both anchors retain their existing full references. Every new expected value remains unset; see the original `prepared.json` bound in this plan receipt.

After parent admission, verify the exact release receipt, complete raw CASE framing, unchanged input/oracle/runtime bindings and both anchor matches. Bind the whole selected AST/NODES/EDGES/FLOWS reference for every project before deriving a type expectation. Compare raw MEMBER NAME, CODE, TYPE_FULL_NAME and ORDER by aggregate and field occurrence, plus the complete type registration and incident edges. Keep selected alias records and existing alias assertions; do not change the canonical projection to add an edge kind. Ordinary method records are a separate unaffected control.

The live qualifier results decide the implementation. If all measured MEMBER bases and sibling qualifiers agree with the existing declaration-role renderer, the preferred edit is to use that renderer only at `emit_type_decl`'s field base selection. If MEMBER qualifiers differ, use the measured primitive base conversion at that one site with the existing nonprimitive fallback; do not change shared declaration, return or expression semantics to force agreement. A new MEMBER-specific helper/role is justified only by an observed distinction that cannot be expressed clearly at the call site. No choice or new spelling is asserted before the fresh graphs are released.

## Source boundary

`exact.rs:2654` currently selects the field base through `normalize_type` using only the type child's source text. `exact.rs:2674` combines it with the existing `member_decl_suffix`. The shared `declaration_type` and `specifier_type_for_role` entry points are at lines 7123 and 7132; the primitive renderer is at 7157. The shared qualifier path presently treats sibling qualifiers differently from nested pointer qualifiers. Its existing behavior in other roles is evidence to preserve, not an assumed MEMBER contract. The explicit nonprimitive fallback is `normalize_type` at 7318.

Keep MEMBER names, raw declarator CODE, order and placements unchanged. Keep `member_decl_suffix` at 6931, the `<clinit>` path, enum emission, aggregate/tag/alias identity, macro environments, source positions and parser recovery unchanged. Do not edit global `normalize_type`, `primitive_type` or the TypeRole variants unless the fresh evidence establishes a necessary shared change and that expanded scope is reviewed first. No field-expression inference, pointer/array declarator repair, bitfield support, macro-defined base type support, typedef-underlying repair, importer/solver/schema change or formatting normalization is included.

## Freeze and complete replay

Once parent releases the references and source work, create a unique candidate freeze before any meaningful replay. Record all Rust/Cargo inputs before and after compilation and both frozen binary hashes. Use only the coordinated shared target at `astra-nested-casts/.local/target`; never root's target. Keep every unsuccessful candidate, exact command, exit code, raw stdout/stderr, input inventory and complete expected/before/candidate diff. No source edit may overlap a recorded build or replay.

For the new 19-project batch, run the accepted-tenth frozen producer and candidate against identical source bytes and filenames, then perform complete projection comparisons. Report exact projects separately from diagnostic projects. Use multiplicity-aware raw record counters including a stated treatment of blank separators. For every lost previously matching record, retain the full before/current/oracle node and ancestor context; distinguish an ordinal coincidence from a genuine lost source property or edge. A project that was already nonexact can still contain a blocking property regression. Never replace the references or suppress sections to improve exactness.

The two tiny-fixedtables anchors are already present in the old 18-project inventory. Preserve both artifact families; a producer replay may be reused only when full filename-plus-source inventory hashes agree, and the receipt must identify that reuse. Do not double-count them as new production gates. Keep complete methods and external/scaffold records visible even if the seven previously observed type-spelling differences close.

## Retained validation

`preservation-inventory.json` binds 228 existing case instances plus the separate `member_types` diagnostic. These counts describe preserved references, not unique inputs or passing gates.

- Run all 39 complete primitive-role graphs and all three existing `primitive_roles.rs` tests. Their parameter, function return, call-expression and function-pointer distinctions must remain exact; retain existing references and source positions.
- Run all 26 complete typedef-aggregate graphs and their five production tests. Inspect `qualified_pointer_member`, `multidimensional`, `ordinary_multidimensional`, `header_size`, `function_macro_size`, `macro_expression`, `macro_parenthesized`, `dimension_parenthesized`, alias and tag cases explicitly. Preserve declaration order, source-position macro sizes, all initializer dimensions and existing tag/alias assertions.
- Run the 69 complete array-initializer graphs and their existing production tests, including `parameter_types`, pointer/array/parenthesized controls and source-location assertions. Keep the old `diagnostics/member_types` source, full live expected graph, historical baseline/current outputs and diff unchanged. Produce a new accepted-tenth baseline and candidate replay for that diagnostic; historical candidate bytes are not assumed to be the current baseline. Its pointer/array suffix ordering and missing initializer are outside this primitive-base edit.
- Replay every complete project in the prior 18-project union (14 body-includes plus four array-dimension cases) and all 76 body-macro-state projects, including every retained diagnostic and the duplicate-clinit nonzero producer outcome. Run existing body-includes full gates, three complete supported METHOD subtrees and both macro-owner checks, the four complete dimension gates, and ninth's 54 full gates plus two complete ARG METHOD assertions. Do not infer full-family preservation from the exact tests alone; analyze complete accepted-tenth/candidate outputs for all 94 project instances.
- Run the unchanged main 308 gate with the existing checker and oracle. Run formatting and strict Clippy appropriate to the patch, then the parent-owned full workspace/live/native/whole-project/resource campaign after independent source/package review and integration. Do not duplicate that final campaign in the worker unless a concrete concern requires it.

A future `primitive_members.rs` regression file should build a fresh CPG per admitted case and assert the full canonical graph for each proven exact result. Keep complete nonexact references as diagnostics; any narrower MEMBER or scaffold assertion must be clearly labeled and accompany the full preserved graph. Its number of tests and exact cases are pending the live result. Avoid tests that merely restate the helper's implementation. The existing 39-role and suffix suites provide meaningful regression coverage for accidental shared effects.

## Handoff and stop conditions

Deliver an inert source patch against this exact accepted checkpoint, the new full-reference fixture/test package, concise observed-quirk documentation if needed, all frozen candidate/status bindings and independent review. Parent owns root integration, cache decisions, manifest expectations, commits and final acceptance. Stop source expansion and preserve the candidate if qualifiers need unmeasured behavior, prior-correct fields or edges are lost, source/runtime bindings drift, or a retained lowerer diagnostic would require a separate feature. Do not claim whole zlib/fixedtables or language parity from this unit.
'''
(out/'PLAN.md').write_text(plan)
assert git('status','--porcelain').decode()==''
for rel,h in pf['files'].items():assert sha(prep/rel)==h
write('preflight.json',{'status':'PASS_READ_ONLY_PLAN_BINDINGS','atUtc':datetime.now(timezone.utc).isoformat(),'baselineCommit':head,'worktree':str(r),'trackedStateCleanBeforeAndAfter':True,'originalPreparation':str(prep),'originalPreparationFreezeSha256':sha(prep/'freeze.json'),'originalPreparationFilesUnchanged':len(pf['files']),'sourceSha256':sha(source),'sourceCitationsVerified':len(source_refs),'retainedReferenceCaseInstances':sum(x['caseCount'] for x in groups),'memberTypesDiagnosticFiles':len(diagfiles),'parentReleaseRequiredBeforeSourceEdits':True,'newExpectedValues':None,'producerRuns':0,'builds':0,'sourceTestOrFixtureEdits':False})
files={p.name:sha(p) for p in sorted(out.iterdir()) if p.is_file()}
write('freeze.json',{'status':'FROZEN_IMPLEMENTATION_PLAN_PRODUCERS_HELD','files':files,'fileCount':len(files),'baselineCommit':head})
print(json.dumps({'path':str(out),'freezeSha256':sha(out/'freeze.json'),'planSha256':sha(out/'PLAN.md'),'preflightSha256':sha(out/'preflight.json'),'files':len(files),'existingCaseInstances':sum(x['caseCount'] for x in groups)},indent=2))
