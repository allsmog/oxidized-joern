"""Held by default. Promote references only after every release/binding check passes."""
from pathlib import Path, PurePosixPath
import argparse, datetime, hashlib, json, os, re, shutil, signal, subprocess, sys, time, stat

ROOT = Path(__file__).resolve().parent
DIGEST = re.compile(r'[0-9a-f]{64}')
COMMIT = re.compile(r'[0-9a-f]{40}')

def sha_bytes(data):
    return hashlib.sha256(data).hexdigest()

def sha(path):
    return sha_bytes(path.read_bytes())

def bind(path):
    path = Path(path)
    try:
        if path.is_symlink():
            return {'kind': 'symlink', 'target': os.readlink(path)}
        data = path.read_bytes()
        return {'kind': 'file', 'sha256': sha_bytes(data), 'bytes': len(data)}
    except Exception as exc:
        return {'kind': 'unavailable', 'error': type(exc).__name__}

def require_file(path, digest):
    actual = bind(path)
    if actual.get('kind') != 'file' or actual.get('sha256') != digest:
        raise RuntimeError('File binding differs: ' + str(path))
    return actual

def write_json(path, value):
    with Path(path).open('x') as handle:
        json.dump(value, handle, indent=2)
        handle.write('\n')

def inventory(root, excluded=()):
    root = Path(root); files = {}; directories = []
    if not root.is_dir():
        return {'root': str(root), 'unavailable': True, 'files': {}, 'directories': []}
    for current, dirs, names in os.walk(root, followlinks=False):
        rel = Path(current).relative_to(root)
        if rel == Path('.'):
            dirs[:] = [name for name in dirs if name not in excluded]
        dirs.sort(); names.sort()
        for name in list(dirs):
            path = Path(current) / name
            if path.is_symlink():
                files[str(path.relative_to(root))] = bind(path); dirs.remove(name)
            else:
                directories.append(str(path.relative_to(root)))
        for name in names:
            if rel == Path('.') and name in excluded:
                continue
            path = Path(current) / name
            files[str(path.relative_to(root))] = bind(path)
    return {'files': files, 'directories': sorted(directories)}

def runtime_file(path):
    """Bind the resolved bytes plus executable mode and every path symlink."""
    path = Path(path).absolute(); links = {}
    try:
        for part in [*reversed(path.parents), path]:
            if part.is_symlink():
                links[str(part)] = os.readlink(part)
        resolved = path.resolve(strict=True)
        info = resolved.stat()
        if not stat.S_ISREG(info.st_mode):
            return {'kind': 'nonregular', 'resolvedPath': str(resolved), 'symlinks': links}
        digest = hashlib.sha256()
        with resolved.open('rb') as handle:
            for chunk in iter(lambda: handle.read(1024 * 1024), b''):
                digest.update(chunk)
        return {'kind': 'file', 'resolvedPath': str(resolved), 'symlinks': links,
                'sha256': digest.hexdigest(), 'bytes': info.st_size, 'mode': stat.S_IMODE(info.st_mode)}
    except Exception as exc:
        return {'kind': 'unavailable', 'error': type(exc).__name__, 'symlinks': links}

def runtime_inventory(root):
    root = Path(root).absolute(); files = {}; directories = []; links = {}
    try:
        for part in [*reversed(root.parents), root]:
            if part.is_symlink():
                links[str(part)] = os.readlink(part)
        resolved = root.resolve(strict=True)
        if not resolved.is_dir():
            raise NotADirectoryError(str(root))
        for current, dirs, names in os.walk(root, followlinks=False):
            dirs.sort(); names.sort()
            for name in list(dirs):
                path = Path(current) / name
                if path.is_symlink():
                    # A newly added symlink directory changes the inventory. No
                    # symlink directories are accepted in the prepared runtimes.
                    files[str(path.relative_to(root))] = {'kind': 'symlink_directory', 'target': os.readlink(path), 'resolvedPath': str(path.resolve())}
                    dirs.remove(name)
                else:
                    directories.append(str(path.relative_to(root)))
            for name in names:
                path = Path(current) / name
                files[str(path.relative_to(root))] = runtime_file(path)
        return {'configuredRoot': str(root), 'resolvedRoot': str(resolved), 'rootSymlinks': links,
                'files': files, 'directories': sorted(directories)}
    except Exception as exc:
        return {'configuredRoot': str(root), 'rootSymlinks': links, 'unavailable': type(exc).__name__,
                'files': files, 'directories': sorted(directories)}

