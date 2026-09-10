"""Held by default: reopen copied CPGs, prove canonical equality, then admit extras."""
from pathlib import Path
import argparse, collections, datetime, difflib, hashlib, importlib.util, json, os, re, shutil, sys, time
sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parent
DIGEST = re.compile(r'[0-9a-f]{64}')
IDENT = re.compile(r'[a-zA-Z0-9_-]+')
ID = re.compile(r'-?[0-9]+')

def sha(p): return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def read(p): return json.loads(Path(p).read_bytes())
def require(ok, message):
    if not ok: raise RuntimeError(message)
def binding(p):
    p = Path(p)
    try:
        if p.is_symlink(): return {'kind':'symlink','target':os.readlink(p)}
        return {'kind':'file','sha256':sha(p),'bytes':p.stat().st_size}
    except Exception as e: return {'kind':'unavailable','error':type(e).__name__}
def require_binding(row):
    b = binding(row['path'])
    require(b.get('kind') == 'file' and b['sha256'] == row['sha256'], 'Bound file differs: '+row['path'])
    if 'bytes' in row: require(b['bytes'] == row['bytes'], 'Bound size differs: '+row['path'])
def write_json(p, value):
    with Path(p).open('x') as f: json.dump(value,f,indent=2); f.write('\n')
def own_inventory(root):
    return {str(p.relative_to(root)):binding(p) for p in sorted(Path(root).rglob('*'))
            if p.is_file() and p.relative_to(root).parts[0] not in ('runs','offline-checks')}
def load_base(cfg):
    require_binding(cfg['baseRunner'])
    spec = importlib.util.spec_from_file_location('frozen_identity_runtime_checks',cfg['baseRunner']['path'])
    module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module)
    return module

def verify_config(root=ROOT):
    root=Path(root); cfg=read(root/'prepared.json')
    expected={r['path']:{'kind':'file','sha256':r['sha256'],'bytes':r['bytes']} for r in cfg['files']}
    expected['prepared.json']=binding(root/'prepared.json')
    require(own_inventory(root)==expected,'Complete observer preparation differs')
    for k in ['baseRunner','basePrepared','canonicalOracle','historicalObservation']:
        require_binding(cfg[k])
    base=load_base(cfg); manifest,_=base.verify_prepared(Path(cfg['baseRunner']['path']).parent)
    require(sha(Path(cfg['baseRunner']['path']).parent/'prepared.json')==cfg['basePrepared']['sha256'],'Canonical prepared mismatch')
    require(cfg['newCases']==manifest['cases'] and len(cfg['newCases'])==6,'Six fresh cases differ')
    require(cfg['retainedCases']==[],'No historical CPG observation jobs permitted')
    require(cfg['anchorCases']==['duplicate_supplied_macro_local','tiny_fixedtables_include','tiny_fixedtables_inline'],'Three historical supplemental anchors differ')
    require(cfg['checkpointAdmission']==manifest['checkpointAdmission'],'Observer checkpoint differs')
    require(cfg['canonicalOracle']['sha256']==manifest['oracle']['sha256']=='56431f14868678e64f9eb41026b44f1d3d7738404d28d2db62711b520c98c742','Canonical oracle changed')
    verify_historical_anchors(cfg,base,manifest)
    return cfg,base,manifest

def verify_observer_release(path, approved_sha, cfg, root):
    require(isinstance(approved_sha,str) and DIGEST.fullmatch(approved_sha),'Exact parent-approved observer receipt SHA required')
    require_binding({'path':str(path),'sha256':approved_sha}); rel=read(path)
    require(rel.get('schemaVersion')==1 and rel.get('status')=='PARENT_APPROVED_INCLUDE_REFERENCE_OBSERVER_RELEASE' and rel.get('observationHoldReleased') is True,'Observer hold remains active')
    require(rel.get('preparedManifestSha256')==sha(root/'prepared.json'),'Observer release not for frozen preparation')
    for k in ['sourceCommit','documentationCommit','frozenBuild']:
        require(rel.get(k)==cfg['checkpointAdmission'][k],'Observer release checkpoint mismatch: '+k)
    for k in ['canonicalRun','canonicalRelease']:
        require(isinstance(rel.get(k),dict) and DIGEST.fullmatch(rel[k].get('sha256','')),'Bound canonical run/release required')
        require_binding(rel[k])
    require(rel.get('historicalObservation')==cfg['historicalObservation'],'Historical observation release mismatch')
    return rel

