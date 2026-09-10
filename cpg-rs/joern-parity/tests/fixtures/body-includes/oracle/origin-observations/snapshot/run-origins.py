"""Small separate origin observer launch, using unchanged V4 verification/process helpers.
No canonical reference extraction or replacement is performed here.
"""
from pathlib import Path
import argparse, datetime, json, os, shutil, sys, time
ROOT=Path(__file__).resolve().parent
ns={'__name__':'v4_runtime_helpers','__file__':str(ROOT/'run-oracle.py')}
exec(compile((ROOT/'run-oracle.py').read_bytes(),str(ROOT/'run-oracle.py'),'exec'),ns)

def complete_origins(raw,cases):
    counts={};ends={};current=None
    for line in raw.split(b'\n'):
        if line.startswith(b'ORIGIN_CASE|'):
            current=line.split(b'|')[1].decode()
            if current not in cases or current in counts:raise RuntimeError('Unexpected/duplicate origin case')
            counts[current]={'AST_OCCURRENCES':0,'TYPE_DECL_RECORDS':0,'SOURCE_FILE_EDGES':0}
        elif line.startswith(b'ORIGIN_END|'):
            fields=line.decode().split('|');name=fields[1]
            if name!=current or name in ends:raise RuntimeError('Unexpected/duplicate origin completion')
            ends[name]={k:int(v) for k,v in (field.split('=',1) for field in fields[2:])}
        elif line.startswith((b'ORIGIN_AST|',b'ORIGIN_TYPE_DECL|',b'ORIGIN_SOURCE_FILE|')):
            if current is None or current in ends:raise RuntimeError('Origin row outside active case')
            key={b'ORIGIN_AST':'AST_OCCURRENCES',b'ORIGIN_TYPE_DECL':'TYPE_DECL_RECORDS',b'ORIGIN_SOURCE_FILE':'SOURCE_FILE_EDGES'}[line.split(b'|')[0]]
            counts[current][key]+=1
    return set(counts)==set(cases)==set(ends) and counts==ends and all(row['AST_OCCURRENCES']>0 for row in counts.values()),counts

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--producer-hold-released',action='store_true');p.add_argument('--checkpoint-receipt',required=True);p.add_argument('--approved-receipt-sha256',required=True)
    p.add_argument('--canonical-run-receipt',required=True);p.add_argument('--canonical-run-sha256',required=True);p.add_argument('--run-name',required=True);p.add_argument('--timeout-seconds',type=int,default=180)
    args=p.parse_args()
    if not args.producer_hold_released:p.error('Parent producer hold remains active')
    if not ns['re'].fullmatch(r'[a-zA-Z0-9_-]+',args.run_name) or not 1<=args.timeout_seconds<=600:p.error('Unique alphanumeric run name and bounded timeout required')
    run=ROOT/'runs'/args.run_name;run.mkdir(parents=True,exist_ok=False)
    start=time.monotonic();errors=[];before=None;after=None;git_before=None;git_after=None;runtime=None;manifest=None;external={};command=None;counts={};complete=False
    process={'exitCode':None,'timedOut':False,'processError':None}
    try:
        observed=json.loads((ROOT/'prepared.json').read_bytes());runtime=observed.get('runtime')
        external={args.checkpoint_receipt:args.approved_receipt_sha256,args.canonical_run_receipt:args.canonical_run_sha256}
    except Exception:pass
    preflight=ns['relevant_snapshot'](ROOT,run,external,runtime);before=preflight
    ns['write_json'](run/'preflight-before.json',preflight)
    ns['write_json'](run/'started.json',{'status':'HELD_RAW_ORIGINS_PENDING_CHECKS','startedAtUtc':datetime.datetime.now(datetime.timezone.utc).isoformat()})
    with (run/'live.stdout').open('xb') as out,(run/'live.stderr').open('xb') as err:
        try:
            manifest,prepared=ns['verify_prepared'](ROOT);runtime=manifest['runtime'];ns['verify_runtime'](runtime)
            _,git_before=ns['verify_release'](args.checkpoint_receipt,args.approved_receipt_sha256,ns['sha'](ROOT/'prepared.json'))
            external={**git_before['externalBindings'],**{x['path']:x['sha256'] for x in manifest['frozenOriginals']},**{x['path']:x['inventory'] for x in manifest['frozenOriginalInventories']}}
            ns['require_file'](Path(args.canonical_run_receipt),args.canonical_run_sha256);canonical=json.loads(Path(args.canonical_run_receipt).read_bytes())
            if (canonical.get('status')!='COMPLETE_RAW_REFERENCE_BATCH' or canonical.get('admittedAsReferences') is not True or canonical.get('retainedAnchorByteIdentical') is not True or canonical['before']['prepared']['files']['prepared.json']['sha256']!=ns['sha'](ROOT/'prepared.json')):raise RuntimeError('Successful canonical companion for this exact preparation is required')
            external[str(Path(args.canonical_run_receipt).resolve())]=args.canonical_run_sha256
            for key in ['rawStdout','rawStderr']:
                row=canonical[key];external[row['path']]=row['sha256']
            snap=run/'snapshot';snap.mkdir();shutil.copytree(ROOT/'input',snap/'input')
            for name in ['oracle.sc','origin-observation.sc','run-oracle.py','run-origins.py','prepared.json']:
                shutil.copy2(ROOT/name,snap/name);ns['require_file'](snap/name,ns['sha'](ROOT/name))
            if ns['inventory'](snap/'input')!=ns['inventory'](ROOT/'input') or ns['prepared_snapshot'](ROOT)!=prepared:raise RuntimeError('Source/script copy drift')
            before=ns['relevant_snapshot'](ROOT,run,external,runtime);ns['write_json'](run/'before.json',before)
            for path,value in external.items():ns['require_bound'](Path(path),value)
            ns['verify_runtime'](runtime)
            workspace=run/'workspace';workspace.mkdir()
            command=[manifest['joern']['executable']['path'],'--script',str(snap/'origin-observation.sc'),'--param','inputPath='+str(snap/'input')]
            env=dict(os.environ);env['JAVA_HOME']=manifest['jdk21Home'];env['PATH']=runtime['launchPath']
            process=ns['run_process'](command,workspace,env,out,err,args.timeout_seconds)
        except BaseException as exc:errors.append(type(exc).__name__+': '+str(exc))
        finally:
            after=ns['relevant_snapshot'](ROOT,run,external,runtime);ns['write_json'](run/'after.json',after)
            if not (run/'before.json').exists():ns['write_json'](run/'before.json',before)
            try:
                if ns['verify_prepared'](ROOT)[0]!=manifest:raise RuntimeError('Prepared manifest drift')
                ns['verify_runtime'](runtime)
                _,git_after=ns['verify_release'](args.checkpoint_receipt,args.approved_receipt_sha256,ns['sha'](ROOT/'prepared.json'))
            except BaseException as exc:errors.append('postrun: '+type(exc).__name__+': '+str(exc))
    raw_before=ns['bind'](run/'live.stdout')
    try:complete,counts=complete_origins((run/'live.stdout').read_bytes(),manifest['cases'] if manifest else [])
    except BaseException as exc:errors.append('raw origin framing: '+type(exc).__name__+': '+str(exc))
    final=ns['relevant_snapshot'](ROOT,run,external,runtime)
    if final!=after or ns['bind'](run/'live.stdout')!=raw_before:errors.append('Finalization drift')
    admitted,reasons=ns['admissibility'](process,complete,True,errors,before,final,git_before,git_after)
    receipt={'status':'COMPLETE_BOUND_RAW_ORIGIN_OBSERVATIONS' if admitted else 'HELD_RAW_ORIGIN_OBSERVATIONS','notCanonicalReferences':True,'command':command,'cwd':str(run/'workspace'),'seconds':time.monotonic()-start,**process,'holdReasons':reasons,'caseCounts':counts,'canonicalCompanion':{'path':args.canonical_run_receipt,'sha256':args.canonical_run_sha256},'preflightBefore':preflight,'before':before,'after':final,'gitBefore':git_before,'gitAfter':git_after,'rawStdout':{'path':str(run/'live.stdout'),**raw_before},'rawStderr':{'path':str(run/'live.stderr'),**ns['bind'](run/'live.stderr')}}
    ns['write_json'](run/'run.json',receipt);print(json.dumps({'receipt':str(run/'run.json'),'sha256':ns['sha'](run/'run.json'),'status':receipt['status']}));return 0 if admitted else 1
if __name__=='__main__':sys.exit(main())
