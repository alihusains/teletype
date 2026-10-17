//! cargo run --release --example quickstart -- path/to/TinyDecide
//! The directory holds meta.json and model.bin (default: "..", the Hugging Face repo layout).
use tinydecide::{Question, TinyDecide};

fn main() -> Result<(), tinydecide::Error> {
    let dir = std::env::args().nth(1).unwrap_or_else(|| "..".into());
    let model = TinyDecide::load(&dir)?;

    let r = model.answer(
        "Book a table for 4 at an Italian place near the station on Friday at 7:30",
        &[
            Question::choice(
                "Which app should handle this?",
                &["reminders", "music", "calendar", "restaurants", "weather"],
            ),
            Question::noul("The message is urgent."),
            Question::score(
                "How positive is the tone?",
                &["negative", "neutral", "positive"],
            ),
            Question::span("Extract the time."),
        ],
    )?;

    let (app, urgent, tone, time) = (&r.answers[0], &r.answers[1], &r.answers[2], &r.answers[3]);
    println!("app    {:?} pick {:?}", app.probs, app.pick); // one probability per option
    println!("urgent {:?}", urgent.p); // P(true)
    println!("tone   {:?}", tone.score); // 0 = negative, 1 = positive
    println!("time   {:?}", time.text); // "7:30"
    println!("{} tokens, {:.0} ms", r.tokens.total, r.ms);
    Ok(())
}