def verified_run(row, prepared_row, cases, base, historical=False):
    require_binding(row); require_binding(prepared_row)
    run_path=Path(row['path']).resolve(); folder=run_path.parent; rec=read(run_path); prep=read(prepared_row['path'])
    require(rec.get('status')=='COMPLETE_RAW_REFERENCE_BATCH' and rec.get('admittedAsReferences') is True and rec.get('allCasesComplete') is True,'Canonical run was not admitted')
    require(rec.get('exitCode')==0 and rec.get('timedOut') is False and rec.get('processError') is None and rec.get('holdReasons')==[],'Canonical process failed')
    require(rec['before']==rec['after'] and rec['gitBefore']==rec['gitAfter'] and rec['gitBefore']['gitObjectAndAncestryChecksPassed'] is True,'Canonical input/checkpoint checks failed')
    for k in ['rawStdout','rawStderr']: require_binding(rec[k])
    require(Path(rec['rawStdout']['path']).resolve()==folder/'live.stdout' and Path(rec['rawStderr']['path']).resolve()==folder/'live.stderr','Canonical raw paths outside run')
    for name in ['prepared.json','oracle.sc']:
        expected=prepared_row['sha256'] if name=='prepared.json' else prep['oracle']['sha256']
        require(sha(folder/'snapshot'/name)==expected,'Canonical copied script/manifest differs')
    expected_inputs={k:{'kind':'file','sha256':v} for k,v in prep['inputHashes'].items()}
    actual_inputs=base.inventory(folder/'snapshot/input')['files']
    require(set(actual_inputs)==set(expected_inputs) and all(actual_inputs[k].get('kind')=='file' and actual_inputs[k]['sha256']==v['sha256'] for k,v in expected_inputs.items()),'Canonical input bytes differ')
    selected,kinds,complete=base.extract_selected(Path(rec['rawStdout']['path']).read_bytes(),prep['cases'])
    require(complete,'Canonical raw extraction incomplete')
    rows={r['case']:r for r in rec['outputs']}
    require(len(rows)==len(rec['outputs']) and set(rows)==set(prep['cases']),'Canonical output inventory differs')
    for name,r in rows.items():
        require_binding(r); require(r.get('completeReference') is True and Path(r['path']).resolve()==folder/'expected'/name/'expected.txt','Non-reference or misplaced canonical output')
        require(Path(r['path']).read_bytes()==selected[name],'Canonical complete raw/reference bytes differ')
    require(set(cases)<=set(rows),'Requested case not in canonical run')
    workspace=folder/'workspace'
    if not historical:
        require(Path(rec['workspaceArtifacts']['path']).resolve()==workspace and base.inventory(workspace)==rec['workspaceArtifacts']['inventory'],'Produced CPG inventory differs from canonical receipt')
    jobs=[]
    for name in cases:
        require(IDENT.fullmatch(name),'Invalid case name')
        project=workspace/'workspace'/name; persistent=project/'cpg.bin'; working=project/'cpg.bin.tmp'; metadata=project/'project.json'
        for f in [persistent,working,metadata]: require(f.is_file() and not f.is_symlink() and f.stat().st_size>0,'Missing/nonregular saved CPG or metadata')
        require(not any(x.is_symlink() for x in [project,project.parent,workspace]),'Symlink project directory')
        meta=read(metadata); require(meta['name']==name and Path(meta['inputPath']).resolve()==folder/'snapshot/input'/name,'Saved project source ownership differs')
        require(persistent.read_bytes()==working.read_bytes(),'Persistent and working CPGs differ; no automatic candidate selection')
        jobs.append({'case':name,'group':'retained' if historical else 'new','originalCpg':str(persistent),'originalWorkingCpg':str(working),'projectMetadata':str(metadata),'cpgSha256':sha(persistent),'reference':rows[name],'canonicalRunSha256':row['sha256']})
    return rec,jobs

