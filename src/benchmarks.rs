use cpuprofiler::PROFILER;
use criterion::{criterion_group, criterion_main, Criterion};
use jed::jq::{jv::JV, run_jq_query, JQ};
use serde_json::{value::Value, Deserializer};
use std::{fs, io, path::Path};

fn bench_jq_roundtrip(c: &mut Criterion) {
    c.bench_function("jq_roundtrip", |bench| {
        let mut prog = jq_rs::compile(".").expect("jq compilation error");
        let f = fs::File::open("example.json").expect("cannot open file");
        let r = io::BufReader::new(f);
        let content: Vec<Value> = Deserializer::from_reader(r)
            .into_iter::<Value>()
            .collect::<Result<Vec<Value>, _>>()
            .expect("serde deserialization error");
        bench.iter(|| run_jq_query(&content, &mut prog))
    });
}

fn bench_jq_roundtrip_2(c: &mut Criterion) {
    c.bench_function("jq_roundtrip", |bench| {
        let mut prog = JQ::compile(".").expect("jq compilation error");
        let f = fs::File::open("example.json").expect("cannot open file");
        let r = io::BufReader::new(f);
        let content: Vec<Value> = Deserializer::from_reader(r)
            .into_iter::<Value>()
            .collect::<Result<Vec<Value>, _>>()
            .expect("serde deserialization error");
        bench.iter(|| {
            let mut results: Vec<Value> = Vec::new();
            for value in &content {
                let jv = JV::from_serde(value);
                for res in prog.execute(jv) {
                    results.push(res.to_serde().unwrap());
                }
            }
        })
    });
}

struct Profiler {}

impl criterion::profiler::Profiler for Profiler {
    fn start_profiling(&mut self, benchmark_id: &str, _benchmark_dir: &Path) {
        let mut profiler = PROFILER.lock().unwrap();
        let path = format!("profiling/{}.profile", benchmark_id);
        profiler.start(path).unwrap();
    }
    fn stop_profiling(&mut self, _benchmark_id: &str, _benchmark_dir: &Path) {
        let mut profiler = PROFILER.lock().unwrap();
        profiler.stop().unwrap();
    }
}

fn profiled() -> Criterion {
    Criterion::default()
        .sample_size(10)
        .with_profiler(Profiler {})
}
criterion_group!(
    name = benches;
    config = profiled();
    targets =
        bench_jq_roundtrip,
        bench_jq_roundtrip_2,
);
criterion_main!(benches);