def runtime_snapshot(spec):
    if not spec:
        return {'unavailable': 'Runtime specification not loaded'}
    try:
        commands = {}
        for name in spec['hostCommands']:
            path = shutil.which(name, path=spec['launchPath'])
            commands[name] = {'selectedPath': path, 'binding': runtime_file(path) if path else {'kind': 'unavailable'}}
        # Values are hashed, never printed. These direct JVM/shell/loader
        # overrides must remain absent or empty for the selected JDK claim.
        overrides = {name: {'nonempty': bool(os.environ.get(name)),
                            'sha256': sha_bytes(os.environ.get(name, '').encode())}
                     for name in spec['emptyOverrides']}
        return {'trees': {name: runtime_inventory(path) for name, path in spec['trees'].items()},
                'commands': commands, 'overrideEnvironment': overrides,
                'hostIdentity': list(os.uname())}
    except Exception as exc:
        return {'unavailable': type(exc).__name__}

def verify_runtime(spec):
    current = runtime_snapshot(spec)
    if current != spec['expectedSnapshot']:
        raise RuntimeError('Complete Joern/JDK runtime or host-launch binding differs')
    if any(row['nonempty'] for row in current['overrideEnvironment'].values()):
        raise RuntimeError('Unbound JVM/shell/loader override is nonempty')
    return current

def prepared_snapshot(root=ROOT):
    # These are the only dynamic output directories; all original prepared
    # files, including the manifest/driver/oracle/anchor, remain in the inventory.
    return inventory(root, excluded=('runs', 'offline-checks'))

def verify_prepared(root=ROOT):
    root = Path(root); data = (root / 'prepared.json').read_bytes(); manifest = json.loads(data)
    expected = {row['path']: {'kind': 'file', 'sha256': row['sha256'], 'bytes': row['bytes']}
                for row in manifest['files']}
    expected['prepared.json'] = {'kind': 'file', 'sha256': sha_bytes(data), 'bytes': len(data)}
    current = prepared_snapshot(root)
    if current['files'] != expected:
        raise RuntimeError('Complete prepared file inventory differs')
    inputs = {name: value['sha256'] for name, value in inventory(root / 'input')['files'].items()
              if value.get('kind') == 'file'}
    if inputs != manifest['inputHashes']:
        raise RuntimeError('Prepared input inventory differs')
    require_file(root / 'oracle.sc', manifest['oracle']['sha256'])
    return manifest, current

def extract_selected(raw, cases):
    sections = {}; seen_kinds = {}; current = None
    # LF-only protocol conversion: no strip(), splitlines(), normalizing or dedup.
    for line in raw.split(b'\n'):
        if line.startswith(b'CASE|'):
            current = line[5:].decode('utf-8')
            if current in sections or current not in cases:
                raise RuntimeError('Unexpected or duplicate CASE marker: ' + current)
            sections[current] = []; seen_kinds[current] = set()
        elif line.startswith((b'AST|', b'NODES|', b'EDGES|', b'FLOWS|')):
            if current is None:
                raise RuntimeError('Selected record before CASE marker')
            kind = line.split(b'|', 1)[0].decode('ascii')
            seen_kinds[current].add(kind)
            sections[current].append(line[4:] if kind == 'AST' else line)
    selected = {name: b'\n'.join(lines) + b'\n' for name, lines in sections.items()}
    complete = (set(sections) == set(cases)
                and all({'AST', 'NODES', 'EDGES'} <= seen_kinds[n] for n in cases))
    return selected, seen_kinds, complete

def git(repo, *args):
    result = subprocess.run(['git', '-C', str(repo), *args], capture_output=True, timeout=30)
    if result.returncode:
        raise RuntimeError('Read-only git verification failed: ' + ' '.join(args))
    return result.stdout

