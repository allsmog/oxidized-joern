from pathlib import Path
import json,hashlib,re
O=Path(__file__).parent
P=Path('/Users/shayaunnejad/vibe-code/.codex-worktrees/joern-oxidized-astra-body-includes/.local/body-includes')
S=P/'frozen-v2/source/cpg-rs/cpg-lang-c/src'
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def bind(p):return {'path':str(p),'sha256':sha(p),'bytes':p.stat().st_size}
v=json.loads((O/'verification.json').read_text());ret=json.loads((O/'retention-verification.json').read_text())
repeat=next(x for x in v['rows'] if x['case']=='repeated_include_context');data={k:Path(repeat[k]['path']).read_text() for k in ['live','baseline','candidate']}
first_facts={k:[x for x in s.split('\n') if x.startswith(('EDGES|','FLOWS|')) and re.search(r'(?: |-> )first#\d+(?: |$)',x)] for k,s in data.items()}
from collections import Counter
assert list((Counter(first_facts['live'])-Counter(first_facts['candidate'])).elements())==['EDGES|REF main.c:<global>#80 -> first#0']
assert list((Counter(first_facts['candidate'])-Counter(first_facts['live'])).elements())==['EDGES|REF main.c:<global>#84 -> first#0']
def path_to(text,key,index):
 lines=next(block.split('\n') for block in text.split('\n\n') if block.startswith('METHOD ') and (' FULL_NAME='+key+' ') in block.split('\n')[0]);stack=[]
 for i,line in enumerate(lines[:index+1]):
  depth=(len(line)-len(line.lstrip()))//2
  while stack and stack[-1][0]>=depth:stack.pop()
  stack.append((depth,i,line))
 return [{'ordinal':i,'record':l} for _,i,l in stack]
