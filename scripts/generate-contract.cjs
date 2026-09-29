const fs = require('node:fs');
const path = require('node:path');
const root = path.join(__dirname, '..', 'mcp');
const spec = JSON.parse(fs.readFileSync(path.join(root, 'openapi.json'), 'utf8'));
function expand(value) {
  if (Array.isArray(value)) return value.map(expand);
  if (!value || typeof value !== 'object') return value;
  if (value.$ref) {
    if (!value.$ref.startsWith('#/components/schemas/')) throw Error('Only local schema references are supported');
    const name = value.$ref.split('/').at(-1);
    if (!spec.components.schemas[name]) throw Error('Missing schema: ' + name);
    return expand(spec.components.schemas[name]);
  }
  return Object.fromEntries(Object.entries(value).map(([k, v]) => [k, expand(v)]));
}
const tools = Object.entries(spec.paths).map(([route, methods]) => {
  const operation = methods.post;
  const schema = expand(operation.requestBody.content['application/json'].schema);
  schema.additionalProperties = false;
  return {name: operation.operationId, description: operation.summary, inputSchema: schema,
    annotations: {readOnlyHint: !!operation['x-read-only'], destructiveHint: !!operation['x-destructive'], openWorldHint: false},
    'x-openapi': {path: route, method: 'post'}};
});
if (new Set(tools.map(t => t.name)).size !== tools.length) throw Error('Duplicate tool names');
const content = JSON.stringify({server: 'forma', version: spec.info.version, tools}, null, 2) + '\n';
const target = path.join(root, 'tool_manifest.json');
if (process.argv.includes('--check')) {
  if (fs.readFileSync(target, 'utf8').replace(/\r\n/g, '\n') !== content) throw Error('Regenerate tool manifest with npm run mcp:contract');
} else fs.writeFileSync(target, content);
console.log('Forma contract: ' + tools.length + ' tools');