def repo_path(value):
    path = PurePosixPath(value)
    if path.is_absolute() or '..' in path.parts or not path.parts or ':' in value:
        raise RuntimeError('Invalid repository-relative blob path')
    return value

def git_blob(repo, commit, path):
    return git(repo, 'show', commit + ':' + repo_path(path))

def verify_git_checkpoint(checkpoint):
    """Read actual objects/blobs. This alone does NOT authorize a producer."""
    repo = Path(checkpoint['repository']).resolve()
    source = checkpoint['sourceCommit']; docs = checkpoint['documentationCommit']
    for commit in (source, docs):
        if not COMMIT.fullmatch(commit) or git(repo, 'cat-file', '-t', commit) != b'commit\n':
            raise RuntimeError('Checkpoint does not name an actual full commit object')
    git(repo, 'merge-base', '--is-ancestor', source, docs)
    acceptance_ref = checkpoint['acceptance']
    acceptance_bytes = git_blob(repo, docs, acceptance_ref['repositoryPath'])
    if sha_bytes(acceptance_bytes) != acceptance_ref['sha256']:
        raise RuntimeError('Committed acceptance blob differs')
    acceptance = json.loads(acceptance_bytes)
    # Tenth source and docs share one commit; prove identity from pinned release
    # IDs plus every frozen Git blob below, without a self-referential doc hash.
    if (acceptance.get('all138FrozenBuildInputsMatchDeliveredSource') is not True
            or not acceptance['status'].startswith('accepted bounded C parity increment')):
        raise RuntimeError('Acceptance does not bind this accepted source')
    gates = acceptance['gates']
    if (gates['officialRealProjectAcceptance'] != 'passed'
            or gates['formatting'] != 'passed' or gates['strictClippy'] != 'passed'
            or gates['committedBlocks'] != 308 or gates['freshLiveBlocks'] != 308
            or gates['allFailedIgnoredFiltered'] != 0):
        raise RuntimeError('Required checkpoint gates are not recorded as passed')
    documentation = checkpoint['documentationBlobs']
    if len(documentation) < 2 or len({row['repositoryPath'] for row in documentation}) != len(documentation):
        raise RuntimeError('Distinct report and metrics blob bindings are required')
    blob_hashes = {acceptance_ref['repositoryPath']: sha_bytes(acceptance_bytes)}
    for record in documentation:
        data = git_blob(repo, docs, record['repositoryPath'])
        if sha_bytes(data) != record['sha256']:
            raise RuntimeError('Committed documentation blob differs')
        blob_hashes[record['repositoryPath']] = sha_bytes(data)
    changed_paths = {p.decode('utf-8') for p in git(repo, 'diff', '--name-only', '-z', source, docs).split(b'\0') if p}
    if not changed_paths <= set(blob_hashes):
        raise RuntimeError('Documentation commit contains an unbound changed path')
    external = {}
    correction_ref = acceptance.get('documentationCorrection')
    if correction_ref is not None:
        correction_path = Path(correction_ref['path'])
        correction_path = correction_path if correction_path.is_absolute() else repo / repo_path(correction_ref['path'])
        require_file(correction_path, correction_ref['sha256'])
        external[str(correction_path.resolve())] = correction_ref['sha256']
        correction = json.loads(correction_path.read_bytes())
        if (correction_ref['correctedAfterCodeRevision'] != source
                or correction['sourceCommit'] != source or correction['rustOrCargoInputChanges'] is not False):
            raise RuntimeError('Documentation correction is not bound to the accepted source')
        for row in correction['changes']:
            if blob_hashes.get(row['path']) != row['afterSha256']:
                raise RuntimeError('Documentation correction differs from the committed bound blob')
    frozen_ref = checkpoint['frozenBuild']
    frozen_path = Path(frozen_ref['path']).resolve()
    require_file(frozen_path, frozen_ref['sha256'])
    frozen = json.loads(frozen_path.read_bytes())
    if (frozen != acceptance['frozenBuild']
            or gates['frozenBuildSha256'] != frozen_ref['sha256']
            or frozen['allInputsUnchangedDuringBuild'] is not True):
        raise RuntimeError('Frozen build and committed acceptance disagree')
    inputs = frozen['allRustAndCargoInputs']
    if len(inputs) != 138 or not set(frozen['sources']) <= set(inputs):
        raise RuntimeError('Frozen source/input bindings are incomplete')
    for path, digest in inputs.items():
        for commit in (source, docs):
            if sha_bytes(git_blob(repo, commit, path)) != digest:
                raise RuntimeError('Frozen build input differs from committed source/docs: ' + path)
    if any(inputs[path] != digest for path, digest in frozen['sources'].items()):
        raise RuntimeError('Frozen primary source bindings disagree')
    external[str(frozen_path)] = frozen_ref['sha256']
    if (set(acceptance['binaries']) != {'cpg', 'joern-parity'}
            or acceptance['binaries'] != frozen['binaries']):
        raise RuntimeError('Frozen binary names or hashes disagree')
    for name, digest in acceptance['binaries'].items():
        # Tenth stores name->hash; frozenBuildReceipt binds their sibling bin.
        path = frozen_path.parent / name
        require_file(path, digest); external[str(path.resolve())] = digest
    return {'repository': str(repo), 'sourceCommit': source, 'documentationCommit': docs,
            'sourceTree': git(repo, 'rev-parse', source + '^{tree}').decode().strip(),
            'documentationTree': git(repo, 'rev-parse', docs + '^{tree}').decode().strip(),
            'committedBlobHashes': blob_hashes, 'documentationChangedPaths': sorted(changed_paths),
            'frozenInputHashes': inputs,
            'externalBindings': external, 'gitObjectAndAncestryChecksPassed': True}