first_ref_context={'live':path_to(data['live'],'main.c:<global>',80),'candidate':path_to(data['candidate'],'main.c:<global>',84)}
assert [x['record'] for x in first_ref_context['live']]==[x['record'] for x in first_ref_context['candidate']]
nested=next(x for x in v['rows'] if x['case']=='nested_scope_typedef_macro');nt=Path(nested['candidate']['path']).read_text();alias=[x for x in nt.split('\n') if x.startswith('NODES|TYPE_DECL ') and re.search(r' FULL_NAME=Local(?:<duplicate>0)? ',x)]
assert len(alias)==2 and all('FILENAME=scope.h ' in x for x in alias)
closure=json.loads((O/'ownership-closure.json').read_text());closure['firstSelectedFacts']=first_facts;closure['oneIncomingRefAddressShiftWithIdenticalContext']=first_ref_context;closure['nestedTypedefEntireProjectionExact']=True;closure['nestedTypedefPhysicalOriginRecords']=alias
(O/'ownership-closure.json').write_text(json.dumps(closure,indent=2)+'\n')
v['bindings']=[bind(Path(x['path'])) if Path(x['path'])==O/'ownership-closure.json' else x for x in v['bindings']]
(O/'verification.json').write_text(json.dumps(v,indent=2)+'\n')
md='''# Frozen body-include V2 review

PASS for the bounded source and saved-reference review. No concrete introduced semantic loss was identified. This does not accept a final package, whole-project behavior, resource budget, or a complete port.

The frozen candidate is exact.rs `1b23a6834a9c68ae7c1639c3d5f92116bf2eaeb496af2b9ee32acc6e0605b750`, import.rs `f7c20320c10edf3597f921dbc5cf43bed7401856737418985fd3c12042d43ee2`, lib.rs `b0f413451969d4f62a08f451e0e5a7fd63cb9f7a11540deb0e2dd58694e8e968`, and parity binary `589ad4d85fb44e8b0b54c71731bb412bcf9aee271a84a1721d41dc8489c81fb4`. The build binding covers 190 files: three frozen changed source copies and 187 byte-identical accepted baseline blobs. The review read these immutable sources and saved outputs; it ran no Rust/Joern producer or build and changed no implementation.

## Complete reference and loss checks

All 12 original fresh references and all 24 input files remain byte-identical. Complete exact projects improve from 2 to 8, with no old exact project lost. Multiplicity accounting finds 1,201 gained live-matching records and two lost serialized records. All complete added/removed and matching record lists are retained per project. The unchanged raw oracle and its earlier extraction review are bound, including all AST separators.

Both genuine V1 macro ownership losses close: `nested/table.h:N:int(0)` again has CONTAINS from the header global TYPE_DECL and SOURCE_FILE to `nested/table.h`. Its complete METHOD record remains exactly the live/baseline record. The `first` METHOD AST is exact. All its selected edges/flows match except the incoming global METHOD_REF address shifting 80→84; the complete source-node/ancestor path is identical and retained. `nested_scope_typedef_macro` is fully exact, with Local and Local<duplicate>0 TYPE_DECL filenames both `scope.h`, and the first identity emitted inside the caller before the standalone header duplicate.

The two remaining lost raw rows are `EVAL_TYPE second#12 -> T:ANY` and `EVAL_TYPE second#13 -> T:int`. They were accidental ordinal matches: baseline #12/#13 are the last_table identifier/literal under the return expression, while live #12/#13 are first_table's initializer call/literal. Complete endpoint and ancestor paths from live/baseline/V2 are retained in ownership-closure.json. They do not establish loss of the same source-occurrence fact.

All 76 accepted ninth projects are also bound and checked offline against their original source/reference hashes. Exact projects improve 54→55. Across the 75 successful pairs, matching records gain 25 and lose zero. duplicate_clinit_tag still fails on the same duplicate-method-properties assertion (a.c:N versus b.c:N), with empty stdout; actual exits are accepted release −6 and V2 debug 101. It is not counted as a successful graph comparison.

The three independent origin controls have exact complete selected projections. Their separate raw Joern metadata and the saved production line test are bound. The test checks header initializer/local line 1, following caller call line 4, return line 5, and absence of invented SOURCE_FILE edges. This is bounded line evidence, not complete metadata parity: source columns and unusual CDT end/anchor coordinates remain explicitly retained.

## Source assessment

- IncludedBodyView borrows the original supplied unit tree, bytes, and filename for one include occurrence. `(parent view, include node)` identifies nested/repeated occurrences; view-aware keys protect type snapshots, recovery caches, macro-use deduplication, and typedef identity. Tree-root and byte ownership guards reject temporary trees. Contexts are constructed while all source units are alive, and consumers share those views rather than preprocessing the include a second time.
- Emitter, prototype discovery, type-site traversal, declaration discovery, and phantom discovery all switch to the same selected header item list and physical bytes. Emission shares the caller's running ORDER and symbol scope; a header typedef stays within its surrounding C block while preprocessor state persists. Inactive declarations/directives remain separated as measured by the controls.
- Macro metadata uses a lexical include-path ordering for actual uses, while the pinned expansion queue still sorts numeric file-local offsets with stable ties. Duplicate renderings share one use. Complete MethodInfo registration gives a generated .h-pass entry priority over a source-pass entry, matching pinned C2Cpg/AstCreationPass accumulator merge behavior. Temporary-tree registration uses the same priority helper. Queue exhaustion still falls back to current-use metadata: this retained limitation is not full MacroHandler equivalence.
- Typedef reservation transfer targets the exact physical header declarator and only a still-unemitted identity, checking placements first. Standalone emission then reads its updated reservation. This closes the measured local/header identity ordering without renaming an already emitted alias. General cross-TU duplicate aggregate/tag identity remains outside this bounded proof.
- IncludedSourceOrigin carries owned block/range/file/span values; no tree borrow escapes canonical lowering. The importer resolves origin roots while address/raw maps are alive, deduplicates mirrored method views, skips foreign child roots during caller token matching, then locates those roots in physical header spans. Nested origins skip each other during ancestor traversal. Caller graph ownership and physical line anchoring remain separate. Public canonical text import without this side channel retains its existing interface and has no promise of included-header line recovery.
- Array dimension TYPE spelling now expands the declaration's own macro environment. All original controls and 76 prior projects retain old exactness; no normalization/filter is applied to the retained primitive-word-order differences.

## Remaining diagnostics

`repeated_include_context` still emits N wrappers for second()'s first_table literals where live queue exhaustion emits plain literals. Its standalone header still lacks the preexisting N phantom. `tiny_fixedtables_include` now has the correct included method but shares the unchanged seven-record `shortunsigned` versus `unsigned short` primitive scaffold family with the byte-identical inline control. `unresolved_header_controls` retains the preexisting missing quoted-decoy resolution; only its direct include-derived global ORDER improves. These four complete projects remain nonexact and are not weakened or filtered.

No additional producer is needed to establish the repairs above. Parent-owned whole-project and resource gates, plus the final test/fixture package review, remain outstanding.
'''
(O/'review.md').write_text(md)
prior=O.parent/'tenth-body-include-v1-review'
refs=O.parent/'tenth-body-include-reference-review'
result={'status':'PASS_BOUNDED_FROZEN_V2_SOURCE_AND_SAVED_REFERENCE_REVIEW','sources':{n:bind(S/n) for n in ['exact.rs','import.rs','lib.rs']},'binary':bind(P/'frozen-v2/joern-parity'),'binding':bind(P/'frozen-v2/bindings.json'),'original12':{k:v[k] for k in ['projects','baselineExact','candidateExact','formerlyExactLost','rawMatchingGained','rawMatchingLost']},'genuineV1OwnershipLossesRestored':2,'remainingRawOrdinalCollisions':2,'prior76':{'projects':76,'successfulPairs':75,'baselineExact':54,'candidateExact':55,'matchingGained':25,'matchingLost':0,'failure':{'case':'duplicate_clinit_tag','baselineExit':-6,'candidateExit':101,'sameAssertion':'duplicate method node properties drift; a.c:N:int(0) vs b.c:N:int(0)'}},'originCompleteExact':3,'evidenceChecks':v['checks']+ret['checks'],'heldV1Receipt':bind(prior/'final-review.json'),'pinnedMacroPassEvidence':bind(prior/'pass-order/final-review.json'),'originalRawReferenceReview':bind(refs/'final-review.json'),'originRawReview':ret['originReferenceReceipt'],'readableReview':bind(O/'review.md'),'artifacts':[bind(p) for p in sorted(O.iterdir()) if p.is_file() and p.name!='final-review.json'],'limitations':['Four original projects remain fully recorded nonexact.','No source column/full metadata parity claim.','No parent package, whole-project, resource, or full Joern port acceptance.'],'noProducersBuildsOrSourceEdits':True}
(O/'final-review.json').write_text(json.dumps(result,indent=2)+'\n')
print('final-review.json',sha(O/'final-review.json'));print('review.md',sha(O/'review.md'));print('checks',result['evidenceChecks'])
