from pathlib import Path
import subprocess,os,time,json,hashlib
p=Path(__file__).resolve().parent;w=p.parents[2];sha=lambda q:hashlib.sha256(q.read_bytes()).hexdigest();cases=json.loads((p/'cases.json').read_text());env=dict(os.environ);env['JAVA_HOME']='/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home';env['PATH']=env['JAVA_HOME']+'/bin:'+env['PATH'];cwd=p/'oracle-workspace';cwd.mkdir(exist_ok=True);cmd=['/Users/shayaunnejad/vibe-code/joern-oxidized/.local/astra-sprint/oracle/runtime/joern-cli/joern','--script',str(p/'oracle.sc'),'--param','inputPath='+str(p/'input')];t=time.monotonic()
with (p/'live.stdout').open('wb') as out,(p/'live.stderr').open('wb') as err:r=subprocess.run(cmd,cwd=cwd,env=env,stdout=out,stderr=err,timeout=180)
run={'command':cmd,'cwd':str(cwd),'exitCode':r.returncode,'seconds':time.monotonic()-t,'inputHashes':{n:{f:sha(p/'input'/n/f) for f in files} for n,files in cases.items()},'scriptSha256':sha(p/'oracle.sc'),'joernVersion':'4.0.555','officialArchiveSha256':'12989883d6b5aeacc97b2dc0ecc4e2951bf50e48cb244f8c8f333d11a8be0c7e','rawStdoutSha256':sha(p/'live.stdout'),'rawStderrSha256':sha(p/'live.stderr')};(p/'live-run.json').write_text(json.dumps(run,indent=2)+'\n');assert r.returncode==0
sections={};name=None
for line in (p/'live.stdout').read_bytes().splitlines(keepends=True):
 if line.startswith(b'CASE|'):name=line[5:].strip().decode();sections[name]=[]
 elif line.startswith((b'AST|',b'NODES|',b'EDGES|',b'FLOWS|')):sections[name].append(line[4:] if line.startswith(b'AST|') else line)
assert set(sections)==set(cases)
for name,lines in sections.items():(p/'input'/name/'expected.txt').write_bytes(b''.join(lines))
print('complete',len(cases),run['seconds'])
