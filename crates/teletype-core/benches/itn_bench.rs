//! ITN micro-bench: `itn::normalize` over a fixed sentence corpus.
//!
//! Corpus choice (recorded per P0.3): a fixed inline sentence list, NOT the
//! `teletype-eval` corpus. `Corpus::load` requires an on-disk manifest plus
//! one audio file per case, which is awkward for a micro-bench and would tie
//! bench time to filesystem fixtures. The sentences below cover the same ITN
//! passes the eval references exercise (cardinals, years, dates, times,
//! money/percent, phones, decimals, emails/URLs, ordinals, ranges, spoken
//! punctuation, plus ambiguous minimal pairs that must stay spelled).

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use teletype_core::itn;

/// Representative dictation sentences spanning every ITN pass.
const SENTENCES: &[&str] = &[
    "the meeting is on january fifteenth twenty twenty six at three thirty p m",
    "it costs eighty million dollars and fifty cents for the whole project",
    "she is twenty one years old and lives at number forty two",
    "call five five five zero one zero nine nine nine nine tomorrow morning",
    "my email is john doe at example dot com please reply soon",
    "visit example dot com slash docs slash intro for the full details",
    "the score was ninety nine point nine percent which is excellent",
    "he finished third in the race on the fourth of july",
    "we need two thousand five hundred units delivered by friday",
    "call five five five one two three four after lunch",
    "the range is ten to twenty meters from the north entrance",
    "she said twenty twenty was a strange year for everyone",
    "the price is forty two dollars and thirty three cents total",
    "arrive at nine o clock sharp for the standup meeting",
    "the deploy went out this morning and it is all good",
    "remind me to buy milk eggs and bread on the way home",
    "the invoice for one hundred and twenty dollars is overdue",
    "we will meet at half past noon comma bring the report period",
    "the temperature dropped to minus five degrees overnight",
    "send the contract to suite three hundred on fifth avenue",
];

fn itn_corpus(c: &mut Criterion) {
    c.bench_function("itn_normalize_20_sentences", |b| {
        b.iter(|| {
            for s in SENTENCES {
                black_box(teletype_core::itn::normalize(black_box(s)));
            }
        })
    });
    c.bench_function("itn_normalize_single_short", |b| {
        b.iter(|| itn::normalize(black_box("remind me to buy milk tomorrow")))
    });
}

criterion_group!(benches, itn_corpus);
criterion_main!(benches);
