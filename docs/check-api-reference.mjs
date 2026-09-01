// Fails when the built site has no API reference.
//
// The plugin that renders it is configured inside Starlight rather than beside
// it, and getting that wrong produces a green build with the section silently
// missing - which is exactly what happened once. A build that "succeeds" while
// dropping a third of the site is worse than one that fails.
import fs from 'node:fs';
import path from 'node:path';

const OPERATIONS = 'dist/api/operations';

if (!fs.existsSync(OPERATIONS)) {
	console.error(`${OPERATIONS} is missing: the API reference was not rendered`);
	process.exit(1);
}

const pages = fs.readdirSync(OPERATIONS).filter((entry) => fs.statSync(path.join(OPERATIONS, entry)).isDirectory());
if (pages.length === 0) {
	console.error(`${OPERATIONS} is empty: the spec reached the plugin but produced nothing`);
	process.exit(1);
}

console.log(`the API reference documents ${pages.length} operations`);
