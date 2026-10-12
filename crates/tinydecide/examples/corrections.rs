//! cargo run --release --example corrections -- path/to/TinyDecide
//! Stores one correction per option and asks again with the resulting prototypes.
use tinydecide::corrections::{make_protos, Example};
use tinydecide::{Question, TinyDecide};

fn main() -> Result<(), tinydecide::Error> {
    let dir = std::env::args().nth(1).unwrap_or_else(|| "..".into());
    let model = TinyDecide::load(&dir)?;
    let q = Question::choice("What kind of note is this?", &["shopping", "task", "event"]);

    let mut lists: Vec<Vec<Example>> = vec![vec![]; 3];
    for (note, k) in [("buy oat milk", 0), ("call the plumber", 1), ("dentist thursday 4pm", 2)] {
        let a = &model.answer(note, std::slice::from_ref(&q))?.answers[0];
        lists[k].push(Example::from_answer(a)); // a person picked option k
    }
    let protos = make_protos(q.kind, &lists, model.beta(), None);

    let before = model.answer("pick up eggs", std::slice::from_ref(&q))?;
    let after = model.answer_with("pick up eggs", std::slice::from_ref(&q), &[protos])?;
    println!("zero-shot        {:?}", before.answers[0].probs);
    println!("with corrections {:?}", after.answers[0].probs);
    Ok(())
}
