"""Derive Binding assertions only from complete admitted stored-CPG snapshots.

No source interpretation, property normalization, or graph producer is used.
The raw snapshot retains unsupported endpoint properties and null edge payloads.
"""
from pathlib import Path
import argparse
import hashlib
import json

CASES = ['function_like_repeated_use', 'local_object_used_unused',
         'repeated_direct_include', 'shared_header_two_callers']

def encode(value):
    return json.dumps(value, sort_keys=True, ensure_ascii=False, separators=(',', ':'))

def strings(node, names=None):
    props = node['properties']
    selected = props if names is None else {name: props[name] for name in names}
    result = {}
    for name, typed in selected.items():
        assert typed['kind'] == 'string' and set(typed) == {'kind', 'value'}, (name, typed)
        result[name] = typed['value']
    return result

def derive(raw):
    rows = [json.loads(line) for line in raw.split(b'\n') if line]
    nodes = {row['id']: row for row in rows if 'id' in row}
    edges = [row for row in rows if 'source' in row]
    assert len(nodes) + len(edges) == len(rows)
    assert all(edge['source'] in nodes and edge['destination'] in nodes for edge in edges)
    bindings = []
    for node in nodes.values():
        if node['label'] != 'BINDING':
            continue
        incident = [edge for edge in edges if node['id'] in (edge['source'], edge['destination'])]
        owners = [nodes[edge['source']] for edge in incident
                  if edge['label'] == 'BINDS' and edge['destination'] == node['id']]
        targets = [nodes[edge['destination']] for edge in incident
                   if edge['label'] == 'REF' and edge['source'] == node['id']]
        assert len(owners) == len(targets) == 1
        owner, target = owners[0], targets[0]
        assert owner['label'] == 'TYPE_DECL' and target['label'] == 'METHOD'
        endpoints = {owner['id']: 'owner', target['id']: 'method', node['id']: 'binding'}
        represented_edges = []
        for edge in incident:
            assert edge['property'] == {'kind': 'null'}, edge
            represented_edges.append([endpoints[edge['source']], edge['label'], endpoints[edge['destination']]])
        # All Binding properties, including their exact missingness, enter the
        # assertion. None of these four actual cases has non-string properties.
        props = strings(node)
        assert set(props) == {'NAME', 'METHOD_FULL_NAME', 'SIGNATURE'}
        method = strings(target, ['NAME', 'FULL_NAME', 'CODE', 'SIGNATURE', 'FILENAME'])
        is_macro = method['CODE'].startswith('#define')
        bindings.append({
            'macro': is_macro,
            'binding': props,
            'owner': strings(owner, ['NAME', 'FULL_NAME', 'FILENAME']),
            'method': method,
            'incomingCallCount': sum(edge['label'] == 'CALL' and edge['destination'] == target['id'] for edge in edges),
            'incidentEdges': sorted(represented_edges, key=encode),
        })
    assert sum(row['macro'] for row in bindings) == 1
    return {'bindings': sorted(bindings, key=encode)}

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('family', type=Path)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    summaries = []
    for name in CASES:
        folder = args.family / 'cases' / name
        raw = (folder / 'supplement.jsonl').read_bytes()
        result = derive(raw)
        expected = (json.dumps(result, indent=2, sort_keys=True, ensure_ascii=False) + '\n').encode()
        path = folder / 'expected-binding-metadata.json'
        if args.check:
            assert path.read_bytes() == expected, name
        else:
            with path.open('xb') as stream:
                stream.write(expected)
        summaries.append({'case': name, 'rawSnapshotSha256': hashlib.sha256(raw).hexdigest(),
                          'derivedSha256': hashlib.sha256(expected).hexdigest(),
                          'bindings': len(result['bindings']), 'macroBindings': 1})
    print(json.dumps({'status': 'VERIFIED_DERIVATION' if args.check else 'DERIVED_FROM_ADMITTED_SNAPSHOT',
                      'cases': summaries, 'producerRun': False}, indent=2))

if __name__ == '__main__':
    main()
