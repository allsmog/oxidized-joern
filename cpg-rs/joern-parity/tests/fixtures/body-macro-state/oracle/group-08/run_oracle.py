from pathlib import Path
import os,json,time,hashlib,subprocess,signal
P=Path(__file__).resolve().parent
H=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
cases=json.loads((P/'cases.json').read_text()); inputs={n:{f:H(P/'input'/n/f) for f in v} for n,v in cases.items()}
cmd=['/Users/shayaunnejad/vibe-code/joern-oxidized/.local/astra-sprint/oracle/runtime/joern-cli/joern','--script',str(P/'oracle.sc'),'--param','inputPath='+str(P/'input')]
env=os.environ.copy();env['JAVA_HOME']='/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home';env['PATH']=env['JAVA_HOME']+'/bin:'+env['PATH'];env['JAVA_OPTS']='-Xmx4g -Xms256m'
cwd=P/'oracle-workspace';cwd.mkdir(exist_ok=True);start=time.time();r={'command':cmd,'cwd':str(cwd),'javaHome':env['JAVA_HOME'],'scriptSha256':H(P/'oracle.sc'),'inputHashes':inputs,'joernVersion':'4.0.555','archiveSha256':'12989883d6b5aeacc97b2dc0ecc4e2951bf50e48cb244f8c8f333d11a8be0c7e','timeoutSeconds':180}
with (P/'live.stdout').open('wb') as out,(P/'live.stderr').open('wb') as err:
 proc=subprocess.Popen(cmd,cwd=cwd,env=env,stdout=out,stderr=err,start_new_session=True);r['pid']=proc.pid;(P/'live-run.json').write_text(json.dumps(r,indent=2)+'\n')
 try:r['exitCode']=proc.wait(timeout=180)
 except subprocess.TimeoutExpired:
  r['timedOut']=True;os.killpg(proc.pid,signal.SIGTERM)
  try:proc.wait(timeout=10)
  except subprocess.TimeoutExpired:os.killpg(proc.pid,signal.SIGKILL);proc.wait()
  r['exitCode']=proc.returncode
r['wallSeconds']=time.time()-start;r['rawStdoutSha256']=H(P/'live.stdout');r['rawStderrSha256']=H(P/'live.stderr');(P/'live-run.json').write_text(json.dumps(r,indent=2)+'\n');assert r['exitCode']==0,r
sections={};n=None
for line in (P/'live.stdout').read_text().splitlines():
 if line.startswith('CASE|'):n=line[5:];sections[n]=[]
 elif n and line.startswith(('AST|','NODES|','EDGES|','FLOWS|')):sections[n].append(line[4:] if line.startswith('AST|') else line)
assert set(sections)==set(cases)
r['projects']=[]
for n,lines in sections.items():
 assert all(H(P/'input'/n/f)==v for f,v in inputs[n].items())
 q=P/'input'/n/'expected.txt';q.write_text('\n'.join(lines)+'\n');r['projects'].append({'case':n,'canonicalLinesIncludingSeparators':len(lines),'nonemptySelectedRecords':sum(bool(l) for l in lines),'expectedSha256':H(q)})
(P/'live-run.json').write_text(json.dumps(r,indent=2)+'\n');print(json.dumps({'exit':r['exitCode'],'seconds':r['wallSeconds'],'cases':len(sections),'records':sum(x['nonemptySelectedRecords'] for x in r['projects'])}))
