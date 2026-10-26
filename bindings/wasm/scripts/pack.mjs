// npm pack / publish の前後に動く（package.json の prepack / postpack）。
//   prepack:  ルートのライセンスと NOTICE をこのディレクトリにコピーする（npm はシンボリックリンクを
//             パッケージに入れないため）。ビルド済みの sortition.wasm がなければ、公開を止める
//   postpack: コピーを消す
import { copyFileSync, existsSync, rmSync } from "node:fs";
import { fileURLToPath } from "node:url";

const here = (p) => fileURLToPath(new URL(`../${p}`, import.meta.url));
const root = (p) => fileURLToPath(new URL(`../../../${p}`, import.meta.url));
const LEGAL = ["LICENSE-MIT", "LICENSE-APACHE", "NOTICE"];

if (process.argv[2] === "prepack") {
  if (!existsSync(here("sortition.wasm"))) {
    console.error("sortition.wasm is missing. Run ./build.sh first (it needs: rustup target add wasm32-unknown-unknown).");
    process.exit(1);
  }
  for (const f of LEGAL) copyFileSync(root(f), here(f));
} else if (process.argv[2] === "postpack") {
  for (const f of LEGAL) rmSync(here(f), { force: true });
} else {
  console.error("usage: node scripts/pack.mjs prepack|postpack");
  process.exit(2);
}
