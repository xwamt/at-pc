# 不绑卡使用 Jev（或同类决策模型）的平台对比

> 实测时间：2026-09-20。网络位置：中国大陆，家庭宽带（DNS `192.168.66.1`）。
> 所有「可达性」结论来自本机 `curl` 实测，不是文档抄录——同一个平台在不同网络下结论会完全不同。

## 一句话结论

**真·Jev 目前有三条路，其中两条不需要信用卡：OpenRouter（注册不需卡，充值可用支付宝）与 TypeSafe 官方（早期访问）。**
Vercel 网关的促销免费价看着最香，但账户必须先绑卡，否则一律 `403 customer_verification_required`（已实测）。
Venice 那边 Jev 确实是免费的，**但大陆网络连不上**。

## 一、能拿到 Jev 的路径

| 平台 | Jev 是否免费 | 需要信用卡 | 本机可达 | 备注 |
| --- | --- | --- | --- | --- |
| **OpenRouter** | 否（$0.042/MTok，输出免费） | **不需要** | ✅ 200 / 1.4s | 充值支持**支付宝**、USDC。新用户有小额赠送额度 |
| **TypeSafe 官方** | 早期访问 | 不需要 | ✅ 405（GET 不允许）→ 可达 | key 取自 `console.typesafe.ai/keys`；有 waitlist，但已批量放行 |
| Vercel AI Gateway | 促销期标价免费（至 2026-09-25） | **需要** | ✅ 200 / 0.5s | 无卡时一律 403，**免费额度也要先绑卡才解锁** |
| Venice API | **是**（beta 免费） | 不需要 | ❌ 不通 | DNS 解析到 `199.59.150.12`（典型污染地址），10s 超时 |
| OpenRouter（`/v1/chat/completions`） | — | — | ✅ | ⚠️ **调不到 Jev**：446 条对话模型清单里没有它 |
| 任意平台的 `/v1/chat/completions` | — | — | — | ⚠️ Jev 是决策模型，OpenAI 兼容层一律无效 |

### 三条路的端点（已实测复核，手工 curl 照抄）

```
OpenRouter  : POST https://openrouter.ai/api/alpha/decisions      model=typesafe/jev-latest
TypeSafe    : POST https://api.typesafe.ai/v1/systemone           model=jev-latest
Vercel 网关 : POST https://ai-gateway.vercel.sh/v4/ai/evaluation-model
              头 ai-gateway-protocol-version: 0.0.1
                 ai-evaluation-model-specification-version: 4
                 ai-model-id: typesafe-ai/jev
```

**三者共同点：`/v1/chat/completions` 都调不到 Jev。** 它是「评估/决策模型」，不是对话模型。

### OpenRouter 的两个坑

1. 端点路径里有 `/alpha`，不是 `/v1/decisions`，也不是 `/v1/chat/completions`。
2. 余额不足返回 **402**，不是 401。把 402 当 401 排查会得出「key 无效」的错误结论。

### 实测到的端点存在性证据

```bash
# 用无效 key 打一下，看是 401（端点存在、鉴权失败）还是 404（端点不存在）
curl -s -X POST https://openrouter.ai/api/alpha/decisions \
  -H "Authorization: Bearer sk-invalid" -H "Content-Type: application/json" \
  -d '{"model":"typesafe/jev-1.13","state":"t","questions":{"q":{"type":"noul","instructions":"?"}}}'
# → 401 {"error":{"message":"Missing Authentication header","code":401}}
```

## 二、免费不绑卡、但**不是 Jev** 的替代品

这些是通用对话模型。要让它们扮演 Jev 的角色，需要自己用 prompt + JSON schema 模拟
`noul / choice / score` 三种原语——**会失去两样东西**：校准置信度（confidence 是训练目标，
不是让模型「顺口说一个数」能得到的），以及 70–500ms 的端到端延迟。

### 海外（本机可达，均无需信用卡）

