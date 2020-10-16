use cpuprofiler::PROFILER;
use criterion::{criterion_group, criterion_main, Criterion};
use jed::{
    jq::{run_jq_query, JQ},
    lines::{json_to_lines, render_lines},
    shadow_tree,
    shadow_tree::construct_shadow_tree,
};
use serde_json::{value::Value, Deserializer};
use std::{fs, io, path::Path};

fn bench_jq_roundtrip(c: &mut Criterion) {
    c.bench_function("jq_roundtrip", |bench| {
        let mut prog = JQ::compile(".").expect("jq compilation error");
        let f = fs::File::open("example.json").expect("cannot open file");
        let r = io::BufReader::new(f);
        let content: Vec<Value> = Deserializer::from_reader(r)
            .into_iter::<Value>()
            .collect::<Result<Vec<Value>, _>>()
            .expect("serde deserialization error");
        bench.iter(|| run_jq_query(&content, &mut prog))
    });
}

fn bench_render_preprocessing_old(c: &mut Criterion) {
    c.bench_function("render_preprocessing_old", |bench| {
        let f = fs::File::open("example.json").expect("cannot open file");
        let r = io::BufReader::new(f);
        let content: Vec<Value> = Deserializer::from_reader(r)
            .into_iter::<Value>()
            .collect::<Result<Vec<Value>, _>>()
            .expect("serde deserialization error");
        bench.iter(|| json_to_lines(content.iter()))
    });
}

fn bench_render_preprocessing_new(c: &mut Criterion) {
    c.bench_function("render_preprocessing_new", |bench| {
        let f = fs::File::open("example.json").expect("cannot open file");
        let r = io::BufReader::new(f);
        let content: Vec<Value> = Deserializer::from_reader(r)
            .into_iter::<Value>()
            .collect::<Result<Vec<Value>, _>>()
            .expect("serde deserialization error");
        bench.iter(|| construct_shadow_tree(&content))
    });
}

fn bench_render_old(c: &mut Criterion) {
    c.bench_function("render_old", |bench| {
        let f = fs::File::open("citylots.json").expect("cannot open file");
        let r = io::BufReader::new(f);
        let content: Vec<Value> = Deserializer::from_reader(r)
            .into_iter::<Value>()
            .collect::<Result<Vec<Value>, _>>()
            .expect("serde deserialization error");
        let lines = json_to_lines(content.iter());
        bench.iter(|| render_lines(3, 10, Some(5), &lines))
    });
}

fn bench_render_new(c: &mut Criterion) {
    c.bench_function("render_new", |bench| {
        let f = fs::File::open("citylots.json").expect("cannot open file");
        let r = io::BufReader::new(f);
        let content: Vec<Value> = Deserializer::from_reader(r)
            .into_iter::<Value>()
            .collect::<Result<Vec<Value>, _>>()
            .expect("serde deserialization error");
        let shadow_tree = construct_shadow_tree(&content);
        bench.iter(|| shadow_tree::render_lines(3, 10, Some(5), &shadow_tree, &content))
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
        bench_render_preprocessing_old,
        bench_render_preprocessing_new,
        bench_render_old,
        bench_render_new,
);
criterion_main!(benches);
