"""Derive the complete observed include components; never edit oracle snapshots."""
from pathlib import Path
import argparse, hashlib, json

ROOT = Path(__file__).resolve().parent

def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def properties(node):
    result = {}
    for name, value in node['properties'].items():
        if value['kind'] == 'string':
            result[name] = value['value']
        else:
            assert value['kind'] == 'number' and value['class'] == 'java.lang.Integer'
            assert name in {'COLUMN_NUMBER', 'LINE_NUMBER', 'ORDER'}
            integer = int(value['value'])
            assert str(integer) == value['value'] and -(2**31) <= integer < 2**31
            if name == 'LINE_NUMBER':
                assert integer >= 0
            result[name] = integer
    return result

def component(raw):
    rows = [json.loads(line) for line in raw.split(b'\n') if line]
    nodes = {row['id']: row for row in rows if 'id' in row}
    assert len(nodes) == sum('id' in row for row in rows)
    edges = [row for row in rows if 'source' in row]
    imports = [node for node in nodes.values() if node['label'] == 'IMPORT']
    dependencies = {node['id'] for node in nodes.values() if node['label'] == 'DEPENDENCY'}
    seen_dependencies = set()
    pairs = []
    owner_orders = {}
    for imported in imports:
        parents = [edge['source'] for edge in edges
                   if edge['destination'] == imported['id'] and edge['label'] == 'AST']
        targets = [edge['destination'] for edge in edges
                   if edge['source'] == imported['id'] and edge['label'] == 'IMPORTS']
        assert len(parents) == len(targets) == 1
        owner = nodes[parents[0]]
        dependency = nodes[targets[0]]
        assert owner['label'] == 'NAMESPACE_BLOCK' and dependency['label'] == 'DEPENDENCY'
        assert dependency['id'] not in seen_dependencies
        seen_dependencies.add(dependency['id'])
        endpoint_names = {owner['id']: 'owner', imported['id']: 'import', dependency['id']: 'dependency'}
        selected = {imported['id'], dependency['id']}
        incident = [edge for edge in edges if edge['source'] in selected or edge['destination'] in selected]
        bridged_edges = []
        for edge in incident:
            # The actual Joern payload is null. Rust does not store edge payloads;
            # only complete edge kinds/endpoints/multiplicity are compared.
            assert edge['property'] == {'kind': 'null'}
            bridged_edges.append([endpoint_names[edge['source']], edge['label'], endpoint_names[edge['destination']]])
        owner_props = owner['properties']
        owner_view = {key: owner_props[key]['value'] for key in ['NAME', 'FULL_NAME', 'FILENAME']}
        assert all(owner_props[key]['kind'] == 'string' for key in owner_view)
        assert owner_view['NAME'] == '<global>'
        global_types = [nodes[edge['destination']] for edge in edges
                        if edge['source'] == owner['id'] and edge['label'] == 'AST'
                        and nodes[edge['destination']]['label'] == 'TYPE_DECL'
                        and nodes[edge['destination']]['properties'].get('NAME', {}).get('value') == '<global>']
        assert len(global_types) == 1
        global_type = global_types[0]
        order = global_type['properties']['ORDER']
        assert order['kind'] == 'number' and order['class'] == 'java.lang.Integer'
        dense_order = int(order['value'])
        assert str(dense_order) == order['value'] and -(2**31) <= dense_order < 2**31
        owner_orders[owner['id']] = {'fullName': global_type['properties']['FULL_NAME']['value'],
                                    'file': owner_view['FILENAME'], 'denseOrder': dense_order}
        pairs.append({'owner': owner_view, 'import': properties(imported),
                      'dependency': properties(dependency), 'incidentEdges': sorted(bridged_edges)})
    assert dependencies == seen_dependencies
    pairs.sort(key=lambda value: json.dumps(value, sort_keys=True, separators=(',', ':')))
    return {'schemaVersion': 1, 'pairs': pairs,
            'ownerGlobalTypeDeclOrders': sorted(owner_orders.values(), key=lambda value: json.dumps(value, sort_keys=True)),
            'integerBridge': 'Exact integer values: observed Java Integer is checked in i32 domain; nonnegative LINE_NUMBER is represented by Rust u32. Java class names remain only in the untouched raw reference.',
            'absenceBridge': 'Every observed Import/Dependency property is retained. Omitted properties remain omitted; ORDER requires explicit Absent or Present, never Unknown.',
            'globalOrderBoundary': 'The existing owner global TYPE_DECL dense ORDER value is compared separately. No claim is made that its Joern ORDER presence is stored.',
            'edgePayloadBoundary': 'All observed incident payloads are null; Rust payload storage is unsupported. The gate compares every incident edge kind, endpoint and occurrence, not payload storage.'}

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--verify', action='store_true')
    args = parser.parse_args()
    rows = []
    for case in sorted((ROOT/'cases').iterdir()):
        raw = case/'joern-supplement.jsonl'
        value = component(raw.read_bytes())
        encoded = (json.dumps(value, indent=2, ensure_ascii=False, sort_keys=True) + '\n').encode()
        target = case/'expected-include-metadata.json'
        if args.verify:
            assert target.read_bytes() == encoded, str(target)
        else:
            with target.open('xb') as handle:
                handle.write(encoded)
        rows.append({'case': case.name, 'rawSha256': sha(raw), 'derivedSha256': sha(target), 'pairs': len(value['pairs'])})
    print(json.dumps({'status': 'VERIFIED' if args.verify else 'DERIVED_FROM_COMPLETE_RAW_OBSERVATIONS', 'cases': rows}))

if __name__ == '__main__':
    main()
