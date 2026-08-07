import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const read = (relative) => fs.readFileSync(path.join(root, relative), "utf8");
const fail = (message) => {
  throw new Error(`release verification failed: ${message}`);
};
const requireText = (text, needle, label) => {
  if (!text.includes(needle)) fail(`${label} is missing ${JSON.stringify(needle)}`);
};

const pkg = JSON.parse(read("package.json"));
const lock = JSON.parse(read("package-lock.json"));
const tauri = JSON.parse(read("src-tauri/tauri.conf.json"));
const cargo = read("src-tauri/Cargo.toml");
const cargoLock = read("src-tauri/Cargo.lock");
const cargoVersion = cargo.match(/name = "coding-tools-mcp-desktop"\r?\nversion = "([^"]+)"/)?.[1];
const cargoLockVersion = cargoLock.match(/name = "coding-tools-mcp-desktop"\r?\nversion = "([^"]+)"/)?.[1];
const versions = {
  package: pkg.version,
  packageLock: lock.version,
  packageLockRoot: lock.packages?.[""]?.version,
  cargo: cargoVersion,
  cargoLock: cargoLockVersion,
  tauri: tauri.version,
};
const uniqueVersions = new Set(Object.values(versions));
if (uniqueVersions.size !== 1 || uniqueVersions.has(undefined)) {
  fail(`version mismatch: ${JSON.stringify(versions)}`);
}
const version = pkg.version;

const appLinks = read("src/lib/app-links.ts");
const updateModule = read("src-tauri/src/update/mod.rs");
requireText(appLinks, "https://github.com/LostInsight/coding-tools-mcp", "frontend repo link");
requireText(
  updateModule,
  "https://api.github.com/repos/LostInsight/coding-tools-mcp/releases/latest",
  "backend update source",
);
if (appLinks.includes("mybolide/coding-tools-mcp") || updateModule.includes("mybolide/coding-tools-mcp")) {
  fail("runtime update/repository source still references mybolide");
}

const buildDir = path.join(root, "build");
if (!fs.existsSync(buildDir)) fail("build directory does not exist; run npm run build first");
const frontendFiles = [];
const walk = (dir) => {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) walk(full);
    else if (/\.(?:html|js|css)$/.test(entry.name)) frontendFiles.push(full);
  }
};
walk(buildDir);
const frontend = frontendFiles.map((file) => fs.readFileSync(file, "utf8")).join("\n");
for (const needle of [
  "Paseo Integration",
  "paseo_allow_permission",
  "paseo_deny_permission",
  "paseo_create_agent",
  "github.com/LostInsight/coding-tools-mcp",
  version,
]) {
  requireText(frontend, needle, "built frontend");
}
if (frontend.includes("github.com/mybolide/coding-tools-mcp/releases/latest")) {
  fail("built frontend still points to mybolide releases");
}

const exe = path.join(root, "src-tauri", "target", "release", "coding-tools-mcp-desktop.exe");
if (fs.existsSync(exe)) {
  const binary = fs.readFileSync(exe);
  for (const needle of [
    "paseo_allow_permission",
    "paseo_deny_permission",
    "paseo_create_agent",
    "api.github.com/repos/LostInsight/coding-tools-mcp/releases/latest",
    version,
  ]) {
    if (!binary.includes(Buffer.from(needle))) fail(`release EXE is missing ${JSON.stringify(needle)}`);
  }

  const bundleRoot = path.join(root, "src-tauri", "target", "release", "bundle");
  const bundleNames = [];
  const collect = (dir) => {
    if (!fs.existsSync(dir)) return;
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      const full = path.join(dir, entry.name);
      if (entry.isDirectory()) collect(full);
      else bundleNames.push(entry.name);
    }
  };
  collect(bundleRoot);
  if (!bundleNames.some((name) => name.endsWith(".msi") && name.includes(version))) fail("versioned MSI not found");
  if (!bundleNames.some((name) => name.endsWith("-setup.exe") && name.includes(version))) fail("versioned NSIS installer not found");
}

console.log(`release verification passed for v${version}`);