def verify_checkpoint_admission(receipt, admission):
    # Pin the atomic tenth source/docs commit; the parent still must supply
    # a separately approved receipt. Git objects and every blob are checked below.
    if not all(isinstance(receipt.get(k), str) and COMMIT.fullmatch(receipt[k])
               for k in ('sourceCommit', 'documentationCommit')):
        raise RuntimeError('Committed tenth source and documentation IDs are required')
    if (receipt['sourceCommit'] != admission['sourceCommit']
            or receipt['documentationCommit'] != admission['documentationCommit']):
        raise RuntimeError('Release differs from the pinned accepted tenth commits')
    if Path(receipt.get('repository', '')).resolve() != Path(admission['repository']).resolve():
        raise RuntimeError('Release repository differs from the prepared checkpoint')
    if receipt.get('acceptance', {}).get('repositoryPath') != admission['acceptanceRepositoryPath']:
        raise RuntimeError('Release must bind the committed tenth acceptance')
    if not set(admission['requiredDocumentationPaths']) <= {
            row.get('repositoryPath') for row in receipt.get('documentationBlobs', [])}:
        raise RuntimeError('Committed tenth report and metrics bindings are required')
    frozen = receipt.get('frozenBuild', {})
    if (frozen.get('sha256') != admission['frozenBuild']['sha256']
            or Path(frozen.get('path', '')).resolve() != Path(admission['frozenBuild']['path']).resolve()):
        raise RuntimeError('Release must bind the prepared final tenth build')


def check_retained_anchors(root, selected, anchors):
    names = [row['case'] for row in anchors]
    if not names or len(set(names)) != len(names):
        raise RuntimeError('Distinct retained anchors are required')
    result = {}
    for row in anchors:
        path = Path(root) / row['referencePath']
        require_file(path, row['expectedSha256'])
        result[row['case']] = selected.get(row['case']) == path.read_bytes()
    return result


def verify_release(path, approved_sha, prepared_sha, admission):
    # Approval comes from the parent's separately supplied exact receipt hash,
    # never a user-selectable pair of unverified commit-shaped strings.
    if not approved_sha or not DIGEST.fullmatch(approved_sha):
        raise RuntimeError('An explicitly parent-approved release receipt SHA is required')
    require_file(path, approved_sha); receipt = json.loads(Path(path).read_bytes())
    if (receipt.get('schemaVersion') != 1
            or receipt.get('status') != 'PARENT_APPROVED_PRIMITIVE_MEMBER_PRODUCER_RELEASE'
            or receipt.get('producerHoldReleased') is not True
            or receipt.get('preparedManifestSha256') != prepared_sha):
        raise RuntimeError('Receipt does not release this exact prepared batch')
    verify_checkpoint_admission(receipt, admission)
    verification = verify_git_checkpoint(receipt)
    verification['releaseReceiptSha256'] = approved_sha
    verification['externalBindings'][str(Path(path).resolve())] = approved_sha
    return receipt, verification

