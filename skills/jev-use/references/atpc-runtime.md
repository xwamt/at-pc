# at-pc 运行示例

以下代码在 WorkBuddy / 任意 Node ≥ 20 环境里即可运行，**不依赖 Codex 的 `cua_repl`**。

## 一、最小端到端（CLI）

```bash
cd {{REPO_DIR}}

# 看有哪些终端在线
node scripts/run-atpc.mjs --list

# 预览一步（不点击任何东西）
node scripts/run-atpc.mjs --terminal laptop-roeh3p85-b6a918 \
  --app Calculator --goal "enter 7 then multiply by 8 and show the result" --steps 6

# 预览通过后实际执行；--verify window:xxx 用窗口标题做总目标判据
node scripts/run-atpc.mjs --terminal laptop-roeh3p85-b6a918 \
  --app Calculator --goal "enter 7 then multiply by 8 and show the result" \
  --steps 6 --run --verify window:计算器
```

`--verify` 两种写法：

- `--verify 文本` → 界面上出现名称包含该文本的控件即算达成（`hasElement`）
- `--verify window:文本` → 活动窗口标题包含该文本即算达成（`hasActiveWindow`）

## 二、代码调用

```js
import { runAtpcTask } from "{{REPO_DIR}}/scripts/run-atpc.mjs";
import { hasActiveWindow, atpcVerifier } from "{{REPO_DIR}}/scripts/atpc-ui.mjs";

const result = await runAtpcTask({
  terminalId: "laptop-roeh3p85-b6a918",
  appName: "Calculator",              // 窗口标题子串，同时用于白名单与聚焦
  goal: "enter 7 then multiply by 8 and show the result",  // 英文，Jev 英文最准
  dryRun: true,                       // 确认后改 false
  maxSteps: 6,                        // at-pc 单次调用默认 30s 超时，别一次跑太长
  verify: atpcVerifier((els) => els.some((e) => e.label.includes("56"))),
});
console.log(result.status, result.tracePath);
```

`runAtpcTask` 已自动接好：

| 环节 | 接线 |
| --- | --- |
| 观测 | `driver.observe()` → Tier 2 `get_ui_tree`，交互候选 < 2 时自动降级 Tier 3 `get_marked_screen`（SoM） |
| 解析 | `parse=parseAtpcTree`（支持 Windows UIA JSON 树与 Set-of-Mark 标记文本） |
| 上下文 | `context=buildAtpcContext` |
| 门槛 | `ATPC_ALLOWED_APPS` / `ATPC_LOW_RISK_APPS` / `ATPC_THRESHOLDS` |
| 执行 | `driver.click()` → `click_element(id)`（UIA）或 `click_mark(mark_id)`（SoM），失效时回落元素中心坐标；输入支持 `driver.pasteText()` |

想换自己的一套白名单：

```js
await runAtpcTask({
  terminalId, appName: "notepad", goal: "...",
  allowedApps: ["Notepad", "记事本"],
  lowRiskApps: ["Notepad", "记事本"],
  thresholds: { doneProbability: 0.95, riskConfirm: 0.15, minConfidence: 0.5, stopConfidence: 0.3, lowRiskMinConfidence: 0.4 },
});
```

## 三、只读探针（先确认这个窗口能不能用）

```js
import { createAtpcDriver } from "{{REPO_DIR}}/scripts/driver-atpc.mjs";
import { parseAtpcTree, buildAtpcContext } from "{{REPO_DIR}}/scripts/atpc-ui.mjs";
import { selectCandidates } from "{{REPO_DIR}}/scripts/loop.mjs";

const d = createAtpcDriver({ terminalId: "laptop-roeh3p85-b6a918" });
await d.bind("计算器");
const obs = await d.observe();
const els = parseAtpcTree(obs);
console.log("元素数:", els.length, "总元素:", els.totalElements, "模式:", els.mode);
console.log(buildAtpcContext(obs));
console.log("候选:", selectCandidates(els, "press the digit seven").map((c) => `i${c.index} ${c.role} ${c.label}`));
```

**若 UIA 返回空（如 Electron 应用），driver 会自动进入 `mode: "som"` 模式提取屏幕视觉标记，Jev 可直接选择 `mark_id` 执行 `click_mark`。**

## 四、直接问 Jev 一次（不碰界面）

```bash
cd {{REPO_DIR}}
echo '{"state":{"goal":"enter the digit seven","app":"Calculator","candidates":[{"id":"i7","desc":"Button: 七"},{"id":"i8","desc":"Button: 乘"}]},
 "questions":{"target":{"type":"choice","instructions":"Which element?","criteria":{"i7":"Button: 七","i8":"Button: 乘"}}}}' \
| node scripts/jev-decide.mjs
```

输出会带上 `provider`（`gateway` 或 `systemone`）、`latencyMs`、`costUsd`、`confidence`。

## 五、注意事项

- `driver.observe()` 返回的是 JSON 字符串而非对象，这样 `loop.mjs` 里 `observation === previousObservation` 的「界面是否变化」判定才成立。要对象就自己 `JSON.parse`。
- `verify` 收到的是同一个 JSON 字符串；用 `atpcVerifier(predicate)` 包一层可直接拿到解析后的元素数组。
- element_id 在两次观测之间必然失效，**不要缓存它跨步骤使用**。driver 内部保留上一份 rect 只为兜底坐标点击。
- 涉及权限、支付、删除、发送的弹窗一律停在 `confirm`，不得自动代为确认。权限类对话框（如 Windows 的联网许可）必须由人处理。
