from pathlib import Path
import subprocess,json,os,hashlib,time,shutil,difflib,datetime
p=Path(__file__).resolve().parent
roots=Path('/Users/shayaunnejad/vibe-code/.codex-worktrees')
owner=roots/'joern-oxidized-astra-body-macro-state/.local/body-macro-state'
peer=roots/'joern-oxidized-astra-typedef-aggregates/.local/ninth-macro-definition-review'
root=roots/'joern-oxidized-astra-sprint'
sha=lambda f:hashlib.sha256(Path(f).read_bytes()).hexdigest()
def bind(f):return {'path':str(f),'sha256':sha(f),'bytes':Path(f).stat().st_size}
def save(f,j):
 with f.open('x') as o:json.dump(j,o,indent=2);o.write('\n')
cases={
'ordinary_control':'#define ARG 1\nint consume(int value);\nint sample(void) { consume(ARG);\n#undef ARG\n#define ARG 2\nreturn ARG; }\n',
'object_call':'#define CALL consume\n#define ARG 1\nint consume(int value);\nint sample(void) { return CALL(ARG); }\n',
'object_redefinition':'#define CALL consume\n#define ARG 1\nint consume(int value);\nint sample(void) { CALL(ARG);\n#undef ARG\n#define ARG 2\nreturn ARG; }\n',
'function_wrap_redefinition':'#define WRAP(x) consume(x)\n#define ARG 1\nint consume(int value);\nint sample(void) { WRAP(ARG);\n#undef ARG\n#define ARG 2\nreturn ARG; }\n',
'object_function_alias':'#define CALL WRAP\n#define WRAP(x) consume(x)\n#define ARG 1\nint consume(int value);\nint sample(void) { CALL(ARG);\n#undef ARG\n#define ARG 2\nreturn ARG; }\n',
'object_nested_argument':'#define CALL consume\n#define WRAP(x) (x)\n#define ARG 1\nint consume(int value);\nint sample(void) { CALL(WRAP(ARG));\n#undef WRAP\n#define WRAP(x) ((x)+1)\nreturn WRAP(ARG); }\n'
}
for name,code in cases.items():
 d=p/'input'/name;d.mkdir(parents=True);(d/'main.c').write_bytes(code.encode())
shutil.copy2(owner/'nested-ranges/oracle.sc',p/'oracle.sc')
v4=owner/'candidate-v4';baseline=root/'.local/astra-sprint/eighth-batch/final-real-differential/bin/joern-parity'
assert sha(v4/'exact.rs')=='35dd9430be3309294dc84aa7e76796f9997c4ff0845a2f56013aa3e01513b096'
assert sha(v4/'joern-parity')=='5584f11047e1358ef7645386cfe79244787cd5cd9e942645e38e2540f7304673'
assert sha(baseline)=='81a9d4c8c078496c2dc57b863cea8fb2a9538d72882408df08ca217dd7af6d9b'
save(p/'setup.json',{'status':'HELD_V4_BOUNDARY_REVIEW_NOT_ACCEPTANCE','time':datetime.datetime.now(datetime.timezone.utc).isoformat(),'source':bind(v4/'exact.rs'),'binary':bind(v4/'joern-parity'),'ownerReceipt':bind(v4/'bindings.json'),'buildLog':bind(owner/'build-v4.log'),'baseline':bind(baseline),'inputs':{n:bind(p/'input'/n/'main.c') for n in cases},'script':bind(p/'oracle.sc'),'traceSource':bind(peer/'cdt-trace/CdtTrace.scala'),'traceRun':bind(peer/'cdt-trace/run.json'),'upstream':bind(peer/'upstream/MacroHandler.scala')})
def run(cmd,folder,env=None):
 folder.mkdir();t=time.monotonic()
 with (folder/'stdout').open('xb') as o,(folder/'stderr').open('xb') as e:r=subprocess.run(cmd,cwd=folder,env=env,stdout=o,stderr=e,timeout=180)
 receipt={'command':cmd,'cwd':str(folder),'exitCode':r.returncode,'seconds':time.monotonic()-t,'stdout':bind(folder/'stdout'),'stderr':bind(folder/'stderr')};save(folder/'run.json',receipt);assert r.returncode==0
 return (folder/'stdout').read_bytes()
tracecmd=json.loads((peer/'cdt-trace/run.json').read_bytes())['command'];tracecmd[-1]=str(p/'input');run(tracecmd,p/'trace')
env=dict(os.environ);env['JAVA_HOME']='/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home';env['PATH']=env['JAVA_HOME']+'/bin:'+env['PATH']
raw=run(['/Users/shayaunnejad/vibe-code/joern-oxidized/.local/astra-sprint/oracle/runtime/joern-cli/joern','--script',str(p/'oracle.sc'),'--param','inputPath='+str(p/'input')],p/'oracle-workspace',env)
sections={};name=None
for line in raw.split(b'\n'):
 if line.startswith(b'CASE|'):name=line[5:].decode();sections[name]=[]
 elif line.startswith((b'AST|',b'NODES|',b'EDGES|',b'FLOWS|')):
  assert name is not None;sections[name].append(line[4:] if line.startswith(b'AST|') else line)
assert set(sections)==set(cases)
for n,lines in sections.items():(p/'input'/n/'expected.txt').write_bytes(b'\n'.join(lines)+b'\n')
rows=[]
for variant,binary in [('accepted-eighth',baseline),('held-v4',v4/'joern-parity')]:
 for n in sorted(cases):
  d=p/variant/n;d.parent.mkdir(exist_ok=True);actual=run([str(binary),str(p/'input'/n/'main.c')],d);expected=(p/'input'/n/'expected.txt').read_bytes()
  diff=''.join(difflib.unified_diff(expected.decode().split('\n'),actual.decode().split('\n'),fromfile='pinned-Joern',tofile=variant,lineterm='\n'))
  (d/'complete.diff').write_bytes(diff.encode());row={'variant':variant,'case':n,'exact':actual==expected,'expected':bind(p/'input'/n/'expected.txt'),'actual':bind(d/'stdout'),'diff':bind(d/'complete.diff'),'run':bind(d/'run.json')};rows.append(row);print(variant,n,row['exact'],len(diff),flush=True)
save(p/'replay.json',{'results':rows,'oracleRun':bind(p/'oracle-workspace/run.json'),'traceRun':bind(p/'trace/run.json'),'sourceUnchanged':sha(v4/'exact.rs'),'binaryUnchanged':sha(v4/'joern-parity')})