def require_bound(path, expected):
    if isinstance(expected, dict):
        if inventory(path) != expected:
            raise RuntimeError('Frozen original directory inventory differs: ' + str(path))
    else:
        require_file(Path(path), expected)

def relevant_snapshot(root, run, external, runtime_spec=None):
    return {'prepared': prepared_snapshot(root),
            'copied': inventory(run / 'snapshot'),
            'runtime': runtime_snapshot(runtime_spec),
            'external': {str(path): (inventory(path) if isinstance(external[path], dict) else bind(Path(path)))
                         for path in sorted(external)}}

def run_process(command, workspace, env, stdout, stderr, timeout):
    process = None; timed_out = False; error = None
    try:
        process = subprocess.Popen(command, cwd=workspace, env=env, stdout=stdout,
                                   stderr=stderr, start_new_session=True)
        try:
            process.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            timed_out = True
            os.killpg(process.pid, signal.SIGTERM)
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL); process.wait()
    except BaseException as exc:
        error = type(exc).__name__ + ': ' + str(exc)
        if process is not None and process.poll() is None:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            process.wait()
    return {'exitCode': process.returncode if process else None, 'timedOut': timed_out,
            'processError': error}

def admissibility(process, complete, anchor_matches, errors, before, after, git_before, git_after):
    reasons = list(errors)
    if process.get('exitCode') != 0 or process.get('timedOut') or process.get('processError'):
        reasons.append('process_did_not_complete_successfully')
    if not complete:
        reasons.append('canonical_projection_incomplete')
    if anchor_matches is not True:
        reasons.append('retained_anchor_mismatch_or_missing')
    if before is None or after != before:
        reasons.append('relevant_file_inventory_or_bytes_changed')
    if git_before is None or git_after != git_before:
        reasons.append('accepted_checkpoint_unverified_or_changed')
    return not reasons, reasons

def write_selected(run, selected, kinds, admitted):
    # Called only after the final admissibility decision; HELD output can never
    # receive an expected filename or completeReference=true.
    output = run / ('expected' if admitted else 'held-selected'); output.mkdir()
    rows = []
    for name, content in sorted(selected.items()):
        folder = output / name; folder.mkdir()
        path = folder / ('expected.txt' if admitted else 'selected.non-reference.txt')
        path.write_bytes(content)
        rows.append({'case': name, 'path': str(path), **bind(path),
                     'canonicalLinesIncludingSeparators': content.count(b'\n'),
                     'nonemptySelectedRecords': sum(bool(line) for line in content.split(b'\n')),
                     'sectionsSeen': sorted(kinds[name]), 'completeReference': admitted})
    return rows

