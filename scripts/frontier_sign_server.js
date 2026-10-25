#!/usr/bin/env node
// Keep fp.data loaded; each stdin line is X-MS-STUB, stdout is X-Bogus.
const fs = require("fs");
const readline = require("readline");
const vm = require("vm");

const fp = process.argv[2] || "";
if (!fp) {
  process.stderr.write("usage: frontier_sign_server.js <fp.data>\n");
  process.exit(2);
}

let code = fs.readFileSync(fp, "utf8");
code = code.replace(/catvm\.print\.getAll\(\);/g, "");
code = code.replace(/debugger;/g, "");

const noop = () => {};
const sandbox = {
  console: {
    log: noop, warn: noop, error: noop, info: noop, debug: noop,
    table: noop, dir: noop, time: noop, timeEnd: noop,
  },
  setTimeout, clearTimeout, setInterval, clearInterval,
  Date, Math, JSON, Number, String, Boolean, Array, Object, RegExp,
  Error, TypeError, Map, Set, WeakMap, WeakSet, Promise, Symbol,
  Reflect, Proxy, parseInt, parseFloat, isNaN, encodeURIComponent,
  decodeURIComponent, undefined,
};
sandbox.globalThis = sandbox;
sandbox.window = sandbox;
sandbox.self = sandbox;
sandbox.top = sandbox;
sandbox.parent = sandbox;

vm.runInNewContext(code, sandbox, { timeout: 20000 });
const ac = sandbox.byted_acrawler;
if (!ac || typeof ac.frontierSign !== "function") {
  process.stderr.write("frontierSign missing\n");
  process.exit(3);
}
process.stderr.write("frontier_sign_server ready\n");

const rl = readline.createInterface({ input: process.stdin });
rl.on("line", (stub) => {
  stub = String(stub || "").trim();
  if (!stub) {
    process.stdout.write("\n");
    return;
  }
  try {
    const out = ac.frontierSign({ "X-MS-STUB": stub });
    const sig = (out && (out["X-Bogus"] || out["X-bogus"])) || "";
    process.stdout.write(String(sig) + "\n");
  } catch (e) {
    process.stderr.write(String(e) + "\n");
    process.stdout.write("\n");
  }
});
