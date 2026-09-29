const Ajv = require('ajv');
const manifest = require('./tool_manifest.json');
const ajv = new Ajv({allErrors: true, strict: true});
const validators = new Map(manifest.tools.map(tool => [tool.name, ajv.compile(tool.inputSchema)]));
function fail(code, message) { return Object.assign(new Error(message), {code}); }
function validate(name, args) {
  const check = validators.get(name);
  if (!check) throw fail('UNKNOWN_TOOL', 'Unknown Forma tool: ' + name);
  if (!check(args)) throw fail('INVALID_ARGUMENTS', ajv.errorsText(check.errors, {separator: '; '}));
}
module.exports = {manifest, validate, fail};
