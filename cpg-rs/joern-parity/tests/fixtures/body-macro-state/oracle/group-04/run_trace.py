from pathlib import Path
import os,json,time,hashlib,subprocess,signal
P=Path(__file__).resolve().parent
H=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
cases=json.loads((P/'cases.json').read_text()); inputs={n:{f:H(P/'input'/n/f) for f in v} for n,v in cases.items()}
cmd=['/Users/shayaunnejad/vibe-code/joern-oxidized/.local/astra-sprint/oracle/runtime/joern-cli/joern','--script',str(P/'trace.sc'),'--param','inputPath='+str(P/'input')]
env=os.environ.copy();env['JAVA_HOME']='/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home';env['PATH']=env['JAVA_HOME']+'/bin:'+env['PATH'];env['JAVA_OPTS']='-Xmx4g -Xms256m'
cwd=P/'trace-workspace';cwd.mkdir(exist_ok=True);start=time.time();r={'command':cmd,'cwd':str(cwd),'javaHome':env['JAVA_HOME'],'scriptSha256':H(P/'trace.sc'),'inputHashes':inputs,'joernVersion':'4.0.555','archiveSha256':'12989883d6b5aeacc97b2dc0ecc4e2951bf50e48cb244f8c8f333d11a8be0c7e','timeoutSeconds':180}
with (P/'trace.stdout').open('wb') as out,(P/'trace.stderr').open('wb') as err:
 proc=subprocess.Popen(cmd,cwd=cwd,env=env,stdout=out,stderr=err,start_new_session=True);r['pid']=proc.pid;(P/'trace-run.json').write_text(json.dumps(r,indent=2)+'\n')
 try:r['exitCode']=proc.wait(timeout=180)
 except subprocess.TimeoutExpired:
  r['timedOut']=True;os.killpg(proc.pid,signal.SIGTERM)
  try:proc.wait(timeout=10)
  except subprocess.TimeoutExpired:os.killpg(proc.pid,signal.SIGKILL);proc.wait()
  r['exitCode']=proc.returncode
r['wallSeconds']=time.time()-start;r['rawStdoutSha256']=H(P/'trace.stdout');r['rawStderrSha256']=H(P/'trace.stderr');(P/'trace-run.json').write_text(json.dumps(r,indent=2)+'\n');assert r['exitCode']==0,r
print(json.dumps(r,indent=2))