def verify_historical_anchors(cfg,base,manifest):
    require_binding(cfg['historicalObservation']);path=Path(cfg['historicalObservation']['path']);rec=read(path)
    require(rec.get('status')=='COMPLETE_ADMITTED_SAVED_CPG_OBSERVATION' and rec.get('admittedSupplement') is True and rec.get('exitCode')==0 and not rec.get('timedOut') and not rec.get('processError') and rec.get('holdReasons')==[],'Historical observation not admitted')
    require(rec['before']==rec['after'] and rec['gitBefore']==rec['gitAfter'],'Historical observation was unstable')
    for k in ['rawStdout','rawStderr']:require_binding(rec[k])
    names=[j['case'] for j in rec['jobs']];require(len(names)==len(set(names))==11,'Historical eleven-case inventory differs')
    raw=Path(rec['rawStdout']['path']).read_bytes();selected,_,complete=base.extract_selected(raw,names);extras=extract_supplement(raw,names);require(complete,'Historical canonical extraction incomplete')
    outputs={r['case']:r for r in rec['outputs']};jobs={j['case']:j for j in rec['jobs']};require(set(outputs)==set(names) and len(outputs)==len(rec['outputs']),'Historical output inventory differs')
    anchors={}
    for name in cfg['anchorCases']:
        output=outputs[name];job=jobs[name];require(output['canonicalByteIdentical'] and output['supplementAdmitted'],'Historical anchor was not admitted')
        folder=path.parent/'admitted-supplement'/name;supplement=folder/'supplement.jsonl'
        for filename,row in output['files'].items():require_binding({'path':str(folder/filename),**row})
        require(supplement.read_bytes()==extras[name]['rawJsonl'],'Historical full snapshot differs from raw extraction')
        require_binding(job['reference']);require(selected[name]==Path(job['reference']['path']).read_bytes(),'Historical complete canonical reference differs')
        canonical_folder=Path(job['reference']['path']).resolve().parents[2];canonical_run=canonical_folder/'run.json';require(sha(canonical_run)==job['canonicalRunSha256'],'Historical source-run binding differs')
        prep=read(canonical_folder/'snapshot/prepared.json');expected={k[len(name)+1:]:v for k,v in prep['inputHashes'].items()if k.startswith(name+'/')}
        current={k[len(name)+1:]:v for k,v in manifest['inputHashes'].items()if k.startswith(name+'/')};require(expected and expected==current,'Historical/fresh anchor source bytes differ')
        observed=base.inventory(canonical_folder/'snapshot/input'/name)['files'];require(set(observed)==set(expected) and all(observed[k].get('kind')=='file' and observed[k]['sha256']==v for k,v in expected.items()),'Historical complete source inventory differs')
        anchors[name]={'supplement':{'path':str(supplement),**binding(supplement)},'reference':job['reference'],'sourceHashes':expected,'canonicalRunSha256':job['canonicalRunSha256']}
    return anchors

def compare_anchor_rows(old,new):
    # No property/path normalization: classes, missingness and dynamic ROOT
    # values remain visible. The ID-free result is not graph-isomorphism proof.
    encode=lambda v:json.dumps(v,sort_keys=True,separators=(',',':'),ensure_ascii=False)
    def parse(data):
        rows=[json.loads(line)for line in data.split(b'\n')if line]
        nodes={};edges=[]
        for row in rows:
            if 'id' in row:
                require(set(row)=={'id','label','properties'} and row['id'] not in nodes,'Invalid historical anchor node');nodes[row['id']]=row
                for value in row['properties'].values():verify_typed(value)
            else:
                require(set(row)=={'source','destination','label','property'},'Invalid historical anchor edge');verify_typed(row['property']);edges.append(row)
        require(nodes and all(e['source']in nodes and e['destination']in nodes for e in edges),'Missing anchor endpoint')
        content={id:encode({k:v for k,v in n.items()if k!='id'})for id,n in nodes.items()};groups={}
        for id,signature in content.items():groups.setdefault(signature,[]).append(id)
        full=collections.Counter(encode(row)for row in rows);node_rows=collections.Counter(content.values())
        edge_rows=collections.Counter(encode({'sourceNode':json.loads(content[e['source']]),'destinationNode':json.loads(content[e['destination']]),'label':e['label'],'property':e['property']})for e in edges)
        return full,node_rows,edge_rows,{k:sorted(v)for k,v in groups.items()if len(v)>1},len(nodes),len(edges)
    a,b=parse(old),parse(new)
    def delta(left,right):
        return {'historicalOnly':[{'row':json.loads(row),'multiplicity':n}for row,n in sorted((left-right).items())], 'freshOnly':[{'row':json.loads(row),'multiplicity':n}for row,n in sorted((right-left).items())]}
    return {'rawBytesEqual':old==new,'idSensitiveFullRowMultisetEqual':a[0]==b[0],'idFreeNodeContentMultisetEqual':a[1]==b[1],'idFreeEndpointContentMultisetEqual':a[2]==b[2],'counts':{'historicalNodes':a[4],'freshNodes':b[4],'historicalEdges':a[5],'freshEdges':b[5]},'fullRecordDeltas':delta(a[0],b[0]),'nodeContentDeltas':delta(a[1],b[1]),'edgeEndpointContentDeltas':delta(a[2],b[2]),'ambiguousNodeSignatures':{'historical':a[3],'fresh':b[3]},'idFreeComparisonIsNotGraphIsomorphismProof':True,'noPropertyOrPathNormalization':True,'differencesRequireReviewNotGuessedReplacement':True}

