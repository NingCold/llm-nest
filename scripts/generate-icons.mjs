import { execFileSync } from "node:child_process"
import { copyFileSync, mkdirSync, readFileSync, writeFileSync } from "node:fs"
import { dirname, join, resolve } from "node:path"
import { fileURLToPath } from "node:url"

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..")
const source = join(root, "assets/branding/app-icon.svg")
const svg = readFileSync(source, "utf8")
const mark = svg.match(/  <g id="mark"[\s\S]*?<\/g>/)?.[0]
if (!mark) throw new Error("The canonical icon must contain a mark group")

const publicDir = join(root, "frontends/web/public")
mkdirSync(join(publicDir, "brand"), { recursive: true })
// The UI uses a closer crop and transparent background. Dark mode inverts
// only this silhouette; the desktop/favicons always retain their white tile.
writeFileSync(join(publicDir, "brand/mark.svg"),
  '<svg xmlns="http://www.w3.org/2000/svg" viewBox="180 180 894 894">\n' +
  '  <title>LLM Nest</title>\n' + mark + '\n</svg>\n')
copyFileSync(source, join(publicDir, "icon.svg"))

const output = join(root, "target/icon-assets")
mkdirSync(output, { recursive: true })
execFileSync(process.execPath, [
  join(root, "frontends/tauri/node_modules/@tauri-apps/cli/tauri.js"),
  "icon", source, "--output", output,
], { cwd: join(root, "frontends/tauri"), stdio: "inherit" })

const iconsDir = join(root, "frontends/tauri/src-tauri/icons")
mkdirSync(iconsDir, { recursive: true })
for (const name of ["32x32.png", "128x128.png", "128x128@2x.png", "icon.ico", "icon.icns"]) {
  copyFileSync(join(output, name), join(iconsDir, name))
}
copyFileSync(join(output, "32x32.png"), join(publicDir, "icon.png"))
console.log("Updated desktop icons, favicon, and theme-aware UI mark from", source)
