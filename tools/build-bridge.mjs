// Bundle the parser with the bridge. Runtime never resolves target-profile modules.
import { build } from 'rolldown';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
await build({input:path.join(root,'src-tauri/src/dsh_bridge.cjs'),platform:'node',output:{file:path.join(root,'src-tauri/src/dsh_bridge.bundle.cjs'),format:'cjs',minify:true}});
console.log('BRIDGE_BUNDLE_PASS parser=bundled target_profile_modules=NONE');
