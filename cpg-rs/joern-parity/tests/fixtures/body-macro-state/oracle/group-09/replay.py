from pathlib import Path
import datetime
import difflib
import hashlib
import json
import subprocess
import sys
import time

p = Path(__file__).resolve().parent
binary = Path(sys.argv[1]).resolve()
variant = sys.argv[2]
assert variant and Path(variant).name == variant
sha = lambda q: hashlib.sha256(q.read_bytes()).hexdigest()
binary_before = sha(binary)
out = p / variant
out.mkdir()
rows = []
for d in sorted((p / 'input').iterdir()):
    inputs = sorted(f for f in d.rglob('*') if f.suffix in ['.c', '.h'])
    before = {str(f.relative_to(d)): sha(f) for f in inputs}
    reference = d / 'expected.txt'
    expected = reference.read_bytes()
    expected_sha = sha(reference)
    command = [str(binary), *map(str, inputs)]
    q = out / d.name
    q.mkdir()
    started = datetime.datetime.now(datetime.timezone.utc).isoformat()
    begin = time.monotonic()
    timed_out = False
    with (q / 'actual.txt').open('xb') as stdout, (q / 'stderr.txt').open('xb') as stderr:
        proc = subprocess.Popen(command, stdout=stdout, stderr=stderr)
        try:
            exit_code = proc.wait(timeout=30)
        except subprocess.TimeoutExpired:
            timed_out = True
            proc.kill()
            exit_code = proc.wait()
    actual = (q / 'actual.txt').read_bytes()
    diff = '\n'.join(difflib.unified_diff(
        expected.decode().split('\n'), actual.decode().split('\n'),
        fromfile='joern', tofile=variant, lineterm=''))
    if diff:
        diff += '\n'
    with (q / 'complete.diff').open('x') as f:
        f.write(diff)
    after_inputs = sorted(f for f in d.rglob('*') if f.suffix in ['.c', '.h'])
    after = {str(f.relative_to(d)): sha(f) for f in after_inputs}
    row = {
        'case': d.name, 'command': command, 'startedAtUtc': started,
        'exitCode': exit_code, 'timedOut': timed_out,
        'wallSeconds': time.monotonic() - begin,
        'exact': exit_code == 0 and actual == expected,
        'inputHashesBefore': before, 'inputHashesAfter': after,
        'expectedSha256Before': expected_sha, 'expectedSha256After': sha(reference),
        'actualSha256': sha(q / 'actual.txt'), 'stderrSha256': sha(q / 'stderr.txt'),
        'completeDiffSha256': sha(q / 'complete.diff'),
        'binarySha256Before': binary_before, 'binarySha256After': sha(binary),
    }
    with (q / 'run.json').open('x') as f:
        json.dump(row, f, indent=2)
    assert before == after and expected_sha == sha(reference)
    assert binary_before == sha(binary)
    assert exit_code == 0 and not timed_out, row
    rows.append(row)
    print(json.dumps({'case': d.name, 'exitCode': exit_code, 'exact': row['exact']}), flush=True)
receipt = {
    'binary': {'path': str(binary), 'sha256': binary_before},
    'liveReceipt': {'path': str(p / 'live-run.json'), 'sha256': sha(p / 'live-run.json')},
    'helper': {'path': str(Path(__file__)), 'sha256': sha(Path(__file__))},
    'results': rows,
}
with (out / 'replay.json').open('x') as f:
    json.dump(receipt, f, indent=2)
