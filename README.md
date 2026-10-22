# gpui-graph

基于 [gpui-base](https://gpui-kit.com/zh-CN/base/) 的 GPUI 网络图（force-directed graph）组件库。

```
crates/gpui-graph     组件库
examples/basic        最小接入示例
examples/query-app    图查询工具原型（查询框 / 图例 / 社区列表 / 详情 / 右键菜单）
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
gpui-base = "=0.8.0"
gpui = { package = "gpui-pre", version = "=0.3.9" }   # 必须与 gpui-base 一致
```

```rust
use gpui_graph::{Edge, GraphData, LabelMode, NetworkGraph, NetworkGraphEvent, Node, Selection};

// 启动时：gpui_base::init(cx); gpui_graph::init(cx);  // 注册键盘绑定

let graph = cx.new(|cx| {
    NetworkGraph::new(cx)
        .with_data(GraphData::new(
            vec![Node::new("a", "Alice", "Person"), Node::new("x", "Acme", "Company")],
            vec![Edge::new("a", "x", "WORKS_AT")],
        ))
        .with_label_mode(LabelMode::Auto)
});

// 订阅保存在 owner 上（_subscriptions），不要 detach
let sub = cx.subscribe(&graph, |this, graph, event: &NetworkGraphEvent, cx| match event {
    NetworkGraphEvent::SelectionChanged(Selection::Node(node)) => { /* 详情面板 */ }
    NetworkGraphEvent::NodeActivated(node) => {
        // 双击：查询邻居后增量合并，新节点从该节点周围长出
        graph.update(cx, |g, cx| g.merge(fetch_neighbors(&node.id), Some(&node.id), cx));
    }
    NetworkGraphEvent::ContextMenu { target, position } => { /* 弹出菜单 */ }
    _ => {}
});

// 组件填满父容器，父容器需要有尺寸
div().flex_1().child(graph.clone())
```

## API 概览

| 分类 | API |
|---|---|
| 构建选项 | `with_data` `with_force_config` `with_label_mode` `with_color_by` `with_palette` `with_node_sizing` `with_colors` `with_tooltip`；布尔：`arrows` `edge_labels` `group_hulls` `sticky_drag` `tooltips` |
| 数据 | `set_data`（替换并重置视口）`merge(data, anchor)`（增量）`remove_nodes` `clear` `data()` |
| 视口 | `fit_view` `zoom_by` `relayout` `select_node(id, center)` |
| 固定 | `set_pinned` `is_pinned` `unpin_all` |
| 查询 | `node` `degree` `neighbors_of` `edges_of` `categories` `color_of(node, cx)` `resolved_colors` `selection` `zoom` `is_simulating` `has_arrows` `has_edge_labels` `has_group_hulls` `has_tooltips` `is_sticky_drag` |
| 运行时设置 | `set_label_mode` `set_color_by` `set_arrows` `set_edge_labels` `set_group_hulls` `set_sticky_drag` `set_tooltips` `set_force_config` `set_colors` |
| 社区 | `groups` `group` `group_members` `collapse_group` `expand_group` `toggle_group` `collapse_all_groups` `expand_all_groups` `set_group_label` |
| 事件 `NetworkGraphEvent` | `SelectionChanged(Selection)` `NodeActivated(Node)` `GroupToggled { group, collapsed }` `ContextMenu { target, position }` |
| Action（`gpui_graph::actions`） | `ZoomIn` `ZoomOut` `FitView` `ClearSelection` `SelectLeft/Right/Up/Down` `Activate` `OpenContextMenu` |

命名遵循 [GPUI Kit 编码指南](https://gpui-kit.com/zh-CN/docs/coding-guides)：非布尔 builder 用 `with_<field>`，布尔 builder 用字段名、reader 用 `is_` / `has_`，可变修改用 `set_<field>`；公开结构体与枚举均为 `#[non_exhaustive]`（`ForceConfig` / `GraphColors` 用 `with_*` 构建）。

- **社区**：组件不计算社区，读取 `Node::group`（由数据库算好，如 Neo4j GDS Louvain 写回的 `community` 属性）。
  - 展示：按社区着色、圆角凸包轮廓 + 社区名、布局中的社区聚拢力（`ForceConfig::group_strength`，0 关闭）。
  - 折叠：社区收成一个超级节点（显示成员数，外圈双环），指向它的边按方向合并成一条加粗边并标 `×N`；双击超级节点展开，成员按折叠前的相对位置还原。
  - 折叠只改变显示，`data()` / `node_count()` 等仍是完整数据；选中状态按 id 保持，折叠再展开不丢。
  - `Selection` 新增 `Group(GroupInfo)`（选中超级节点）和 `EdgeBundle(Vec<Edge>)`（选中合并边）。
- **样式**：颜色、字号、圆角、间距、阴影都取自 gpui-base `Theme` tokens，跟随深浅色与字号缩放；`GraphColors` 可整体覆盖。唯一的原始色值是数据分类色板（base 主题没有图表色角色），默认 AntV，用 `with_palette` 替换为产品主题色。节点上的文字自动选对比度更高的颜色。
- **节点大小**：`NodeSizing::Degree { min, max }`（默认，按度数）或 `NodeSizing::Fixed(r)`。
- **Tooltip**：`with_tooltip(|selection, colors, window, cx| Some(element))` 自定义，返回 `None` 隐藏。
- **布局**：`ForceConfig` 字段与 d3-force 对应（`link_distance` `charge` `theta` `collide_padding` `gravity` `velocity_decay` …）。

## 交互

指针：滚轮缩放（以光标为中心）· 拖空白平移 · 拖节点 · 单击选中节点/边 · 双击节点 → `NodeActivated`，双击社区 → 展开 · 右键 → `ContextMenu` · 悬停高亮一度邻居 · 布局进行中自动 fit，用户操作视口后停止。

键盘（图是 Tab 停靠点，焦点在图上时生效）：

| 按键 | 作用 |
|---|---|
| ← → ↑ ↓ | 选中该方向最近的节点（无选中时从视口中心开始），必要时平移使其可见 |
| Enter | 激活：节点发出 `NodeActivated`，社区展开/折叠 |
| Shift-F10 | 对选中项发出 `ContextMenu`（位置为该项的屏幕坐标） |
| Esc | 清除选中 |
| = / - / 0 | 放大 / 缩小 / 适配 |

键盘操作时显示焦点环（图外框 + 选中项外圈），与选中、悬停样式区分；指针操作时隐藏。光标只在拖动时变为抓手，悬停用光晕表示。

开启系统“减少动态效果”时，布局在后台直接收敛（每帧最多 120ms），视口适配直接跳转，不播放动画。

## 实现

| 文件 | 说明 |
|---|---|
| `layout.rs` | d3-force 风格模拟：link / many-body（Barnes–Hut 四叉树）/ collide（空间哈希）/ gravity / 社区聚拢，alpha 退火 |
| `view/mod.rs` | `NetworkGraph`：模型层（全部节点、持久位置、社区状态）、API、输入、视口 |
| `view/visual.rs` | 可见图：由模型 + 折叠状态派生（超级节点、合并边），布局在其上运行 |
| `view/paint.rs` | `canvas` 绘制：社区轮廓、边按状态批量成 path、节点、标签、tooltip |
| `geometry.rs` | 直线/二次曲线边的几何、命中检测、凸包与圆角轮廓 |
| `style.rs` | `GraphColors` `Palette` `NodeSizing` `LabelMode` `ColorBy` |
| `model.rs` | `Node` `Edge` `GraphData` |