def verify_typed(v):
    require(isinstance(v,dict) and isinstance(v.get('kind'),str),'Invalid typed stored property')
    k=v['kind']
    if k=='null':require(set(v)=={'kind'},'Null payload differs')
    elif k in ('string','character'):require(set(v)=={'kind','value'} and isinstance(v['value'],str),'String payload differs')
    elif k=='boolean':require(set(v)=={'kind','value'} and isinstance(v['value'],bool),'Boolean payload differs')
    elif k=='number':require(set(v)=={'kind','class','value'} and isinstance(v['class'],str) and isinstance(v['value'],str),'Number payload must preserve class and exact spelling')
    elif k in ('array','sequence'):
        require(set(v)=={'kind','class','values'} and isinstance(v['class'],str) and isinstance(v['values'],list),'Sequence payload differs')
        for x in v['values']:verify_typed(x)
    elif k=='map':
        require(set(v)=={'kind','class','entries'} and isinstance(v['class'],str) and isinstance(v['entries'],list),'Map payload differs')
        for x in v['entries']:
            require(set(x)=={'key','value'},'Map entry differs');verify_typed(x['key']);verify_typed(x['value'])
    else:raise RuntimeError('Unknown typed stored property kind')

def extract_supplement(raw,cases):
    result={}; current=None; raw_rows=[]; nodes={}; edges=[]; latest_case=None
    for line in raw.split(b'\n'):
        if line.startswith(b'CASE|'):latest_case=line[5:].decode('utf-8')
        if not line.startswith(b'OBS_'):continue
        prefix,body=line.split(b'|',1);v=json.loads(body)
        if prefix==b'OBS_BEGIN':
            name=v.get('case');require(current is None and name in cases and name not in result and latest_case==name and v=={'schemaVersion':1,'case':name},'Invalid supplemental opening frame')
            current=name;raw_rows=[];nodes={};edges=[]
        elif prefix==b'OBS_NODE':
            require(current is not None and set(v)=={'id','label','properties'} and isinstance(v['id'],str) and ID.fullmatch(v['id']) and v['id'] not in nodes and isinstance(v['label'],str) and isinstance(v['properties'],dict),'Invalid or duplicate node')
            for prop in v['properties'].values():verify_typed(prop)
            nodes[v['id']]=v;raw_rows.append(line)
        elif prefix==b'OBS_EDGE':
            require(current is not None and set(v)=={'source','destination','label','property'} and all(isinstance(v[k],str) for k in ['source','destination','label']),'Invalid edge')
            require(ID.fullmatch(v['source']) and ID.fullmatch(v['destination']),'Invalid edge endpoint ID');verify_typed(v['property']);edges.append(v);raw_rows.append(line)
        elif prefix==b'OBS_END':
            require(current is not None and set(v)=={'case','nodeCount','edgeCount'} and v['case']==current and v['nodeCount']==str(len(nodes)) and v['edgeCount']==str(len(edges)) and bool(nodes),'Incomplete supplemental counts')
            require(all(e['source'] in nodes and e['destination'] in nodes for e in edges),'Missing supplemental endpoint')
            result[current]={'nodeCount':len(nodes),'edgeCount':len(edges),'nodeLabels':{label:sum(n['label']==label for n in nodes.values()) for label in sorted({n['label'] for n in nodes.values()})},'rawJsonl':b'\n'.join(x.split(b'|',1)[1] for x in raw_rows)+b'\n','rawProtocol':b'\n'.join(raw_rows)+b'\n'};current=None
        else:raise RuntimeError('Unknown supplemental record prefix')
    require(current is None and set(result)==set(cases),'Missing supplemental case/end frame')
    return result

