import{u as _}from"./index-RHUnjAG_.js";const y="llm-nest-demo-v1",$="llm-nest-demo-config-v1",C=[{id:"deepseek",displayName:"DeepSeek",models:[{id:"deepseek-reasoner",displayName:"DeepSeek-R1"},{id:"deepseek-chat",displayName:"DeepSeek-V3"}]},{id:"anthropic",displayName:"Anthropic",models:[{id:"claude-5-sonnet",displayName:"Claude 5 Sonnet"},{id:"claude-4-5-opus",displayName:"Claude Opus 4.5"}]},{id:"openai",displayName:"OpenAI",models:[{id:"gpt-5.6",displayName:"GPT-5.6"},{id:"gpt-4.1-mini",displayName:"GPT-4.1 mini"}]},{id:"gemini",displayName:"Google",models:[{id:"gemini-3.7-pro",displayName:"Gemini 3.7 Pro"}]},{id:"ecnu",displayName:"ECNU",models:[{id:"ecnu-max",displayName:"ECNU Max"}]}],O={provider:"deepseek",model:"deepseek-reasoner"};function v(e){return[`用户的问题：${e.trim().slice(0,60)||"（未提供具体内容）"}`,"我需要先理解问题的边界：是想要完整可运行的示例，还是侧重讲解原理？",'从提问方式看，用户希望兼顾「可运行」与「关键点解释」，所以我按"代码 + 要点清单"的结构组织回答。',"回答中会包含一段带语法高亮的代码块，并配一个对照表帮助理解。"]}function M(e){return/rust|代码|服务器|http|编程/i.test(e)?`## 一个最小可用的异步 HTTP 服务器

下面用 **tokio + hyper** 实现一个返回 \`Hello, world\` 的服务器，整套代码只需要一个文件：

\`\`\`rust
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Request, Response, Server};
use std::convert::Infallible;

async fn handle(_req: Request<Body>) -> Result<Response<Body>, Infallible> {
    Ok(Response::new(Body::from("Hello, world!")))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let make_svc = make_service_fn(|_conn| async {
        Ok::<_, Infallible>(service_fn(handle))
    });

    let addr = "127.0.0.1:3000".parse()?;
    println!("listening on {}", addr);

    Server::bind(&addr).serve(make_svc).await?;
    Ok(())
}
\`\`\`

### 几个关键点

1. **异步运行时**：\`#[tokio::main]\` 宏负责启动 tokio runtime，\`serve().await\` 之后进程不会退出；
2. **Service 模型**：\`make_service_fn\` 为每个连接构造服务，\`service_fn\` 把普通函数包装成 hyper 的 \`Service\` trait；
3. **错误处理**：返回 \`Result<_, Infallible>\` 表示该服务永不失败，简化了示例。

> 提示：生产环境建议叠加超时与并发限制（\`tower\` 的 \`Timeout\` / \`ConcurrencyLimit\`）。

### 下一步

| 需求 | 推荐 |
| --- | --- |
| 路由匹配 | 引入 \`axum\`（基于 hyper 的路由 DSL） |
| TLS | \`rustls\` + \`axum-server\` |
| 优雅停机 | \`tokio::signal\` 监听 SIGTERM |

如果还想继续深入，我可以帮你把这段代码改造成带 \`/api\` 路由的 axum 版本。`:/翻译|translate/i.test(e)?`## 翻译结果

**原文**：${e.trim()}

**译文**：Your message has been translated into natural, idiomatic English with the tone preserved.

### 词句对照

| 原文 | 译文 | 说明 |
| --- | --- | --- |
| 译文示例 | translation sample | 术语保持直译 |
| 语气自然 | natural tone | 意译优先 |

> 说明：如果这是商务场景，我可以再调整一版更正式的语气。`:/公式|latex|math|数学/i.test(e)?`## 三个常见的数学公式

### 1. 一元二次方程的求根公式

对于 $ax^2 + bx + c = 0$（$a \\ne 0$），解为：

$$
x = \\frac{-b \\pm \\sqrt{b^2 - 4ac}}{2a}
$$

### 2. 欧拉公式

$$
e^{i\\theta} = \\cos\\theta + i\\sin\\theta
$$

当 $\\theta = \\pi$ 时得到 $e^{i\\pi} + 1 = 0$，常被称为数学中最优美的恒等式之一。

### 3. 正态分布的概率密度函数

$$
f(x) = \\frac{1}{\\sigma\\sqrt{2\\pi}} e^{-\\frac{(x-\\mu)^2}{2\\sigma^2}}
$$

> 需要我展开推导其中某一个，或补充微积分、线性代数、概率统计方向的常用公式吗？`:/推导|求导|梯度|反向传播|backprop/i.test(e)?`## 反向传播的梯度推导

以最简单的两层网络为例，损失 $L$ 对权重 $w$ 的梯度可以通过链式法则逐层回传：

$$
\\frac{\\partial L}{\\partial w} = \\frac{\\partial L}{\\partial y} \\cdot \\frac{\\partial y}{\\partial z} \\cdot \\frac{\\partial z}{\\partial w}
$$

其中 $z = w x + b$，激活函数为 $\\sigma$，则：

- $\\dfrac{\\partial z}{\\partial w} = x$
- $\\dfrac{\\partial y}{\\partial z} = \\sigma'(z)$

### 关键结论

1. **链式法则**：梯度是各层局部导数的乘积；
2. **参数更新**：$w \\leftarrow w - \\eta \\frac{\\partial L}{\\partial w}$，$\\eta$ 为学习率；
3. **数值稳定性**：Softmax 常与交叉熵配合，梯度可化简为 $y - t$。

> 练习：对 $L = \\frac{1}{2}(y - t)^2$ 手推一次，结果应与 $\\sigma$ 的导数形式一致。`:/架构|mermaid|流程图|拓扑|画图/i.test(e)?`## 系统架构流程

下面用 Mermaid 画一个典型的前后端 + LLM 网关架构：

\`\`\`mermaid
graph TD
    A["桌面客户端<br/>React + Tauri"] -->|invoke| B[Runtime]
    B --> C[SessionManager]
    B --> D[AiClient]
    D --> E{ModelRouter}
    E -->|protocol| F["OpenAI Provider"]
    E -->|protocol| G["Anthropic Provider"]
    E -->|protocol| H["Gemini Provider"]
    F --> I[("LLM API")]
    G --> I
    H --> I
    B --> J[("Session Store")]
\`\`\`

### 说明

- 客户端只调用 **Feature 业务方法**，消费 \`ChatEvent\` 流渲染；
- \`AiClient\` 负责模型路由 + 协议分发，\`ModelRouter\` 在首个 await 前冻结快照；
- 会话持久化写穿透：先落盘成功再改内存，失败不静默。

> 修改 \`graph TD\` 为 \`graph LR\` 可切换横向布局。`:`## 关于这个问题

这是一个很好的问题，我从几个角度展开：

### 核心思路

- **先明确目标**：搞清楚真正要解决的问题，而不是直接动手；
- **拆解步骤**：把大问题切成 2–3 个小步骤，逐个击破；
- **验证反馈**：每完成一步都检查结果，及时纠偏。

### 推荐的做法

1. 先写一版最简实现，跑通主路径；
2. 再补充边界情况与错误处理；
3. 最后做优化（性能 / 可读性）。

| 阶段 | 关注点 | 产出 |
| --- | --- | --- |
| 明确目标 | 需求、约束 | 一句话描述 |
| 最简实现 | 主路径 | 可运行的 Demo |
| 打磨 | 边界、错误 | 稳定版本 |

### 一个小示例

\`\`\`ts
const result = items
  .filter((it) => it.active)
  .map((it) => it.value)
  .reduce((a, b) => a + b, 0)
\`\`\`

如果还有更具体的背景，欢迎补充细节，我可以给出更贴合你场景的方案。`}let I=!1;function d(){try{!I&&new URLSearchParams(location.search).get("reset")&&(I=!0,localStorage.removeItem(y),localStorage.removeItem($));const t=localStorage.getItem(y);if(t)return JSON.parse(t)}catch{}const e=L();return u(e),e}function u(e){try{localStorage.setItem(y,JSON.stringify(e))}catch{}}function L(){const e=Date.now(),t=r=>new Date(e-r).toISOString(),s=36e5,i=24*s,a={id:"s-demo-1",title:"用 Rust 写一个异步 HTTP 服务器",createdAt:t(2*s),updatedAt:t(20*6e4),messageCount:2},n=(r,p,l)=>({id:r,title:p,createdAt:t(l),updatedAt:t(l-25*6e4),messageCount:0}),c=[{id:"m-demo-u1",role:"user",content:"用 Rust 写一个简单的异步 HTTP 服务器，并解释关键点",status:"done",createdAt:e-21*6e4},{id:"m-demo-a1",role:"assistant",content:M("rust"),reasoning:v("用 Rust 写一个简单的异步 HTTP 服务器，并解释关键点").join(`
`),status:"done",thinkingMs:2840,createdAt:e-20*6e4}];return{sessions:[a,n("s-demo-2","React 组件性能优化技巧",5*s),n("s-demo-3","Tailwind CSS v4 新特性总结",1*i+3*s),n("s-demo-4","神经网络反向传播推导",3*i),n("s-demo-5","深夜食堂推荐",9*i),n("s-demo-6","项目周报模板",26*i)],messages:{"s-demo-1":c}}}function x(){try{const e=localStorage.getItem($);if(e)return JSON.parse(e)}catch{}return{currentModel:O,temperature:.7,maxTokens:4096}}function P(e){try{localStorage.setItem($,JSON.stringify(e))}catch{}}const g=new Map,h=e=>new Promise(t=>setTimeout(t,e));function b(e){const t=[];let s=0;for(;s<e.length;){const i=8+Math.floor(Math.random()*14);let a=Math.min(s+i,e.length);if(a<e.length){const n=e.indexOf(`
`,s);if(n!==-1&&n<=a)a=n+1;else{const c=e.lastIndexOf(" ",a);c>s+4&&(a=c+1)}}t.push(e.slice(s,a)),s=a}return t}const G={async init(){const e=d(),t=C,s=x(),i=[...e.sessions].sort((a,n)=>+new Date(n.updatedAt)-+new Date(a.updatedAt));return{config:s,providers:t,sessions:i,version:"0.1.0 (web demo)"}},async chat(e,t){var k;const s=e.sessionId,i=(g.get(s)??0)+1;g.set(s,i);const a=()=>g.get(s)===i,n=d(),c=n.messages[s]??[],r=e.messageId??`ai-${Date.now()}-${Math.random().toString(36).slice(2,7)}`;t({type:"message_start",messageId:r});const p={id:`u-${Date.now()}`,role:"user",content:e.input,status:"done",...e.attachments&&e.attachments.length>0?{attachments:e.attachments}:{},createdAt:Date.now()};c.push(p);const l=_.getState().reasoningEffort!=="off",A=Date.now();let w="",S="";if(l){const o=v(e.input);for(const f of o){for(const D of b(f)){if(!a()){t({type:"cancelled",messageId:r});return}w+=D,t({type:"reasoning_delta",messageId:r,content:D}),await h(18+Math.random()*42)}await h(90+Math.random()*130)}}const R=Date.now()-A,T=((k=e.attachments)!=null&&k.length?`> 已收到 ${e.attachments.length} 个附件：${e.attachments.map(o=>o.name).join("、")}（${e.attachments.filter(o=>o.mime.startsWith("image/")).length} 张图片）。

`:"")+M(e.input);for(const o of b(T)){if(!a()){t({type:"cancelled",messageId:r});return}S+=o,t({type:"delta",messageId:r,content:o});const f=o.endsWith(`

`)?90:0;await h(10+Math.random()*30+f)}if(!a()){t({type:"cancelled",messageId:r});return}const N={id:r,role:"assistant",content:S,reasoning:l?w:void 0,status:"done",thinkingMs:l?R:void 0,createdAt:Date.now()};c.push(N);const m=n.sessions.find(o=>o.id===s);if(m){if(m.title==="新对话"||!m.title){const o=e.input.trim().replace(/\s+/g," ");m.title=o.slice(0,30)+(o.length>30?"…":"")}m.updatedAt=new Date().toISOString(),m.messageCount=c.length}n.messages[s]=c,u(n),t({type:"finished",messageId:r})},async cancelChat(e){g.set(e,(g.get(e)??0)+1)},async getMessages(e){return d().messages[e]??[]},async listSessions(){return[...d().sessions].sort((t,s)=>+new Date(s.updatedAt)-+new Date(t.updatedAt))},async newSession(){const e=d(),t={id:`s-${Date.now()}-${Math.random().toString(36).slice(2,7)}`,title:"新对话",createdAt:new Date().toISOString(),updatedAt:new Date().toISOString(),messageCount:0};return e.sessions.push(t),e.messages[t.id]=[],u(e),t},async deleteSession(e){const t=d();t.sessions=t.sessions.filter(s=>s.id!==e),delete t.messages[e],u(t)},async renameSession(e,t){const s=d(),i=s.sessions.find(a=>a.id===e);i&&(i.title=t,u(s))},async setConfig(e){P(e)},async setMessageFeedback(e,t,s){const i=d(),n=(i.messages[e]??[]).find(c=>c.id===t);n&&(n.feedback=s,u(i))}};export{G as webApi};
