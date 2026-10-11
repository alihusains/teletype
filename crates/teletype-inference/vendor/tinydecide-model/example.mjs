import { readFileSync } from "node:fs";
import { TinyDecide } from "./tinydecide.js";

const meta = JSON.parse(readFileSync("meta.json", "utf8"));
const bin = readFileSync("model.bin");
const model = new TinyDecide(meta, bin.buffer.slice(bin.byteOffset, bin.byteOffset + bin.byteLength));

const { answers } = model.answer({
  state: "Book a table for 4 at an Italian place near the station on Friday at 7:30",
  questions: [
    { type: "choice", text: "Which app should handle this?", options: ["reminders", "music", "calendar", "restaurants", "weather"] },
    { type: "noul", text: "The message is urgent." },
    { type: "score", text: "How positive is the tone?", options: ["negative", "neutral", "positive"] },
    { type: "span", text: "Extract the time." },
  ],
});

const [app, urgent, tone, time] = answers;
console.log(app.probs, app.pick); // one probability per option
console.log(urgent.p);            // P(true)
console.log(tone.score);          // 0 = negative, 1 = positive
console.log(time.text);           // "7:30"