def lf_lines(data):
    pieces=data.split(b'\n')
    return [x+b'\n' for x in pieces[:-1]]+([pieces[-1]] if pieces[-1] else [])

def admission(process, complete, equality, errors, before, after, git_before, git_after):
    reasons=list(errors)
    if process.get('exitCode')!=0 or process.get('timedOut') or process.get('processError'):reasons.append('observer_process_failed')
    if not complete:reasons.append('incomplete_supplement')
    if not equality or not all(equality.values()):reasons.append('complete_canonical_reproduction_mismatch')
    if before is None or before!=after:reasons.append('original_copy_runtime_or_preparation_changed')
    if git_before is None or git_before!=git_after:reasons.append('checkpoint_changed')
    return not reasons,reasons

def state_snapshot(root,base,cfg,manifest,originals,copies,release_path):
    return {'observerPreparation':own_inventory(root),'canonicalPreparation':base.prepared_snapshot(Path(cfg['baseRunner']['path']).parent),
        'runtime':base.runtime_snapshot(manifest['runtime']),'originalRuns':{str(p):base.inventory(p) for p in originals},
        'originalFileMetadata':{str(f):base.runtime_file(f) for p in originals for f in sorted(p.rglob('*')) if f.is_file()},
        'copiedInputs':base.inventory(copies),'copiedFileMetadata':{str(f):base.runtime_file(f) for f in sorted(Path(copies).rglob('*')) if f.is_file()},'release':binding(release_path)}

