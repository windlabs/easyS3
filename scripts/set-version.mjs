#!/usr/bin/env node
// 同步项目版本号到 package.json、Cargo.toml 与 src-tauri/tauri.conf.json
// 用法：node scripts/set-version.mjs 1.2.3
import { readFileSync, writeFileSync } from "node:fs";

const version = process.argv[2];

if (!version || !/^[0-9]+\.[0-9]+\.[0-9]+(?:[-+][0-9A-Za-z.-]+)?$/.test(version)) {
  console.error(
    "用法: node scripts/set-version.mjs <版本号>，版本需符合 SemVer，例如 1.2.3 或 1.2.3-beta.1"
  );
  process.exit(1);
}

for (const file of ["package.json", "src-tauri/tauri.conf.json"]) {
  const data = JSON.parse(readFileSync(file, "utf8"));
  data.version = version;
  writeFileSync(file, JSON.stringify(data, null, 2) + "\n");
}

const cargo = readFileSync("Cargo.toml", "utf8");
const versionRe = /(\[workspace\.package\][\s\S]*?\nversion\s*=\s*)"[^"]*"/;
if (!versionRe.test(cargo)) {
  console.error("未在 Cargo.toml 的 [workspace.package] 中找到 version 字段");
  process.exit(1);
}
const updated = cargo.replace(versionRe, (_, prefix) => `${prefix}"${version}"`);
writeFileSync("Cargo.toml", updated);
console.log(
  updated === cargo ? `项目版本已为 ${version}，无需更新` : `项目版本已更新为 ${version}`
);
