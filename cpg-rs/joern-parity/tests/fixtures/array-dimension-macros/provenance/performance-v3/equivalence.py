from itertools import product
from pathlib import Path
import json,hashlib
OUT=Path(__file__).resolve().parent
# Inputs model exactly the only fields rd_args reads: child index, has_arg,
# and whether its label is FIELD_IDENTIFIER. Duplicate indices/nodes remain.
child_options=list(product([-2,-1,0,1,2],[False,True],[False,True]))
query=[-3,-2,-1,0,1,2,3]
cases=checks=0
for count in range(4):
 for children in product(child_options,repeat=count):
  cases+=1
  args=sorted([x for x in children if x[1] and not x[2]],key=lambda x:x[0])
  old=[pair for idx,_,_ in args if idx!=0 for pair in [(idx,idx),(idx,-1)]]
  def member(idx):
   return idx!=0 and any(c[1] and not c[2] and c[0]==idx for c in children)
  nonempty=any(c[1] and not c[2] and c[0]!=0 for c in children)
  for src in query:
   assert any(a==src for a,b in old)==member(src)
   assert any(b==src for a,b in old)==(member(src) or (src==-1 and nonempty))
   checks+=2
   for dst in query:
    assert ((src,dst) in old)==(member(src) and (dst==src or dst==-1))
    checks+=1
report={'status':'PASS_OFFLINE_BOUNDED_FORMULA_EQUIVALENCE','childLists':cases,'predicateChecks':checks,'listLengths':[0,1,2,3],'childIndices':[-2,-1,0,1,2],'queries':query,'hasArgValues':[False,True],'fieldIdentifierValues':[False,True],'duplicateIndicesPreserved':True,'testedFunctions':['used','defined','has_flow'],'bound':'Exhaustive small-model equivalence to the current vector construction; no Rust implementation or graph producer executed. Mathematical formula extends to arbitrary finite lists because callers ask only existential membership.','scriptSha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest()}
(OUT/'equivalence.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report))
