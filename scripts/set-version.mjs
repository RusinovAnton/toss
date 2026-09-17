// Writes a version into every file that carries one.
//
// The git tag is what names a release, so CI calls this before building and
// nobody has to remember to bump anything by hand. `src-tauri/tauri.conf.json`
// reads its version from package.json, so it is not listed here.
//
//   node scripts/set-version.mjs 0.2.0
import { readFileSync, writeFileSync } from "node:fs";

const version = (process.argv[2] ?? "").replace(/^v/, "");
if (!/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(version)) {
  console.error(`not a version: ${process.argv[2] ?? "(nothing given)"}`);
  process.exit(1);
}

/** Replaces the first match and complains if there was none. */
function edit(path, pattern, replacement) {
  const before = readFileSync(path, "utf8");
  const after = before.replace(pattern, replacement);
  if (after === before && !pattern.test(before)) {
    console.error(`no version field found in ${path}`);
    process.exit(1);
  }
  writeFileSync(path, after);
}

edit("package.json", /("version":\s*")[^"]+(")/, `$1${version}$2`);
edit("src-tauri/Cargo.toml", /(\nversion = ")[^"]+(")/, `$1${version}$2`);
// Keeping the lock file in step means a build never has to rewrite it.
edit(
  "src-tauri/Cargo.lock",
  /(\[\[package\]\]\nname = "toss"\nversion = ")[^"]+(")/,
  `$1${version}$2`,
);

console.log(`version set to ${version}`);
