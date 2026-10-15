# gpui-graph

基于 [gpui-base](https://gpui-kit.com/zh-CN/base/) 的桌面图查询工具，自带一个 force-directed 的 `NetworkGraph` 组件。

```sh
cargo run
```

## 交互

- 查询框回车 / Run：`keanu depth 2`、`*`（全部）、`random 2000`（压测）
- 滚轮缩放（以光标为中心），拖动空白处平移，拖动节点
- 单击选中（右侧显示属性/邻居），双击展开邻居（增量合并，不打乱现有布局）
- 悬停高亮一度邻居，其余淡出；平行边自动弯曲，自环、箭头、边标签

## 结构

| 文件 | 说明 |
|---|---|
| `src/graph/layout.rs` | d3-force 风格模拟：link / many-body（Barnes–Hut 四叉树）/ collide（空间哈希）/ gravity，alpha 退火 |
| `src/graph/view.rs` | `NetworkGraph` Entity：`canvas` 绘制（边按状态批量成 path）、hitbox + 窗口级鼠标事件、视口变换、tooltip |
| `src/graph/model.rs` | `Node` / `Edge` / `GraphData` |
| `src/source.rs` | `GraphSource` trait + 内存示例数据（接 Neo4j / Nebula 时实现这个 trait） |
| `src/app.rs` | 工具栏（gpui-base `Input` / `Button`）、图、侧边栏 |

颜色取自 gpui-base `Theme` tokens，跟随系统深浅色。
