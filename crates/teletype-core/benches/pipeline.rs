//! Criterion benchmarks for the pipeline's dictionary correction pass.
//!
//! `correct_with_dictionary` is private so we exercise it through
//! `Pipeline::run` with a no-op LLM. This also benchmarks the full
//! deterministic path (ITN + filler removal + dictionary), which is the
//! latency-sensitive portion users feel between hotkey release and paste.
//!
//! Run with: `cargo bench -p teletype-core --bench pipeline`

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use teletype_core::{
    autotext::AutoTextStore,
    dictionary::{Dictionary, DictionaryWord, EditDistanceChecker},
    llm::{GenerationParams, InferenceProvider},
    personalization::UserProfile,
    pipeline::{InputSource, Pipeline, UnifiedInput},
    platform::MockPlatform,
    style::StyleProfileStore,
    transforms::TransformStore,
    vocab::PackTerm,
};

static CHECKER: EditDistanceChecker = EditDistanceChecker;
static EMPTY_PACKS: &[PackTerm] = &[];
static EMPTY_SYSTEM_AT: &[teletype_core::autotext::AutoTextEntry] = &[];

/// A no-op inference provider so the bench never waits on a model.
/// Returns an error immediately so the pipeline falls back to raw text.
struct NoOpLlm;

impl InferenceProvider for NoOpLlm {
    fn model_id(&self) -> &str {
        "bench-noop"
    }
    fn model_name(&self) -> &str {
        "Bench no-op"
    }
    fn generate(&self, _prompt: &str, _params: GenerationParams) -> Result<String, String> {
        Err("no model".into())
    }
}

/// Build a `Dictionary` with `n` random tech-term entries. Simulates a user
/// who has taught Teletype their company vocabulary.
fn make_dictionary(n: usize) -> Dictionary {
    // A realistic set of domain terms a developer might add.
    let terms = [
        "Kubernetes",
        "PostgreSQL",
        "Elasticsearch",
        "Terraform",
        "CircleCI",
        "Prometheus",
        "Grafana",
        "Datadog",
        "Snowflake",
        "Databricks",
        "PagerDuty",
        "Cloudflare",
        "HashiCorp",
        "OpenTelemetry",
        "Backstage",
        "ArgoCD",
        "FluxCD",
        "Istio",
        "Envoy",
        "Consul",
        "Nomad",
        "Vault",
        "Packer",
        "Waypoint",
        "Boundary",
        "TerraCloud",
        "Pulumi",
        "Crossplane",
        "Tanka",
        "Jsonnet",
        "AsyncAPI",
        "OpenAPI",
        "GraphQL",
        "gRPC",
        "Protobuf",
        "Kafka",
        "RabbitMQ",
        "NATS",
        "Pulsar",
        "EventBridge",
        "DynamoDB",
        "Cassandra",
        "CockroachDB",
        "TiDB",
        "YugabyteDB",
        "ClickHouse",
        "Pinecone",
        "Weaviate",
        "Qdrant",
        "Milvus",
    ];
    let mut dict = Dictionary::default();
    for (i, term) in terms.iter().enumerate().take(n) {
        dict.words.push(DictionaryWord::new(*term, ""));
        if i + 1 >= n {
            break;
        }
    }
    dict
}

fn run_pipeline_once(
    input_text: &str,
    dictionary: &Dictionary,
    styles: &StyleProfileStore,
) -> String {
    let platform = MockPlatform::with_app(teletype_core::context::ApplicationContext::unknown());
    let autotext = AutoTextStore::default();
    let transforms = TransformStore::with_built_ins();
    let profile = UserProfile::default();
    let llm = NoOpLlm;
    let mut pipeline = Pipeline {
        platform: &platform,
        autotext: &autotext,
        transforms: &transforms,
        profile: &profile,
        inference: Some(&llm),
        decision: None,
        force_command: false,
        dictionary,
        styles,
        active_style: "",
        explicit_style: "",
        auto_apply: false,
        restore_clipboard: false,
        remove_filler_words: false,
        filler_words: vec![],
        restore_emoji: false,
        spoken_emoji: false,
        spoken_punctuation: false,
        system_autotext: EMPTY_SYSTEM_AT,
        token_sink: None,
        polish_gate_enabled: false,
        polish_gate_threshold_words: 8,
        pack_terms: EMPTY_PACKS,
        list_style: teletype_core::transforms::ListStyle::default(),
        word_checker: &CHECKER,
    };
    let input = UnifiedInput {
        source: InputSource::Voice,
        text: input_text.to_string(),
    };
    pipeline.run(input, None).final_text
}

fn bench_pipeline_no_dictionary(c: &mut Criterion) {
    let dict = Dictionary::default();
    let styles = StyleProfileStore::with_built_ins();
    let input = "the quarterly report is ready to send to the team by end of day";
    c.bench_function("pipeline/no_dictionary", |b| {
        b.iter(|| {
            criterion::black_box(run_pipeline_once(
                criterion::black_box(input),
                criterion::black_box(&dict),
                criterion::black_box(&styles),
            ))
        })
    });
}

fn bench_pipeline_with_dictionary(c: &mut Criterion) {
    let styles = StyleProfileStore::with_built_ins();
    let mut group = c.benchmark_group("pipeline/with_dictionary");

    for n in [10usize, 50, 200] {
        let dict = make_dictionary(n);
        let input = "the kubernetes cluster running on postgresql backed elasticsearch \
                     was monitored by prometheus and grafana with alerts via pagerduty";
        group.bench_with_input(
            BenchmarkId::from_parameter(n),
            &(dict, input),
            |b, (d, i)| {
                b.iter(|| {
                    criterion::black_box(run_pipeline_once(
                        criterion::black_box(i),
                        criterion::black_box(d),
                        criterion::black_box(&styles),
                    ))
                })
            },
        );
    }
    group.finish();
}

fn bench_pipeline_itn_heavy(c: &mut Criterion) {
    // This input hits many ITN passes (cardinals, money, date, phone, ordinal)
    // so the bench captures the combined dictionary + ITN cost.
    let dict = make_dictionary(20);
    let styles = StyleProfileStore::with_built_ins();
    let input = "on january first twenty twenty four we raised two million five hundred \
                 thousand dollars at twelve point five percent and the thirty second \
                 investor called from two oh three nine five four oh six";
    c.bench_function("pipeline/itn_heavy", |b| {
        b.iter(|| {
            criterion::black_box(run_pipeline_once(
                criterion::black_box(input),
                criterion::black_box(&dict),
                criterion::black_box(&styles),
            ))
        })
    });
}

criterion_group!(
    benches,
    bench_pipeline_no_dictionary,
    bench_pipeline_with_dictionary,
    bench_pipeline_itn_heavy
);
criterion_main!(benches);