def attempt(args,root=ROOT):
    root=Path(root); run=root/'runs'/args.run_name;run.mkdir(parents=True,exist_ok=False)
    started=time.monotonic(); errors=[]; cfg=base=manifest=rel=None; before=after=git_before=git_after=None
    jobs=[]; originals=[]; reference_bytes={}; anchors={}; anchor_comparisons={}; anchor_files=None; process={'exitCode':None,'timedOut':False,'processError':None};command=None;selected={};kinds={};extras={};equality={};complete=False;raw_bind=None
    write_json(run/'started.json',{'status':'HELD_PENDING_ALL_CHECKS','startUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'approvedObserverReleaseSha256':args.approved_receipt_sha256})
    with (run/'observer.stdout').open('xb') as out,(run/'observer.stderr').open('xb') as err:
        try:
            cfg,base,manifest=verify_config(root);base.verify_runtime(manifest['runtime'])
            rel=verify_observer_release(Path(args.release_receipt),args.approved_receipt_sha256,cfg,root)
            canonical_release,git_before=base.verify_release(Path(rel['canonicalRelease']['path']),rel['canonicalRelease']['sha256'],cfg['basePrepared']['sha256'],manifest['checkpointAdmission'])
            newrec,newjobs=verified_run(rel['canonicalRun'],cfg['basePrepared'],cfg['newCases'],base)
            require(newrec['parentApprovedReceiptSha256']==rel['canonicalRelease']['sha256'] and newrec['gitAfter']==git_before,'Canonical run release/checkpoint differs from approved observer input')
            require(sha(Path(rel['canonicalRun']['path']).parent/'snapshot/checkpoint-release.json')==rel['canonicalRelease']['sha256'],'Canonical copied release differs')
            anchors=verify_historical_anchors(cfg,base,manifest)
            jobs=newjobs;require(len({j['case'] for j in jobs})==6,'Six unique fresh observer jobs required')
            reference_bytes={j['case']:Path(j['reference']['path']).read_bytes() for j in jobs}
            originals=[Path(rel['canonicalRun']['path']).resolve().parent,Path(cfg['historicalObservation']['path']).resolve().parent]
            copy_root=run/'snapshot';copy_root.mkdir()
            # Hash-bound snapshots are input copies; observer output uses stdout only.
            for n in ['observer.sc','prepared.json','run-observer.py']:
                with (copy_root/n).open('xb') as f:f.write((root/n).read_bytes())
            with (copy_root/'observer-release.json').open('xb') as f:f.write(Path(args.release_receipt).read_bytes())
            for j in jobs:
                dest=copy_root/'cpg'/j['case']/'cpg.bin';dest.parent.mkdir(parents=True)
                with dest.open('xb') as f:f.write(Path(j['originalCpg']).read_bytes())
                require(sha(dest)==j['cpgSha256'],'Copied CPG digest differs');dest.chmod(0o444);j['copiedCpg']=str(dest.resolve())
            write_json(copy_root/'jobs.json',{'cases':jobs})
            before=state_snapshot(root,base,cfg,manifest,originals,copy_root,Path(args.release_receipt));write_json(run/'before.json',before)
            base.verify_runtime(manifest['runtime']);require(before['runtime']==manifest['runtime']['expectedSnapshot'],'Runtime changed before launch')
            workspace=run/'workspace';workspace.mkdir()
            command=[manifest['joern']['executable']['path'],'--script',str((copy_root/'observer.sc').resolve()),'--param','manifestPath='+str((copy_root/'jobs.json').resolve())]
            env=dict(os.environ);env['JAVA_HOME']=manifest['jdk21Home'];env['PATH']=manifest['runtime']['launchPath']
            process=base.run_process(command,workspace,env,out,err,args.timeout_seconds)
        except BaseException as exc:errors.append('prelaunch_or_execution: '+type(exc).__name__+': '+str(exc))
    raw_bind=binding(run/'observer.stdout')
    try:
        if base is not None and jobs:
            names=[j['case'] for j in jobs];raw=(run/'observer.stdout').read_bytes()
            selected,kinds,canonical_complete=base.extract_selected(raw,names);extras=extract_supplement(raw,names)
            complete=canonical_complete and set(extras)==set(names)
            for j in jobs:equality[j['case']]=selected.get(j['case'])==reference_bytes[j['case']]
    except BaseException as exc:errors.append('extraction: '+type(exc).__name__+': '+str(exc))
    try:
        if cfg is not None and rel is not None:
            cfg2,base2,manifest2=verify_config(root);require(cfg2==cfg and manifest2==manifest,'Preparation changed')
            base.verify_runtime(manifest['runtime']);verify_observer_release(Path(args.release_receipt),args.approved_receipt_sha256,cfg,root)
            _,git_after=base.verify_release(Path(rel['canonicalRelease']['path']),rel['canonicalRelease']['sha256'],cfg['basePrepared']['sha256'],manifest['checkpointAdmission'])
            after=state_snapshot(root,base,cfg,manifest,originals,run/'snapshot',Path(args.release_receipt))
    except BaseException as exc:errors.append('postrun_binding: '+type(exc).__name__+': '+str(exc))
    # Historical anchors are comparisons, not additional saved-CPG jobs.
    # Preserve all raw rows/IDs before deriving any ID-free diagnostic.
    try:
        if cfg is not None and extras:
            current_anchors=verify_historical_anchors(cfg,base,manifest)
            require(current_anchors==anchors,'Historical anchor bytes or source ownership changed')
            comparison_root=run/'anchor-comparisons';comparison_root.mkdir()
            for name in cfg['anchorCases']:
                require(name in extras,'Missing fresh supplemental anchor')
                old=Path(anchors[name]['supplement']['path']).read_bytes();new=extras[name]['rawJsonl']
                folder=comparison_root/name;folder.mkdir()
                (folder/'historical-full.jsonl').write_bytes(old);(folder/'fresh-full.jsonl').write_bytes(new)
                (folder/'complete-raw.diff').write_bytes(b''.join(difflib.diff_bytes(difflib.unified_diff,lf_lines(old),lf_lines(new),fromfile=b'historical-full-snapshot',tofile=b'fresh-full-snapshot')))
                details=compare_anchor_rows(old,new);write_json(folder/'comparison.json',details)
                anchor_comparisons[name]={k:v for k,v in details.items() if k not in ('fullRecordDeltas','nodeContentDeltas','edgeEndpointContentDeltas','ambiguousNodeSignatures')}
            anchor_files=base.inventory(comparison_root)
            require(not anchor_files.get('unavailable') and not any(v.get('kind')=='unavailable' for v in anchor_files['files'].values()),'Unavailable anchor comparison artifact')
    except BaseException as exc:errors.append('historical_anchor_comparison: '+type(exc).__name__+': '+str(exc))
    # Complete all fallible output observations before deciding promotion.
    workspace_output=None
    try:
        if base is not None:
            workspace_output=base.inventory(run/'workspace')
            require(not workspace_output.get('unavailable') and not any(v.get('kind')=='unavailable' for v in workspace_output['files'].values()),'Unavailable observer workspace observation')
    except BaseException as exc:errors.append('workspace_output_inventory: '+type(exc).__name__+': '+str(exc))
    if binding(run/'observer.stdout')!=raw_bind:errors.append('raw_output_changed_during_extraction')
    try:
        if after is not None:
            final_observation=state_snapshot(root,base,cfg,manifest,originals,run/'snapshot',Path(args.release_receipt))
            require(final_observation==after,'Bindings changed during final output observation')
    except BaseException as exc:errors.append('final_binding: '+type(exc).__name__+': '+str(exc))
    admitted,reasons=admission(process,complete,equality,errors,before,after,git_before,git_after)
    write_json(run/'after.json',after)
    if not (run/'before.json').exists():write_json(run/'before.json',before)
    result_root=run/('admitted-supplement' if admitted else 'held-output');result_root.mkdir();outputs=[]
    for j in jobs:
        name=j['case'];folder=result_root/name;folder.mkdir();canonical=selected.get(name,b'');(folder/'reproduced-canonical.txt').write_bytes(canonical)
        reference=reference_bytes.get(name,b'')
        diff=b''.join(difflib.diff_bytes(difflib.unified_diff,lf_lines(reference),lf_lines(canonical),fromfile=b'admitted-original',tofile=b'loaded-cpg'))
        (folder/'complete-canonical.diff').write_bytes(diff)
        if name in extras:(folder/'supplement.jsonl').write_bytes(extras[name]['rawJsonl']);(folder/'supplement.protocol').write_bytes(extras[name]['rawProtocol'])
        outputs.append({'case':name,'group':j['group'],'canonicalByteIdentical':equality.get(name,False),'supplementAdmitted':admitted,'counts':{k:v for k,v in extras.get(name,{}).items() if k not in ('rawJsonl','rawProtocol')},'files':{p.name:binding(p) for p in sorted(folder.iterdir())}})
    receipt={'status':'COMPLETE_ADMITTED_SAVED_CPG_OBSERVATION' if admitted else 'HELD_NON_ADMITTED_OBSERVATION','admittedSupplement':admitted,'holdReasons':reasons,'seconds':time.monotonic()-started,**process,'command':command,'approvedObserverReleaseSha256':args.approved_receipt_sha256,'sourceConfigSha256':sha(root/'prepared.json') if (root/'prepared.json').exists() else None,'jobs':jobs,'before':before,'after':after,'gitBefore':git_before,'gitAfter':git_after,'rawStdout':{'path':str(run/'observer.stdout'),**binding(run/'observer.stdout')},'rawStderr':{'path':str(run/'observer.stderr'),**binding(run/'observer.stderr')},'outputs':outputs,'workspaceOutput':workspace_output,'newCaseCount':6,'retainedCaseCount':0,'historicalSupplementAnchorCount':3,'newSourceCaseCount':2,'reusedSourceCaseCount':4,'anchorComparisons':anchor_comparisons,'anchorComparisonFiles':anchor_files,'anchorDifferencesRequireSemanticReview':True,'noReparse':True,'noDefaultPassesOrOriginalStoreOpen':True,'descendantGroupClosureNotAsserted':True,'coordinateAndModifierMissingnessPreserved':True}
    write_json(run/'run.json',receipt);print(json.dumps({'status':receipt['status'],'path':str(run/'run.json'),'sha256':sha(run/'run.json')}));return 0 if admitted else 1

def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--verify-only',action='store_true');parser.add_argument('--observation-hold-released',action='store_true');parser.add_argument('--release-receipt');parser.add_argument('--approved-receipt-sha256');parser.add_argument('--run-name');parser.add_argument('--timeout-seconds',type=int,default=180);a=parser.parse_args()
    if a.verify_only:
        cfg,base,manifest=verify_config();base.verify_runtime(manifest['runtime']);print('PREPARED_OBSERVER_VERIFIED_NO_EXECUTION');return 0
    if not a.observation_hold_released:parser.error('Observer hold remains active')
    if not a.release_receipt or not a.approved_receipt_sha256:parser.error('Exact separately approved observer release required')
    if not a.run_name or not IDENT.fullmatch(a.run_name):parser.error('New alphanumeric attempt name required')
    if not 1<=a.timeout_seconds<=600:parser.error('Timeout must be between 1 and 600 seconds')
    return attempt(a)

if __name__=='__main__':sys.exit(main())
