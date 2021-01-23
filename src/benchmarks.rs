use cpuprofiler::PROFILER;
use criterion::{criterion_group, criterion_main, Criterion};
use jex::{
    app::App,
    jq::{
        jv::JV,
        query::{run_jq_query, JQ},
    },
    layout::JexLayout,
    view_tree::View,
};
use serde_json::{value::Value, Deserializer};
use std::{fs, io, path::Path};
use tui::layout::Rect;

fn bench_jq_roundtrip(c: &mut Criterion) {
    c.bench_function("jq_roundtrip", |bench| {
        let mut prog = JQ::compile(".").expect("jq compilation error");
        let f = fs::File::open("testdata/example.json").expect("cannot open file");
        let r = io::BufReader::new(f);
        let content: Vec<JV> = Deserializer::from_reader(r)
            .into_iter::<JV>()
            .collect::<Result<Vec<JV>, _>>()
            .expect("serde deserialization error");
        bench.iter(|| run_jq_query(&content, &mut prog))
    });
}

fn bench_load_direct(c: &mut Criterion) {
    c.bench_function("bench_load_direct", |bench| {
        let s = fs::read_to_string("testdata/example.json").expect("cannot read file");
        bench.iter(|| {
            let content: Vec<JV> = Deserializer::from_str(&s)
                .into_iter::<JV>()
                .collect::<Result<Vec<JV>, _>>()
                .expect("serde deserialization error");
            content
        })
    });
}

fn bench_load_indirect(c: &mut Criterion) {
    c.bench_function("bench_load_indirect", |bench| {
        let s = fs::read_to_string("testdata/example.json").expect("cannot read file");
        bench.iter(|| {
            let content: Vec<Value> = Deserializer::from_str(&s)
                .into_iter::<Value>()
                .collect::<Result<Vec<Value>, _>>()
                .expect("serde deserialization error");
            let jvs: Vec<JV> = content.iter().map(JV::from).collect();
            jvs
        })
    });
}

fn bench_load_native(c: &mut Criterion) {
    c.bench_function("bench_load_native", |bench| {
        let s = fs::read_to_string("testdata/example.json").expect("cannot read file");
        bench.iter(|| JV::parse_native(&s))
    });
}

fn bench_scroll_long_string(c: &mut Criterion) {
    c.bench_function("bench_scroll_long_string", |bench| {
        let path = "testdata/war-and-peace.json";
        let f = fs::File::open(&path).expect("couldn't open test file");
        let r = io::BufReader::new(f);
        let rect = Rect::new(0, 0, 100, 100);
        let initial_layout = JexLayout::new(rect, false);
        let mut app =
            App::new(r, path.to_string(), initial_layout).expect("couldn't initalize app");
        let view = if let View::Json(Some(view)) = &mut app.focused_view_mut().view {
            view
        } else {
            panic!("Can't get view");
        };
        bench.iter(|| {
            view.advance_cursor();
            view.regress_cursor();
        })
    });
}

struct Profiler<'a> {
    profiler: std::sync::MutexGuard<'a, cpuprofiler::Profiler>,
}

impl<'a> criterion::profiler::Profiler for Profiler<'a> {
    fn start_profiling(&mut self, benchmark_id: &str, _benchmark_dir: &Path) {
        let path = format!("profiling/{}.profile", benchmark_id);
        self.profiler.start(path).unwrap();
    }
    fn stop_profiling(&mut self, _benchmark_id: &str, _benchmark_dir: &Path) {
        self.profiler.stop().unwrap();
    }
}

fn profiled() -> Criterion {
    let profiler = PROFILER.lock().unwrap();
    Criterion::default()
        .sample_size(10)
        .with_profiler(Profiler { profiler })
}
criterion_group!(
    name = benches;
    config = profiled();
    targets =
        bench_jq_roundtrip,
        bench_load_direct,
        bench_load_indirect,
        bench_load_native,
        bench_scroll_long_string,
);
criterion_main!(benches);
