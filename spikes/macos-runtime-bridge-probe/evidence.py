import hashlib
import json
from pathlib import Path
import sys

source=Path(__file__).resolve().parent
repo=source.parent.parent
output=Path(sys.argv[1])
target=Path(sys.argv[2])
def sha(path): return hashlib.sha256(path.read_bytes()).hexdigest()
files=sorted(path for path in source.rglob('*') if path.is_file() and not {'target','__pycache__'}.intersection(path.parts))
shared=['identity.rs','request_contract.rs','call_session.rs','data_call_gate.rs','manifest.rs','runtime.rs','scheduler.rs',
        'data_scope.rs','storage_document.rs','private_file_macos.rs','storage_macos.rs','storage_macos_tests.rs',
        'settings_values.rs','settings_macos.rs','settings_macos_tests.rs','runtime_data_macos.rs','runtime_data_macos_tests.rs',
        'main_result_contract.rs','main_result_contract_tests.rs','runtime_error.rs','runtime_result_macos.rs','runtime_result_macos_tests.rs',
        'main_result_macos.rs','main_result_macos_tests.rs','activation.rs',
        'package.rs','package_macos.rs','package_macos_tests.rs','package_rules.rs','package_zip_macos.rs','icon.rs',
        'lifecycle_macos.rs','lifecycle_macos_tests.rs','lifecycle_registry_macos_tests.rs']
files += [repo/'src-tauri/src/public_plugins'/name for name in shared]
files += [repo/'src-tauri/src'/name for name in ['public_plugins.rs','public_plugins_macos.rs']]
files += [repo/'src-tauri/src'/name for name in ['atomic_file_macos.rs','result_registry.rs','model.rs']]
files += [repo/'examples/public-plugins/com.uipilot.demo-settings/package'/name for name in ['plugin.json','runtime.js']]
(output/'source-SHA256SUMS').write_text(''.join(f'{sha(path)}  {path.relative_to(repo)}\n' for path in files))
records=[]
for line in (output/'tests.log').read_text().splitlines():
    if 'BRIDGE_CASE ' in line: records.append(json.loads(line.split('BRIDGE_CASE ',1)[1]))
(output/'bridge-cases.json').write_text(json.dumps(records,indent=2)+'\n')
metadata=json.loads((output/'cargo-metadata.json').read_text())
packages={item['id']:item for item in metadata['packages']}
features={packages[node['id']]['name']:node['features'] for node in metadata['resolve']['nodes'] if packages[node['id']]['name'] in ['rquickjs','rquickjs-core','rquickjs-sys']}
assert features=={'rquickjs':['loader','std'],'rquickjs-core':['loader','relative-path','std'],'rquickjs-sys':[]}
assert records and all(row['report']['reaped'] for row in records)
(output/'artifact-evidence.json').write_text(json.dumps({
    'testedHelper':'helper','testedSignedHelperSha256':sha(output/'helper'),
    'cargoTargetBeforeResigningSha256':sha(target/'debug/uipilot-runtime-bridge-probe'),
    'probeCargoLockSha256':sha(source/'Cargo.lock'),'productCargoLockSha256':sha(repo/'src-tauri/Cargo.lock'),
    'rquickjs':'0.14.0','features':features,'actualProcessCases':len(records),
},indent=2)+'\n')
