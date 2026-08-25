import{u as E}from"./index-DajdMm3L.js";const S="llm-nest-demo-v1",v="llm-nest-demo-config-v1",k=[{id:"deepseek",displayName:"DeepSeek",models:[{id:"deepseek-reasoner",displayName:"DeepSeek-R1"},{id:"deepseek-chat",displayName:"DeepSeek-V3"}]},{id:"anthropic",displayName:"Anthropic",models:[{id:"claude-5-sonnet",displayName:"Claude 5 Sonnet"},{id:"claude-4-5-opus",displayName:"Claude Opus 4.5"}]},{id:"openai",displayName:"OpenAI",models:[{id:"gpt-5.6",displayName:"GPT-5.6"},{id:"gpt-4.1-mini",displayName:"GPT-4.1 mini"}]},{id:"gemini",displayName:"Google",models:[{id:"gemini-3.7-pro",displayName:"Gemini 3.7 Pro"}]},{id:"ecnu",displayName:"ECNU",models:[{id:"ecnu-max",displayName:"ECNU Max"}]}],G={provider:"deepseek",model:"deepseek-reasoner"};function _(e){return[`用户的问题：${e.trim().slice(0,60)||"（未提供具体内容）"}`,"我需要先理解问题的边界：是想要完整可运行的示例，还是侧重讲解原理？",'从提问方式看，用户希望兼顾「可运行」与「关键点解释」，所以我按"代码 + 要点清单"的结构组织回答。',"回答中会包含一段带语法高亮的代码块，并配一个对照表帮助理解。"]}function C(e){return/rust|代码|服务器|http|编程/i.test(e)?`## 一个最小可用的异步 HTTP 服务器

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

如果还有更具体的背景，欢迎补充细节，我可以给出更贴合你场景的方案。`}let T=!1;function c(){try{!T&&new URLSearchParams(location.search).get("reset")&&(T=!0,localStorage.removeItem(S),localStorage.removeItem(v));const t=localStorage.getItem(S);if(t){const s=JSON.parse(t);return s.providers??(s.providers=k),s}}catch{}const e=H();return m(e),e}function m(e){try{localStorage.setItem(S,JSON.stringify(e))}catch{}}function H(){const e=Date.now(),t=r=>new Date(e-r).toISOString(),s=36e5,n=24*s,a={id:"s-demo-1",title:"用 Rust 写一个异步 HTTP 服务器",createdAt:t(2*s),updatedAt:t(20*6e4),messageCount:2},i=(r,g,l)=>({id:r,title:g,createdAt:t(l),updatedAt:t(l-25*6e4),messageCount:0}),d=[{id:"m-demo-u1",role:"user",content:"用 Rust 写一个简单的异步 HTTP 服务器，并解释关键点",status:"done",createdAt:e-21*6e4},{id:"m-demo-a1",role:"assistant",content:C("rust"),reasoning:_("用 Rust 写一个简单的异步 HTTP 服务器，并解释关键点").join(`
`),status:"done",thinkingMs:2840,createdAt:e-20*6e4}];return{sessions:[a,i("s-demo-2","React 组件性能优化技巧",5*s),i("s-demo-3","Tailwind CSS v4 新特性总结",1*n+3*s),i("s-demo-4","神经网络反向传播推导",3*n),i("s-demo-5","深夜食堂推荐",9*n),i("s-demo-6","项目周报模板",26*n)],messages:{"s-demo-1":d},providers:k}}function z(){try{const e=localStorage.getItem(v);if(e)return JSON.parse(e)}catch{}return{currentModel:G,temperature:.7,maxTokens:4096}}function U(e){try{localStorage.setItem(v,JSON.stringify(e))}catch{}}const u=new Map,w=e=>new Promise(t=>setTimeout(t,e));function R(e){const t=[];let s=0;for(;s<e.length;){const n=8+Math.floor(Math.random()*14);let a=Math.min(s+n,e.length);if(a<e.length){const i=e.indexOf(`
`,s);if(i!==-1&&i<=a)a=i+1;else{const d=e.lastIndexOf(" ",a);d>s+4&&(a=d+1)}}t.push(e.slice(s,a)),s=a}return t}const J={async init(){const e=c(),t=e.providers,s=z(),n=[...e.sessions].sort((a,i)=>+new Date(i.updatedAt)-+new Date(a.updatedAt));return{config:s,providers:t,sessions:n,version:"0.1.0 (web demo)"}},async chat(e,t){var N;const s=e.sessionId,n=(u.get(s)??0)+1;u.set(s,n);const a=()=>u.get(s)===n,i=c(),d=i.messages[s]??[],r=e.messageId??`ai-${Date.now()}-${Math.random().toString(36).slice(2,7)}`;t({type:"message_start",messageId:r});const g={id:`u-${Date.now()}`,role:"user",content:e.input,status:"done",...e.attachments&&e.attachments.length>0?{attachments:e.attachments}:{},createdAt:Date.now()};d.push(g);const l=E.getState().reasoningEffort!=="off",x=Date.now(),O=Date.now();let M="",f="";if(l){const o=_(e.input);for(const $ of o){for(const A of R($)){if(!a()){t({type:"cancelled",messageId:r});return}M+=A,t({type:"reasoning_delta",messageId:r,content:A}),await w(18+Math.random()*42)}await w(90+Math.random()*130)}}const h=Date.now()-O,P=((N=e.attachments)!=null&&N.length?`> 已收到 ${e.attachments.length} 个附件：${e.attachments.map(o=>o.name).join("、")}（${e.attachments.filter(o=>o.mime.startsWith("image/")).length} 张图片）。

`:"")+C(e.input);for(const o of R(P)){if(!a()){t({type:"cancelled",messageId:r});return}f+=o,t({type:"delta",messageId:r,content:o});const $=o.endsWith(`

`)?90:0;await w(10+Math.random()*30+$)}if(!a()){t({type:"cancelled",messageId:r});return}const y=Math.max(32,Math.round(e.input.length/2)+56),D=Math.max(1,Math.round(f.length/2)),L=Math.floor(y*.4),b={promptTokens:y,completionTokens:D,totalTokens:y+D,cachedTokens:L},I={ttftMs:l?h:Math.round(400+Math.random()*600),...l?{reasoningMs:h}:{},totalMs:Date.now()-x},B={id:r,role:"assistant",content:f,reasoning:l?M:void 0,status:"done",thinkingMs:l?h:void 0,usage:b,timings:I,createdAt:Date.now()};d.push(B);const p=i.sessions.find(o=>o.id===s);if(p){if(p.title==="新对话"||!p.title){const o=e.input.trim().replace(/\s+/g," ");p.title=o.slice(0,30)+(o.length>30?"…":"")}p.updatedAt=new Date().toISOString(),p.messageCount=d.length}i.messages[s]=d,m(i),t({type:"finished",messageId:r,usage:b,timings:I})},async cancelChat(e){u.set(e,(u.get(e)??0)+1)},async getMessages(e){return c().messages[e]??[]},async listSessions(){return[...c().sessions].sort((t,s)=>+new Date(s.updatedAt)-+new Date(t.updatedAt))},async newSession(){const e=c(),t={id:`s-${Date.now()}-${Math.random().toString(36).slice(2,7)}`,title:"新对话",createdAt:new Date().toISOString(),updatedAt:new Date().toISOString(),messageCount:0};return e.sessions.push(t),e.messages[t.id]=[],m(e),t},async deleteSession(e){const t=c();t.sessions=t.sessions.filter(s=>s.id!==e),delete t.messages[e],m(t)},async renameSession(e,t){const s=c(),n=s.sessions.find(a=>a.id===e);n&&(n.title=t,m(s))},async setConfig(e){U(e)},async setMessageFeedback(e,t,s){const n=c(),i=(n.messages[e]??[]).find(d=>d.id===t);i&&(i.feedback=s,m(n))},async listProviderTemplates(){return k.map(e=>{var t;return{id:e.id,displayName:e.displayName,protocol:"openai",baseUrl:"",apiKeyEnv:`${e.id.toUpperCase().replace(/[^A-Z0-9]+/g,"_")}_API_KEY`,defaultModel:((t=e.models[0])==null?void 0:t.id)??"",models:e.models.map(s=>({id:s.id,displayName:s.displayName}))}})},async addProvider(e){const t=c(),s=t.providers.find(a=>a.id===e.id),n={id:e.id,displayName:e.id,models:(e.models??[]).map(a=>({id:a.model??a.id,displayName:a.displayName??a.model??a.id}))};return s?(s.displayName=n.displayName,s.models=n.models):t.providers.push(n),m(t),t.providers},async deleteProvider(e){const t=c();return t.providers=t.providers.filter(s=>s.id!==e),m(t),t.providers}};export{J as webApi};