| 平台 | 免费额度 | 本机可达 | 适合 |
| --- | --- | --- | --- |
| **Groq** | 250–14,400 req/天（按模型），~30 rpm | ✅ 403→可达 | 速度最快，适合高频决策循环 |
| **Cerebras** | ~100 万 tokens/天 | ✅ 403→可达 | 吞吐最高 |
| **NVIDIA NIM** | ~40 rpm | ✅ 200 / 0.3s | 模型最全（部分需手机验证） |
| **Cloudflare Workers AI** | 10,000 neurons/天 | ✅ 403→可达 | 边缘部署 |
| **Cohere** | ~1,000 次/月 | ✅ 403→可达 | rerank / 分类 |
| **Together** | 免费档 ~60 rpm | ✅ 403→可达 | 多种开源模型 |
| ~~Google AI Studio~~ | 永久免费、无卡 | ❌ 解析正常但连接超时 | 大陆网络阻断 |
| ~~HuggingFace Inference~~ | $0.10/月 | ❌ DNS 污染 | 大陆网络阻断 |
| ~~Mistral~~ | 免费 Experiment 档 | ❌ 超时 | 大陆网络阻断 |
| ~~GitHub Models~~ | — | ❌ 410 | **已于 2026-07-30 停服**，不要再照旧文档配置 |

### 国内（无需绑卡，延迟最优，约 130ms）

| 平台 | 免费额度 | 注册门槛 | 本机可达 |
| --- | --- | --- | --- |
| **硅基流动 SiliconFlow** | **6 个模型完全免费**（Qwen3.5-4B、Qwen3-8B、DeepSeek-R1-Distill-7B 等） | 手机号 | ✅ 401 / 0.13s |
| **阿里云百炼** | 新用户 **7000 万 tokens**（每个模型各 100 万，覆盖 70+ 模型），90 天有效 | **明确「无需绑定支付工具」** | ✅ 401 / 0.13s |
| **智谱 BigModel** | GLM Flash 系列免费 | 手机号 | ✅ 401 / 0.16s |
| **魔搭 ModelScope** | 免费 API 调用额度 | 阿里/手机号 | ✅ 200 / 0.13s |

> 阿里云百炼的额度**仅限华北 2（北京）地域**，且只抵扣实时推理；开通后在控制台可开启
> 「免费额度用完即停」避免自动转按量付费。

## 三、怎么选

| 你的目标 | 建议 |
| --- | --- |
| 要用**真 Jev** 且不想绑卡 | OpenRouter（`JEV_PROVIDER=openrouter`）。$5 ≈ 1.19 亿输入 token |
| 要用真 Jev 且想完全零成本 | 等 Vercel 促销期结束前绑卡；或走 TypeSafe 官方早期访问 |
| 只是想让 jev-use 这个决策层**跑起来**，成本敏感 | 硅基流动免费模型 / 阿里云百炼，自己封装三原语的 prompt（需改代码） |
| 对延迟敏感（决策循环每步都要问） | 国内平台（130ms）> Groq/Cerebras；OpenRouter 到 Jev 约 0.2–0.4s |

## 四、复现这份结论

```bash
# 可达性（HTTP 000 = 超时/不可达；403/401/405 = 服务端活着，只是拒绝未鉴权请求）
for u in https://openrouter.ai/api/v1/models \
         https://api.typesafe.ai/v1/systemone \
         https://api.venice.ai/api/v1/models \
         https://api.siliconflow.cn/v1/models \
         https://dashscope.aliyuncs.com/compatible-mode/v1/models; do
  printf "%-56s %s\n" "$u" "$(curl -s -o /dev/null -w '%{http_code}' -m 8 "$u")"
done

# DNS 污染判定：解析到 199.59.148.0/22 段基本可确认被污染
dig +short api.venice.ai
```

本仓库已把这套探测做成了命令：

```bash
node scripts/doctor.mjs            # 自动分辨「槽位错 / 缺卡 / 余额不足 / 端点错」
node scripts/doctor.mjs --no-live  # 只看本地配置，不联网
```
