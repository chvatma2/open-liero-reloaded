// Runs a wasm32-wasip1 binary with Node.js's WASI: node tools/wasi-run.mjs <file.wasm> <dir to preopen as /> [args...]
import { readFile } from 'node:fs/promises';
import { WASI } from 'node:wasi';
import { argv, exit } from 'node:process';

const [wasmPath, root, ...args] = argv.slice(2);
const wasi = new WASI({ version: 'preview1', args: ['rl-detcheck', ...args], preopens: { '/': root }, returnOnExit: true });
const module = await WebAssembly.compile(await readFile(wasmPath));
const instance = await WebAssembly.instantiate(module, wasi.getImportObject());
exit(wasi.start(instance));
