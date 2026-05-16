# TaskBar 实时分段进度条设计

## 概述

在现有 TaskBar 折叠态中嵌入一条 4px 分段进度条，实时反映批量缓存任务的整体进度和各状态占比。纯前端改动，无需修改 Rust 后端。

## 视觉规格

### 进度条位置

统计文字行下方，按钮行上方（折叠态内）：

```
┌──────────────────────────────────────────────────────────┐
│ ▲  任务: 62/100 完成  [3 进行中] [2 失败]       [重试][清除] │
│ ████████████████████████████░░░░██░░░░░░░░░░░░░░░░░░░░░░ │
└──────────────────────────────────────────────────────────┘
```

### 尺寸

- 高度：4px
- 圆角：2px
- 水平 padding：与 TaskBar 内容对齐（px-4）
- 上方间距：6px

### 分段颜色

| 状态 | 颜色 | Tailwind class | 动画 |
|------|------|----------------|------|
| Success | 绿色 | `bg-green-500` | 无 |
| Downloading / Uploading | 蓝色 | `bg-blue-500` | `animate-pulse` |
| Failed | 红色 | `bg-red-500` | 无 |
| Pending + Skipped | 灰色 | `bg-muted-foreground/30` | 无 |

### 百分比文字

在统计行右侧（重试/清除按钮左边）显示百分比数字，如 `62%`。仅在有进行中或等待中任务时显示。

## 行为

### 显示条件

- 有任务（`tasks.length > 0`）且存在非 Success 状态的任务时显示进度条
- 无任务时保持现有"暂无任务"空状态

### 渐隐逻辑

1. 所有任务完成（无 Pending/Downloading/Uploading 状态）时触发
2. 进度条保持 100% 绿色状态停留 3 秒
3. 通过 `opacity` transition（300ms）淡出
4. 淡出完成后从 DOM 移除（或 `hidden`）

### 动画

- 各段宽度变化：`transition: width 300ms ease`
- 进行中段：Tailwind `animate-pulse`
- 渐隐：`transition: opacity 300ms ease`

## 数据流

```
tasks[] (zustand store)
  → useMemo 计算各状态 count
  → 转换为百分比宽度
  → 渲染分段 div
```

无需新增事件或后端接口。现有 `task-progress` 事件已实时更新 store 中的 tasks 数组。

## 实现范围

仅修改一个文件：`src/components/layout/TaskBar.tsx`

新增内容：
- `ProgressBar` 组件（内部组件，不导出）
- 渐隐计时逻辑（useEffect + setTimeout）

不涉及：
- Rust 后端改动
- 新增依赖
- 新增 store 或事件
