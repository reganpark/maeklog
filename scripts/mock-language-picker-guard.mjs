#!/usr/bin/env node
// Isolated language-picker guard: records any forbidden Codex/GitHub/opener call.
import {appendFileSync} from 'node:fs';
if(!process.env.MAEKLOG_LANGUAGE_TEST_CALLS)throw Error('isolated call path required');
appendFileSync(process.env.MAEKLOG_LANGUAGE_TEST_CALLS,JSON.stringify(process.argv.slice(2))+'\n');
console.error('Unexpected external invocation in language-picker fixture');
process.exit(1);
