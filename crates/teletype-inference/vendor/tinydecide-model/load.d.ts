import { TinyDecide } from "./tinydecide.js";
export const HF_URL: string;
/** A folder (Node) or URL with meta.json and model.bin. Default: the Hugging Face repo. */
export function load(base?: string): Promise<TinyDecide>;
