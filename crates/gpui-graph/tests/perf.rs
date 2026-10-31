//! Frame timings for a large graph, through GPUI's test window: layout,
//! prepaint and paint (path tessellation included), but no GPU.
//!
//! ```text
//! cargo test -p gpui-graph --release --test perf -- --ignored --nocapture
//! GRAPH_EDGES=path/to/graph.edges cargo test ...   # "source target" per line
//! ```

use std::time::{Duration, Instant};

use gpui::{AnyView, ParentElement as _, Styled as _, TestAppContext, div, point, px, size};
use gpui_graph::{Edge, GraphData, NetworkGraph, Node};

/// Edge list from `GRAPH_EDGES`, or a hub-heavy synthetic graph about the size
/// of the fly medulla connectome (1,781 nodes, 9,735 edges).
fn edges() -> Vec<(u32, u32)> {
    if let Ok(path) = std::env::var("GRAPH_EDGES") {
        let text = std::fs::read_to_string(&path).expect("GRAPH_EDGES is readable");
        let mut pairs: Vec<(u32, u32)> = text
            .lines()
            .filter(|l| !l.starts_with('%') && !l.starts_with('#'))
            .filter_map(|l| {
                let mut it = l.split_whitespace();
                Some((it.next()?.parse().ok()?, it.next()?.parse().ok()?))
            })
            .collect();
        pairs.sort_unstable();
        pairs.dedup();
        return pairs;
    }
    let (n, m) = (1781u32, 9735usize);
    let mut rng = 0x2545_f491u32;
    let mut next = move || {
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        rng
    };
    let mut pairs = Vec::with_capacity(m);
    while pairs.len() < m {
        let s = next() % n;
        // A tenth of the edges go into one hub, the rest favour low ids.
        let t = if next() % 10 == 0 {
            0
        } else {
            (next() % n).min(next() % n)
        };
        pairs.push((s, t));
    }
    pairs.sort_unstable();
    pairs.dedup();
    pairs
}

fn data(pairs: &[(u32, u32)]) -> GraphData {
    let mut ids: Vec<u32> = pairs.iter().flat_map(|&(s, t)| [s, t]).collect();
    ids.sort_unstable();
    ids.dedup();
    GraphData::new(
        ids.iter()
            .map(|i| Node::new(i.to_string(), format!("n{i}"), "Neuron"))
            .collect(),
        pairs
            .iter()
            .map(|(s, t)| Edge::new(s.to_string(), t.to_string(), "SYNAPSE"))
            .collect(),
    )
}

fn report(name: &str, mut frames: Vec<Duration>) {
    frames.sort_unstable();
    let ms = |d: Duration| d.as_secs_f64() * 1e3;
    let mean = frames.iter().map(|d| ms(*d)).sum::<f64>() / frames.len() as f64;
    println!(
        "{name:<22} mean {mean:7.2} ms   p50 {:7.2}   p95 {:7.2}   max {:7.2}   ({} frames)",
        ms(frames[frames.len() / 2]),
        ms(frames[frames.len() * 95 / 100]),
        ms(*frames.last().unwrap()),
        frames.len(),
    );
}

#[gpui::test]
#[ignore = "benchmark; run with --release --ignored --nocapture"]
fn frame_times(cx: &mut TestAppContext) {
    let pairs = edges();
    let data = data(&pairs);
    println!(
        "graph: {} nodes, {} edges",
        data.nodes.len(),
        data.edges.len()
    );
    cx.update(|cx| {
        gpui_base::init(cx);
        gpui_graph::init(cx);
    });
    let (graph, cx) = cx.add_window_view(|_, cx| NetworkGraph::new(cx).with_data(data));
    let view = AnyView::from(graph.clone());
    let frame = |cx: &mut gpui::VisualTestContext| {
        let start = Instant::now();
        cx.draw(point(px(0.), px(0.)), size(px(1600.), px(900.)), |_, _| {
            div().size_full().child(view.clone())
        });
        start.elapsed()
    };

    // The layout anneals over ~300 ticks; the first frames include it.
    let settling: Vec<_> = (0..300).map(|_| frame(cx)).collect();
    for _ in 0..200 {
        frame(cx);
    }
    let idle: Vec<_> = (0..100).map(|_| frame(cx)).collect();
    report("while laying out", settling);
    report("settled (pan/hover)", idle);
}
