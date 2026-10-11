// Loads a TinyDecide build from a folder (Node) or a URL (browser, Deno, Node 18+).
//   import { load } from "./load.js";
//   const model = await load("./");                                    // meta.json + model.bin here
//   const model = await load("https://huggingface.co/TheREZOR/TinyDecide/resolve/main/");
//   const full  = await load("https://huggingface.co/TheREZOR/TinyDecide/resolve/main/full/");
import { TinyDecide } from "./tinydecide.js";

export const HF_URL = "https://huggingface.co/TheREZOR/TinyDecide/resolve/main/";

export async function load(base = HF_URL) {
  const dir = base.endsWith("/") ? base : base + "/";
  if (/^(https?|blob|data):/.test(dir) || typeof window !== "undefined") {
    const [meta, bin] = await Promise.all([
      fetch(dir + "meta.json").then((r) => { if (!r.ok) throw new Error(`meta.json: HTTP ${r.status}`); return r.json(); }),
      fetch(dir + "model.bin").then((r) => { if (!r.ok) throw new Error(`model.bin: HTTP ${r.status}`); return r.arrayBuffer(); }),
    ]);
    return new TinyDecide(meta, bin);
  }
  const { readFile } = await import("node:fs/promises");
  const [meta, bin] = await Promise.all([readFile(dir + "meta.json", "utf8"), readFile(dir + "model.bin")]);
  return new TinyDecide(JSON.parse(meta), bin.buffer.slice(bin.byteOffset, bin.byteOffset + bin.byteLength));
}
