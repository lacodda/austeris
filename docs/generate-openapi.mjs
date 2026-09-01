// Writes the OpenAPI document the site renders its API reference from.
//
// The spec comes from the binary rather than from a file in the tree: a
// checked-in copy is one nothing keeps honest, and the reference would then
// describe whatever the last person remembered to regenerate.
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';

const OUT = 'openapi.json';

try {
	// `--quiet` so cargo's progress does not land in the JSON.
	const spec = execFileSync('cargo', ['run', '--quiet', '--bin', 'austeris', '--', 'openapi'], {
		cwd: '..',
		encoding: 'utf8',
		maxBuffer: 32 * 1024 * 1024,
	});
	// Fail here rather than halfway through the site build - and check the
	// document is a document, not merely valid JSON. An empty spec renders an
	// empty API section, and the build stays green while the reference is gone.
	const document = JSON.parse(spec);
	const paths = Object.keys(document.paths ?? {});
	if (paths.length === 0) {
		throw new Error('the spec carries no paths; the API reference would be empty');
	}

	fs.writeFileSync(OUT, spec);
	console.log(`wrote ${OUT} with ${paths.length} paths`);
} catch (error) {
	console.error('could not generate the OpenAPI document:', error.message);
	process.exit(1);
}
