# gpui-graph

基于 [gpui-base](https://gpui-kit.com/zh-CN/base/) 的 GPUI 网络图（force-directed graph）组件库。

```
crates/gpui-graph     组件库
examples/basic        最小接入示例
examples/query-app    图查询工具原型（查询框 / 图例 / 详情 / 右键菜单）
```

```sh
cargo run                 # query-app
cargo run -p basic
cargo test -p gpui-graph
```

## 使用

```toml
[dependencies]
gpui-graph = { path = "crates/gpui-graph" }
gpui-base = "=0.7.3"
gpui = { package = "gpui-pre", version = "=0.3.9" }   # 必须与 gpui-base 一致
```

```rust
use gpui_graph::{Edge, GraphData, GraphEvent, LabelMode, NetworkGraph, Node, Selection};

let graph = cx.new(|cx| {
    NetworkGraph::new(cx)
        .with_data(GraphData::new(
            vec![Node::new("a", "Alice", "Person"), Node::new("x", "Acme", "Company")],
            vec![Edge::new("a", "x", "WORKS_AT")],
        ))
        .with_label_mode(LabelMode::Auto)
});

cx.subscribe(&graph, |this, graph, event: &GraphEvent, cx| match event {
    GraphEvent::SelectionChanged(Selection::Node(node)) => { /* 详情面板 */ }
    GraphEvent::NodeActivated(node) => {
        // 双击：查询邻居后增量合并，新节点从该节点周围长出
        graph.update(cx, |g, cx| g.merge(fetch_neighbors(&node.id), Some(&node.id), cx));
    }
    GraphEvent::ContextMenu { target, position } => { /* 弹出菜单 */ }
    _ => {}
}).detach();

// 组件填满父容器，父容器需要有尺寸
div().flex_1().child(graph.clone())
```

## API 概览

| 分类 | API |
|---|---|
| 构建选项 | `with_data` `with_force_config` `with_label_mode` `with_arrows` `with_edge_labels` `with_pin_on_drag` `with_palette` `with_node_sizing` `with_colors` `with_tooltip` `without_tooltip` |
| 数据 | `set_data`（替换并重置视口）`merge(data, anchor)`（增量）`remove_nodes` `clear` `data()` |
| 视口 | `fit_view` `zoom_by` `relayout` `select_node(id, center)` |
| 固定 | `set_pinned` `is_pinned` `unpin_all` |
| 查询 | `node` `degree` `neighbors_of` `edges_of` `categories` `color_of` `selection` `zoom` `is_simulating` |
| 运行时设置 | `set_label_mode` `set_show_arrows` `set_show_edge_labels` `set_pin_on_drag` `set_force_config` `set_colors` |
| 事件 | `SelectionChanged(Selection)` `NodeActivated(Node)` `ContextMenu { target, position }` |

- **样式**：默认每帧读取 gpui-base `Theme` tokens，跟随深浅色；`GraphColors` 可整体覆盖。分类色默认 AntV 色板（`Palette`），`Node::color` / `Node::size` 可单独覆盖。
- **节点大小**：`NodeSizing::Degree { min, max }`（默认，按度数）或 `NodeSizing::Fixed(r)`。
- **Tooltip**：`with_tooltip(|selection, colors, window, cx| Some(element))` 自定义，返回 `None` 隐藏。
- **布局**：`ForceConfig` 字段与 d3-force 对应（`link_distance` `charge` `theta` `collide_padding` `gravity` `velocity_decay` …）。

## 交互

滚轮缩放（以光标为中心）· 拖空白平移 · 拖节点 · 单击选中节点/边 · 双击节点 → `NodeActivated` · 右键 → `ContextMenu` · 悬停高亮一度邻居 · 平行边自动弯曲、自环、箭头、边标签 · 布局进行中自动 fit，用户操作视口后停止。

## 实现

| 文件 | 说明 |
|---|---|
| `layout.rs` | d3-force 风格模拟：link / many-body（Barnes–Hut 四叉树）/ collide（空间哈希）/ gravity，alpha 退火 |
| `view.rs` | `NetworkGraph`：`canvas` 绘制（边按状态批量成 path）、hitbox + 窗口级鼠标事件、视口变换、tooltip |
| `geometry.rs` | 直线/二次曲线边的几何、命中检测 |
| `style.rs` | `GraphColors` `Palette` `NodeSizing` `LabelMode` |
| `model.rs` | `Node` `Edge` `GraphData` |
