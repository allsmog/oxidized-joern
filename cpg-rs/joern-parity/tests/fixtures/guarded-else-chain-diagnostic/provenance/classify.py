from pathlib import Path
import re,json,collections,hashlib
p=Path(__file__).resolve().parent;sha=lambda q:hashlib.sha256(q.read_bytes()).hexdigest()
def parse(path):
 text=path.read_text();block=next(b for b in text.split('\n\n') if b.startswith('METHOD NAME=choose '));nodes=[];stack=[]
 for line in block.splitlines():
  depth=(len(line)-len(line.lstrip()))//2;record=line.lstrip();parent=None
  while stack and nodes[stack[-1]]['depth']>=depth:stack.pop()
  if stack:parent=stack[-1]
  node={'index':len(nodes),'address':'choose#'+str(len(nodes)),'depth':depth,'record':record,'parent':parent,'ancestors':list(stack)};nodes.append(node);stack.append(node['index'])
 def basic(rec):return re.sub(r' ORDER=-?\d+','',rec)
 def key(node):
  record=basic(node['record']);label=record.split(' ',1)[0]
  if label in ['METHOD','METHOD_PARAMETER_IN','METHOD_PARAMETER_OUT','METHOD_RETURN','LOCAL']:return record
  if label in ['IDENTIFIER','LITERAL']:
   parent=next((nodes[i] for i in reversed(node['ancestors']) if nodes[i]['record'].split(' ',1)[0] in ['CALL','RETURN']),None)
   return json.dumps([record,basic(parent['record']) if parent else None])
  return record
 bykey=collections.defaultdict(list)
 for n in nodes:n['key']=key(n);bykey[n['key']].append(n['index'])
 facts=[]
 for line in text.splitlines():
  if line.startswith('EDGES|CFG ') or line.startswith('FLOWS|'):
   match=re.fullmatch(r'(.*?) choose#(\d+) -> choose#(\d+)',line)
   if not match:continue
   kind,s,t=match.group(1),int(match.group(2)),int(match.group(3));facts.append({'raw':line,'kind':kind,'source':s,'target':t,'key':(kind,nodes[s]['key'],nodes[t]['key'])})
 used={f[i] for f in facts for i in ['source','target']};ambiguous={key:ids for key,ids in bykey.items() if len(ids)>1 and any(i in used for i in ids)}
 return nodes,facts,ambiguous
reports=[]
for case in ['guarded_chain','ordinary_chain']:
 files={'joern':p/'input'/case/'expected.txt','old7':p/'old7'/case/'actual.txt','current8':p/'current8'/case/'actual.txt'};parsed={v:parse(path) for v,path in files.items()};assert all(not x[2] for x in parsed.values()),{k:x[2] for k,x in parsed.items()}
 facts={v:collections.Counter(f['key'] for f in x[1]) for v,x in parsed.items()};oldmatch=facts['joern']&facts['old7'];curmatch=facts['joern']&facts['current8'];lost=oldmatch-curmatch;gained=curmatch-oldmatch
 def entries(counter):
  out=[]
  for key,count in counter.items():
   row={'count':count,'canonicalSourceOccurrenceKey':key,'variants':{}}
   for variant,(nodes,fs,_) in parsed.items():
    matching=[f for f in fs if f['key']==key];row['variants'][variant]=[{'raw':f['raw'],'source':{**nodes[f['source']],'ancestorRecords':[nodes[i]['record'] for i in nodes[f['source']]['ancestors']]},'target':{**nodes[f['target']],'ancestorRecords':[nodes[i]['record'] for i in nodes[f['target']]['ancestors']]}} for f in matching]
   out.append(row)
  return out
 missing=facts['joern']-facts['current8']
 report={'case':case,'inputSha256':sha(p/'input'/case/'main.c'),'outputHashes':{v:sha(path) for v,path in files.items()},'scope':'All CFG and REACHING_DEF records with both endpoints in standalone choose. Every used endpoint is uniquely identified by complete properties ignoring ORDER; repeated leaves include their nearest CALL/RETURN owner. Full raw records and complete ancestor records are retained.','ambiguousEndpointKeys':0,'formerlyMatchingSourceOccurrenceFactsLost':sum(lost.values()),'sourceOccurrenceFactsGained':sum(gained.values()),'lost':entries(lost),'gained':entries(gained),'currentMissingLiveFacts':entries(missing),'counts':{v:len(x[1]) for v,x in parsed.items()},'nodes':{v:x[0] for v,x in parsed.items()},'allFacts':{v:x[1] for v,x in parsed.items()}}
 reports.append(report);print(case,'lost',sum(lost.values()),'gained',sum(gained.values()),'currentmissing',sum(missing.values()))
(p/'source-occurrence-review.json').write_text(json.dumps(reports,indent=2)+'\n')
