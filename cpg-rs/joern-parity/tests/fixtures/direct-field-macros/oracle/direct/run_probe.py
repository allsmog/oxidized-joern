from pathlib import Path
import subprocess,os,time,json,hashlib,difflib
HERE=Path(__file__).resolve().parent;ROOT=HERE.parents[2]
input=HERE/'input';input.mkdir(exist_ok=True)
base='struct Item { int len; int code; };\n'
cases={
'arrow':base+'#define Len len\nint read(struct Item *p) { return p->Len; }\n',
'dot':base+'#define Len len\nint read(struct Item item) { return item.Len; }\n',
'array_receiver':base+'#define Len len\n#define Code code\nint read(struct Item *p, int i) { return p[i].Len + p[i].Code; }\n',
'field_subscript':'struct Item { int values[2]; };\n#define Len values[1]\nint read(struct Item *p) { return p->Len; }\n',
'object_chain':base+'#define FIELD len\n#define Len FIELD\nint read(struct Item *p) { return p->Len; }\n',
'receiver_macro':base+'#define Len len\n#define PTR(p) (p)\nint read(struct Item *p) { return PTR(p)->Len; }\n',
'binary_replacement':base+'#define Len len + 1\nint read(struct Item *p) { return p->Len; }\n',
'repeated':'struct Item { int values[2]; };\n#define Len values[1]\nint read(struct Item *p) { p->Len = 2; return p->Len; }\n',
'cycle':'struct Item { int Len; };\n#define Len Len\nint read(struct Item *p) { return p->Len; }\n',
'function_name':'struct Item { int Len; };\n#define Len(x) (x)\nint read(struct Item *p) { return p->Len; }\n',
'declaration':'#define Len len\nstruct Item { int Len; };\nint read(struct Item *p) { return p->Len; }\n',
}
for n,s in cases.items():
 p=input/n;p.mkdir(exist_ok=True);(p/'main.c').write_text(s)
script=(ROOT/'cpg-rs/joern-parity/oracle.sc').read_text()
script=script.replace('@main def exec(inputPath: String) = {','def dumpProject(inputPath: String, projectName: String) = {',1).replace('importCode(inputPath, "proj")','importCode(inputPath, projectName)',1)
script+='\n@main def exec(inputPath: String) = {\n  os.list(os.Path(inputPath)).filter(os.isDir(_)).sortBy(_.last).foreach { path =>\n    println("CASE|" + path.last)\n    dumpProject(path.toString, path.last)\n  }\n}\n'
(HERE/'oracle.sc').write_text(script)
cmd=['/Users/shayaunnejad/vibe-code/joern-oxidized/.local/astra-sprint/oracle/runtime/joern-cli/joern','--script',str(HERE/'oracle.sc'),'--param',f'inputPath={input}']
env=os.environ.copy();env['JAVA_HOME']='/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home';env['PATH']=env['JAVA_HOME']+'/bin:'+env['PATH']
cwd=HERE/'oracle-workspace';cwd.mkdir(exist_ok=True)
t=time.monotonic()
with (HERE/'live.stdout').open('wb') as out,(HERE/'live.stderr').open('wb') as err:r=subprocess.run(cmd,cwd=cwd,env=env,stdout=out,stderr=err)
receipt={'command':cmd,'cwd':str(cwd),'exit':r.returncode,'seconds':time.monotonic()-t,'inputHashes':{n:hashlib.sha256(s.encode()).hexdigest() for n,s in cases.items()},'scriptHash':hashlib.sha256(script.encode()).hexdigest()}
(HERE/'live-run.json').write_text(json.dumps(receipt,indent=2)+'\n');assert r.returncode==0,r.returncode
sections={};current=None
for line in (HERE/'live.stdout').read_bytes().decode().split('\n'):
 if line.startswith('CASE|'):current=line[5:];sections[current]=[]
 elif line.startswith(('AST|','NODES|','EDGES|','FLOWS|')):sections[current].append(line[4:] if line.startswith('AST|') else line)
assert set(sections)==set(cases)
binary=ROOT/'.local/field-macros/frozen-nested/joern-parity'
for n,lines in sections.items():
 p=input/n;expected='\n'.join(lines)+'\n';(p/'expected.txt').write_text(expected)
 r=subprocess.run([str(binary),str(p/'main.c')],capture_output=True);assert r.returncode==0
 (p/'before.txt').write_bytes(r.stdout);d=''.join(difflib.unified_diff(expected.splitlines(keepends=True),r.stdout.decode().splitlines(keepends=True),fromfile='joern',tofile='baseline'))
 (p/'before.diff').write_text(d);print(n,'records',len(lines),'before_diff',len(d.splitlines()),flush=True)
print('oracle seconds',receipt['seconds'])
