"""Validate public JSON Schema with Draft 2020-12 and shared Rust fixtures.
Install test-only dependencies in target/cli-validation-deps; never a runtime dependency.
"""
import copy
import json
from pathlib import Path
import sys
ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'target/cli-validation-deps'))
from jsonschema import Draft202012Validator
schema = json.loads((ROOT / 'crates/luciddesk-api/protocol.schema.json').read_text(encoding='utf-8'))
Draft202012Validator.check_schema(schema)
validator = Draft202012Validator(schema)
operations = json.loads((ROOT / 'crates/luciddesk-api/tests/fixtures/operations.json').read_text(encoding='utf-8'))
base = dict(instance_id='instance',state_version='1',inventory_version='2',topology_token='3')
count = 0
for op in operations:
    request = dict(protocol_version=1,request_id='fixture',command='plan.preview',plan=dict(protocol_version=1,base=base,operations=[op]))
    validator.validate(request)
    invalid = copy.deepcopy(request)
    invalid['plan']['operations'][0]['unexpected'] = True
    assert not validator.is_valid(invalid), op
    invalid = copy.deepcopy(request)
    invalid['plan']['operations'][0]['op'] = 'unsupported'
    assert not validator.is_valid(invalid), op
    count += 3
for command in schema['$defs']['request']['properties']['command']['enum']:
    request = dict(protocol_version=1, request_id='fixture', command=command)
    if command in ['pane.get','folder.get','search.get','request.get']: request['id']='1'
    if command == 'plan.apply': request['token']='token'
    if command == 'plan.preview': request['plan']=dict(protocol_version=1,base=base,operations=[operations[0]])
    validator.validate(request)
    count += 1
print(f'Passed {count} schema fixtures across {len(operations)} operations')