def attempt(args, root=ROOT):
    root = Path(root); run = root / 'runs' / args.run_name
    run.mkdir(parents=True, exist_ok=False)
    start = time.monotonic(); errors = []; before = None; after = None
    git_before = None; git_after = None; manifest = None; selected = {}; kinds = {}; complete = False
    anchor_matches = None; anchor_results = {}; external = {}; command = None; runtime_spec = None
    process = {'exitCode': None, 'timedOut': False, 'processError': None}
    # This snapshot precedes even manifest validation. Paths read here are only
    # observations, not trusted inputs or authorization. Malformed-manifest and
    # prelaunch failures still retain both complete observed inventories.
    external[str(Path(args.checkpoint_receipt).resolve())] = args.approved_receipt_sha256
    try:
        observed = json.loads((root / 'prepared.json').read_bytes())
        runtime_spec = observed.get('runtime')
        external[observed['joern']['executable']['path']] = observed['joern']['executable']['sha256']
        external.update({row['path']: row['sha256'] for row in observed.get('frozenOriginals', [])})
        external.update({row['path']: row['inventory'] for row in observed.get('frozenOriginalInventories', [])})
    except Exception:
        pass
    preflight_before = relevant_snapshot(root, run, external, runtime_spec)
    before = preflight_before
    write_json(run / 'preflight-before.json', preflight_before)
    receipt = {'status': 'HELD_PENDING_ALL_CHECKS', 'startUtc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
               'parentApprovedReceiptSha256': args.approved_receipt_sha256, 'producerStarted': False}
    write_json(run / 'started.json', receipt)
    # Raw output files exist even for prelaunch rejection; no failure path
    # bypasses the final inventory snapshot and immutable run.json receipt.
    with (run / 'live.stdout').open('xb') as out, (run / 'live.stderr').open('xb') as err:
        try:
            manifest, prepared_before = verify_prepared(root)
            runtime_spec = manifest['runtime']
            verify_runtime(runtime_spec)
            prepared_sha = sha(root / 'prepared.json')
            external = {manifest['joern']['executable']['path']: manifest['joern']['executable']['sha256'],
                        **{row['path']: row['sha256'] for row in manifest['frozenOriginals']},
                        **{row['path']: row['inventory'] for row in manifest['frozenOriginalInventories']},
                        str(Path(args.checkpoint_receipt).resolve()): args.approved_receipt_sha256}
            # Save an early complete inventory even if checkpoint/copy validation fails.
            before = relevant_snapshot(root, run, external, runtime_spec)
            checkpoint, git_before = verify_release(args.checkpoint_receipt, args.approved_receipt_sha256, prepared_sha, manifest['checkpointAdmission'])
            external.update(git_before['externalBindings'])
            for path, digest in external.items():
                require_bound(Path(path), digest)
            snap = run / 'snapshot'; snap.mkdir()
            shutil.copytree(root / 'input', snap / 'input')
            for name in ['oracle.sc', 'prepared.json', 'run-oracle.py']:
                shutil.copy2(root / name, snap / name)
            shutil.copytree(root / 'retained-anchors', snap / 'retained-anchors')
            shutil.copy2(args.checkpoint_receipt, snap / 'checkpoint-release.json')
            frozen_oracle_sha = manifest['oracle']['sha256']
            require_file(snap / 'oracle.sc', frozen_oracle_sha)
            require_file(snap / 'prepared.json', prepared_sha)
            require_file(snap / 'run-oracle.py', sha(root / 'run-oracle.py'))
            if inventory(snap / 'retained-anchors') != inventory(root / 'retained-anchors'):
                raise RuntimeError('Copied retained-anchor inventory differs')
            require_file(snap / 'checkpoint-release.json', args.approved_receipt_sha256)
            if inventory(snap / 'input') != inventory(root / 'input'):
                raise RuntimeError('Copied input inventory differs')
            if prepared_snapshot(root) != prepared_before:
                raise RuntimeError('Prepared files changed during prelaunch checks')
            before = relevant_snapshot(root, run, external, runtime_spec)
            for path, digest in external.items():
                require_bound(Path(path), digest)
            require_file(snap / 'oracle.sc', frozen_oracle_sha)
            write_json(run / 'before.json', before)
            workspace = run / 'workspace'; workspace.mkdir()
            command = [manifest['joern']['executable']['path'], '--script', str(snap / 'oracle.sc'),
                       '--param', 'inputPath=' + str(snap / 'input')]
            env = dict(os.environ); env['JAVA_HOME'] = manifest['jdk21Home']
            env['PATH'] = runtime_spec['launchPath']
            verify_runtime(runtime_spec)
            receipt['producerStarted'] = True
            process = run_process(command, workspace, env, out, err, args.timeout_seconds)
        except BaseException as exc:
            errors.append('prelaunch_or_execution_exception: ' + type(exc).__name__ + ': ' + str(exc))
        finally:
            after = relevant_snapshot(root, run, external, runtime_spec)
            write_json(run / 'after.json', after)
            if not (run / 'before.json').exists():
                write_json(run / 'before.json', before)
            try:
                # This is repeated even for nonzero/timeout/failure outcomes.
                prepared_after, _ = verify_prepared(root)
                verify_runtime(runtime_spec)
                if manifest is not None and prepared_after != manifest:
                    raise RuntimeError('Prepared manifest changed')
                _, git_after = verify_release(args.checkpoint_receipt, args.approved_receipt_sha256,
                                              sha(root / 'prepared.json'), manifest['checkpointAdmission'])
            except BaseException as exc:
                errors.append('postrun_binding_check: ' + type(exc).__name__ + ': ' + str(exc))
    raw_binding = bind(run / 'live.stdout')
    if manifest is not None:
        try:
            selected, kinds, complete = extract_selected((run / 'live.stdout').read_bytes(), manifest['cases'])
            anchor_results = check_retained_anchors(root, selected, manifest['anchors'])
            anchor_matches = all(anchor_results.values())
        except BaseException as exc:
            errors.append('extraction_or_anchor_check: ' + type(exc).__name__ + ': ' + str(exc))
    # Recheck inventories after extraction/checkpoint reads and immediately
    # before promotion; do not trust a stale copied-script or driver snapshot.
    final_snapshot = relevant_snapshot(root, run, external, runtime_spec)
    if final_snapshot != after:
        errors.append('bindings_changed_during_finalization')
    after = final_snapshot
    if bind(run / 'live.stdout') != raw_binding:
        errors.append('raw_output_changed_during_extraction')
    admitted, reasons = admissibility(process, complete, anchor_matches, errors, before, after, git_before, git_after)
    rows = write_selected(run, selected, kinds, admitted)
    receipt.update({'status': 'COMPLETE_RAW_REFERENCE_BATCH' if admitted else 'HELD_NON_REFERENCE_BATCH',
                    'admittedAsReferences': admitted, 'allCasesComplete': admitted,
                    'extractedProjectionComplete': complete, 'holdReasons': reasons,
                    **process, 'command': command, 'seconds': time.monotonic() - start,
                    'rawStdout': {'path': str(run / 'live.stdout'), **raw_binding},
                    'rawStderr': {'path': str(run / 'live.stderr'), **bind(run / 'live.stderr')},
                    'preflightBefore': preflight_before, 'before': before, 'after': after,
                    'gitBefore': git_before, 'gitAfter': git_after,
                    'retainedAnchorByteIdentical': anchor_matches, 'retainedAnchors': anchor_results, 'outputs': rows,
                    'conversion': 'LF-only split/join and AST| prefix removal; no trimming, deduplication or graph filtering'})
    write_json(run / 'run.json', receipt)
    print(json.dumps({'receipt': str(run / 'run.json'), 'sha256': sha(run / 'run.json'),
                      'status': receipt['status'], 'producerStarted': receipt['producerStarted']}))
    return 0 if admitted else 1

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--verify-only', action='store_true')
    parser.add_argument('--producer-hold-released', action='store_true')
    parser.add_argument('--checkpoint-receipt')
    parser.add_argument('--approved-receipt-sha256')
    parser.add_argument('--run-name')
    parser.add_argument('--timeout-seconds', type=int, default=180)
    args = parser.parse_args()
    if args.verify_only:
        manifest, _ = verify_prepared()
        verify_runtime(manifest['runtime'])
        print(json.dumps({'status': 'PREPARED_VERIFIED_NO_PRODUCER', 'projects': len(manifest['cases']),
                          'preparedManifestSha256': sha(ROOT / 'prepared.json')})); return 0
    if not args.producer_hold_released:
        parser.error('Producer hold active; exact parent release receipt and explicit release required')
    if not args.checkpoint_receipt or not args.approved_receipt_sha256:
        parser.error('Parent checkpoint receipt path and approved SHA are required')
    if not args.run_name or not re.fullmatch(r'[a-zA-Z0-9_-]+', args.run_name):
        parser.error('A new alphanumeric run name is required')
    if not 1 <= args.timeout_seconds <= 600:
        parser.error('Timeout must be between 1 and 600 seconds')
    return attempt(args)

if __name__ == '__main__':
    sys.exit(main())
